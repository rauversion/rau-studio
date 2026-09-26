use super::*;

static REFRESH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn refresh_converted_file(
    app: &AppHandle,
    item_id: &str,
    path: &Path,
) -> Result<(), String> {
    let mut conn = open_db(app)?;
    let metadata = probe_local_track_metadata(app, path);
    if metadata.sample_rate.is_none() {
        return Err("Could not read regenerated audio metadata".into());
    }
    let tags = crate::local_conversion::read_audio_tags(system::ffprobe_command(app), path)?;
    // Serialize the generated library XML as well as the per-file DB updates.
    let _refresh = REFRESH_LOCK.lock().map_err(|e| e.to_string())?;
    refresh_tracks(&mut conn, item_id, path, &metadata, &tags)?;
    sync_local_library_xml(&conn)?;
    let _ = app.emit("conversion-catalog-updated", json!({ "path": path }));
    Ok(())
}

fn refresh_tracks(
    conn: &mut Connection,
    item_id: &str,
    path: &Path,
    metadata: &LocalTrackMetadata,
    tags: &BTreeMap<String, String>,
) -> Result<(), String> {
    let path_text = path.to_string_lossy();
    let id = format!("local-{item_id}");
    // Resolve an alias only to an existing survivor. Do not redirect a survivor
    // that represents another physical file to this newly regenerated output.
    let alias: Option<String> = conn.query_row(
        "SELECT track_id FROM playlist_track_merge_aliases WHERE library_id = ?1 AND old_track_id = ?2",
        params![LOCAL_CONVERSION_LIBRARY_ID, id], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
    let rows = conn.prepare("SELECT library_id, track_id, source_path, attributes_json FROM playlist_index_tracks WHERE source_path = ?1 OR (library_id = ?2 AND track_id IN (?3, ?4))")
        .map_err(|e| e.to_string())?.query_map(params![path_text, LOCAL_CONVERSION_LIBRARY_ID, id, alias], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?)))
        .map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let name = metadata.title.clone().unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    let location = path_to_rekordbox_location(path).map_err(|e| e.to_string())?;
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    let now = timestamp();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (library, track, old_path, attributes) in rows {
        if alias.as_deref() == Some(&track) && old_path.as_deref() != Some(path_text.as_ref()) {
            continue;
        }
        let mut attributes = parse_track_attributes_json(Some(attributes));
        for (key, value) in [
            ("Genre", &metadata.genre),
            ("Comments", &metadata.comments),
            ("Year", &metadata.year),
        ] {
            if let Some(value) = value {
                attributes.insert(key.into(), value.clone());
            }
        }
        for (field, names) in [
            ("Tonality", &["initial_key", "key", "tkey"][..]),
            ("AverageBpm", &["bpm", "tbpm"][..]),
            ("Label", &["label", "publisher"][..]),
            ("Composer", &["composer"][..]),
        ] {
            if let Some(value) = names.iter().find_map(|key| tags.get(*key)) {
                attributes.insert(field.into(), value.clone());
            }
        }
        let playlists = indexed_playlist_paths_for_track(&tx, &library, &track)?;
        let search_text = std::iter::once(name.as_str())
            .chain(metadata.artist.as_deref())
            .chain(metadata.album.as_deref())
            .chain(attributes.values().map(String::as_str))
            .chain(playlists.iter().map(String::as_str))
            .chain(std::iter::once(path_text.as_ref()))
            .collect::<Vec<_>>()
            .join(" ");
        tx.execute("UPDATE playlist_index_tracks SET name = ?3, artist = ?4, album = ?5, kind = 'AIFF File',
            location = ?6, source_path = ?7, size_bytes = ?8, total_time = ?9, sample_rate = ?10, bitrate = ?11,
            source_exists = 1, search_text = ?12, attributes_json = ?13, updated_at = ?14 WHERE library_id = ?1 AND track_id = ?2",
            params![library, track, name, metadata.artist, metadata.album, location, path_text, size,
                metadata.total_time, metadata.sample_rate, metadata.bitrate, search_text,
                serde_json::to_string(&attributes).map_err(|e| e.to_string())?, now]).map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM playlist_track_embeddings WHERE library_id = ?1 AND track_id = ?2",
            params![library, track],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM playlist_track_fts WHERE library_id = ?1 AND track_id = ?2",
            params![library, track],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("INSERT INTO playlist_track_fts (library_id, track_id, name, artist, album, kind, location, source_path, search_text)
            SELECT library_id, track_id, COALESCE(name, ''), COALESCE(artist, ''), COALESCE(album, ''), kind, location, source_path, search_text
            FROM playlist_index_tracks WHERE library_id = ?1 AND track_id = ?2", params![library, track]).map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE playlist_index_libraries SET updated_at = ?2 WHERE id = ?1",
            params![library, now],
        )
        .map_err(|e| e.to_string())?;
    }
    duplicates::refresh_regenerated_aliases(&tx, path)?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regeneration_catalog_refresh_preserves_ids_ratings_membership_and_custom_attributes() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let now = timestamp();
        let path = std::env::temp_dir().join(format!("rau-refresh-test-{}.aiff", Uuid::new_v4()));
        fs::write(&path, b"fixture").unwrap();
        conn.execute("INSERT INTO playlist_index_libraries (id, source_path, source_name, track_count, playlist_count, indexed_at, updated_at) VALUES (?1, 'test.xml', 'Test', 1, 1, ?2, ?2)", params![LOCAL_CONVERSION_LIBRARY_ID, now]).unwrap();
        conn.execute("INSERT INTO playlist_index_tracks (library_id, track_id, name, source_path, search_text, attributes_json, user_rating, created_at, updated_at) VALUES (?1, 'local-item', 'Old', ?2, 'Old', '{\"Genre\":\"Old genre\",\"CustomTag\":\"Keep\"}', 5, ?3, ?3)", params![LOCAL_CONVERSION_LIBRARY_ID, path.to_string_lossy(), now]).unwrap();
        conn.execute("INSERT INTO playlist_index_playlists (library_id, path, name, node_type, track_count, position, created_at, updated_at) VALUES (?1, 'Favorites', 'Favorites', '1', 1, 0, ?2, ?2)", params![LOCAL_CONVERSION_LIBRARY_ID, now]).unwrap();
        conn.execute("INSERT INTO playlist_index_memberships (library_id, playlist_path, track_id, position) VALUES (?1, 'Favorites', 'local-item', 0)", params![LOCAL_CONVERSION_LIBRARY_ID]).unwrap();
        conn.execute("INSERT INTO playlist_track_embeddings (library_id, track_id, model, dimensions, text_hash, embedding_json, updated_at) VALUES (?1, 'local-item', 'test', 1, 'old', '[1]', ?2)", params![LOCAL_CONVERSION_LIBRARY_ID, now]).unwrap();
        conn.execute("INSERT INTO playlist_track_merge_aliases (library_id, old_track_id, track_id, source_path, signature, size_bytes, checksum, created_at) VALUES (?1, 'local-before-merge', 'local-item', ?2, 'old signature', 1, 'old checksum', ?3)", params![LOCAL_CONVERSION_LIBRARY_ID, path.to_string_lossy(), now]).unwrap();
        let metadata = LocalTrackMetadata {
            title: Some("Updated".into()),
            genre: Some("House".into()),
            sample_rate: Some(44100),
            ..Default::default()
        };
        refresh_tracks(
            &mut conn,
            "before-merge",
            &path,
            &metadata,
            &BTreeMap::from([("initial_key".into(), "Am".into())]),
        )
        .unwrap();
        assert_eq!(
            duplicates::resolve_alias(
                &conn,
                LOCAL_CONVERSION_LIBRARY_ID,
                "local-before-merge",
                Some(&path)
            )
            .unwrap(),
            "local-item"
        );
        // A repeated refresh is idempotent and must not create tracks.
        refresh_tracks(&mut conn, "item", &path, &metadata, &BTreeMap::new()).unwrap();
        let (name, rating, attributes): (String, i64, String) = conn
            .query_row(
                "SELECT name, user_rating, attributes_json FROM playlist_index_tracks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "Updated");
        assert_eq!(rating, 5);
        let attributes: Value = serde_json::from_str(&attributes).unwrap();
        assert_eq!(attributes["CustomTag"], "Keep");
        assert_eq!(attributes["Genre"], "House");
        assert_eq!(attributes["Tonality"], "Am");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM playlist_index_memberships WHERE track_id = 'local-item'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM playlist_track_embeddings", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            0
        );
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM playlist_track_fts WHERE playlist_track_fts MATCH 'Updated Favorites'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        fs::remove_file(path).unwrap();
    }
}
