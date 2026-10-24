use aifficator_core::rekordbox::{PlaylistNode, RekordboxLibrary, Track};
use aifficator_core::usb_export::{
    analysis::{CuePoint, TempoAnchor},
    anlz, devicesql, onelibrary, plan_library, read_pcm_wave, write_classic_folder, AudioFormat,
    AudioInfo, ExportPlan, ExportPlaylist, ExportTrack, LibraryIdentity,
};
use std::fs;
use std::path::{Path, PathBuf};

fn plan(count: u32) -> ExportPlan {
    ExportPlan {
        source_bytes: 0,
        tracks: (1..=count)
            .map(|id| ExportTrack {
                id,
                source_key: id.to_string(),
                source_path: PathBuf::from("/no-source"),
                device_path: format!("/Contents/Rau/Prototype/{id:08}.wav"),
                copy_source: false,
                title: format!("Canción 日本語 🎵 {id}"),
                artist: "Artista".into(),
                album: "Álbum".into(),
                genre: "House".into(),
                label: "Label".into(),
                key: "Am".into(),
                comment: "x".repeat(200),
                bpm_x100: 12800,
                rating: 5,
                year: 2026,
                track_number: id,
                analysis: Default::default(),
            })
            .collect(),
        playlists: vec![
            ExportPlaylist {
                id: 1,
                parent_id: 0,
                position: 0,
                name: "Folder".into(),
                is_folder: true,
                tracks: vec![],
            },
            ExportPlaylist {
                id: 2,
                parent_id: 1,
                position: 0,
                name: "Set ñ".into(),
                is_folder: false,
                tracks: (1..=count).rev().chain([1]).collect(),
            },
        ],
    }
}
fn audio(count: usize) -> Vec<AudioInfo> {
    vec![
        AudioInfo {
            format: AudioFormat::Wave,
            bitrate_kbps: 1411,
            sample_rate: 44100,
            channels: 2,
            bit_depth: 16,
            frames: 441000,
            size_bytes: 1764044
        };
        count
    ]
}
fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

#[test]
fn unicode_long_strings_repeated_entries_and_multiple_pages() {
    let model = plan(300);
    let media = audio(300);
    let bytes = devicesql::encode(&model, &media, "2026-10-23").unwrap();
    let db = devicesql::inspect(&bytes).unwrap();
    devicesql::compare(&model, &media, &db).unwrap();
    assert_eq!(db.tracks[0].title, "Canción 日本語 🎵 1");
    assert_eq!(db.entries.len(), 301);
    assert_eq!(db.entries[0], [1, 300, 2]);
    assert_eq!(db.entries[300], [301, 1, 2]);
    assert_eq!(db.table_rows.len(), 20);
    assert!(db.page_count > 41);
}

#[test]
fn protocol_page_and_track_fields_have_documented_layout() {
    let bytes = devicesql::encode(&plan(20), &audio(20), "2026-10-23").unwrap();
    assert_eq!(u32_at(&bytes, 4), 4096);
    assert_eq!(u32_at(&bytes, 8), 20);
    assert_eq!(bytes[4096 + 27], 0x64); // First table begins with an index page.
    let data_index = u32_at(&bytes, 4096 + 12) as usize;
    let page = &bytes[data_index * 4096..(data_index + 1) * 4096];
    let count = u32_at(page, 24) & 0x1fff;
    assert!(count > 1);
    assert_eq!(page[27], 0x24);
    assert_eq!(u16_at(page, 34), 0); // First row touched, not row count.
    for slot in 0..count as usize {
        let base = 4096 - (slot / 16) * 36;
        let at = 40 + u16_at(page, base - 6 - (slot % 16) * 2) as usize;
        assert_eq!(u16_at(page, at), 0x24);
        assert_eq!(u16_at(page, at + 2), (slot * 32) as u16);
        assert_eq!(u32_at(page, at + 4), 0x000c_0700);
        assert_eq!(u16_at(page, at + 86), 0x29);
        assert_eq!(page[at + 89], 5);
        assert_eq!(u16_at(page, at + 90), 11);
        assert_eq!(u16_at(page, at + 92), 3);
        for string in 0..21 {
            let offset = u16_at(page, at + 94 + string * 2) as usize;
            assert!(offset >= 136);
            if page[at + offset] & 1 == 0 {
                assert_eq!(offset % 4, 0);
            }
        }
    }
}

