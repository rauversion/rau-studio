use super::*;

pub(super) fn conflicting_destinations(conn: &Connection) -> Result<BTreeSet<PathBuf>, String> {
    let mut owners: BTreeMap<PathBuf, BTreeSet<PathBuf>> = BTreeMap::new();
    let rows = conn
        .prepare("SELECT source_path, target_path FROM local_conversion_items")
        .map_err(|e| e.to_string())?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    for (source, target) in rows {
        let Ok(target) = regeneration::destination_key(Path::new(&target)) else {
            continue;
        };
        let source = fs::canonicalize(&source).unwrap_or_else(|_| PathBuf::from(source));
        owners.entry(target).or_default().insert(source);
    }
    Ok(owners
        .into_iter()
        .filter_map(|(target, sources)| (sources.len() > 1).then_some(target))
        .collect())
}

fn stage(
    app: &AppHandle,
    item: &mut LocalConversionItem,
    phase: &str,
    es: &str,
    en: &str,
) -> Result<(), String> {
    item.state = "running".into();
    item.message = Some(settings::localized(app, es, en));
    update_item_state(app, &item.id, "running", item.message.as_deref(), None)?;
    let mut event = item_progress_event(item, None, None, None);
    event.phase = phase.into();
    emit_progress(app, event);
    Ok(())
}

