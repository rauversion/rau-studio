//! Classic player menu records, encoded from fields rather than captured rows.
//! IDs, selector codes and field widths were observed in the user's read-only
//! reference export. Visibility/order below are Rau's own defaults. These tables
//! are navigation settings, not played-track history (which uses tables 11/12).

use super::{invalid, Result};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BrowseColumn {
    pub id: u16,
    pub selector: u16,
    pub name: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BrowseCategory {
    pub column_id: u16,
    pub id: u16,
    pub kind: u8,
    pub flags: u8,
    pub position: u16,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BrowseSort {
    pub column_id: u16,
    pub id: u16,
    pub flags: u8,
    pub position: u8,
}

pub fn columns() -> Vec<BrowseColumn> {
    // Protocol vocabulary; names carry the player's translation delimiters.
    [
        (1, 128, "GENRE"),
        (2, 129, "ARTIST"),
        (3, 130, "ALBUM"),
        (4, 131, "TRACK"),
        (5, 133, "BPM"),
        (6, 134, "RATING"),
        (7, 135, "YEAR"),
        (8, 136, "REMIXER"),
        (9, 137, "LABEL"),
        (10, 138, "ORIGINAL ARTIST"),
        (11, 139, "KEY"),
        (12, 141, "CUE"),
        (13, 142, "COLOR"),
        (14, 146, "TIME"),
        (15, 147, "BITRATE"),
        (16, 148, "FILE NAME"),
        (17, 132, "PLAYLIST"),
        (18, 152, "HOT CUE BANK"),
        (19, 149, "HISTORY"),
        (20, 145, "SEARCH"),
        (21, 150, "COMMENTS"),
        (22, 140, "DATE ADDED"),
        (23, 151, "DJ PLAY COUNT"),
        (24, 144, "FOLDER"),
        (25, 161, "DEFAULT"),
        (26, 162, "ALPHABET"),
        (27, 170, "MATCHING"),
    ]
    .into_iter()
    .map(|(id, selector, name)| BrowseColumn {
        id,
        selector,
        name: format!("\u{fffa}{name}\u{fffb}"),
    })
    .collect()
}

pub fn categories() -> Vec<BrowseCategory> {
    // Each tuple is (category ID, column ID, category kind). The kind's numeric
    // values are protocol selectors; their full semantics remain uncharacterized.
    // Keep unsupported features in the vocabulary, but hide them in our menu.
    let visible = [17, 4, 2, 3, 1, 11, 24, 20]; // Playlists first, then tracks.
    [
        (1, 1, 99),
        (2, 2, 2),
        (3, 3, 3),
        (4, 4, 1),
        (5, 17, 99),
        (6, 5, 5),
        (7, 6, 99),
        (8, 7, 99),
        (9, 8, 99),
        (10, 9, 99),
        (11, 10, 99),
        (12, 11, 99),
        (15, 13, 99),
        (17, 24, 99),
        (18, 20, 99),
        (19, 14, 4),
        (20, 15, 6),
        (21, 16, 99),
        (22, 19, 99),
        (23, 18, 99),
        (26, 27, 99),
        (27, 22, 99),
    ]
    .into_iter()
    .map(|(id, column_id, kind)| {
        let position = visible.iter().position(|c| *c == column_id);
        BrowseCategory {
            column_id,
            id,
            kind,
            flags: u8::from(position.is_none()),
            position: position.unwrap_or(0) as u16,
        }
    })
    .collect()
}

pub fn sorts() -> Vec<BrowseSort> {
    let visible = [25, 26, 2, 5]; // Default, alphabet, artist, BPM.
    [
        (0, 25),
        (1, 26),
        (2, 2),
        (3, 3),
        (4, 5),
        (5, 6),
        (6, 1),
        (7, 21),
        (8, 14),
        (9, 8),
        (10, 9),
        (11, 10),
        (12, 11),
        (13, 15),
        (15, 13),
        (16, 23),
        (17, 22),
    ]
    .into_iter()
    .map(|(id, column_id)| {
        let position = visible.iter().position(|c| *c == column_id);
        BrowseSort {
            column_id,
            id,
            flags: u8::from(position.is_none()),
            position: position.map(|p| p + 1).unwrap_or(0) as u8,
        }
    })
    .collect()
}

pub fn validate_references(
    columns: &[BrowseColumn],
    categories: &[BrowseCategory],
    sorts: &[BrowseSort],
) -> Result<()> {
    let ids: BTreeSet<_> = columns.iter().map(|c| c.id).collect();
    if ids.len() != columns.len()
        || ids.contains(&0)
        || categories
            .iter()
            .map(|c| c.id)
            .collect::<BTreeSet<_>>()
            .len()
            != categories.len()
        || sorts.iter().map(|s| s.id).collect::<BTreeSet<_>>().len() != sorts.len()
        || categories.iter().any(|c| !ids.contains(&c.column_id))
        || sorts.iter().any(|s| !ids.contains(&s.column_id))
    {
        return Err(invalid("invalid browse setting identifiers/references"));
    }
    Ok(())
}

pub fn validate_defaults(
    actual_columns: &[BrowseColumn],
    actual_categories: &[BrowseCategory],
    actual_sorts: &[BrowseSort],
) -> Result<()> {
    if actual_columns != columns() || actual_categories != categories() || actual_sorts != sorts() {
        return Err(invalid(
            "read-back browse settings differ from the export profile",
        ));
    }
    Ok(())
}