#[test]
fn analysis_content_link_survives_read_back_and_is_compared() {
    let model = plan(2);
    let media = audio(2);
    let bytes = devicesql::encode(&model, &media, "2026-10-24").unwrap();
    let mut db = devicesql::inspect(&bytes).unwrap();
    assert!(db.tracks.iter().all(|t| t.content_link == 0x000c_0700));
    devicesql::compare(&model, &media, &db).unwrap();
    db.tracks[0].content_link = 0;
    assert!(devicesql::compare(&model, &media, &db).is_err());
}

#[test]
fn dual_libraries_preserve_unicode_ratings_folders_repeats_and_audio_formats() {
    let mut model = plan(6);
    let mut media = audio(6);
    for (i, track) in model.tracks.iter_mut().enumerate() {
        track.rating = i as u8;
    }
    model.tracks[0].device_path = "/Contents/Artista/Original.mp3".into();
    model.tracks[0].copy_source = true;
    media[0].format = AudioFormat::Mp3;
    media[0].bit_depth = 0;
    media[0].bitrate_kbps = 320;
    model.tracks[1].device_path = "/Contents/Artista/Original.aiff".into();
    media[1].format = AudioFormat::Aiff;
    model.tracks[2].artist.clear();
    model.tracks[2].album.clear();
    model.tracks[3].analysis.cues.push(CuePoint {
        hot_cue: 1,
        start_ms: 0,
        end_ms: None,
        name: "Inicio".into(),
        rgb: None,
    });
    let identity = LibraryIdentity {
        name: "Rau Prueba ñ".into(),
        master_database_id: 0xf1234567,
    };
    let classic = devicesql::inspect(
        &devicesql::encode_with_identity(&model, &media, "2026-10-24", &identity).unwrap(),
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("exportLibrary.db");
    onelibrary::write(&path, &model, &media, "2026-10-24", &identity).unwrap();
    let mut plus = onelibrary::inspect(&path).unwrap();
    onelibrary::compare(&model, &media, &identity, &plus).unwrap();
    onelibrary::compare_classic(&classic, &plus).unwrap();
    assert_eq!(plus.table_rows.len(), 22);
    assert_eq!(
        plus.tracks.iter().map(|t| t.rating).collect::<Vec<_>>(),
        [0, 1, 2, 3, 4, 5]
    );
    assert_eq!(plus.tracks[0].duration_seconds, 10); // Native field is seconds, despite old public prose saying milliseconds.
    assert!(plus.tracks[3].hot_cue_auto_load);
    assert_eq!(plus.entries.iter().filter(|e| e[1] == 1).count(), 2);
    assert_eq!(plus.tracks[2].artist, "");
    assert!(plus.playlists[0].is_folder);
    assert_eq!(plus.playlists[1].tracks, model.playlists[1].tracks);
    assert_eq!(plus.tracks[0].format_code, 1);
    assert_eq!(plus.tracks[1].format_code, 12);
    assert!(!temp.path().join("exportLibrary.db-wal").exists());
    assert!(!temp.path().join("exportLibrary.db-journal").exists());
    let cipher_bytes = fs::read(&path).unwrap();
    assert_ne!(&cipher_bytes[..16], b"SQLite format 3\0");
    let plain =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    assert!(plain
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
            .get::<_, u32>(0))
        .is_err());
    plus.tracks[0].master_content_id += 1;
    assert!(onelibrary::compare_classic(&classic, &plus).is_err());
    assert!(onelibrary::compare(&model, &media, &identity, &plus).is_err());
}