pub(super) fn regenerate_item(
    app: &AppHandle,
    mut item: LocalConversionItem,
) -> LocalConversionItem {
    let target = PathBuf::from(&item.target_path);
    let existed = target.is_file();
    let operation_id = item
        .operation_id
        .clone()
        .expect("operation ID allocated before worker");
    let mut published = false;
    let result = (|| {
        stage(
            app,
            &mut item,
            "preparing",
            "Preparando regeneración",
            "Preparing regeneration",
        )?;
        fs::create_dir_all(target.parent().ok_or("Destination has no parent")?)
            .map_err(|e| e.to_string())?;
        let job = Regeneration::prepare(Path::new(&item.source_path), &target, || {
            system::ffprobe_command(app)
        })?;
        let conn = open_db(app)?;
        conn.execute("UPDATE local_conversion_attempts SET phase = 'encoding', temporary_path = ?2 WHERE id = ?1",
            params![operation_id, job.temporary.to_string_lossy()]).map_err(|e| e.to_string())?;
        stage(
            app,
            &mut item,
            "encoding",
            "Regenerando AIFF",
            "Regenerating AIFF",
        )?;
        run_ffmpeg_with_args(
            app,
            &item,
            Path::new(&item.source_path),
            &job.temporary,
            job.args.clone(),
        )?;
        stage(
            app,
            &mut item,
            "verifying",
            "Verificando AIFF",
            "Verifying AIFF",
        )?;
        let fingerprint =
            job.validate(|| system::ffprobe_command(app), system::ffmpeg_command(app))?;
        conn.execute("UPDATE local_conversion_attempts SET phase = 'publishing', fingerprint_json = ?2 WHERE id = ?1",
            params![operation_id, serde_json::to_string(&fingerprint).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        stage(
            app,
            &mut item,
            "publishing",
            "Actualizando archivo AIFF",
            "Publishing AIFF",
        )?;
        job.publish()?;
        published = true;
        finish_published(app, &conn, &item, &operation_id)
    })();

    match result {
        Ok(message) => {
            item.state = "converted".into();
            item.message = Some(message);
        }
        Err(error) if published => {
            // The durable publication fingerprint lets the next refresh reconcile this.
            item.state = "converted".into();
            item.message = Some(settings::localized(
                app,
                &format!("AIFF regenerado; historial pendiente de actualizar: {error}"),
                &format!("AIFF regenerated; history refresh pending: {error}"),
            ));
        }
        Err(error) => {
            if let Ok(conn) = open_db(app) {
                let _ = conn.execute(
                    "UPDATE local_conversion_attempts SET phase = 'failed' WHERE id = ?1",
                    params![operation_id],
                );
            }
            let message = if existed {
                settings::localized(
                    app,
                    &format!("No se pudo regenerar; el AIFF anterior se conservó: {error}"),
                    &format!("Could not regenerate; the previous AIFF was preserved: {error}"),
                )
            } else {
                settings::localized(
                    app,
                    &format!("No se pudo regenerar: {error}"),
                    &format!("Could not regenerate: {error}"),
                )
            };
            return fail_item(app, item, &message);
        }
    }
    item.target_exists = target.is_file();
    if let Ok(conn) = open_db(app) {
        if let Ok(Some(saved)) = get_item(&conn, &item.id) {
            item.completed_at = saved.completed_at;
            item.updated_at = saved.updated_at;
        }
    }
    emit_progress(app, item_progress_event(&item, Some(100.0), None, None));
    emit_log(
        app,
        LocalConversionLogEvent {
            level: "info".into(),
            item_id: Some(item.id.clone()),
            name: Some(item.source_name.clone()),
            message: item.message.clone().unwrap_or_default(),
        },
    );
    let _ = app.emit(
        "conversion-file-updated",
        json!({ "path": item.target_path, "operation_id": operation_id }),
    );
    item
}

fn finish_published(
    app: &AppHandle,
    conn: &Connection,
    item: &LocalConversionItem,
    operation_id: &str,
) -> Result<String, String> {
    // Keep catalog refresh retryable without re-encoding a successfully published file.
    conn.execute(
        "UPDATE local_conversion_attempts SET phase = 'catalog_pending' WHERE id = ?1",
        params![operation_id],
    )
    .map_err(|e| e.to_string())?;
    let message = match crate::playlist_index::refresh_converted_file(
        app,
        &item.id,
        Path::new(&item.target_path),
    ) {
        Ok(()) => settings::localized(app, "AIFF regenerado", "AIFF regenerated"),
        Err(error) => {
            let message = settings::localized(app,
                &format!("AIFF regenerado; catálogo pendiente. Pulsa Actualizar para reintentar: {error}"),
                &format!("AIFF regenerated; catalog refresh pending. Click Refresh to retry: {error}"));
            update_item_state(app, &item.id, "converted", Some(&message), None)?;
            return Ok(message);
        }
    };
    update_item_state(app, &item.id, "converted", Some(&message), None)?;
    conn.execute(
        "UPDATE local_conversion_attempts SET phase = 'completed' WHERE id = ?1",
        params![operation_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(message)
}

pub(super) fn reconcile_regenerations(app: &AppHandle, conn: &Connection) -> Result<(), String> {
    let attempts = conn.prepare("SELECT id, item_id FROM local_conversion_attempts WHERE phase NOT IN ('completed', 'failed')")
        .map_err(|e| e.to_string())?.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    for (id, item_id) in attempts {
        let Some(item) = get_item(conn, &item_id)? else {
            continue;
        };
        let Ok(_lease) = DestinationLease::acquire(Path::new(&item.target_path)) else {
            continue;
        };
        let interrupted = settings::localized(
            app,
            "Regeneración interrumpida; revisa el archivo y vuelve a intentar",
            "Regeneration interrupted; check the file and retry",
        );
        recover_attempt(conn, &id, &item, &interrupted, || {
            finish_published(app, conn, &item, &id).map(|_| ())
        })?;
    }
    Ok(())
}

fn recover_attempt(
    conn: &Connection,
    id: &str,
    item: &LocalConversionItem,
    interrupted_message: &str,
    finish: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    // Re-read after reserving the destination, as another request may have
    // completed since reconciliation took its snapshot.
    let current = get_item(conn, &item.id)?.ok_or("Conversion item not found")?;
    if current.operation_id.as_deref() != Some(id) {
        conn.execute(
            "UPDATE local_conversion_attempts SET phase = 'failed' WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }
    let (phase, temporary_path, fingerprint_json): (String, String, Option<String>) = conn.query_row(
        "SELECT phase, temporary_path, fingerprint_json FROM local_conversion_attempts WHERE id = ?1",
        params![id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(|e| e.to_string())?;
    if phase == "completed" || phase == "failed" {
        return Ok(());
    }
    let expected = fingerprint_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<Fingerprint>(s).ok());
    let published =
        expected.is_some() && Fingerprint::read(Path::new(&item.target_path)).ok() == expected;
    if published {
        finish()?;
    } else {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("UPDATE local_conversion_items SET state = 'failed', message = ?2, updated_at = ?3 WHERE id = ?1", params![item.id, interrupted_message, timestamp()]).map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE local_conversion_attempts SET phase = 'failed' WHERE id = ?1",
            params![id],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    let temporary = Path::new(&temporary_path);
    if temporary.parent() == Path::new(&item.target_path).parent()
        && temporary
            .file_name()
            .and_then(|v| v.to_str())
            .is_some_and(|v| v.starts_with(".rau-regenerate-") && v.ends_with(".aiff"))
    {
        let _ = fs::remove_file(temporary);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeneration_journal_recovers_both_sides_of_publication_and_retains_history() {
        let root = std::env::temp_dir().join(format!("rau-journal-test-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("converted")).unwrap();
        let source = root.join("track.flac");
        fs::write(&source, b"original").unwrap();
        let db = root.join("test.sqlite3");
        let conn = Connection::open(&db).unwrap();
        init_db(&conn).unwrap();
        let item = register_source_path(&conn, source.clone())
            .unwrap()
            .unwrap();
        let temp = root.join("converted/.rau-regenerate-test.aiff");
        fs::write(&item.target_path, b"old output").unwrap();
        fs::write(&temp, b"verified new output").unwrap();
        let fingerprint = serde_json::to_string(&Fingerprint::read(&temp).unwrap()).unwrap();
        conn.execute("UPDATE local_conversion_items SET state = 'running', last_operation = 'regenerate', operation_id = 'attempt', completed_at = 'previous success' WHERE id = ?1", params![item.id]).unwrap();
        conn.execute("INSERT INTO local_conversion_attempts (id, item_id, phase, temporary_path, fingerprint_json) VALUES ('attempt', ?1, 'publishing', ?2, ?3)", params![item.id, temp.to_string_lossy(), fingerprint]).unwrap();
        drop(conn);
        // Reopen as after an application restart before the rename.
        let conn = Connection::open(&db).unwrap();
        init_db(&conn).unwrap();
        let item = get_item(&conn, &item.id).unwrap().unwrap();
        assert_eq!(item.last_operation, ConversionMode::Regenerate);
        recover_attempt(&conn, "attempt", &item, "Interrupted", || {
            panic!("unpublished output must not be reported as success")
        })
        .unwrap();
        assert_eq!(fs::read(&item.target_path).unwrap(), b"old output");
        assert!(!temp.exists());
        let saved = get_item(&conn, &item.id).unwrap().unwrap();
        assert_eq!(saved.state, "failed");
        assert_eq!(saved.completed_at.as_deref(), Some("previous success"));
        assert!(saved.target_exists);
        // Simulate a crash immediately after the rename, before SQLite success.
        fs::write(&temp, b"verified new output").unwrap();
        let fingerprint = serde_json::to_string(&Fingerprint::read(&temp).unwrap()).unwrap();
        conn.execute(
            "UPDATE local_conversion_attempts SET phase = 'publishing', fingerprint_json = ?1",
            params![fingerprint],
        )
        .unwrap();
        fs::rename(&temp, &item.target_path).unwrap();
        recover_attempt(&conn, "attempt", &saved, "Interrupted", || {
            conn.execute(
                "UPDATE local_conversion_items SET state = 'converted' WHERE id = ?1",
                params![item.id],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(
            get_item(&conn, &item.id).unwrap().unwrap().state,
            "converted"
        );
        assert_eq!(fs::read(&source).unwrap(), b"original");
        // A worker may finish after reconciliation reads its list of attempts.
        conn.execute(
            "UPDATE local_conversion_attempts SET phase = 'completed', fingerprint_json = NULL",
            [],
        )
        .unwrap();
        recover_attempt(&conn, "attempt", &saved, "Interrupted", || {
            panic!("already completed")
        })
        .unwrap();
        assert_eq!(
            get_item(&conn, &item.id).unwrap().unwrap().state,
            "converted"
        );
        // A new operation's state wins over any stale recovery work.
        conn.execute(
            "UPDATE local_conversion_items SET operation_id = 'newer', state = 'converted'",
            [],
        )
        .unwrap();
        recover_attempt(&conn, "attempt", &saved, "Interrupted", || {
            panic!("stale operation")
        })
        .unwrap();
        assert_eq!(
            get_item(&conn, &item.id).unwrap().unwrap().state,
            "converted"
        );
        drop(conn);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn regeneration_blocks_distinct_originals_with_the_same_output_name() {
        let root = std::env::temp_dir().join(format!("rau-collision-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        for extension in ["flac", "mp3"] {
            let source = root.join(format!("track.{extension}"));
            fs::write(&source, b"source").unwrap();
            register_source_path(&conn, source).unwrap();
        }
        let conflicts = conflicting_destinations(&conn).unwrap();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts
            .contains(&regeneration::destination_key(&root.join("converted/track.aiff")).unwrap()));
        fs::remove_dir_all(root).unwrap();
    }
}
