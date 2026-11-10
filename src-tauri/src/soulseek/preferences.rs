use super::{message, partial};
use crate::settings;
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::AppHandle;

#[derive(Serialize)]
pub struct DownloadSettings {
    download_directory: Option<String>,
}

#[tauri::command]
pub async fn soulseek_download_settings(app: AppHandle) -> Result<DownloadSettings, String> {
    Ok(DownloadSettings {
        download_directory: settings::load_soulseek_download_directory(&app)?,
    })
}

#[tauri::command]
pub async fn soulseek_save_download_directory(
    app: AppHandle,
    directory: Option<String>,
) -> Result<DownloadSettings, String> {
    let directory = directory
        .map(|path| canonical_directory(Path::new(&path)).map(|p| p.to_string_lossy().into_owned()))
        .transpose()?;
    settings::save_soulseek_download_directory(&app, directory.clone())?;
    Ok(DownloadSettings {
        download_directory: directory,
    })
}

pub fn preferred_directory(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    Ok(settings::load_soulseek_download_directory(app)?.map(PathBuf::from))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("Elige una carpeta de descarga con ruta absoluta.".into());
    }
    let path = path.canonicalize().map_err(message)?;
    if !path.is_dir() {
        return Err("El destino de descarga debe ser una carpeta.".into());
    }
    Ok(path)
}

fn remote_name(path: &str) -> Result<&str, String> {
    let name = path
        .trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or_default();
    if name.is_empty() || name == "." || name == ".." || name.contains([':', '\0']) {
        return Err("El nombre remoto no es válido para una descarga local.".into());
    }
    Ok(name)
}

pub fn file_destination(
    preferred: Option<&Path>,
    remote_path: &str,
    selected: Option<&Path>,
) -> Result<PathBuf, String> {
    let destination = if let Some(selected) = selected {
        if !selected.is_absolute() {
            return Err("Elige un destino absoluto.".into());
        }
        let root = canonical_directory(selected.parent().ok_or("Destino inválido.")?)?;
        root.join(selected.file_name().ok_or("Falta el nombre de archivo.")?)
    } else {
        let root = preferred.ok_or("Elige una carpeta de descarga antes de continuar.")?;
        root.join(remote_name(remote_path)?)
    };
    let root = selected
        .and_then(|_| destination.parent())
        .or(preferred)
        .ok_or("Destino inválido.")?;
    partial::validate_destination(root, &destination)?;
    Ok(destination)
}

pub fn folder_destination(
    preferred: Option<&Path>,
    remote_folder: &str,
    selected: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(selected) = selected {
        return canonical_directory(selected);
    }
    let root = preferred.ok_or("Elige una carpeta de descarga antes de continuar.")?;
    let destination = root.join(remote_name(remote_folder)?);
    partial::validate_destination(root, &destination)?;
    match fs::create_dir(&destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.to_string()),
    }
    partial::validate_destination(root, &destination)?;
    if !destination.is_dir() {
        return Err("El destino de la carpeta ya existe como archivo.".into());
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_downloads_stay_inside_the_selected_directory_and_keep_album_names() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        assert_eq!(
            file_destination(Some(&root), "Music\\Album\\track.flac", None).unwrap(),
            root.join("track.flac")
        );
        let album = folder_destination(Some(&root), "Music\\Album", None).unwrap();
        assert_eq!(album, root.join("Album"));
        assert!(album.is_dir());
        assert_eq!(
            folder_destination(Some(&root), "Music\\Album", None).unwrap(),
            album
        );
        assert!(file_destination(Some(&root), "Music\\..", None).is_err());
        assert!(folder_destination(Some(&root), "Music\\..", None).is_err());
        assert!(file_destination(None, "Music\\track.flac", None).is_err());
    }

    #[test]
    fn explicit_destinations_override_the_preference_without_changing_it() {
        let preferred = tempfile::tempdir().unwrap();
        let selected = tempfile::tempdir().unwrap();
        let preferred = preferred.path().canonicalize().unwrap();
        let selected = selected.path().canonicalize().unwrap();
        assert_eq!(
            file_destination(
                Some(&preferred),
                "Music\\track.flac",
                Some(&selected.join("custom.flac"))
            )
            .unwrap(),
            selected.join("custom.flac")
        );
        assert_eq!(
            folder_destination(Some(&preferred), "Music\\Album", Some(&selected)).unwrap(),
            selected
        );
        assert!(!preferred.join("Album").exists());
    }

    #[cfg(unix)]
    #[test]
    fn default_downloads_reject_symlinked_albums() {
        let directory = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("Album")).unwrap();
        assert!(folder_destination(Some(&root), "Music\\Album", None).is_err());
        assert!(fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}