#[test]
fn onelibrary_refuses_existing_files_and_detects_damaged_ciphertext() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("exportLibrary.db");
    let model = plan(2);
    let media = audio(2);
    let identity = LibraryIdentity::new();
    fs::write(&path, b"existing export").unwrap();
    assert!(onelibrary::write(&path, &model, &media, "2026-10-24", &identity).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing export");
    let path = temp.path().join("new.db");
    onelibrary::write(&path, &model, &media, "2026-10-24", &identity).unwrap();
    let mut damaged = fs::read(&path).unwrap();
    damaged[0] ^= 0xff;
    fs::write(&path, damaged).unwrap();
    assert!(onelibrary::inspect(&path).is_err());
}

#[test]
fn index_capacity_does_not_overwrite_reserved_bytes() {
    let good = devicesql::encode(&plan(2), &audio(2), "2026-10-23").unwrap();
    for table in 0..20 {
        let index = u32_at(&good, 28 + table * 16 + 8) as usize;
        let page = &good[index * 4096..(index + 1) * 4096];
        assert_eq!(u16_at(page, 36), 1004);
        for offset in (60..4076).step_by(4) {
            assert_eq!(u32_at(page, offset), 0x1fff_fff8);
        }
        assert_eq!(&page[4076..], &[0; 20]);
        if [9, 10, 11, 12, 13, 14, 15].contains(&table) {
            assert_eq!(u32_at(page, 44), 0x03ff_ffff);
            let pointer = 28 + table * 16;
            assert_eq!(u32_at(&good, pointer + 12) as usize, index);
            let candidate = u32_at(&good, pointer + 4) as usize;
            assert_eq!(u32_at(page, 12) as usize, candidate);
            assert_eq!(&good[candidate * 4096..(candidate + 1) * 4096], &[0; 4096]);
        }
    }
    // Reproduce the old writer's five extra sentinels; its semantic track
    // round-trip previously missed this malformed reserved area.
    let mut bad = good.clone();
    bad[4096 + 4076..4096 + 4080].copy_from_slice(&0x1fff_fff8u32.to_le_bytes());
    assert!(devicesql::inspect(&bad)
        .unwrap_err()
        .to_string()
        .contains("reserved trailer"));
}

