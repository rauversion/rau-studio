//! Plan artist folders and readable filenames before database/analysis encoding.

use super::{anlz, invalid, preserves_source_audio, AudioFormat, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

const PREFIX: &str = "/Contents/";
const MAX_PATH_BYTES: usize = 240;
const MAX_ARTIST_BYTES: usize = 80;

/// Conservative collision key for case-insensitive and Unicode-normalizing
/// filesystems. Keep the original spelling in the actual device path.
pub(super) fn collision_key(value: &str) -> String {
    value.nfkc().flat_map(char::to_uppercase).nfkc().collect()
}

fn reserved_name(stem: &str) -> bool {
    let base = stem
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end()
        .to_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || base
            .strip_prefix("COM")
            .or_else(|| base.strip_prefix("LPT"))
            .is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

fn component(value: &str, fallback: &str, max_bytes: usize, suffix: &str) -> String {
    let mut clean: String = value
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    clean = clean.trim_matches([' ', '.']).to_owned();
    if clean.is_empty() {
        clean = fallback.into();
    }
    if reserved_name(&clean) {
        clean.insert(0, '_');
    }
    let mut end = clean.len().min(max_bytes - suffix.len());
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean.truncate(end);
    format!("{}{suffix}", clean.trim_end_matches([' ', '.']))
}

fn suffix(number: u32) -> String {
    if number == 1 {
        String::new()
    } else {
        format!(" ({number})")
    }
}

#[derive(Default)]
pub(super) struct Allocator {
    artists: BTreeMap<String, String>,
    directories: BTreeSet<String>,
    audio_paths: BTreeSet<String>,
    analysis_paths: BTreeSet<String>,
    next_suffix: BTreeMap<String, u32>,
}

impl Allocator {
    fn artist_directory(&mut self, artist: &str) -> Result<String> {
        let artist = if artist.trim().is_empty() {
            "Artista desconocido"
        } else {
            artist.trim()
        };
        let key = collision_key(artist);
        if let Some(directory) = self.artists.get(&key) {
            return Ok(directory.clone());
        }
        for number in 1..=10_000 {
            let directory = component(
                artist,
                "Artista desconocido",
                MAX_ARTIST_BYTES,
                &suffix(number),
            );
            if self.directories.insert(collision_key(&directory)) {
                self.artists.insert(key, directory.clone());
                return Ok(directory);
            }
        }
        Err(invalid(format!(
            "could not assign an artist folder for {artist}"
        )))
    }

    pub(super) fn assign(&mut self, source: &Path, title: &str, artist: &str) -> Result<String> {
        let directory = self.artist_directory(artist)?;
        let extension = if preserves_source_audio(source) {
            source.extension().and_then(|s| s.to_str()).unwrap()
        } else {
            "aiff"
        };
        let stem = source
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(title);
        // Bound the complete path, including the artist and filename extension.
        let max_stem_bytes = MAX_PATH_BYTES - PREFIX.len() - directory.len() - 2 - extension.len();
        let base = component(stem, "Track", max_stem_bytes, "");
        let counter_key = collision_key(&format!("{directory}/{base}.{extension}"));
        let first = *self.next_suffix.get(&counter_key).unwrap_or(&1);
        for number in first..first + 10_000 {
            let name = component(stem, "Track", max_stem_bytes, &suffix(number));
            let path = format!("{PREFIX}{directory}/{name}.{extension}");
            let path_key = collision_key(&path);
            let analysis_path = anlz::path_for_audio(&path)?;
            // A suffix resolves both filesystem and analysis-hash collisions.
            if !self.audio_paths.contains(&path_key)
                && !self.analysis_paths.contains(&analysis_path)
            {
                self.audio_paths.insert(path_key);
                self.analysis_paths.insert(analysis_path);
                self.next_suffix.insert(counter_key, number + 1);
                return Ok(path);
            }
        }
        Err(invalid(format!(
            "could not assign a unique export path for {title}"
        )))
    }
}

pub(super) fn valid(path: &str, id: u32) -> bool {
    // Retain support for plans produced by the first prototype.
    if path == format!("/Contents/Rau/Prototype/{id:08}.wav") {
        return true;
    }
    if path.len() > MAX_PATH_BYTES {
        return false;
    }
    let Some(relative) = path.strip_prefix(PREFIX) else {
        return false;
    };
    let Some((directory, name)) = relative.split_once('/') else {
        return false;
    };
    let Some((stem, _)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        AudioFormat::from_path(Path::new(name)),
        Some(AudioFormat::Mp3 | AudioFormat::Aiff)
    ) && !directory.is_empty()
        && !stem.is_empty()
        && component(directory, "Artista desconocido", MAX_ARTIST_BYTES, "") == directory
        && component(stem, "Track", MAX_PATH_BYTES, "") == stem
}
