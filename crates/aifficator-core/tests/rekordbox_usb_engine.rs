#![cfg(unix)]
use aifficator_core::usb_export::{
    self,
    analysis::{CuePoint, TempoAnchor, TrackAnalysis},
    incremental, onelibrary, AudioFormat, ExportPlan, ExportPlaylist, ExportTrack,
};
use rekordbox_usb::{
    anlz::AnalysisFile,
    pdb::{Kind, Pdb},
    profile,
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn aiff(path: &Path) {
    let frames = 44_100u32;
    let mut data = b"FORM".to_vec();
    data.extend((46 + frames * 4).to_be_bytes());
    data.extend(b"AIFFCOMM");
    data.extend(18u32.to_be_bytes());
    data.extend(2u16.to_be_bytes());
    data.extend(frames.to_be_bytes());
    data.extend(16u16.to_be_bytes());
    data.extend([0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
    data.extend(b"SSND");
    data.extend((8 + frames * 4).to_be_bytes());
    data.extend([0; 8]);
    for i in 0..frames {
        let sample =
            ((f64::from(i) * std::f64::consts::TAU * 440.0 / 44100.0).sin() * 9000.0) as i16;
        data.extend(sample.to_be_bytes());
        data.extend(sample.to_be_bytes());
    }
    fs::write(path, data).unwrap();
}

fn plan(source: &Path) -> ExportPlan {
    ExportPlan {
        tracks: vec![ExportTrack {
            id: 1,
            source_key: "catalog:1".into(),
            source_path: source.into(),
            device_path: "/Contents/Artista/Original.aiff".into(),
            copy_source: true,
            title: "Canción 日本語 𝄞".into(),
            artist: "Artista".into(),
            album: "Álbum".into(),
            genre: "House".into(),
            label: "Sello".into(),
            key: "Am".into(),
            comment: "Prueba".into(),
            bpm_x100: 12000,
            rating: 4,
            year: 2026,
            track_number: 1,
            analysis: TrackAnalysis {
                tempos: vec![TempoAnchor {
                    seconds: 0.0,
                    bpm: 120.0,
                    beat_in_bar: 1,
                }],
                cues: vec![
                    CuePoint {
                        hot_cue: 0,
                        start_ms: 100,
                        end_ms: None,
                        name: "Memoria".into(),
                        rgb: None,
                    },
                    CuePoint {
                        hot_cue: 1,
                        start_ms: 200,
                        end_ms: None,
                        name: "Entrada 𝄞".into(),
                        rgb: Some([255, 30, 80]),
                    },
                    CuePoint {
                        hot_cue: 4,
                        start_ms: 300,
                        end_ms: Some(600),
                        name: "Loop D".into(),
                        rgb: None,
                    },
                ],
                ..Default::default()
            },
            analysis_path: None,
        }],
        playlists: vec![
            ExportPlaylist {
                id: 1,
                parent_id: 0,
                position: 0,
                name: "Carpeta".into(),
                is_folder: true,
                tracks: vec![],
            },
            ExportPlaylist {
                id: 2,
                parent_id: 1,
                position: 0,
                name: "Set ñ".into(),
                is_folder: false,
                tracks: vec![1, 1],
            },
        ],
        source_bytes: fs::metadata(source).unwrap().len(),
        omitted_tracks: vec![],
        playlist_sources: BTreeMap::from([(1, "folder:1".into()), (2, "playlist:1".into())]),
    }
}
fn file(root: &Path, relative: &str) -> PathBuf {
    root.join(relative.trim_start_matches('/'))
}

#[test]
fn new_engine_exports_native_analysis_both_catalogs_and_reuses_usb_and_cache() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("Original.aiff");
    aiff(&source);
    let plan = plan(&source);
    let usb = temp.path().join("usb");
    fs::create_dir(&usb).unwrap();
    let cache = temp.path().join("cache");
    // PCM copying/analysis needs neither native Rekordbox libraries nor FFmpeg.
    let absent = Path::new("/not-installed");
    let report =
        usb_export::write_destination_cached(&plan, &usb, absent, absent, 2, Some(&cache), |_| {})
            .unwrap();
    assert_eq!(report.schema_version, 8);
    let track = &report.tracks[0];
    assert_eq!(track.analysis.renderer, "rekordbox-usb-0.1.0-v1");
    assert_eq!(track.audio.format, AudioFormat::Aiff);
    assert_eq!(
        fs::read(file(&usb, &track.track.device_path)).unwrap(),
        fs::read(&source).unwrap()
    );
    let db = Pdb::parse(
        &fs::read(usb.join("PIONEER/rekordbox/export.pdb")).unwrap(),
        Kind::Export,
    )
    .unwrap();
    db.validate_catalog().unwrap();
    db.validate_indexes().unwrap();
    profile::validate_track_count(&db).unwrap();
    let ext =
        AnalysisFile::parse(&fs::read(file(&usb, &track.analysis.ext_path)).unwrap()).unwrap();
    let dat =
        AnalysisFile::parse(&fs::read(file(&usb, &track.analysis.dat_path)).unwrap()).unwrap();
    for id in [*b"PWV3", *b"PWV4", *b"PWV5"] {
        assert_eq!(ext.find(id).count(), 1);
    }
    // The native partition keeps hot D in EXT; DAT only contains hot A.
    let hot_dat = dat
        .find(*b"PCOB")
        .find(|g| g.cue_group_kind().unwrap() == 1)
        .unwrap();
    assert_eq!(hot_dat.cue_entries().unwrap().len(), 1);
    let hot_ext = ext
        .find(*b"PCOB")
        .find(|g| g.cue_group_kind().unwrap() == 1)
        .unwrap();
    assert_eq!(hot_ext.cue_entries().unwrap().len(), 1);
    let plus = onelibrary::inspect(&usb.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(plus.cues[&1].len(), 3);
    assert_eq!(plus.entries.len(), 2);
    let first_hash = track.dat_sha256.clone();
    let mut incoming = plan.clone();
    incoming.playlists.retain(|p| !p.is_folder);
    let update = incremental::prepare(
        incremental::load_existing(&usb).unwrap(),
        incoming,
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(update.counts.analysis_tracks, 0);
    assert_eq!(update.work[&1], incremental::Work::Reuse);
    let updated = incremental::write(&update, absent, absent, 2, Some(&cache), |_| {}).unwrap();
    assert_eq!(updated.tracks[0].dat_sha256, first_hash);
    assert_eq!(updated.playlists, report.playlists);
    assert_eq!(
        updated.tracks[0].analysis.plus_cues,
        track.analysis.plus_cues
    );
    let second = temp.path().join("usb2");
    fs::create_dir(&second).unwrap();
    let mut cache_hit = false;
    usb_export::write_destination_cached(&plan, &second, absent, absent, 1, Some(&cache), |p| {
        cache_hit |= p.action.as_deref() == Some("cache_hit");
    })
    .unwrap();
    assert!(cache_hit);
}

#[test]
fn legacy_analysis_is_migrated_at_new_paths_without_recopying_audio() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("Original.aiff");
    aiff(&source);
    let mut model = plan(&source);
    model.tracks[0].analysis.cues.clear();
    let usb = temp.path().join("usb");
    fs::create_dir(&usb).unwrap();
    let absent = Path::new("/not-installed");
    let mut report =
        usb_export::write_destination_cached(&model, &usb, absent, absent, 1, None, |_| {})
            .unwrap();
    let original_analysis = report.tracks[0].analysis.dat_path.clone();
    report.schema_version = 7;
    report.tracks[0].analysis.renderer.clear();
    // Earlier decoders could record a different sample count. Migration must
    // publish metadata from the new decode together with its new analysis.
    report.tracks[0].audio.frames -= 100;
    fs::write(
        usb.join("rau-export-report.json"),
        serde_json::to_vec(&report).unwrap(),
    )
    .unwrap();
    let mut incoming = model;
    incoming.playlists.retain(|p| !p.is_folder);
    let update = incremental::prepare(
        incremental::load_existing(&usb).unwrap(),
        incoming,
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(update.work[&1], incremental::Work::Analyze);
    assert_ne!(
        usb_export::anlz::path_for_track(&update.plan.tracks[0]).unwrap(),
        original_analysis
    );
    let result = incremental::write(&update, absent, absent, 1, None, |_| {}).unwrap();
    assert_eq!(result.tracks[0].sha256, report.tracks[0].sha256);
    assert_eq!(result.tracks[0].audio.frames, 44_100);
    assert!(file(&usb, &original_analysis).exists());
    assert_eq!(result.tracks[0].analysis.renderer, "rekordbox-usb-0.1.0-v1");
}

#[test]
#[ignore = "requires ffmpeg on PATH (or RAU_FFMPEG)"]
fn mp3_cbr_and_vbr_keep_bytes_and_native_seek_with_conversion_and_mixed_ids() {
    use rekordbox_usb::{mpeg::MpegFrames, seek::VbrTable};
    use std::process::Command;
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("Original.aiff");
    aiff(&source);
    let ffmpeg = std::env::var_os("RAU_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| "ffmpeg".into());
    let mut model = plan(&source);
    for (i, args) in [(2, vec!["-b:a", "192k"]), (3, vec!["-q:a", "2"])] {
        let mp3 = temp.path().join(format!("{i}.mp3"));
        assert!(Command::new(&ffmpeg)
            .args(["-nostdin", "-loglevel", "error", "-i"])
            .arg(&source)
            .args(["-c:a", "libmp3lame"])
            .args(args)
            .arg(&mp3)
            .status()
            .unwrap()
            .success());
        let mut track = model.tracks[0].clone();
        track.id = i;
        track.source_key = format!("catalog:{i}");
        track.source_path = mp3;
        track.device_path = format!("/Contents/Artista/{i}.mp3");
        model.tracks.push(track);
        model.playlists[1].tracks.push(i);
    }
    // A WAV source follows Rau's conversion policy before native analysis.
    let wav = temp.path().join("convert.wav");
    assert!(Command::new(&ffmpeg)
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(&source)
        .args(["-c:a", "pcm_s24le", "-ar", "48000"])
        .arg(&wav)
        .status()
        .unwrap()
        .success());
    let mut converted = model.tracks[0].clone();
    converted.id = 4;
    converted.source_key = "catalog:4".into();
    converted.source_path = wav;
    converted.copy_source = false;
    converted.device_path = "/Contents/Artista/4.aiff".into();
    model.tracks.push(converted);
    model.playlists[1].tracks.push(4);
    let usb = temp.path().join("usb");
    fs::create_dir(&usb).unwrap();
    let report = usb_export::write_destination_cached(
        &model,
        &usb,
        &ffmpeg,
        Path::new("/not-installed"),
        3,
        None,
        |_| {},
    )
    .unwrap();
    let plus = onelibrary::inspect(&usb.join("PIONEER/rekordbox/exportLibrary.db")).unwrap();
    assert_eq!(plus.table_rows["cue"], 12);
    for item in &report.tracks[1..3] {
        let original = fs::read(&item.track.source_path).unwrap();
        assert_eq!(
            fs::read(file(&usb, &item.track.device_path)).unwrap(),
            original
        );
        let frames = MpegFrames::parse(&original).unwrap();
        let dat =
            AnalysisFile::parse(&fs::read(file(&usb, &item.analysis.dat_path)).unwrap()).unwrap();
        assert_eq!(
            VbrTable::from_tag(dat.find(*b"PVBR").next().unwrap()).unwrap(),
            frames.vbr_table().unwrap()
        );
        assert_eq!(item.audio.sample_rate, frames.sample_rate);
        let cue = &plus.cues[&item.track.id][0];
        assert_eq!(
            cue.values[12],
            onelibrary::CueValue::Integer(frames.cue_seek(300).unwrap().byte_offset.into())
        );
    }
    let converted = &report.tracks[3].audio;
    assert_eq!(
        (
            converted.format,
            converted.sample_rate,
            converted.bit_depth,
            converted.channels
        ),
        (AudioFormat::Aiff, 44100, 16, 2)
    );
}

#[test]
fn inconsistent_external_cue_edits_are_refused_without_modifying_the_usb() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("Original.aiff");
    aiff(&source);
    let model = plan(&source);
    let usb = temp.path().join("usb");
    fs::create_dir(&usb).unwrap();
    let absent = Path::new("/not-installed");
    let report =
        usb_export::write_destination_cached(&model, &usb, absent, absent, 1, None, |_| {})
            .unwrap();
    let path = file(&usb, &report.tracks[0].analysis.ext_path);
    let mut ext = AnalysisFile::parse(&fs::read(&path).unwrap()).unwrap();
    let hot = ext
        .tags
        .iter_mut()
        .find(|g| g.id == *b"PCO2" && g.cue_group_kind().unwrap() == 1)
        .unwrap();
    let mut entries = hot.cue_entries().unwrap();
    let mut cue = rekordbox_usb::anlz::ExtendedCue::from_tag(entries[0].clone()).unwrap();
    cue.set_position(2, 350, 600).unwrap();
    entries[0] = cue.into_tag();
    hot.set_cue_entries(&entries).unwrap();
    fs::write(&path, ext.to_bytes().unwrap()).unwrap();
    let old = incremental::load_existing(&usb).unwrap();
    let error = match incremental::prepare(old, model, BTreeMap::new()) {
        Ok(_) => panic!("inconsistent edit was accepted"),
        Err(e) => e,
    };
    assert!(error.to_string().contains("cues"));
    assert_eq!(
        usb_export::hash_file(&usb.join("PIONEER/rekordbox/export.pdb")).unwrap(),
        report.database_sha256
    );
}