#[test]
fn browse_records_are_encoded_from_fields_and_checked_on_readback() {
    let model = plan(2);
    let media = audio(2);
    let good = devicesql::encode(&model, &media, "2026-10-23").unwrap();
    let mut db = devicesql::inspect(&good).unwrap();
    assert_eq!(db.table_rows[&16], 27);
    assert_eq!(db.table_rows[&17], 22);
    assert_eq!(db.table_rows[&18], 17);
    let playlist = db.browse_columns.iter().find(|c| c.id == 17).unwrap();
    assert_eq!(playlist.selector, 132);
    assert_eq!(playlist.name, "\u{fffa}PLAYLIST\u{fffb}");
    let mut visible = db
        .browse_categories
        .iter()
        .filter(|c| c.flags == 0)
        .collect::<Vec<_>>();
    visible.sort_by_key(|c| c.position);
    assert_eq!(
        visible.iter().map(|c| c.column_id).collect::<Vec<_>>(),
        [17, 4, 2, 3, 1, 11, 24, 20]
    );
    assert_eq!(
        visible.iter().map(|c| c.position).collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
    devicesql::compare(&model, &media, &db).unwrap();
    db.browse_categories.clear();
    assert!(devicesql::compare(&model, &media, &db).is_err());

    // Read the wire independently of the production browse decoder.
    let category_index = u32_at(&good, 28 + 17 * 16 + 8) as usize;
    let data = u32_at(&good, category_index * 4096 + 12) as usize * 4096;
    let row = data + 40 + usize::from(u16_at(&good, data + 4090));
    assert_eq!(u16_at(&good, row), 1); // GENRE column
    assert_eq!(u16_at(&good, row + 2), 1); // category ID
    assert_eq!(good[row + 4], 99);
    assert_eq!(good[row + 5], 0); // visible
    assert_eq!(u16_at(&good, row + 6), 4); // Rau's order, not reference preferences
    let mut bad = good;
    bad[row..row + 2].copy_from_slice(&999u16.to_le_bytes());
    assert!(devicesql::inspect(&bad).is_err());
}

#[test]
fn inspector_rejects_broken_chains_offsets_and_truncation_without_panics() {
    let good = devicesql::encode(&plan(2), &audio(2), "2026-10-23").unwrap();
    let mut cycle = good.clone();
    cycle[4096 + 12..4096 + 16].copy_from_slice(&1u32.to_le_bytes());
    assert!(devicesql::inspect(&cycle).is_err());
    let mut bad = good.clone();
    bad[8192 + 4090..8192 + 4092].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(devicesql::inspect(&bad).is_err());
    for end in [0, 1, 4095, 4096, good.len() - 1] {
        assert!(devicesql::inspect(&good[..end]).is_err());
    }
    // A damaged row index must not underflow slice bounds.
    let mut overflow = good;
    overflow[8192 + 24..8192 + 27].fill(255);
    assert!(devicesql::inspect(&overflow).is_err());
}

#[test]
fn rejects_paths_cycles_missing_references_and_oversized_rows() {
    let mut p = plan(1);
    p.tracks[0].device_path = "/../../escape.wav".into();
    assert!(devicesql::encode(&p, &audio(1), "2026-10-23").is_err());
    let mut p = plan(1);
    p.playlists[0].parent_id = 1;
    assert!(devicesql::validate_plan(&p).is_err());
    let mut p = plan(1);
    p.playlists[1].tracks.push(999);
    assert!(devicesql::validate_plan(&p).is_err());
    let mut p = plan(1);
    p.tracks[0].title = "ñ".repeat(1000);
    p.tracks[0].comment = "ñ".repeat(1000);
    assert!(devicesql::encode(&p, &audio(1), "2026-10-23").is_err());
}

#[test]
fn selection_preserves_repeats_and_rejects_unknown_playlists_and_files() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("source.wav");
    fs::write(&file, b"placeholder").unwrap();
    let library = RekordboxLibrary {
        tracks: vec![Track {
            track_id: "42".into(),
            file_path: Some(file),
            ..Track::default()
        }],
        playlists: vec![PlaylistNode {
            name: "ROOT".into(),
            node_type: Some("0".into()),
            children: vec![PlaylistNode {
                name: "Set".into(),
                node_type: Some("1".into()),
                track_keys: vec!["42".into(), "42".into()],
                ..PlaylistNode::default()
            }],
            ..PlaylistNode::default()
        }],
        ..RekordboxLibrary::default()
    };
    let p = plan_library(&library, &["ROOT/Set".into()]).unwrap();
    assert_eq!(p.tracks.len(), 1);
    assert_eq!(p.playlists.len(), 1);
    assert_eq!(p.playlists[0].tracks, vec![1, 1]);
    assert_eq!(p.playlists[0].parent_id, 0);
    assert!(plan_library(&library, &[]).is_err());
    assert!(plan_library(&library, &["typo".into()]).is_err());
    fs::remove_file(&p.tracks[0].source_path).unwrap();
    assert!(plan_library(&library, &["ROOT".into()]).is_err());
}

