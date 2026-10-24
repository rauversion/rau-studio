//! Independently authored DeviceSQL encoder/inspector from public wire-format
//! descriptions (Deep Symmetry and rbxport format notes). No upstream source,
//! captured default-table blobs, or GPL libraries are incorporated.

pub use super::browse::{BrowseCategory, BrowseColumn, BrowseSort};
use super::{
    analysis, anlz, browse, catalog::Catalog, invalid, paths, AudioFormat, AudioInfo, ExportPlan,
    LibraryIdentity, Result,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const PAGE: usize = 4096;
const HEADER: usize = 40;
const TABLES: usize = 20;
const INDEX_ENTRIES_START: usize = 60;
const INDEX_CAPACITY: usize = 1004;
const INDEX_ENTRIES_END: usize = INDEX_ENTRIES_START + INDEX_CAPACITY * 4;
// Bound each transaction to the older counted-row representation.
const ROWS_PER_PAGE: usize = 240;
// Opaque CONTENTLINK profile observed on all 1,231 native reference tracks.
// A controlled Rekordbox 7.2.3 comparison (identical audio and DAT/EXT/2EX)
// removed the legacy-analysis `?` when this word replaced zero. Individual
// bit meanings remain unknown; this is not a claim of phrase/vocal analysis.
// Our export pipeline writes and verifies all three companions before the DB.
pub const GENERATED_ANALYSIS_CONTENT_LINK: u32 = 0x000c_0700;

fn put16(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn word(bytes: &[u8], at: usize) -> Result<u16> {
    let b = bytes
        .get(at..at + 2)
        .ok_or_else(|| invalid("truncated DeviceSQL word"))?;
    Ok(u16::from_le_bytes(b.try_into().unwrap()))
}
fn number(bytes: &[u8], at: usize) -> Result<u32> {
    let b = bytes
        .get(at..at + 4)
        .ok_or_else(|| invalid("truncated DeviceSQL number"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}
fn align(n: usize) -> usize {
    (n + 3) & !3
}

fn string(row: &mut Vec<u8>, text: &str) -> Result<u16> {
    if text.contains('\0') {
        return Err(invalid("NUL is not supported in device metadata"));
    }
    if text.is_ascii() && text.len() < 126 {
        let offset = u16::try_from(row.len()).map_err(|_| invalid("row too large"))?;
        row.push(((text.len() + 1) * 2 + 1) as u8);
        row.extend_from_slice(text.as_bytes());
        return Ok(offset);
    }
    row.resize(align(row.len()), 0);
    let offset = u16::try_from(row.len()).map_err(|_| invalid("row too large"))?;
    let payload = if text.is_ascii() {
        text.as_bytes().to_vec()
    } else {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    };
    let length =
        u16::try_from(payload.len() + 4).map_err(|_| invalid("DeviceSQL string too long"))?;
    row.push(if text.is_ascii() { 0x40 } else { 0x90 });
    row.extend_from_slice(&length.to_le_bytes());
    row.push(0);
    row.extend(payload);
    Ok(offset)
}

fn decode_string(row: &[u8], at: usize) -> Result<String> {
    let kind = *row
        .get(at)
        .ok_or_else(|| invalid("string offset outside row"))?;
    let (start, length) = if kind & 1 != 0 {
        (
            at + 1,
            (kind as usize / 2)
                .checked_sub(1)
                .ok_or_else(|| invalid("invalid short string length"))?,
        )
    } else {
        (
            at + 4,
            usize::from(word(row, at + 1)?)
                .checked_sub(4)
                .ok_or_else(|| invalid("invalid long string length"))?,
        )
    };
    let bytes = row
        .get(start..start + length)
        .ok_or_else(|| invalid("string crosses row boundary"))?;
    if kind == 0x90 {
        if length % 2 != 0 {
            return Err(invalid("odd UTF-16 byte length"));
        }
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| invalid("invalid UTF-16 string"))
    } else if kind & 1 != 0 || kind == 0x40 {
        if !bytes.is_ascii() {
            return Err(invalid("non-ASCII bytes in ASCII string"));
        }
        Ok(String::from_utf8(bytes.to_vec()).unwrap())
    } else {
        Err(invalid(format!("unsupported string encoding {kind:#x}")))
    }
}

pub fn validate_plan(plan: &ExportPlan) -> Result<()> {
    let included: BTreeSet<_> = plan.tracks.iter().map(|track| &track.source_key).collect();
    let omitted: BTreeSet<_> = plan
        .omitted_tracks
        .iter()
        .map(|track| &track.source_key)
        .collect();
    if included.len() + plan.omitted_tracks.len() > 10_000
        || omitted.len() != plan.omitted_tracks.len()
        || !included.is_disjoint(&omitted)
    {
        return Err(invalid("invalid or overlapping omitted source tracks"));
    }
    if plan.tracks.is_empty()
        || plan.tracks.len() > 10_000
        || plan.playlists.is_empty()
        || plan.playlists.len() > 10_000
    {
        return Err(invalid(
            "prototype requires 1–10000 tracks and playlist nodes",
        ));
    }
    let mut tracks = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut analysis_paths = BTreeSet::new();
    for t in &plan.tracks {
        if t.id == 0 || !tracks.insert(t.id) || t.rating > 5 {
            return Err(invalid("invalid or duplicate track ID/rating"));
        }
        if !paths::valid(&t.device_path, t.id)
            || !paths.insert(paths::collision_key(&t.device_path))
        {
            return Err(invalid("unexpected prototype destination path"));
        }
        if !t.copy_source
            && AudioFormat::from_path(Path::new(&t.device_path)) == Some(AudioFormat::Mp3)
        {
            return Err(invalid("MP3 export requires copying the source"));
        }
        analysis::validate(&t.analysis)?;
        if !analysis_paths.insert(anlz::path_for_track(t)?) {
            return Err(invalid("analysis path hash collision; this prototype refuses to overwrite a track's analysis"));
        }
        for field in [
            &t.title, &t.artist, &t.album, &t.genre, &t.label, &t.key, &t.comment,
        ] {
            if field.len() > 2048 || field.contains('\0') {
                return Err(invalid("metadata field is too long or contains NUL"));
            }
        }
    }
    let mut playlists = BTreeMap::new();
    let mut sibling_positions = BTreeMap::<u32, BTreeSet<u32>>::new();
    for p in &plan.playlists {
        if p.id == 0 || playlists.insert(p.id, p).is_some() {
            return Err(invalid("duplicate or zero playlist ID"));
        }
        if p.name.len() > 2048 || p.name.contains('\0') {
            return Err(invalid("invalid playlist name"));
        }
        if p.is_folder && !p.tracks.is_empty() {
            return Err(invalid("folder has track entries"));
        }
        if p.tracks.iter().any(|id| !tracks.contains(id)) {
            return Err(invalid("playlist references a missing track"));
        }
        if !sibling_positions
            .entry(p.parent_id)
            .or_default()
            .insert(p.position)
        {
            return Err(invalid("duplicate playlist position"));
        }
    }
    for positions in sibling_positions.values() {
        if positions.iter().copied().ne(0..positions.len() as u32) {
            return Err(invalid("playlist positions are not contiguous"));
        }
    }
    for p in &plan.playlists {
        let mut seen = BTreeSet::from([p.id]);
        let mut parent = p.parent_id;
        while parent != 0 {
            if !seen.insert(parent) {
                return Err(invalid("playlist parent cycle"));
            }
            let node = playlists
                .get(&parent)
                .ok_or_else(|| invalid("missing playlist parent"))?;
            if !node.is_folder {
                return Err(invalid("playlist parent is not a folder"));
            }
            parent = node.parent_id;
        }
    }
    Ok(())
}

/// Encode the classic library for Rau's complete DAT/EXT/2EX analysis profile.
/// Callers must prepare the analysis companions before publishing this database.
pub fn encode(plan: &ExportPlan, audio: &[AudioInfo], created_date: &str) -> Result<Vec<u8>> {
    encode_with_identity(
        plan,
        audio,
        created_date,
        &LibraryIdentity {
            name: "Rau Studio prototype".into(),
            master_database_id: 0,
        },
    )
}

pub fn encode_with_identity(
    plan: &ExportPlan,
    audio: &[AudioInfo],
    created_date: &str,
    identity: &LibraryIdentity,
) -> Result<Vec<u8>> {
    validate_plan(plan)?;
    if audio.len() != plan.tracks.len()
        || created_date.len() != 10
        || chrono::NaiveDate::parse_from_str(created_date, "%Y-%m-%d").is_err()
    {
        return Err(invalid("audio metadata count or export date is invalid"));
    }
    let Catalog {
        artists,
        genres,
        labels,
        keys,
        albums,
    } = Catalog::new(plan);
    let mut tables: Vec<Vec<Vec<u8>>> = vec![Vec::new(); TABLES];
    let id_for = |map: &BTreeMap<String, u32>, name: &String| map.get(name).copied().unwrap_or(0);
    for (t, a) in plan.tracks.iter().zip(audio) {
        if AudioFormat::from_path(Path::new(&t.device_path)) != Some(a.format)
            || !(1..=384_000).contains(&a.sample_rate)
            || !matches!(a.channels, 1 | 2)
            || (a.format == AudioFormat::Mp3 && a.bit_depth != 0)
            || (a.format != AudioFormat::Mp3 && !matches!(a.bit_depth, 16 | 24))
            || a.frames == 0
            || a.size_bytes < 44
            || (a.format != AudioFormat::Mp3
                && (a.frames
                    > u64::from(a.size_bytes)
                        / (u64::from(a.channels) * u64::from(a.bit_depth) / 8)
                    || a.bitrate_kbps
                        != a.sample_rate * u32::from(a.channels) * u32::from(a.bit_depth) / 1000))
            || a.frames.div_ceil(u64::from(a.sample_rate)) > u16::MAX as u64
        {
            return Err(invalid("invalid audio duration"));
        }
        let mut row = vec![0; 136];
        put16(&mut row, 0, 0x24);
        for (at, value) in [
            (4, GENERATED_ANALYSIS_CONTENT_LINK),
            (8, a.sample_rate),
            (16, a.size_bytes),
            (
                20,
                if identity.master_database_id == 0 {
                    0
                } else {
                    t.id
                },
            ),
            (24, identity.master_database_id),
            (32, id_for(&keys, &t.key)),
            (40, id_for(&labels, &t.label)),
            (48, a.bitrate_kbps),
            (52, t.track_number),
            (56, t.bpm_x100),
            (60, id_for(&genres, &t.genre)),
            (
                64,
                albums
                    .get(&(t.artist.clone(), t.album.clone()))
                    .copied()
                    .unwrap_or(0),
            ),
            (68, id_for(&artists, &t.artist)),
            (72, t.id),
        ] {
            put32(&mut row, at, value);
        }
        put16(&mut row, 80, t.year);
        put16(&mut row, 82, a.bit_depth);
        put16(
            &mut row,
            84,
            a.frames.div_ceil(u64::from(a.sample_rate)) as u16,
        );
        put16(&mut row, 86, 0x29); // Documented required format word; semantics not fully known.
        row[89] = t.rating;
        put16(&mut row, 90, a.format.device_code());
        put16(&mut row, 92, 3);
        let filename = t.device_path.rsplit('/').next().unwrap();
        let analysis_path = anlz::path_for_track(t)?;
        for slot in 0..21 {
            let text = match slot {
                7 if t.analysis.cues.iter().any(|c| c.hot_cue > 0) => "ON",
                10 => created_date,
                14 => &analysis_path,
                15 => created_date,
                16 => &t.comment,
                17 => &t.title,
                19 => filename,
                20 => &t.device_path,
                _ => "",
            };
            let offset = string(&mut row, text)?;
            put16(&mut row, 94 + 2 * slot, offset);
        }
        tables[0].push(row);
    }
    for (kind, names) in [(1, &genres), (4, &labels), (5, &keys)] {
        for (name, id) in names {
            let mut row = id.to_le_bytes().to_vec();
            if kind == 5 {
                row.extend_from_slice(&id.to_le_bytes());
            }
            string(&mut row, name)?;
            tables[kind].push(row);
        }
    }
    for (name, id) in &artists {
        let mut row = vec![0; 12];
        put16(&mut row, 0, 0x64); // Always use a 16-bit name pointer.
        put32(&mut row, 4, *id);
        put16(&mut row, 8, 3);
        let offset = string(&mut row, name)?;
        put16(&mut row, 10, offset);
        tables[2].push(row);
    }
    for ((artist, name), id) in &albums {
        let mut row = vec![0; 24];
        put16(&mut row, 0, 0x84);
        put32(&mut row, 8, id_for(&artists, artist));
        put32(&mut row, 12, *id);
        put16(&mut row, 20, 3);
        let offset = string(&mut row, name)?;
        put16(&mut row, 22, offset);
        tables[3].push(row);
    }
    for (index, name) in [
        "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple",
    ]
    .iter()
    .enumerate()
    {
        let mut row = vec![0; 8];
        row[4] = (index + 1) as u8;
        put16(&mut row, 5, (index + 1) as u16);
        string(&mut row, name)?;
        tables[6].push(row);
    }
    for p in &plan.playlists {
        let mut row = vec![0; 20];
        for (at, value) in [
            (0, p.parent_id),
            (8, p.position),
            (12, p.id),
            (16, u32::from(p.is_folder)),
        ] {
            put32(&mut row, at, value);
        }
        string(&mut row, &p.name)?;
        tables[7].push(row);
        for (index, track) in p.tracks.iter().enumerate() {
            let mut row = Vec::new();
            for value in [index as u32 + 1, *track, p.id] {
                row.extend_from_slice(&value.to_le_bytes());
            }
            tables[8].push(row);
        }
    }
    for column in browse::columns() {
        let mut row = vec![0; 4];
        put16(&mut row, 0, column.id);
        put16(&mut row, 2, column.selector);
        string(&mut row, &column.name)?;
        tables[16].push(row);
    }
    for category in browse::categories() {
        let mut row = vec![0; 8];
        put16(&mut row, 0, category.column_id);
        put16(&mut row, 2, category.id);
        row[4] = category.kind;
        row[5] = category.flags;
        put16(&mut row, 6, category.position);
        tables[17].push(row);
    }
    for sort in browse::sorts() {
        let mut row = vec![0; 8];
        put16(&mut row, 0, sort.column_id);
        put16(&mut row, 2, sort.id);
        row[4] = sort.flags;
        row[5] = sort.position;
        tables[18].push(row);
    }
    let mut property = vec![0; 12];
    put16(&mut property, 0, 0x280);
    put32(&mut property, 4, plan.tracks.len() as u32);
    string(&mut property, created_date)?;
    property.extend_from_slice(&[0x19, 0x1e]);
    string(&mut property, "1000")?;
    string(&mut property, &identity.name)?;
    property.extend_from_slice(&[0; 8]);
    tables[19].push(property);
    build_pages(tables)
}

fn pack(rows: Vec<Vec<u8>>) -> Result<Vec<Vec<Vec<u8>>>> {
    let mut pages = vec![Vec::new()];
    let mut used = 0;
    for row in rows {
        let length = align(row.len());
        if length + HEADER + 6 > PAGE {
            return Err(invalid("metadata row does not fit one DeviceSQL page"));
        }
        let count = pages.last().unwrap().len() + 1;
        if count > ROWS_PER_PAGE
            || HEADER + used + length + count * 2 + count.div_ceil(16) * 4 > PAGE
        {
            pages.push(Vec::new());
            used = 0;
        }
        used += length;
        pages.last_mut().unwrap().push(row);
    }
    Ok(pages)
}

fn build_pages(tables: Vec<Vec<Vec<u8>>>) -> Result<Vec<u8>> {
    let packed: Vec<_> = tables.into_iter().map(pack).collect::<Result<_>>()?;
    let mut indices: Vec<Vec<usize>> = (0..TABLES)
        .map(|t| {
            if packed[t].iter().all(Vec::is_empty) {
                Vec::new()
            } else {
                vec![2 + t * 2]
            }
        })
        .collect();
    let mut next = 1 + TABLES * 2;
    for t in std::iter::once(19).chain(0..19) {
        for _ in 1..packed[t].len() {
            indices[t].push(next);
            next += 1;
        }
    }
    let mut candidates: Vec<_> = (0..TABLES).map(|t| 2 + t * 2).collect();
    let mut next_unused = next;
    for t in std::iter::once(19).chain(0..19) {
        if !indices[t].is_empty() {
            candidates[t] = next_unused;
            next_unused += 1;
        }
    }
    let mut bytes = vec![0; next * PAGE];
    put32(&mut bytes, 4, PAGE as u32);
    put32(&mut bytes, 8, TABLES as u32);
    put32(&mut bytes, 12, next_unused as u32);
    put32(&mut bytes, 16, 1);
    put32(&mut bytes, 20, next as u32 + 1);
    for t in 0..TABLES {
        let index = 1 + t * 2;
        let candidate = candidates[t];
        let entry = 28 + t * 16;
        for (at, value) in [
            (entry, t),
            (entry + 4, candidate),
            (entry + 8, index),
            (entry + 12, indices[t].last().copied().unwrap_or(index)),
        ] {
            put32(&mut bytes, at, value as u32);
        }
        let page = &mut bytes[index * PAGE..(index + 1) * PAGE];
        let first_data = indices[t].first().copied();
        page_header(page, index, t, first_data.unwrap_or(candidate));
        page[27] = 0x64;
        put16(page, 32, 0x1fff);
        put16(page, 34, 0x1fff);
        put16(page, 36, INDEX_CAPACITY as u16);
        put32(page, 40, index as u32);
        // A fresh empty table ends at its index. The next page is its zeroed
        // allocation candidate, not an allocated data page in the table chain.
        put32(
            page,
            44,
            first_data.map(|p| p as u32).unwrap_or(0x03ff_ffff),
        );
        put32(page, 48, 0x03ff_ffff);
        put16(page, 58, 0x1fff);
        // Only the declared slots are entries. The final 20 bytes are reserved,
        // and remain zero, as in every index page of the reference export.
        for at in (INDEX_ENTRIES_START..INDEX_ENTRIES_END).step_by(4) {
            put32(page, at, 0x1fff_fff8);
        }
        for (part, rows) in packed[t].iter().enumerate() {
            if rows.is_empty() {
                continue;
            }
            let page_index = indices[t][part];
            let next_page = indices[t].get(part + 1).copied().unwrap_or(candidate);
            let page = &mut bytes[page_index * PAGE..(page_index + 1) * PAGE];
            page_header(page, page_index, t, next_page);
            let n = rows.len();
            let packed_counts = (n as u32) | ((n as u32) << 13);
            page[24..27].copy_from_slice(&packed_counts.to_le_bytes()[..3]);
            page[27] = 0x24;
            put16(page, 32, n as u16);
            let mut cursor = HEADER;
            for (slot, row) in rows.iter().enumerate() {
                page[cursor..cursor + row.len()].copy_from_slice(row);
                if matches!(t, 0 | 2 | 3 | 19) {
                    put16(page, cursor + 2, (slot * 32) as u16);
                }
                let base = PAGE - (slot / 16) * 36;
                put16(page, base - 6 - (slot % 16) * 2, (cursor - HEADER) as u16);
                cursor += align(row.len());
            }
            for group in 0..n.div_ceil(16) {
                let bits = (n - group * 16).min(16);
                let mask = ((1u32 << bits) - 1) as u16;
                let base = PAGE - group * 36;
                put16(page, base - 4, mask);
                put16(page, base - 2, mask);
            }
            let used = cursor - HEADER;
            put16(page, 30, used as u16);
            put16(
                page,
                28,
                (PAGE - HEADER - used - n * 2 - n.div_ceil(16) * 4) as u16,
            );
        }
    }
    Ok(bytes)
}

fn page_header(page: &mut [u8], index: usize, table: usize, next: usize) {
    for (at, value) in [(4, index), (8, table), (12, next), (16, index)] {
        put32(page, at, value as u32);
    }
}

#[derive(Debug, Serialize)]
pub struct DatabaseInspection {
    pub page_count: usize,
    pub table_rows: BTreeMap<u32, usize>,
    pub tracks: Vec<InspectedTrack>,
    pub playlists: Vec<InspectedPlaylist>,
    pub entries: Vec<[u32; 3]>,
    pub browse_columns: Vec<BrowseColumn>,
    pub browse_categories: Vec<BrowseCategory>,
    pub browse_sorts: Vec<BrowseSort>,
}
#[derive(Debug, Serialize)]
pub struct InspectedTrack {
    pub id: u32,
    pub content_link: u32,
    pub master_content_id: u32,
    pub master_database_id: u32,
    pub title: String,
    pub path: String,
    pub analysis_path: String,
    pub hot_cue_auto_load: String,
    pub size_bytes: u32,
    pub format_code: u16,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
    pub duration_seconds: u16,
}
#[derive(Debug, Serialize)]
pub struct InspectedPlaylist {
    pub id: u32,
    pub parent_id: u32,
    pub position: u32,
    pub name: String,
    pub is_folder: bool,
}

/// Bounded, read-only structural inspector. Useful for reference USBs as well as
/// generated output. It establishes syntax/reference consistency, not firmware acceptance.
pub fn inspect(bytes: &[u8]) -> Result<DatabaseInspection> {
    if bytes.len() < PAGE
        || !bytes.len().is_multiple_of(PAGE)
        || bytes.len() > 128 * 1024 * 1024
        || number(bytes, 4)? != PAGE as u32
    {
        return Err(invalid("unsupported DeviceSQL size/page size"));
    }
    let count = number(bytes, 8)? as usize;
    if count == 0 || 28 + count * 16 > PAGE {
        return Err(invalid("invalid table count"));
    }
    let mut out = DatabaseInspection {
        page_count: bytes.len() / PAGE,
        table_rows: BTreeMap::new(),
        tracks: Vec::new(),
        playlists: Vec::new(),
        entries: Vec::new(),
        browse_columns: Vec::new(),
        browse_categories: Vec::new(),
        browse_sorts: Vec::new(),
    };
    let mut ownership = BTreeSet::new();
    for table in 0..count {
        let pointer = 28 + 16 * table;
        let kind = number(bytes, pointer)?;
        if out.table_rows.insert(kind, 0).is_some() {
            return Err(invalid("duplicate table type"));
        }
        let mut index = number(bytes, pointer + 8)? as usize;
        let last = number(bytes, pointer + 12)? as usize;
        loop {
            if index == 0 || index >= out.page_count || !ownership.insert(index) {
                return Err(invalid("page chain is cyclic, shared or out of bounds"));
            }
            let page = &bytes[index * PAGE..(index + 1) * PAGE];
            if number(page, 4)? as usize != index || number(page, 8)? != kind {
                return Err(invalid("page identity does not match table"));
            }
            if index == number(bytes, pointer + 8)? as usize && page[27] & 0x40 == 0 {
                return Err(invalid("table does not start with an index page"));
            }
            if page[27] & 0x40 != 0 {
                if word(page, 36)? as usize != INDEX_CAPACITY
                    || number(page, 40)? as usize != index
                    || (number(page, 44)? != number(page, 12)? && number(page, 44)? != 0x03ff_ffff)
                    || number(page, 48)? != 0x03ff_ffff
                    || usize::from(word(page, 56)?) > INDEX_CAPACITY
                    || page[INDEX_ENTRIES_END..].iter().any(|b| *b != 0)
                {
                    return Err(invalid(
                        "invalid DeviceSQL index header or reserved trailer",
                    ));
                }
            } else {
                let counts = u32::from_le_bytes([page[24], page[25], page[26], 0]);
                let slots = (counts & 0x1fff) as usize;
                let live = (counts >> 13) as usize;
                if slots == 0 {
                    return Err(invalid("unallocated data page included in table chain"));
                }
                let index_size = slots
                    .checked_mul(2)
                    .and_then(|n| n.checked_add(slots.div_ceil(16) * 4))
                    .ok_or_else(|| invalid("index size overflow"))?;
                let heap_end = HEADER + usize::from(word(page, 30)?);
                if index_size > PAGE - HEADER || heap_end > PAGE - index_size {
                    return Err(invalid("row heap overlaps row index"));
                }
                let mut offsets = Vec::new();
                let mut present = 0;
                for slot in 0..slots {
                    let base = PAGE - slot / 16 * 36;
                    let active = word(page, base - 4)? & (1 << (slot % 16)) != 0;
                    let at = HEADER + usize::from(word(page, base - 6 - slot % 16 * 2)?);
                    if active {
                        present += 1;
                        if at >= heap_end {
                            return Err(invalid("active row offset outside heap"));
                        }
                        offsets.push(at);
                    }
                }
                if present != live {
                    return Err(invalid("live-row count disagrees with presence masks"));
                }
                offsets.sort_unstable();
                if offsets.windows(2).any(|w| w[0] == w[1]) {
                    return Err(invalid("aliased row offsets"));
                }
                for (i, start) in offsets.iter().enumerate() {
                    let row = &page[*start..offsets.get(i + 1).copied().unwrap_or(heap_end)];
                    match kind {
                        0 => {
                            let title = decode_string(row, word(row, 94 + 17 * 2)? as usize)?;
                            let path = decode_string(row, word(row, 94 + 20 * 2)? as usize)?;
                            let analysis_path =
                                decode_string(row, word(row, 94 + 14 * 2)? as usize)?;
                            out.tracks.push(InspectedTrack {
                                id: number(row, 72)?,
                                content_link: number(row, 4)?,
                                master_content_id: number(row, 20)?,
                                master_database_id: number(row, 24)?,
                                title,
                                path,
                                analysis_path,
                                hot_cue_auto_load: decode_string(
                                    row,
                                    word(row, 94 + 7 * 2)? as usize,
                                )?,
                                size_bytes: number(row, 16)?,
                                format_code: word(row, 90)?,
                                sample_rate: number(row, 8)?,
                                bit_depth: word(row, 82)?,
                                bitrate_kbps: number(row, 48)?,
                                duration_seconds: word(row, 84)?,
                            });
                        }
                        7 => out.playlists.push(InspectedPlaylist {
                            parent_id: number(row, 0)?,
                            position: number(row, 8)?,
                            id: number(row, 12)?,
                            is_folder: number(row, 16)? != 0,
                            name: decode_string(row, 20)?,
                        }),
                        8 => out
                            .entries
                            .push([number(row, 0)?, number(row, 4)?, number(row, 8)?]),
                        16 => out.browse_columns.push(BrowseColumn {
                            id: word(row, 0)?,
                            selector: word(row, 2)?,
                            name: decode_string(row, 4)?,
                        }),
                        17 => {
                            let position = word(row, 6)?; // Also checks the fixed row length.
                            out.browse_categories.push(BrowseCategory {
                                column_id: word(row, 0)?,
                                id: word(row, 2)?,
                                kind: row[4],
                                flags: row[5],
                                position,
                            });
                        }
                        18 => {
                            word(row, 6)?;
                            out.browse_sorts.push(BrowseSort {
                                column_id: word(row, 0)?,
                                id: word(row, 2)?,
                                flags: row[4],
                                position: row[5],
                            });
                        }
                        _ => {}
                    }
                }
                *out.table_rows.get_mut(&kind).unwrap() += live;
            }
            if index == last {
                break;
            }
            index = number(page, 12)? as usize;
        }
        let first_page = number(bytes, pointer + 8)? as usize * PAGE;
        let no_data_hint = number(bytes, first_page + 44)? == 0x03ff_ffff;
        // A used library may retain a data-page hint after deleting its last
        // row. The no-page sentinel must never hide live rows, however.
        if no_data_hint && out.table_rows[&kind] != 0 {
            return Err(invalid(
                "table index data-page hint disagrees with live rows",
            ));
        }
    }
    let track_ids: BTreeSet<_> = out.tracks.iter().map(|t| t.id).collect();
    let playlist_ids: BTreeSet<_> = out.playlists.iter().map(|p| p.id).collect();
    if track_ids.len() != out.tracks.len() || playlist_ids.len() != out.playlists.len() {
        return Err(invalid("duplicate database identifiers"));
    }
    if out
        .entries
        .iter()
        .any(|e| !track_ids.contains(&e[1]) || !playlist_ids.contains(&e[2]))
    {
        return Err(invalid("dangling playlist entry"));
    }
    browse::validate_references(
        &out.browse_columns,
        &out.browse_categories,
        &out.browse_sorts,
    )?;
    Ok(out)
}

pub fn compare(plan: &ExportPlan, audio: &[AudioInfo], db: &DatabaseInspection) -> Result<()> {
    browse::validate_defaults(&db.browse_columns, &db.browse_categories, &db.browse_sorts)?;
    if plan.tracks.len() != db.tracks.len()
        || plan.playlists.len() != db.playlists.len()
        || audio.len() != plan.tracks.len()
    {
        return Err(invalid("read-back counts differ from plan"));
    }
    for (t, a) in plan.tracks.iter().zip(audio) {
        let found = db
            .tracks
            .iter()
            .find(|r| r.id == t.id)
            .ok_or_else(|| invalid("missing read-back track"))?;
        if found.title != t.title
            || found.content_link != GENERATED_ANALYSIS_CONTENT_LINK
            || found.path != t.device_path
            || found.size_bytes != a.size_bytes
            || found.format_code != a.format.device_code()
            || found.sample_rate != a.sample_rate
            || found.bit_depth != a.bit_depth
            || found.bitrate_kbps != a.bitrate_kbps
            || found.duration_seconds != a.frames.div_ceil(u64::from(a.sample_rate)) as u16
            || found.analysis_path != anlz::path_for_track(t)?
            || found.hot_cue_auto_load
                != if t.analysis.cues.iter().any(|c| c.hot_cue > 0) {
                    "ON"
                } else {
                    ""
                }
        {
            return Err(invalid("read-back track differs from plan"));
        }
    }
    let mut expected_entries = Vec::new();
    for p in &plan.playlists {
        let found = db
            .playlists
            .iter()
            .find(|r| r.id == p.id)
            .ok_or_else(|| invalid("missing read-back playlist"))?;
        if found.name != p.name
            || found.parent_id != p.parent_id
            || found.position != p.position
            || found.is_folder != p.is_folder
        {
            return Err(invalid("read-back playlist differs from plan"));
        }
        for (index, id) in p.tracks.iter().enumerate() {
            expected_entries.push([index as u32 + 1, *id, p.id]);
        }
    }
    let mut actual = db.entries.clone();
    expected_entries.sort_unstable();
    actual.sort_unstable();
    if expected_entries != actual {
        return Err(invalid(
            "read-back playlist order/membership differs from plan",
        ));
    }
    Ok(())
}
