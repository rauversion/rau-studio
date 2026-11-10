use ring::digest::{Context, SHA256};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, Metadata},
    io::{Read, Seek, SeekFrom},
    path::Path,
    time::UNIX_EPOCH,
};

// Bounded content checkpoints: a growing file must retain the saved prefix and
// the saved tail at its original offset. This is local provenance, not a hash
// supplied by the remote peer or a whole-file integrity guarantee.
const SAMPLE: u64 = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Checkpoint {
    pub bytes: u64,
    identity: String,
    modified_ns: Option<u128>,
    prefix: Vec<u8>,
    tail: Vec<u8>,
}

fn identity(meta: &Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        format!("{:?}", meta.created().ok())
    }
}

fn modified(meta: &Metadata) -> Option<u128> {
    meta.modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|t| t.as_nanos())
}

fn open(path: &Path) -> Result<(File, Metadata), String> {
    let before = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !before.is_file() {
        return Err("El parcial debe ser un archivo regular, sin enlaces simbólicos.".into());
    }
    let file = File::open(path).map_err(|e| e.to_string())?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    if identity(&before) != identity(&after) {
        return Err("El parcial cambió mientras se abría.".into());
    }
    Ok((file, after))
}

fn sample(file: &mut File, offset: u64, length: u64) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut data = vec![0; length as usize];
    file.read_exact(&mut data).map_err(|e| e.to_string())?;
    let mut hash = Context::new(&SHA256);
    hash.update(&data);
    Ok(hash.finish().as_ref().to_vec())
}

pub fn capture(path: &Path, expected: u64) -> Result<Option<Checkpoint>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    let (mut file, meta) = open(path)?;
    if meta.len() > expected {
        return Err("El parcial supera el tamaño remoto esperado.".into());
    }
    let length = meta.len().min(SAMPLE);
    Ok(Some(Checkpoint {
        bytes: meta.len(),
        identity: identity(&meta),
        modified_ns: modified(&meta),
        prefix: sample(&mut file, 0, length)?,
        tail: sample(&mut file, meta.len() - length, length)?,
    }))
}

pub fn verify(
    path: &Path,
    expected: u64,
    saved: Option<&Checkpoint>,
    allow_growth: bool,
) -> Result<Option<u64>, String> {
    let Some(saved) = saved else {
        return match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
            Ok(_) => Err("Existe un archivo sin checkpoint de este trabajo. Elige otro destino o mueve ese archivo antes de reintentar.".into()),
        };
    };
    let (mut file, meta) = open(path)?;
    if identity(&meta) != saved.identity
        || meta.len() < saved.bytes
        || meta.len() > expected
        || (meta.len() == saved.bytes && modified(&meta) != saved.modified_ns)
        || (!allow_growth && (meta.len() != saved.bytes || modified(&meta) != saved.modified_ns))
    {
        return Err("El parcial fue reemplazado, modificado o tiene un tamaño incompatible. No se autorizó la reanudación.".into());
    }
    let length = saved.bytes.min(SAMPLE);
    if sample(&mut file, 0, length)? != saved.prefix
        || sample(&mut file, saved.bytes - length, length)? != saved.tail
    {
        return Err("El contenido del parcial no coincide con el checkpoint guardado.".into());
    }
    Ok(Some(meta.len()))
}

pub fn validate_destination(root: &Path, destination: &Path) -> Result<(), String> {
    if !destination.is_absolute()
        || destination
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err("El destino debe ser absoluto y no puede contener '..'.".into());
    }
    let root_meta = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if !root_meta.is_dir()
        || root_meta.file_type().is_symlink()
        || root.canonicalize().map_err(|e| e.to_string())? != root
    {
        return Err("La carpeta de destino cambió o contiene enlaces simbólicos.".into());
    }
    let relative = destination
        .strip_prefix(root)
        .map_err(|_| "El destino escapa de la carpeta elegida.")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, std::path::Component::Normal(_)) {
            return Err("Ruta de destino inválida.".into());
        }
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("El destino contiene un enlace simbólico.".into())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_checks_content_identity_growth_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track");
        fs::write(&path, b"abc").unwrap();
        let saved = capture(&path, 8).unwrap().unwrap();
        assert_eq!(verify(&path, 8, Some(&saved), false).unwrap(), Some(3));
        use std::io::Write;
        fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"def")
            .unwrap();
        assert_eq!(verify(&path, 8, Some(&saved), true).unwrap(), Some(6));
        assert!(verify(&path, 8, Some(&saved), false).is_err());
        fs::write(&path, b"xxxdef").unwrap();
        assert!(verify(&path, 8, Some(&saved), true).is_err());
        fs::rename(&path, dir.path().join("original")).unwrap();
        fs::write(&path, b"abc").unwrap();
        // Keep the old inode alive so a replacement cannot immediately reuse it.
        assert!(verify(&path, 8, Some(&saved), false).is_err());
        assert!(verify(&path, 8, None, true).is_err());
        fs::write(&path, b"0123456789").unwrap();
        assert!(capture(&path, 8).is_err());
    }

    #[test]
    fn missing_partial_is_only_new_when_no_checkpoint_exists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track");
        assert_eq!(verify(&path, 8, None, true).unwrap(), None);
        fs::write(&path, b"abc").unwrap();
        let saved = capture(&path, 8).unwrap().unwrap();
        fs::remove_file(&path).unwrap();
        assert!(verify(&path, 8, Some(&saved), true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_escaping_destinations_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::write(root.join("real"), b"abc").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        assert!(capture(&root.join("link"), 8).is_err());
        assert!(validate_destination(&root, &root.join("link")).is_err());
        assert!(validate_destination(&root, &root.join("../outside")).is_err());
        assert!(validate_destination(&root, &root.join("album/track")).is_ok());
    }
}