#[test]
fn existing_destination_is_untouched_and_failed_preparation_is_marked() {
    let temp = tempfile::tempdir().unwrap();
    let sentinel = temp.path().join("keep.txt");
    fs::write(&sentinel, b"keep me").unwrap();
    assert!(write_classic_folder(&plan(1), temp.path(), Path::new("/no-ffmpeg")).is_err());
    assert_eq!(fs::read(&sentinel).unwrap(), b"keep me");
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    let output = temp.path().join("new");
    let mut p = plan(1);
    p.tracks[0].source_path = sentinel.clone();
    assert!(write_classic_folder(&p, &output, Path::new("/no-ffmpeg")).is_err());
    assert_eq!(fs::read(sentinel).unwrap(), b"keep me");
    assert!(output.join("RAU_EXPORT_INCOMPLETE.txt").is_file());
    assert!(!output.join("PIONEER/rekordbox/export.pdb").exists());
}

#[test]
fn public_encoder_rejects_impossible_audio_parameters() {
    let mut media = audio(1);
    media[0].sample_rate = u32::MAX;
    assert!(devicesql::encode(&plan(1), &media, "2026-10-23").is_err());
    media[0] = audio(1).remove(0);
    media[0].frames = u64::MAX;
    assert!(devicesql::encode(&plan(1), &media, "2026-10-23").is_err());
}

#[test]
fn wave_validation_uses_chunks_not_filename_or_xml_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("audio.wav");
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    for v in [1u16, 2] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    for v in [44100u32, 176400] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    for v in [4u16, 16] {
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&[0; 4]);
    fs::write(&path, &wav).unwrap();
    assert_eq!(read_pcm_wave(&path).unwrap().frames, 1);
    wav[20..22].copy_from_slice(&3u16.to_le_bytes()); // IEEE float is not integer PCM.
    fs::write(&path, &wav).unwrap();
    assert!(read_pcm_wave(&path).is_err());
    wav.truncate(46);
    fs::write(&path, &wav).unwrap();
    assert!(read_pcm_wave(&path).is_err());
}

#[test]
fn pcm_analysis_includes_seek_record_and_repair_preserves_existing_payloads() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("decoded.wav");
    let pcm: Vec<u8> = (0..44100)
        .flat_map(|frame| {
            let value: i16 = if frame % 100 < 50 { 4000 } else { -4000 };
            value.to_le_bytes()
        })
        .collect();
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(pcm.len() as u32 + 36).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    for value in [1u16, 1] {
        wav.extend(value.to_le_bytes());
    }
    for value in [44100u32, 88200] {
        wav.extend(value.to_le_bytes());
    }
    for value in [2u16, 16] {
        wav.extend(value.to_le_bytes());
    }
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    fs::write(&path, wav).unwrap();
    let info = read_pcm_wave(&path).unwrap();
    let mut track = plan(1).tracks.remove(0);
    track.device_path = "/Contents/Artist/Original.aiff".into();
    track.analysis.tempos.push(TempoAnchor {
        seconds: 0.0,
        bpm: 120.0,
        beat_in_bar: 1,
    });
    track.analysis.cues.push(CuePoint {
        hot_cue: 1,
        start_ms: 250,
        end_ms: None,
        name: "Cue ñ".into(),
        rgb: None,
    });
    let bundle = anlz::encode(&track, &path, &info).unwrap();
    let inspected = anlz::inspect(&bundle.dat).unwrap();
    assert!(inspected.has_seek_table);
    assert_eq!(inspected.tags[0..3], ["PPTH", "PVBR", "PQTZ"]);
    assert_eq!(inspected.beats.unwrap().len(), 2);
    assert_eq!(inspected.cue_lists[0].cues[0].start_ms, 250);
    assert!(!anlz::inspect(&bundle.ext).unwrap().has_seek_table);

    let be = |bytes: &[u8], at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let first = be(&bundle.dat, 4);
    let start = first + be(&bundle.dat, first + 8);
    assert_eq!(&bundle.dat[start..start + 4], b"PVBR");
    assert_eq!(be(&bundle.dat, start + 4), 16);
    assert_eq!(be(&bundle.dat, start + 8), 1620);
    assert!(bundle.dat[start + 12..start + 1620]
        .iter()
        .all(|byte| *byte == 0));

    // Reproduce the previous Rau DAT, then prove that repair changes no other
    // section, including waveform columns, grid timestamps and cue payloads.
    let mut old = bundle.dat.clone();
    old.drain(start..start + 1620);
    let size = old.len() as u32;
    old[8..12].copy_from_slice(&size.to_be_bytes());
    assert!(!anlz::inspect(&old).unwrap().has_seek_table);
    assert_eq!(anlz::add_pcm_seek_table(&old).unwrap(), bundle.dat);
    assert!(anlz::add_pcm_seek_table(&bundle.dat).is_err());
    assert!(anlz::add_pcm_seek_table(&bundle.ext).is_err());

    let mut malformed = bundle.dat.clone();
    malformed[start + 4..start + 8].copy_from_slice(&20u32.to_be_bytes());
    assert!(anlz::inspect(&malformed).is_err());
    let mut duplicate = bundle.dat.clone();
    duplicate.extend_from_slice(&bundle.dat[start..start + 1620]);
    let size = duplicate.len() as u32;
    duplicate[8..12].copy_from_slice(&size.to_be_bytes());
    assert!(anlz::inspect(&duplicate).is_err());

    track.device_path = "/Contents/Artist/Original.mp3".into();
    let mp3 = anlz::encode(&track, &path, &info).unwrap();
    assert!(mp3
        .summary
        .warnings
        .iter()
        .any(|s| s.contains("VBR seeking")));
    let first = be(&mp3.dat, 4);
    let start = first + be(&mp3.dat, first + 8);
    let mut old_mp3 = mp3.dat;
    old_mp3.drain(start..start + 1620);
    let size = old_mp3.len() as u32;
    old_mp3[8..12].copy_from_slice(&size.to_be_bytes());
    assert!(anlz::add_pcm_seek_table(&old_mp3).is_err());
}

