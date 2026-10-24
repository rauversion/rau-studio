//! Local prototype runner. Does not discover, format, erase or publish to USBs.
use aifficator_core::rekordbox::{parse_rekordbox_xml_file, PlaylistNode, RekordboxLibrary, Track};
use aifficator_core::usb_export::{
    anlz, devicesql, onelibrary, plan_library, write_classic_folder,
};
use aifficator_core::usb_export::{
    AudioInfo, ExportPlan, ExportPlaylist, ExportTrack, LibraryIdentity,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run() {
        eprintln!("USB export: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    let ffmpeg = env::var_os("RAU_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    match args.first().map(String::as_str) {
        Some("list") if args.len() == 2 => {
            let library = parse_rekordbox_xml_file(&args[1])?;
            for playlist in library.playlists_flat() {
                println!("{}\t{} tracks", playlist.path, playlist.track_count);
            }
        }
        Some("plan") if args.len() >= 3 => {
            let library = parse_rekordbox_xml_file(&args[1])?;
            println!("{}", serde_json::to_string_pretty(&plan_library(&library, &args[2..])?)?);
        }
        Some("export-classic") if args.len() >= 4 => {
            let library = parse_rekordbox_xml_file(&args[1])?;
            let plan = plan_library(&library, &args[3..])?;
            eprintln!("Preparing {} tracks; experimental DeviceSQL + OneLibrary + DAT/EXT/2EX analysis.", plan.tracks.len());
            let report = write_classic_folder(&plan, Path::new(&args[2]), &ffmpeg)?;
            println!("Prepared {} tracks in {}\nReport: {}/rau-export-report.json", report.tracks.len(), args[2], args[2]);
        }
        Some("export-plan") if args.len() == 3 => {
            if fs::metadata(&args[1])?.len() > 64 * 1024 * 1024 { return Err("plan exceeds 64 MiB".into()); }
            let plan: ExportPlan = serde_json::from_slice(&fs::read(&args[1])?)?;
            let report = write_classic_folder(&plan, Path::new(&args[2]), &ffmpeg)?;
            println!("Prepared both libraries and {} tracks in {}", report.tracks.len(), args[2]);
        }
        Some("inspect") if args.len() == 2 => {
            let metadata = fs::metadata(&args[1])?;
            if metadata.len() > 128 * 1024 * 1024 { return Err("database exceeds the inspector's 128 MiB limit".into()); }
            println!("{}", serde_json::to_string_pretty(&devicesql::inspect(&fs::read(&args[1])?)?)?);
        }
        Some("inspect-onelibrary") if args.len() == 2 => {
            println!("{}", serde_json::to_string_pretty(&onelibrary::inspect(Path::new(&args[1]))?)?);
        }
        Some("rebuild-database") if args.len() == 3 => {
            // Diagnostic preparation only: make a NEW database from an existing
            // receipt, leaving the source receipt, audio and USB untouched.
            if fs::metadata(&args[1])?.len() > 64 * 1024 * 1024 {
                return Err("export receipt exceeds 64 MiB".into());
            }
            let receipt: DatabaseReceipt = serde_json::from_slice(&fs::read(&args[1])?)?;
            if receipt.schema_version < 4 {
                return Err("receipt predates the DAT/EXT/2EX profile; prepare a full new export before rebuilding its database".into());
            }
            let date = chrono::DateTime::parse_from_rfc3339(&receipt.created_at)?;
            let audio = receipt.tracks.iter().map(|t| t.audio.clone()).collect::<Vec<_>>();
            let plan = ExportPlan { tracks: receipt.tracks.into_iter().map(|t| t.track).collect(),
                playlists: receipt.playlists, source_bytes: 0 };
            if receipt.schema_version >= 5 && receipt.library_identity.is_none() {
                return Err("dual-library receipt is missing its export identity".into());
            }
            let date = date.format("%Y-%m-%d").to_string();
            let bytes = match receipt.library_identity {
                Some(identity) => devicesql::encode_with_identity(&plan, &audio, &date, &identity)?,
                None => devicesql::encode(&plan, &audio, &date)?,
            };
            let db = devicesql::inspect(&bytes)?;
            devicesql::compare(&plan, &audio, &db)?;
            let mut output = File::create_new(&args[2])?;
            output.write_all(&bytes)?;
            output.sync_all()?;
            println!("Prepared new database: {} tracks, {} playlists; audio/analysis unchanged.\n{}", db.tracks.len(), db.playlists.len(), args[2]);
        }
        Some("inspect-analysis") if args.len() == 2 => {
            if fs::metadata(&args[1])?.len() > anlz::MAX_FILE_BYTES as u64 {
                return Err("analysis exceeds the inspector's 64 MiB limit".into());
            }
            println!("{}", serde_json::to_string_pretty(&anlz::inspect(&fs::read(&args[1])?)?)?);
        }
        Some("repair-pcm-analysis") if args.len() == 3 => {
            if fs::metadata(&args[1])?.len() > anlz::MAX_FILE_BYTES as u64 {
                return Err("analysis exceeds the inspector's 64 MiB limit".into());
            }
            let repaired = anlz::add_pcm_seek_table(&fs::read(&args[1])?)?;
            let mut output = File::create_new(&args[2])?;
            output.write_all(&repaired)?;
            output.sync_all()?;
            println!("Prepared PCM DAT with PVBR: {}. Source unchanged; publish separately.", args[2]);
        }
        Some("add-rgb-analysis") if args.len() == 4 => {
            if fs::metadata(&args[1])?.len() > anlz::MAX_FILE_BYTES as u64 {
                return Err("analysis exceeds the inspector's 64 MiB limit".into());
            }
            let pcm = Path::new(&args[2]);
            let info = aifficator_core::usb_export::read_pcm_wave(pcm)?;
            let revised = anlz::add_color_waveforms(&fs::read(&args[1])?, pcm, &info)?;
            let mut output = File::create_new(&args[3])?;
            output.write_all(&revised)?;
            output.sync_all()?;
            println!("Prepared EXT with RGB waveforms: {}. Existing sections preserved; publish separately.", args[3]);
        }
        Some("create-three-band-analysis") if args.len() == 4 => {
            if fs::metadata(&args[1])?.len() > anlz::MAX_FILE_BYTES as u64 {
                return Err("analysis exceeds the inspector's 64 MiB limit".into());
            }
            let pcm = Path::new(&args[2]);
            let info = aifficator_core::usb_export::read_pcm_wave(pcm)?;
            let bytes = anlz::create_three_band_analysis(&fs::read(&args[1])?, pcm, &info)?;
            let mut output = File::create_new(&args[3])?;
            output.write_all(&bytes)?;
            output.sync_all()?;
            println!("Prepared 3Band companion: {}. Existing DAT/EXT unchanged; publish separately.", args[3]);
        }
        Some("demo") if args.len() == 2 => {
            let sources = tempfile::tempdir()?;
            let mut tracks = Vec::new();
            for (i, title) in ["Rau USB - Pulse 120", "Rau USB - Señal estéreo"].iter().enumerate() {
                let path = sources.path().join(format!("{title}.wav"));
                tone(&path, if i == 0 { 440.0 } else { 660.0 })?;
                tracks.push(Track {
                    track_id: (i+1).to_string(), name: Some(title.to_string()),
                    artist: Some("Rau Studio".into()), album: Some("USB prototype".into()),
                    file_path: Some(path), total_time: Some(12), sample_rate: Some(44_100),
                    attributes: [("AverageBpm".into(), "120".into())].into(),
                    // These anchors match the generated pulses; they are not
                    // inferred from the AverageBpm metadata above.
                    tempos: vec![attrs(&[("Inizio", "0"), ("Bpm", "120"), ("Metro", "4/4"), ("Battito", "1")])],
                    position_marks: vec![
                        attrs(&[("Name", "Inicio"), ("Type", "0"), ("Start", "0"), ("Num", "0")]),
                        attrs(&[("Name", "Loop 4 beats"), ("Type", "4"), ("Start", "4"), ("End", "6"), ("Num", "1")]),
                        attrs(&[("Name", "Salida"), ("Type", "0"), ("Start", "8"), ("Num", "2")]),
                        attrs(&[("Name", "Memoria ñ"), ("Type", "0"), ("Start", "10"), ("Num", "-1")]),
                    ],
                    ..Track::default()
                });
            }
            let library = RekordboxLibrary { tracks,
                playlists: vec![PlaylistNode { name: "Rau USB Test".into(), node_type: Some("1".into()),
                    track_keys: vec!["1".into(), "2".into(), "1".into()], ..PlaylistNode::default() }],
                ..RekordboxLibrary::default() };
            let plan = plan_library(&library, &["Rau USB Test".into()])?;
            let report = write_classic_folder(&plan, Path::new(&args[1]), &ffmpeg)?;
            println!("Demo prepared in {}: {} unique tracks, playlist order 1 / 2 / 1.\nEach track: 24 beats at 120 BPM, hot A=0s, B=loop 4–6s, C=8s, memory=10s; monochrome, RGB and 3Band preview/detail.\nExperimental; hardware compatibility remains unverified.", args[1], report.tracks.len());
        }
        _ => return Err("usage: usb_export list XML | plan XML PLAYLIST... | export-classic XML NEW_FOLDER PLAYLIST... | export-plan PLAN.JSON NEW_FOLDER | inspect EXPORT.PDB | inspect-onelibrary EXPORTLIBRARY.DB | rebuild-database REPORT NEW_EXPORT.PDB | inspect-analysis ANLZ0000.DAT-or-EXT-or-2EX | repair-pcm-analysis INPUT.DAT NEW.DAT | add-rgb-analysis INPUT.EXT DECODED.WAV NEW.EXT | create-three-band-analysis INPUT.EXT DECODED.WAV NEW.2EX | demo NEW_FOLDER\nSet RAU_FFMPEG to use a specific ffmpeg executable.".into()),
    }
    Ok(())
}

#[derive(Deserialize)]
struct DatabaseReceipt {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    library_identity: Option<LibraryIdentity>,
    created_at: String,
    tracks: Vec<DatabaseReceiptTrack>,
    playlists: Vec<ExportPlaylist>,
}

#[derive(Deserialize)]
struct DatabaseReceiptTrack {
    track: ExportTrack,
    audio: AudioInfo,
}

fn attrs(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn tone(path: &Path, frequency: f64) -> std::io::Result<()> {
    let frames = 44_100u32 * 12;
    let data_size = frames * 4;
    let mut file = File::create_new(path)?;
    file.write_all(b"RIFF")?;
    file.write_all(&(data_size + 36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&44_100u32.to_le_bytes())?;
    file.write_all(&176_400u32.to_le_bytes())?;
    file.write_all(&4u16.to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;
    let mut pcm = Vec::with_capacity(data_size as usize);
    for frame in 0..frames {
        let t = f64::from(frame) / 44_100.0;
        let phase = t % 0.5;
        let envelope = if phase < 0.1 {
            (std::f64::consts::PI * phase / 0.1).sin()
        } else {
            0.0
        };
        for hz in [frequency, frequency * 1.5] {
            let sample =
                (0.12 * envelope * (t * hz * std::f64::consts::TAU).sin() * f64::from(i16::MAX))
                    as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
    }
    file.write_all(&pcm)
}
