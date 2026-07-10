use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum RekordboxParseError {
    #[error("failed to read XML: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse XML: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("failed to decode XML attribute: {0}")]
    Attribute(String),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Product {
    pub name: Option<String>,
    pub version: Option<String>,
    pub company: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Track {
    pub track_id: String,
    pub name: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub kind: Option<String>,
    pub location: Option<String>,
    pub file_path: Option<PathBuf>,
    pub size: Option<u64>,
    pub total_time: Option<u64>,
    pub sample_rate: Option<u32>,
    pub bitrate: Option<u32>,
    pub attributes: BTreeMap<String, String>,
}

impl Track {
    pub fn extension_lower(&self) -> Option<String> {
        self.file_path
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlaylistNode {
    pub name: String,
    pub node_type: Option<String>,
    pub key_type: Option<String>,
    pub entries: Option<usize>,
    pub count: Option<usize>,
    pub track_keys: Vec<String>,
    pub children: Vec<PlaylistNode>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaylistSummary {
    pub path: String,
    pub name: String,
    pub node_type: Option<String>,
    pub track_count: usize,
    pub child_count: usize,
    pub track_keys: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RekordboxLibrary {
    pub product: Option<Product>,
    pub collection_entries_declared: Option<usize>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<PlaylistNode>,
}

impl RekordboxLibrary {
    pub fn track_by_id(&self) -> BTreeMap<String, Track> {
        self.tracks
            .iter()
            .cloned()
            .map(|track| (track.track_id.clone(), track))
            .collect()
    }

    pub fn format_counts(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();

        for track in &self.tracks {
            let kind = track.kind.as_deref().unwrap_or("Unknown").to_string();
            *counts.entry(kind).or_insert(0) += 1;
        }

        counts
    }

    pub fn playlists_flat(&self) -> Vec<PlaylistSummary> {
        let mut summaries = Vec::new();

        for node in &self.playlists {
            flatten_playlist_node(node, "", &mut summaries);
        }

        summaries
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Collection,
    Playlists,
}

pub fn parse_rekordbox_xml_file(
    path: impl AsRef<Path>,
) -> Result<RekordboxLibrary, RekordboxParseError> {
    let xml = fs::read_to_string(path)?;
    parse_rekordbox_xml(&xml)
}

pub fn parse_rekordbox_xml(xml: &str) -> Result<RekordboxLibrary, RekordboxParseError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut library = RekordboxLibrary::default();
    let mut section = None;
    let mut playlist_stack: Vec<PlaylistNode> = Vec::new();

    loop {
        match reader.read_event()? {
            Event::Start(event) => {
                let name = element_name(event.name().as_ref());

                match name.as_str() {
                    "PRODUCT" => {
                        library.product = Some(product_from_attrs(&reader, &event)?);
                    }
                    "COLLECTION" => {
                        section = Some(Section::Collection);
                        let attrs = attributes_to_map(&reader, &event)?;
                        library.collection_entries_declared =
                            attrs.get("Entries").and_then(|value| value.parse().ok());
                    }
                    "PLAYLISTS" => {
                        section = Some(Section::Playlists);
                    }
                    "TRACK" if section == Some(Section::Collection) => {
                        if let Some(track) = track_from_attrs(&reader, &event)? {
                            library.tracks.push(track);
                        }
                    }
                    "TRACK" if section == Some(Section::Playlists) => {
                        append_playlist_track_key(&reader, &event, &mut playlist_stack)?;
                    }
                    "NODE" if section == Some(Section::Playlists) => {
                        playlist_stack.push(playlist_node_from_attrs(&reader, &event)?);
                    }
                    _ => {}
                }
            }
            Event::Empty(event) => {
                let name = element_name(event.name().as_ref());

                match name.as_str() {
                    "PRODUCT" => {
                        library.product = Some(product_from_attrs(&reader, &event)?);
                    }
                    "TRACK" if section == Some(Section::Collection) => {
                        if let Some(track) = track_from_attrs(&reader, &event)? {
                            library.tracks.push(track);
                        }
                    }
                    "TRACK" if section == Some(Section::Playlists) => {
                        append_playlist_track_key(&reader, &event, &mut playlist_stack)?;
                    }
                    "NODE" if section == Some(Section::Playlists) => {
                        let node = playlist_node_from_attrs(&reader, &event)?;
                        attach_playlist_node(node, &mut playlist_stack, &mut library.playlists);
                    }
                    _ => {}
                }
            }
            Event::End(event) => {
                let name = element_name(event.name().as_ref());

                match name.as_str() {
                    "COLLECTION" | "PLAYLISTS" => {
                        section = None;
                    }
                    "NODE" if section == Some(Section::Playlists) => {
                        if let Some(node) = playlist_stack.pop() {
                            attach_playlist_node(node, &mut playlist_stack, &mut library.playlists);
                        }
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(library)
}

fn product_from_attrs(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<Product, RekordboxParseError> {
    let attrs = attributes_to_map(reader, event)?;

    Ok(Product {
        name: attrs.get("Name").cloned(),
        version: attrs.get("Version").cloned(),
        company: attrs.get("Company").cloned(),
    })
}

fn track_from_attrs(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<Option<Track>, RekordboxParseError> {
    let attrs = attributes_to_map(reader, event)?;
    let Some(track_id) = attrs.get("TrackID").cloned() else {
        return Ok(None);
    };
    let location = attrs.get("Location").cloned();
    let file_path = location.as_deref().and_then(location_to_path);

    Ok(Some(Track {
        track_id,
        name: attrs.get("Name").cloned(),
        artist: attrs.get("Artist").cloned(),
        album: attrs.get("Album").cloned(),
        kind: attrs.get("Kind").cloned(),
        location,
        file_path,
        size: attrs.get("Size").and_then(|value| value.parse().ok()),
        total_time: attrs.get("TotalTime").and_then(|value| value.parse().ok()),
        sample_rate: attrs.get("SampleRate").and_then(|value| value.parse().ok()),
        bitrate: attrs.get("BitRate").and_then(|value| value.parse().ok()),
        attributes: attrs,
    }))
}

fn playlist_node_from_attrs(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<PlaylistNode, RekordboxParseError> {
    let attrs = attributes_to_map(reader, event)?;

    Ok(PlaylistNode {
        name: attrs.get("Name").cloned().unwrap_or_default(),
        node_type: attrs.get("Type").cloned(),
        key_type: attrs.get("KeyType").cloned(),
        entries: attrs.get("Entries").and_then(|value| value.parse().ok()),
        count: attrs.get("Count").and_then(|value| value.parse().ok()),
        track_keys: Vec::new(),
        children: Vec::new(),
    })
}

fn append_playlist_track_key(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
    playlist_stack: &mut [PlaylistNode],
) -> Result<(), RekordboxParseError> {
    let attrs = attributes_to_map(reader, event)?;

    if let (Some(current), Some(key)) = (playlist_stack.last_mut(), attrs.get("Key")) {
        current.track_keys.push(key.clone());
    }

    Ok(())
}

fn attach_playlist_node(
    node: PlaylistNode,
    playlist_stack: &mut [PlaylistNode],
    roots: &mut Vec<PlaylistNode>,
) {
    if let Some(parent) = playlist_stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn attributes_to_map(
    reader: &Reader<&[u8]>,
    event: &BytesStart<'_>,
) -> Result<BTreeMap<String, String>, RekordboxParseError> {
    let mut attrs = BTreeMap::new();

    for attr in event.attributes() {
        let attr = attr.map_err(|error| RekordboxParseError::Attribute(error.to_string()))?;
        let key = element_name(attr.key.as_ref());
        let value = attr
            .decode_and_unescape_value(reader.decoder())
            .map_err(|error| RekordboxParseError::Attribute(error.to_string()))?;
        attrs.insert(key, value.into_owned());
    }

    Ok(attrs)
}

fn element_name(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}

fn location_to_path(location: &str) -> Option<PathBuf> {
    if let Ok(url) = Url::parse(location) {
        if url.scheme() == "file" {
            return url.to_file_path().ok();
        }
    }

    let without_prefix = location
        .strip_prefix("file://localhost")
        .or_else(|| location.strip_prefix("file://"))?;

    Some(PathBuf::from(without_prefix))
}

fn flatten_playlist_node(
    node: &PlaylistNode,
    parent_path: &str,
    summaries: &mut Vec<PlaylistSummary>,
) {
    let path = if parent_path.is_empty() {
        node.name.clone()
    } else {
        format!("{}/{}", parent_path, node.name)
    };

    summaries.push(PlaylistSummary {
        path: path.clone(),
        name: node.name.clone(),
        node_type: node.node_type.clone(),
        track_count: node.track_keys.len(),
        child_count: node.children.len(),
        track_keys: node.track_keys.clone(),
    });

    for child in &node.children {
        flatten_playlist_node(child, &path, summaries);
    }
}