fn waveform_fixture(path: &Path, rate: u32, depth: u16, channels: u16, samples: &[[f64; 2]]) {
    let width = depth / 8;
    let alignment = channels * width;
    let pcm: Vec<_> = samples
        .iter()
        .flat_map(|frame| {
            frame[..usize::from(channels)].iter().flat_map(|sample| {
                let scale = (1i32 << (depth - 1)) as f64;
                let signed = (sample * scale).round().clamp(-scale, scale - 1.0) as i32;
                signed.to_le_bytes()[..usize::from(width)].to_vec()
            })
        })
        .collect();
    let mut bytes = b"RIFF".to_vec();
    let padding = pcm.len() % 2;
    bytes.extend((36 + pcm.len() as u32 + padding as u32).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    for value in [1u16, channels] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [rate, rate * u32::from(alignment)] {
        bytes.extend(value.to_le_bytes());
    }
    for value in [alignment, depth] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(b"data");
    bytes.extend((pcm.len() as u32).to_le_bytes());
    bytes.extend(pcm);
    if padding != 0 {
        bytes.push(0);
    }
    fs::write(path, bytes).unwrap();
}

fn analysis_tag<'a>(bytes: &'a [u8], name: &[u8; 4]) -> (usize, &'a [u8]) {
    let be = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = be(4);
    while at < bytes.len() {
        let length = be(at + 8);
        if &bytes[at..at + 4] == name {
            return (at, &bytes[at..at + length]);
        }
        at += length;
    }
    panic!("missing tag {name:?}");
}

