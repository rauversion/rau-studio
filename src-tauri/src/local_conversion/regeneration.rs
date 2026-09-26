//! File publication and destination reservations shared by the conversion writers.
use super::read_audio_tags;
use aifficator_core::conversion::{ffmpeg_args, ConversionSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::UNIX_EPOCH;
use uuid::Uuid;

#[derive(Default)]
struct Reservations {
    paths: BTreeSet<PathBuf>,
    running: usize,
}

static RESERVATIONS: OnceLock<(Mutex<Reservations>, Condvar)> = OnceLock::new();

pub(crate) fn destination_key(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|e| e.to_string());
    }
    let parent = path.parent().ok_or("Destination has no parent")?;
    if parent.exists() {
        return Ok(fs::canonicalize(parent)
            .map_err(|e| e.to_string())?
            .join(path.file_name().ok_or("Destination has no name")?));
    }
    Ok(destination_key(parent)?.join(path.file_name().ok_or("Destination has no name")?))
}

pub(crate) struct DestinationLease(PathBuf);

impl DestinationLease {
    pub(crate) fn acquire(path: &Path) -> Result<Self, String> {
        let key = destination_key(path)?;
        let (mutex, available) = RESERVATIONS.get_or_init(Default::default);
        let mut reservations = mutex.lock().map_err(|e| e.to_string())?;
        if !reservations.paths.insert(key.clone()) {
            return Err("This AIFF is already being processed".into());
        }
        while reservations.running >= 4 {
            reservations = available.wait(reservations).map_err(|e| e.to_string())?;
        }
        reservations.running += 1;
        Ok(Self(key))
    }
}

