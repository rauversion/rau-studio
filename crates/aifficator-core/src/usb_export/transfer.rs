//! One USB writer, fed by prepared local files. Original USB assets are immutable.
use super::*;
use std::time::{Duration, Instant};

pub(super) struct TransferFile {
    pub relative: String,
    source: PathBuf,
    destination: PathBuf,
    pub bytes: u64,
    checksum: String,
}

pub(super) struct PreparedTrack {
    pub record: ExportedTrack,
    pub files: Vec<TransferFile>,
}

impl PreparedTrack {
    pub fn new(
        record: ExportedTrack,
        local: &Path,
        destination: &Path,
        audio: bool,
    ) -> Result<Self> {
        let mut paths = Vec::new();
        if audio {
            paths.push((&record.track.device_path, &record.sha256));
        }
        paths.extend([
            (&record.analysis.dat_path, &record.dat_sha256),
            (&record.analysis.ext_path, &record.ext_sha256),
            (&record.analysis.three_band_path, &record.three_band_sha256),
        ]);
        let mut files = Vec::new();
        for (relative, checksum) in paths {
            let source = incremental::device_file(local, relative)?;
            let metadata = fs::metadata(&source)?;
            if !metadata.is_file() {
                return Err(invalid("prepared media is not a file"));
            }
            files.push(TransferFile {
                relative: relative.clone(),
                source,
                destination: destination.to_owned(),
                bytes: metadata.len(),
                checksum: checksum.clone(),
            });
        }
        Ok(Self { record, files })
    }

    pub fn bytes(&self) -> Result<u64> {
        self.files.iter().try_fold(0u64, |sum, file| {
            sum.checked_add(file.bytes)
                .ok_or_else(|| invalid("USB transfer size limit"))
        })
    }
}

pub(super) fn available_workers() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

pub(super) fn write_file(file: &TransferFile, mut progress: impl FnMut(&str, u64)) -> Result<()> {
    let target = incremental::device_file(&file.destination, &file.relative)?;
    fs::create_dir_all(target.parent().unwrap())?;
    let mut input = File::open(&file.source)?;
    let mut output = File::create_new(&target)?;
    let mut buffer = vec![0; 1024 * 1024];
    let mut digest = Sha256::new();
    let mut copied = 0u64;
    let mut notified = Instant::now();
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        digest.update(&buffer[..count]);
        copied += count as u64;
        if copied > file.bytes {
            return Err(invalid("prepared media grew during copying"));
        }
        if copied == file.bytes || notified.elapsed() >= Duration::from_millis(500) {
            progress("publish_bytes", copied);
            notified = Instant::now();
        }
    }
    if copied != file.bytes || format!("{:x}", digest.finalize()) != file.checksum {
        return Err(invalid("prepared media changed before copying"));
    }
    progress("sync_media", copied);
    output.sync_all()?;
    progress("verify_media", 0);
    let mut reread = File::open(&target)?;
    let mut digest = Sha256::new();
    let mut verified = 0u64;
    notified = Instant::now();
    loop {
        let count = reread.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        verified += count as u64;
        if verified > file.bytes {
            return Err(invalid("USB media grew during verification"));
        }
        if verified == file.bytes || notified.elapsed() >= Duration::from_millis(500) {
            progress("verify_bytes", verified);
            notified = Instant::now();
        }
    }
    if verified != file.bytes || format!("{:x}", digest.finalize()) != file.checksum {
        return Err(invalid("USB media differs from prepared audio or analysis"));
    }
    drop(input);
    fs::remove_file(&file.source)?;
    Ok(())
}

pub(super) fn capture_signatures(root: &Path, record: &mut ExportedTrack) -> Result<()> {
    record.device_signature = Some(incremental::FileSignature::read(
        &incremental::device_file(root, &record.track.device_path)?,
    )?);
    record.analysis_signatures = Some(
        [
            &record.analysis.dat_path,
            &record.analysis.ext_path,
            &record.analysis.three_band_path,
        ]
        .into_iter()
        .map(|p| incremental::FileSignature::read(&incremental::device_file(root, p)?))
        .collect::<Result<Vec<_>>>()?,
    );
    Ok(())
}
