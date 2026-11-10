use super::ExportPlan;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Catalog {
    pub artists: BTreeMap<String, u32>,
    pub albums: BTreeMap<(String, String), u32>,
    pub genres: BTreeMap<String, u32>,
    pub labels: BTreeMap<String, u32>,
    pub keys: BTreeMap<String, u32>,
}

fn names(values: impl Iterator<Item = String>) -> BTreeMap<String, u32> {
    values
        .filter(|v| !v.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .enumerate()
        .map(|(i, name)| (name, i as u32 + 1))
        .collect()
}

impl Catalog {
    pub fn new(plan: &ExportPlan) -> Self {
        Self {
            artists: names(plan.tracks.iter().map(|t| t.artist.clone())),
            albums: plan
                .tracks
                .iter()
                .filter(|t| !t.album.is_empty())
                .map(|t| (t.artist.clone(), t.album.clone()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .enumerate()
                .map(|(i, album)| (album, i as u32 + 1))
                .collect(),
            genres: names(plan.tracks.iter().map(|t| t.genre.clone())),
            labels: names(plan.tracks.iter().map(|t| t.label.clone())),
            keys: names(plan.tracks.iter().map(|t| t.key.clone())),
        }
    }
}
