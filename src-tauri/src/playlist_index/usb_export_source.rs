//! Snapshot the playlists visible in Rau. Existing catalog membership/order and
//! user ratings take precedence. Explicitly saved Rau grids override XML grids;
//! matching XML still supplies existing cues and loops.
use super::*;
use aifficator_core::rekordbox::{PlaylistNode, RekordboxLibrary};
use aifficator_core::usb_export::{
    check_source, plan_library, preserves_source_audio, ExportPlan, UnavailableTrack,
};

type SourceCache = HashMap<String, Result<HashMap<String, Track>, String>>;

pub(crate) fn match_destination(
    app: &AppHandle,
    info: &mut aifficator_core::usb_export::incremental::DestinationInfo,
) -> Result<(), String> {
    let conn = open_db(app)?;
    let targets = list_playlist_targets(&conn)?;
    let mut used = BTreeSet::new();
    for p in &mut info.playlists {
        if p.source_id
            .as_ref()
            .is_some_and(|id| !targets.iter().any(|t| &t.id == id))
        {
            p.source_id = None;
        }
        if let Some(id) = &p.source_id {
            used.insert(id.clone());
        }
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for p in &info.playlists {
        *counts.entry(p.name.clone()).or_default() += 1;
    }
    for p in &mut info.playlists {
        if p.source_id.is_none() && counts[&p.name] == 1 {
            let matches: Vec<_> = targets.iter().filter(|t| t.name == p.name).collect();
            if matches.len() == 1 && used.insert(matches[0].id.clone()) {
                p.source_id = Some(matches[0].id.clone());
            }
        }
    }
    Ok(())
}

pub(crate) struct Snapshot {
    pub plan: Option<ExportPlan>,
    pub playlist_names: Vec<String>,
    pub unavailable_tracks: Vec<UnavailableTrack>,
    pub estimated_audio_bytes: Option<u64>,
}

pub(crate) fn snapshot(
    app: &AppHandle,
    selected: &[String],
    existing: Option<&aifficator_core::usb_export::incremental::Existing>,
) -> Result<Snapshot, String> {
    if selected.is_empty() || selected.len() > 10_000 {
        return Err("Selecciona entre 1 y 10000 playlists.".into());
    }
    let mut conn = open_db(app)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let targets = list_playlist_targets(&tx)?;
    let mut seen = BTreeSet::new();
    let mut tracks = BTreeMap::new();
    let mut playlists = Vec::new();
    let mut names = Vec::new();
    let mut memberships = BTreeMap::<String, BTreeSet<String>>::new();
    for id in selected {
        if !seen.insert(id) {
            return Err("La selección contiene una playlist repetida.".into());
        }
        let target = targets
            .iter()
            .find(|p| &p.id == id)
            .ok_or_else(|| "Una playlist ya no existe. Actualiza la selección.".to_string())?;
        let members = if target.target_kind == "draft" {
            draft_tracks(&tx, id)?
        } else {
            list_playlist_tracks(
                &tx,
                &target.library_id,
                target
                    .playlist_path
                    .as_deref()
                    .ok_or_else(|| "La playlist no tiene una ruta válida.".to_string())?,
            )?
        };
        if members.is_empty() && existing.is_none() {
            return Err(format!("La playlist «{}» está vacía.", target.name));
        }
        let mut keys = Vec::new();
        for track in members {
            let key = serde_json::to_string(&(&track.library_id, &track.track_id))
                .map_err(|e| e.to_string())?;
            keys.push(key.clone());
            memberships
                .entry(key.clone())
                .or_default()
                .insert(target.name.clone());
            tracks.entry(key).or_insert(track);
            if tracks.len() > 10_000 {
                return Err("Esta exportación admite hasta 10000 temas únicos.".into());
            }
        }
        names.push(target.name.clone());
        playlists.push(PlaylistNode {
            name: format!("selection-{}", playlists.len()),
            node_type: Some("1".into()),
            track_keys: keys,
            ..Default::default()
        });
    }
    let mut library = RekordboxLibrary {
        playlists,
        ..Default::default()
    };
    let mut cache = SourceCache::new();
    let mut warnings = BTreeMap::<String, Vec<String>>::new();
    let mut preparations = BTreeMap::new();
    let mut estimated_audio_bytes = Some(0u64);
    let mut unavailable_tracks = Vec::new();
    let mut from_usb = BTreeMap::new();
    for (key, indexed) in tracks {
        let mut file_path = indexed.source_path.as_ref().map(PathBuf::from);
        let title = indexed.name.as_deref().unwrap_or(&key);
        if check_source(file_path.as_deref()).is_err() {
            if let Some((old, record)) = existing.and_then(|e| {
                e.report
                    .tracks
                    .iter()
                    .find(|t| t.track.source_key == key)
                    .map(|t| (e, t))
            }) {
                let path = aifficator_core::usb_export::incremental::device_file(
                    &old.root,
                    &record.track.device_path,
                )
                .map_err(|e| e.to_string())?;
                if check_source(Some(&path)).is_ok() {
                    file_path = Some(path);
                    from_usb.insert(key.clone(), record.track.analysis.clone());
                }
            }
        }
        let size = match check_source(file_path.as_deref()) {
            Ok(size) => size,
            Err(problem) => {
                unavailable_tracks.push(UnavailableTrack {
                    source_key: key.clone(),
                    title: title.into(),
                    artist: indexed.artist.clone().unwrap_or_default(),
                    source_path: file_path,
                    playlist_names: memberships
                        .remove(&key)
                        .unwrap_or_default()
                        .into_iter()
                        .collect(),
                    problem,
                });
                continue;
            }
        };
        let file_path = file_path.expect("available source has a local path");
        let mut attributes = indexed.attributes.clone();
        if let Some(rating) = indexed.user_rating {
            attributes.insert("Rating".into(), (u16::from(rating) * 51).to_string());
        }
        let mut track = Track {
            track_id: key.clone(),
            name: indexed.name.clone(),
            artist: indexed.artist.clone(),
            album: indexed.album.clone(),
            kind: indexed.kind.clone(),
            location: indexed.location.clone(),
            file_path: Some(file_path.clone()),
            size: indexed.size,
            total_time: indexed.total_time,
            sample_rate: indexed.sample_rate,
            bitrate: indexed.bitrate,
            attributes,
            ..Default::default()
        };
        // Copied formats use actual bytes. Only converted AIFF uses an advisory
        // estimate from catalog duration (headers/tags are not a size guarantee).
        let estimated_track = if preserves_source_audio(&file_path) {
            Some(size)
        } else {
            indexed
                .total_time
                .filter(|seconds| *seconds > 0)
                .and_then(|seconds| {
                    seconds
                        .checked_add(1)?
                        .checked_mul(44_100 * 4)?
                        .checked_add(4096)
                })
        };
        estimated_audio_bytes = estimated_audio_bytes
            .zip(estimated_track)
            .and_then(|(total, bytes)| total.checked_add(bytes));
        let (tempos, marks, mut notices) = if from_usb.contains_key(&key) {
            (
                Vec::new(),
                Vec::new(),
                vec!["Se reutiliza la pista del USB porque el original no está disponible.".into()],
            )
        } else {
            source_analysis(&tx, &indexed, &file_path, &mut cache)?
        };
        track.tempos = tempos;
        track.position_marks = marks;
        if let Some(prepared) = if from_usb.contains_key(&key) {
            None
        } else {
            music_analysis::prepared_for_export(
                &tx,
                &indexed.library_id,
                &indexed.track_id,
                &file_path,
            )
            .map_err(|e| format!("«{title}»: {e}"))?
        } {
            if let Some(grid) = &prepared.grid {
                let (seconds, beat) = grid.first_anchor();
                track.tempos = vec![BTreeMap::from([
                    ("Inizio".into(), format!("{seconds:.9}")),
                    ("Bpm".into(), grid.bpm.to_string()),
                    ("Battito".into(), beat.to_string()),
                    ("Metro".into(), "4/4".into()),
                ])];
                track
                    .attributes
                    .insert("AverageBpm".into(), format!("{:.2}", grid.bpm));
                notices.clear();
                notices.push("Grilla de tempo constante en 4/4 preparada y guardada en Rau; los cues existentes se conservan desde el XML cuando están disponibles.".into());
            }
            if let Some(key) = &prepared.key {
                track.attributes.insert("Tonality".into(), key.clone());
            }
            preparations.insert(key.clone(), prepared.source_sha256());
        }
        warnings.insert(key, notices);
        library.tracks.push(track);
    }
    tx.commit().map_err(|e| e.to_string())?;
    let available: BTreeSet<_> = library
        .tracks
        .iter()
        .map(|track| track.track_id.clone())
        .collect();
    for playlist in &mut library.playlists {
        playlist.track_keys.retain(|key| available.contains(key));
    }
    if library.tracks.is_empty() {
        let playlists = names
            .iter()
            .enumerate()
            .map(
                |(index, name)| aifficator_core::usb_export::ExportPlaylist {
                    id: index as u32 + 1,
                    parent_id: 0,
                    position: index as u32,
                    name: name.clone(),
                    is_folder: false,
                    tracks: Vec::new(),
                },
            )
            .collect();
        let plan = ExportPlan {
            tracks: Vec::new(),
            playlists,
            source_bytes: 0,
            omitted_tracks: unavailable_tracks.clone(),
            playlist_sources: selected
                .iter()
                .enumerate()
                .map(|(index, id)| (index as u32 + 1, id.clone()))
                .collect(),
        };
        return Ok(Snapshot {
            plan: Some(plan),
            playlist_names: names,
            unavailable_tracks,
            estimated_audio_bytes,
        });
    }
    let selected_paths: Vec<_> = library.playlists.iter().map(|p| p.name.clone()).collect();
    let mut plan = plan_library(&library, &selected_paths).map_err(|e| e.to_string())?;
    for (playlist, name) in plan.playlists.iter_mut().zip(&names) {
        playlist.name = name.clone();
    }
    plan.omitted_tracks = unavailable_tracks.clone();
    plan.playlist_sources = plan
        .playlists
        .iter()
        .zip(selected)
        .map(|(p, source)| (p.id, source.clone()))
        .collect();
    for track in &mut plan.tracks {
        track.analysis.source_sha256 = preparations.remove(&track.source_key);
        if let Some(analysis) = from_usb.remove(&track.source_key) {
            track.analysis = analysis;
        }
        if let Some(notices) = warnings.remove(&track.source_key) {
            track.analysis.warnings.extend(notices);
        }
    }
    aifficator_core::usb_export::devicesql::validate_plan(&plan).map_err(|e| e.to_string())?;
    Ok(Snapshot {
        plan: Some(plan),
        playlist_names: names,
        unavailable_tracks,
        estimated_audio_bytes,
    })
}

type XmlAttributes = Vec<BTreeMap<String, String>>;

fn source_analysis(
    conn: &Connection,
    track: &PlaylistIndexTrack,
    file_path: &Path,
    cache: &mut SourceCache,
) -> Result<(XmlAttributes, XmlAttributes, Vec<String>), String> {
    let mut stmt = conn.prepare(
        "SELECT st.source_path, st.source_track_id
         FROM playlist_index_source_tracks st
         JOIN playlist_index_sources s ON s.library_id = st.library_id AND s.source_path = st.source_path
         JOIN playlist_index_tracks t ON t.library_id = st.library_id AND t.track_id = st.track_id
         WHERE (st.library_id = ?1 AND st.track_id = ?2) OR t.source_path = ?3
         ORDER BY s.updated_at DESC, st.source_path, st.source_track_id"
    ).map_err(|e| e.to_string())?;
    let mut origins = stmt
        .query_map(
            params![track.library_id, track.track_id, track.source_path],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if origins.is_empty() {
        if let Some(library) = get_library(conn, &track.library_id)? {
            if !library.source_path.contains("://") && library.id != LOCAL_CONVERSION_LIBRARY_ID {
                origins.push((library.source_path, track.track_id.clone()));
            }
        }
    }
    let has_origin = !origins.is_empty();
    let expected = file_path
        .canonicalize()
        .unwrap_or_else(|_| file_path.to_owned());
    let mut notices = Vec::new();
    for (xml, id) in origins {
        let source = cache.entry(xml.clone()).or_insert_with(|| {
            parse_rekordbox_xml_file(&xml)
                .map(|library| {
                    library
                        .tracks
                        .into_iter()
                        .map(|t| (t.track_id.clone(), t))
                        .collect()
                })
                .map_err(|e| e.to_string())
        });
        if let Ok(source) = source {
            if let Some(original) = source.get(&id) {
                let original_path = original
                    .file_path
                    .as_ref()
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.clone()));
                if original_path.as_deref() == Some(expected.as_path()) {
                    return Ok((
                        original.tempos.clone(),
                        original.position_marks.clone(),
                        Vec::new(),
                    ));
                }
            }
        }
        if notices.is_empty() {
            notices.push("No se pudo recuperar el análisis del XML de origen para este archivo; se exportará sin esos cues ni grilla.".into());
        }
    }
    if !has_origin {
        notices.push(
            "Este tema no tiene un XML de origen con cues y grilla disponibles en Rau.".into(),
        );
    }
    Ok((Vec::new(), Vec::new(), notices))
}