impl Drop for DestinationLease {
    fn drop(&mut self) {
        if let Some((mutex, available)) = RESERVATIONS.get() {
            if let Ok(mut reservations) = mutex.lock() {
                reservations.paths.remove(&self.0);
                reservations.running -= 1;
                available.notify_all();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Fingerprint {
    size: u64,
    modified: Option<u128>,
    created: Option<u128>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl Fingerprint {
    pub(super) fn read(path: &Path) -> Result<Self, String> {
        let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        if !meta.is_file() {
            return Err(format!("Expected a regular file: {}", path.display()));
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
                .map(|v| v.as_nanos()),
            created: meta
                .created()
                .ok()
                .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
                .map(|v| v.as_nanos()),
            #[cfg(unix)]
            device: meta.dev(),
            #[cfg(unix)]
            inode: meta.ino(),
        })
    }
}

fn optional_fingerprint(path: &Path) -> Result<Option<Fingerprint>, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Fingerprint::read(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

// Unlike a hard link, these exclusive renames also work on removable volumes
// whose filesystems do not support links. Failure never removes the destination.
#[cfg(target_os = "macos")]
fn publish_new(source: &Path, target: &Path) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let target = CString::new(target.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // RENAME_EXCL from the macOS SDK's sys/stdio.h.
    let result = unsafe { renamex_np(source.as_ptr(), target.as_ptr(), 0x4) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(target_os = "linux")]
fn publish_new(source: &Path, target: &Path) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn renameat2(
            from_fd: i32,
            from: *const std::ffi::c_char,
            to_fd: i32,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let source = CString::new(source.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let target = CString::new(target.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    // AT_FDCWD / RENAME_NOREPLACE.
    let result = unsafe { renameat2(-100, source.as_ptr(), -100, target.as_ptr(), 1) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(target_os = "windows")]
fn publish_new(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // No MOVEFILE_REPLACE_EXISTING: a destination created concurrently wins.
    let result = unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0) };
    if result != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn publish_new(source: &Path, target: &Path) -> Result<(), String> {
    fs::hard_link(source, target).map_err(|e| e.to_string())
}

fn normalized_tags(tags: BTreeMap<String, String>) -> BTreeMap<String, String> {
    tags.into_iter()
        .map(|(key, value)| {
            let key = match key.as_str() {
                "albumartist" | "album artist" => "album_artist",
                "year" => "date",
                "tracknumber" => "track",
                "discnumber" => "disc",
                "comments" | "description" => "comment",
                "key" | "tkey" => "initial_key",
                "tbpm" => "bpm",
                _ => &key,
            }
            .to_string();
            (key, value)
        })
        .collect()
}

fn probe(mut command: Command, path: &Path) -> Result<Value, String> {
    let output = command
        .args([
            "-v",
            "error",
            "-show_format",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())
}

fn duration(info: &Value) -> Result<f64, String> {
    info["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .ok_or_else(|| "Cannot verify audio duration".into())
}

fn artwork(info: &Value) -> Vec<&Value> {
    info["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] == 1)
        .collect()
}

pub(super) struct Regeneration {
    pub temporary: PathBuf,
    source: PathBuf,
    target: PathBuf,
    source_before: Fingerprint,
    target_before: Option<Fingerprint>,
    expected_tags: BTreeMap<String, String>,
    source_duration: f64,
    artwork_codecs: Vec<String>,
    pub args: Vec<String>,
}

impl Regeneration {
    pub(super) fn prepare(
        source: &Path,
        target: &Path,
        ffprobe: impl Fn() -> Command,
    ) -> Result<Self, String> {
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        if destination_key(target)? == source {
            return Err("The original cannot be the regeneration destination".into());
        }
        let source_before = Fingerprint::read(&source)?;
        let target_before = optional_fingerprint(target)?;
        #[cfg(unix)]
        if target_before
            .as_ref()
            .is_some_and(|v| v.device == source_before.device && v.inode == source_before.inode)
        {
            return Err("The destination is a hard link to the original".into());
        }
        let source_duration = duration(&probe(ffprobe(), &source)?)?;
        let mut expected_tags = if target_before.is_some() {
            normalized_tags(read_audio_tags(ffprobe(), target)?)
        } else {
            BTreeMap::new()
        };
        expected_tags.extend(normalized_tags(read_audio_tags(ffprobe(), &source)?));
        let temporary = target.with_file_name(format!(".rau-regenerate-{}.aiff", Uuid::new_v4()));
        let mut args = ffmpeg_args(&source, &temporary, &ConversionSettings::default());
        // Apply a single normalized map instead of retaining conflicting source aliases.
        let metadata_index = args.iter().position(|arg| arg == "-map_metadata").unwrap();
        args[metadata_index + 1] = "-1".into();
        let mut artwork_codecs = Vec::new();
        if target_before.is_some() {
            let info = probe(ffprobe(), target)?;
            let pictures = artwork(&info);
            if !pictures.is_empty() {
                let input_index = args.iter().position(|arg| arg == "-map").unwrap();
                args.splice(
                    input_index..input_index,
                    ["-i".into(), target.to_string_lossy().into_owned()],
                );
                args.retain(|arg| arg != "-vn");
                let output = args.pop().unwrap();
                for picture in pictures {
                    args.extend(["-map".into(), format!("1:{}", picture["index"])]);
                    artwork_codecs.push(
                        picture["codec_name"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                    );
                }
                args.extend([
                    "-c:v".into(),
                    "copy".into(),
                    "-disposition:v".into(),
                    "attached_pic".into(),
                    output,
                ]);
            }
        }
        let output = args.pop().unwrap();
        for (key, value) in &expected_tags {
            args.extend(["-metadata".into(), format!("{key}={value}")]);
        }
        args.push(output);
        Ok(Self {
            temporary,
            source,
            target: target.to_path_buf(),
            source_before,
            target_before,
            expected_tags,
            source_duration,
            artwork_codecs,
            args,
        })
    }

    pub(super) fn validate(
        &self,
        ffprobe: impl Fn() -> Command,
        mut ffmpeg: Command,
    ) -> Result<Fingerprint, String> {
        let info = probe(ffprobe(), &self.temporary)?;
        let audio = info["streams"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["codec_type"] == "audio")
            .ok_or("Generated file has no audio")?;
        if info["format"]["format_name"] != "aiff"
            || audio["codec_name"] != "pcm_s16be"
            || audio["sample_rate"] != "44100"
            || audio["channels"] != 2
        {
            return Err("Generated AIFF does not match the conversion profile".into());
        }
        if (duration(&info)? - self.source_duration).abs() > 0.25 {
            return Err("Generated AIFF duration differs from the original".into());
        }
        let tags = normalized_tags(read_audio_tags(ffprobe(), &self.temporary)?);
        for (key, value) in &self.expected_tags {
            if tags.get(key) != Some(value) {
                return Err(format!("Could not preserve metadata field: {key}"));
            }
        }
        let codecs = artwork(&info)
            .iter()
            .map(|s| s["codec_name"].as_str().unwrap_or_default().to_string())
            .collect::<Vec<_>>();
        if codecs != self.artwork_codecs {
            return Err("Could not preserve the existing artwork".into());
        }
        let output = ffmpeg
            .args(["-v", "error", "-xerror", "-nostdin", "-i"])
            .arg(&self.temporary)
            .args(["-map", "0:a:0", "-f", "null", "-"])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "Generated audio failed verification: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if self.target_before.is_some() {
            fs::set_permissions(
                &self.temporary,
                fs::metadata(&self.target)
                    .map_err(|e| e.to_string())?
                    .permissions(),
            )
            .map_err(|e| e.to_string())?;
        }
        fs::File::open(&self.temporary)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Fingerprint::read(&self.temporary)
    }

    pub(super) fn publish(&self) -> Result<(), String> {
        if Fingerprint::read(&self.source)? != self.source_before
            || optional_fingerprint(&self.target)? != self.target_before
        {
            return Err(
                "Source or destination changed during regeneration; no file was replaced".into(),
            );
        }
        if self.target_before.is_some() {
            fs::rename(&self.temporary, &self.target).map_err(|e| e.to_string())?;
        } else {
            // Publishing a missing output must never overwrite a concurrent creation.
            publish_new(&self.temporary, &self.target)?;
        }
        Ok(())
    }
}

impl Drop for Regeneration {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temporary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        source: PathBuf,
        target: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("rau-regeneration-test-{}", Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            let source = root.join("original.flac");
            let target = root.join("output.aiff");
            run(Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                    "-t",
                    "0.5",
                    "-metadata",
                    "title=Título corregido",
                    "-metadata",
                    "artist=Artista",
                    "-metadata",
                    "BPM=125",
                    "-metadata",
                    "initial_key=Am",
                    "-metadata",
                    "album_artist=Álbum",
                    "-metadata",
                    "track=3/10",
                ])
                .arg(&source));
            Self {
                root,
                source,
                target,
            }
        }
        fn old_output(&self, cover: bool) {
            let mut cmd = Command::new("ffmpeg");
            cmd.args(["-v", "error", "-i"]).arg(&self.source);
            if cover {
                cmd.args([
                    "-f",
                    "lavfi",
                    "-i",
                    "color=c=red:s=8x8",
                    "-map",
                    "0:a:0",
                    "-map",
                    "1:v:0",
                    "-frames:v",
                    "1",
                    "-c:v",
                    "png",
                    "-disposition:v",
                    "attached_pic",
                ]);
            }
            run(cmd
                .args([
                    "-t",
                    "0.5",
                    "-c:a",
                    "pcm_s24be",
                    "-ac",
                    "1",
                    "-map_metadata",
                    "-1",
                    "-metadata",
                    "title=Old title",
                    "-metadata",
                    "comment=Keep my edit",
                    "-write_id3v2",
                    "1",
                    "-id3v2_version",
                    "3",
                ])
                .arg(&self.target));
        }
        fn prepare(&self) -> Regeneration {
            Regeneration::prepare(&self.source, &self.target, || Command::new("ffprobe")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn run(cmd: &mut Command) {
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fn render(job: &Regeneration) {
        run(Command::new("ffmpeg").args(&job.args));
    }

    #[test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn regeneration_preserves_original_tags_artwork_and_publishes_verified_audio() {
        let fixture = Fixture::new();
        fixture.old_output(true);
        let source_bytes = fs::read(&fixture.source).unwrap();
        let old_bytes = fs::read(&fixture.target).unwrap();
        let job = fixture.prepare();
        render(&job);
        let signature = job
            .validate(|| Command::new("ffprobe"), Command::new("ffmpeg"))
            .unwrap();
        assert_eq!(
            fs::read(&fixture.target).unwrap(),
            old_bytes,
            "destination remains untouched until publication"
        );
        job.publish().unwrap();
        assert_eq!(
            Fingerprint::read(&fixture.target).unwrap(),
            signature,
            "publication can be recovered from the journal"
        );
        assert_eq!(fs::read(&fixture.source).unwrap(), source_bytes);
        let tags =
            normalized_tags(read_audio_tags(Command::new("ffprobe"), &fixture.target).unwrap());
        assert_eq!(tags["title"], "Título corregido");
        assert_eq!(tags["comment"], "Keep my edit");
        assert_eq!(tags["bpm"], "125");
        assert_eq!(tags["initial_key"], "Am");
        assert_eq!(
            artwork(&probe(Command::new("ffprobe"), &fixture.target).unwrap()).len(),
            1
        );
        let tmp = job.temporary.clone();
        drop(job);
        assert!(!tmp.exists());
    }

    #[test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn regeneration_failure_and_invalid_output_leave_previous_file_identical() {
        let fixture = Fixture::new();
        fixture.old_output(false);
        let original = fs::read(&fixture.target).unwrap();
        let job = fixture.prepare();
        let failed = Command::new("ffmpeg")
            .arg("-invalid-option-for-regeneration-test")
            .args(&job.args)
            .output()
            .unwrap();
        assert!(!failed.status.success());
        fs::write(&job.temporary, b"partial audio").unwrap();
        assert!(job
            .validate(|| Command::new("ffprobe"), Command::new("ffmpeg"))
            .is_err());
        let temp = job.temporary.clone();
        drop(job);
        assert_eq!(fs::read(&fixture.target).unwrap(), original);
        assert!(!temp.exists());
    }

    #[test]
    #[ignore = "requires ffmpeg and ffprobe on PATH"]
    fn regeneration_recreates_missing_output_but_refuses_concurrent_changes() {
        let fixture = Fixture::new();
        let job = fixture.prepare();
        render(&job);
        job.validate(|| Command::new("ffprobe"), Command::new("ffmpeg"))
            .unwrap();
        fs::write(&fixture.target, b"another writer").unwrap();
        assert!(job.publish().is_err());
        assert_eq!(fs::read(&fixture.target).unwrap(), b"another writer");
        fs::remove_file(&fixture.target).unwrap();
        job.publish().unwrap();
        drop(job);
        let job = fixture.prepare();
        render(&job);
        job.validate(|| Command::new("ffprobe"), Command::new("ffmpeg"))
            .unwrap();
        fs::write(&fixture.source, b"source changed").unwrap();
        let previous = fs::read(&fixture.target).unwrap();
        assert!(job.publish().is_err());
        assert_eq!(fs::read(&fixture.target).unwrap(), previous);
    }

    #[test]
    fn regeneration_reservations_reject_duplicate_destinations() {
        let root = std::env::temp_dir().join(format!("rau-lease-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("converted").join("output.aiff");
        let lease = DestinationLease::acquire(&path).unwrap();
        assert!(DestinationLease::acquire(&path).is_err());
        drop(lease);
        assert!(DestinationLease::acquire(&path).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn regeneration_rejects_symlinks_and_hard_links_to_source() {
        let root = std::env::temp_dir().join(format!("rau-links-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("original.flac");
        let target = root.join("output.aiff");
        fs::write(&source, b"original").unwrap();
        std::os::unix::fs::symlink(&source, &target).unwrap();
        assert!(Regeneration::prepare(&source, &target, || Command::new("ffprobe")).is_err());
        fs::remove_file(&target).unwrap();
        fs::hard_link(&source, &target).unwrap();
        assert!(Regeneration::prepare(&source, &target, || Command::new("ffprobe")).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }
}