#[test]
fn rgb_waveforms_follow_frequency_content_and_validate_wire_lengths() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bands.wav");
    // Three equal-amplitude tones followed by silence. Color changes must
    // follow their spectrum, rather than cycling by time or reflecting volume.
    let samples: Vec<_> = (0..44100 * 4)
        .map(|frame| {
            let hz = [80.0, 1000.0, 8000.0, 0.0][frame / 44100];
            let sample = 0.6 * (std::f64::consts::TAU * hz * frame as f64 / 44100.0).sin();
            [sample, -sample]
        })
        .collect();
    waveform_fixture(&path, 44100, 16, 2, &samples);
    let info = read_pcm_wave(&path).unwrap();
    let bundle = anlz::encode(&plan(1).tracks[0], &path, &info).unwrap();
    let (preview_at, preview) = analysis_tag(&bundle.ext, b"PWV4");
    let (_, detail) = analysis_tag(&bundle.ext, b"PWV5");
    assert_eq!(
        &preview[4..24],
        &[0, 0, 0, 24, 0, 0, 28, 56, 0, 0, 0, 6, 0, 0, 4, 176, 0, 0, 0, 0]
    );
    assert_eq!(&detail[12..16], &2u32.to_be_bytes());
    assert_eq!(&detail[16..20], &600u32.to_be_bytes());
    assert_eq!(&detail[20..24], &0x0096_0305u32.to_be_bytes());
    assert_eq!(bundle.summary.color_preview_columns, 1200);
    assert_eq!(bundle.summary.color_detail_columns, 600);
    for band in 0..3 {
        let color = &preview[24 + (band * 300 + 150) * 6 + 3..][..3];
        for other in 0..3 {
            if other != band {
                assert!(color[band] > color[other] + 15, "{band}: {color:?}");
            }
        }
        let index = 24 + (band * 150 + 75) * 2;
        let word = u16::from_be_bytes(detail[index..index + 2].try_into().unwrap());
        let rgb = [(word >> 13) & 7, (word >> 10) & 7, (word >> 7) & 7];
        assert_eq!(rgb[band], 7);
        assert!(rgb.iter().enumerate().all(|(i, v)| i == band || *v < 7));
        assert_eq!(word & 3, 0);
        assert!((word >> 2) & 31 > 0);
    }
    assert!(preview[24 + 1000 * 6..24 + 1001 * 6]
        .iter()
        .all(|v| *v == 0));
    assert_eq!(&detail[24 + 550 * 2..24 + 551 * 2], &[0, 0]);

    // All original monochrome payloads remain present and uncolored.
    for tag in [b"PWAV", b"PWV2"] {
        let (_, bytes) = analysis_tag(&bundle.dat, tag);
        assert!(bytes[20..].iter().all(|v| *v <= 31));
    }
    let (_, mono) = analysis_tag(&bundle.ext, b"PWV3");
    assert_eq!(mono.len() - 24, 600);
    assert!(mono[24..].iter().all(|v| *v <= 31));
    assert_eq!(anlz::inspect(&bundle.ext).unwrap().waveforms.len(), 3);

    let (rgb_at, _) = analysis_tag(&bundle.ext, b"PWV5");
    let mut old_ext = bundle.ext[..rgb_at].to_vec();
    let length = old_ext.len() as u32;
    old_ext[8..12].copy_from_slice(&length.to_be_bytes());
    assert_eq!(
        anlz::add_color_waveforms(&old_ext, &path, &info).unwrap(),
        bundle.ext
    );
    assert!(anlz::add_color_waveforms(&bundle.ext, &path, &info).is_err());
    assert!(anlz::add_color_waveforms(&bundle.dat, &path, &info).is_err());
    let (mono_at, _) = analysis_tag(&old_ext, b"PWV3");
    old_ext[mono_at + 24] ^= 1;
    assert!(anlz::add_color_waveforms(&old_ext, &path, &info).is_err());

    assert_eq!(
        anlz::create_three_band_analysis(&bundle.ext, &path, &info).unwrap(),
        bundle.three_band
    );
    assert!(anlz::create_three_band_analysis(&bundle.dat, &path, &info).is_err());
    assert!(anlz::create_three_band_analysis(&old_ext, &path, &info).is_err());
    let three = anlz::inspect(&bundle.three_band).unwrap();
    assert_eq!(three.tags, ["PPTH", "PWV7", "PWV6"]);
    assert_eq!(
        three.audio_path,
        anlz::inspect(&bundle.dat).unwrap().audio_path
    );
    assert_eq!(bundle.summary.three_band_preview_columns, 1200);
    assert_eq!(bundle.summary.three_band_detail_columns, 600);
    for (tag, header, points) in [(b"PWV6", 20, 1200), (b"PWV7", 24, 600)] {
        let (at, bytes) = analysis_tag(&bundle.three_band, tag);
        assert_eq!(&bytes[4..8], &(header as u32).to_be_bytes());
        assert_eq!(&bytes[12..16], &3u32.to_be_bytes());
        assert_eq!(&bytes[16..20], &(points as u32).to_be_bytes());
        assert_eq!(bytes.len(), header + points * 3);
        for (segment, channel) in [2, 0, 1].into_iter().enumerate() {
            let column = &bytes[header + (segment * points / 4 + points / 8) * 3..][..3];
            assert!(column
                .iter()
                .enumerate()
                .all(|(i, v)| i == channel || *v < column[channel]));
        }
        assert_eq!(&bytes[header + (points * 7 / 8) * 3..][..3], &[0, 0, 0]);
        let mut bad = bundle.three_band.clone();
        bad[at + 16..at + 20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(anlz::inspect(&bad).is_err());
    }

    for offset in [4, 12, 16] {
        let mut bad = bundle.ext.clone();
        bad[preview_at + offset..preview_at + offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(anlz::inspect(&bad).is_err());
    }
    let mut duplicate = bundle.ext.clone();
    duplicate.extend(preview);
    let length = duplicate.len() as u32;
    duplicate[8..12].copy_from_slice(&length.to_be_bytes());
    assert!(anlz::inspect(&duplicate).is_err());
}

#[test]
fn rgb_waveforms_handle_pcm_depths_short_audio_and_stereo_phase() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("pcm.wav");
    let track = plan(1).tracks.remove(0);
    for rate in [44100, 48000] {
        for depth in [16, 24] {
            // Deliberately stop inside the last 1/150-second column.
            let samples: Vec<_> = (0..rate + 13)
                .map(|frame| {
                    let sample = 0.8
                        * (std::f64::consts::TAU * 1000.0 * f64::from(frame) / f64::from(rate))
                            .sin();
                    [sample, -sample]
                })
                .collect();
            let mut reference = None;
            for channels in [1, 2] {
                waveform_fixture(&path, rate, depth, channels, &samples);
                let bundle = anlz::encode(&track, &path, &read_pcm_wave(&path).unwrap()).unwrap();
                assert_eq!(bundle.summary.color_detail_columns, 151);
                let payload = analysis_tag(&bundle.ext, b"PWV5").1[24..].to_vec();
                assert_eq!(bundle.summary.three_band_detail_columns, 151);
                assert_eq!(
                    analysis_tag(&bundle.three_band, b"PWV7").1.len(),
                    24 + 151 * 3
                );
                if let Some(ref mono) = reference {
                    assert_eq!(&payload, mono);
                } else {
                    reference = Some(payload);
                }
            }
        }
    }
    // A single PCM24 mono frame exercises RIFF padding and mostly empty bins.
    waveform_fixture(&path, 48000, 24, 1, &[[-1.0, 0.0]]);
    let bundle = anlz::encode(&track, &path, &read_pcm_wave(&path).unwrap()).unwrap();
    assert_eq!(bundle.summary.color_detail_columns, 1);
    assert_eq!(bundle.summary.color_preview_columns, 1200);
    let (_, detail) = analysis_tag(&bundle.ext, b"PWV5");
    assert_eq!((u16::from_be_bytes([detail[24], detail[25]]) >> 2) & 31, 31);
}
