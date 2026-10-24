//! Independently authored OneLibrary writer from public format descriptions and
//! read-only schema observations of the user's own export. No upstream writer
//! code or captured database is incorporated. Audio and cues live in the same
//! Contents/ANLZ files as the classic library.

use super::{
    anlz, browse, catalog::Catalog, devicesql, invalid, AudioInfo, ExportPlan, LibraryIdentity,
    Result,
};
use rusqlite::{params_from_iter, types::Value, Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::Path;

// Public, device-independent interoperability key, documented by pyrekordbox:
// github.com/dylanljones/pyrekordbox/blob/master/pyrekordbox/onelibrary/database.py
// This is a format constant; no account, installation or Rekordbox process is used.
const FORMAT_KEY: &str = "r8gddnr4k847830ar6cqzbkk0el6qytmb3trbbx805jm74vez64i5o8fnrqryqls";

// Column vocabulary/order observed in a locally saved native export. '*' is an
// integer primary key, '$' is varchar, other columns are integer. Association
// tables deliberately allow repeated tracks in a playlist.
const TABLES: &[(&str, &str)] = &[
    ("album", "album_id* name$ artist_id image_id isComplation nameForSearch$"),
    ("artist", "artist_id* name$ nameForSearch$"),
    ("category", "category_id* menuItem_id sequenceNo isVisible"),
    ("color", "color_id* name$"),
    ("content", "content_id* title$ titleForSearch$ subtitle$ bpmx100 length trackNo discNo artist_id_artist artist_id_remixer artist_id_originalArtist artist_id_composer artist_id_lyricist album_id genre_id label_id key_id color_id image_id djComment$ rating releaseYear releaseDate$ dateCreated$ dateAdded$ path$ fileName$ fileSize fileType bitrate bitDepth samplingRate isrc$ djPlayCount isHotCueAutoLoadOn isKuvoDeliverStatusOn kuvoDeliveryComment$ masterDbId masterContentId analysisDataFilePath$ analysedBits contentLink hasModified cueUpdateCount analysisDataUpdateCount informationUpdateCount"),
    ("cue", "cue_id* content_id kind colorTableIndex cueComment$ isActiveLoop beatLoopNumerator beatLoopDenominator inUsec outUsec in150FramePerSec out150FramePerSec inMpegFrameNumber outMpegFrameNumber inMpegAbs outMpegAbs inDecodingStartFramePosition outDecodingStartFramePosition inFileOffsetInBlock OutFileOffsetInBlock inNumberOfSampleInBlock outNumberOfSampleInBlock"),
    ("genre", "genre_id* name$"),
    ("history", "history_id* sequenceNo name$ attribute history_id_parent"),
    ("history_content", "history_id content_id sequenceNo"),
    ("hotCueBankList", "hotCueBankList_id* sequenceNo name$ image_id attribute hotCueBankList_id_parent"),
    ("hotCueBankList_cue", "hotCueBankList_id cue_id sequenceNo"),
    ("image", "image_id* path$"),
    ("key", "key_id* name$"),
    ("label", "label_id* name$"),
    ("menuItem", "menuItem_id* kind name$"),
    ("myTag", "myTag_id* sequenceNo name$ attribute myTag_id_parent"),
    ("myTag_content", "myTag_id content_id"),
    ("playlist", "playlist_id* sequenceNo name$ image_id attribute playlist_id_parent"),
    ("playlist_content", "playlist_id content_id sequenceNo"),
    ("property", "deviceName$ dbVersion$ numberOfContents createdDate$ backGroundColorType myTagMasterDBID"),
    ("recommendedLike", "content_id_1 content_id_2 rating createdDate"),
    ("sort", "sort_id* menuItem_id sequenceNo isVisible isSelectedAsSubColumn"),
];

fn column(token: &str) -> &str {
    token.trim_end_matches(['*', '$'])
}
fn integer(value: impl Into<i64>) -> Value {
    Value::Integer(value.into())
}
fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}

fn unlock(db: &Connection) -> Result<()> {
    let version: String = db.query_row("PRAGMA cipher_version", [], |r| r.get(0))?;
    if version.is_empty() {
        return Err(invalid("SQLCipher is unavailable"));
    }
    db.pragma_update(None, "key", FORMAT_KEY)?;
    db.pragma_update(None, "cipher_compatibility", 4)?;
    db.pragma_update(None, "trusted_schema", false)?;
    Ok(())
}

fn insert(db: &Connection, table: &str, fields: &[(&str, Value)]) -> Result<()> {
    let spec = TABLES
        .iter()
        .find(|s| s.0 == table)
        .ok_or_else(|| invalid("unknown OneLibrary table"))?
        .1;
    let tokens = spec.split_whitespace().collect::<Vec<_>>();
    let mut values = tokens
        .iter()
        .map(|t| {
            if t.ends_with('$') {
                text("")
            } else {
                integer(0)
            }
        })
        .collect::<Vec<_>>();
    for (name, value) in fields {
        let index = tokens
            .iter()
            .position(|t| column(t) == *name)
            .ok_or_else(|| invalid(format!("unknown OneLibrary column {name}")))?;
        values[index] = value.clone();
    }
    let placeholders = vec!["?"; tokens.len()].join(",");
    db.prepare_cached(&format!("INSERT INTO \"{table}\" VALUES ({placeholders})"))?
        .execute(params_from_iter(values))?;
    Ok(())
}

/// Write only a new encrypted database. The caller must have already prepared
/// and verified the audio and DAT/EXT/2EX companions referenced by the plan.
pub fn write(
    path: &Path,
    plan: &ExportPlan,
    audio: &[AudioInfo],
    created_date: &str,
    identity: &LibraryIdentity,
) -> Result<()> {
    // Reuse the classic serializer's complete plan/audio/date validation.
    devicesql::encode_with_identity(plan, audio, created_date, identity)?;
    if identity.master_database_id == 0
        || identity.name.is_empty()
        || identity.name.len() > 80
        || identity.name.contains('\0')
    {
        return Err(invalid(
            "OneLibrary requires a nonzero export identity and a device name of at most 80 bytes",
        ));
    }
    File::create_new(path)?;
    let mut db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    unlock(&db)?;
    db.pragma_update(None, "journal_mode", "DELETE")?;
    db.pragma_update(None, "synchronous", "FULL")?;
    let tx = db.transaction()?;
    for (name, spec) in TABLES {
        let fields = spec
            .split_whitespace()
            .map(|t| {
                let kind = if t.ends_with('*') {
                    "INTEGER PRIMARY KEY"
                } else if t.ends_with('$') {
                    "VARCHAR"
                } else {
                    "INTEGER"
                };
                format!("\"{}\" {kind}", column(t))
            })
            .collect::<Vec<_>>()
            .join(",");
        tx.execute_batch(&format!("CREATE TABLE \"{name}\" ({fields});"))?;
    }
    let catalog = Catalog::new(plan);
    for (table, names, id_name) in [
        ("artist", &catalog.artists, "artist_id"),
        ("genre", &catalog.genres, "genre_id"),
        ("label", &catalog.labels, "label_id"),
        ("key", &catalog.keys, "key_id"),
    ] {
        for (name, id) in names {
            insert(&tx, table, &[(id_name, integer(*id)), ("name", text(name))])?;
        }
    }
    let id_for = |names: &BTreeMap<String, u32>, name: &str| names.get(name).copied().unwrap_or(0);
    for ((artist, name), id) in &catalog.albums {
        insert(
            &tx,
            "album",
            &[
                ("album_id", integer(*id)),
                ("name", text(name)),
                ("artist_id", integer(id_for(&catalog.artists, artist))),
            ],
        )?;
    }
    for (i, name) in [
        "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple",
    ]
    .iter()
    .enumerate()
    {
        insert(
            &tx,
            "color",
            &[("color_id", integer(i as i64 + 1)), ("name", text(*name))],
        )?;
    }
    for (track, media) in plan.tracks.iter().zip(audio) {
        let album = catalog
            .albums
            .get(&(track.artist.clone(), track.album.clone()))
            .copied()
            .unwrap_or(0);
        insert(
            &tx,
            "content",
            &[
                ("content_id", integer(track.id)),
                ("title", text(&track.title)),
                ("bpmx100", integer(track.bpm_x100)),
                (
                    "length",
                    integer(media.frames.div_ceil(u64::from(media.sample_rate)) as i64),
                ),
                ("trackNo", integer(track.track_number)),
                (
                    "artist_id_artist",
                    integer(id_for(&catalog.artists, &track.artist)),
                ),
                ("album_id", integer(album)),
                ("genre_id", integer(id_for(&catalog.genres, &track.genre))),
                ("label_id", integer(id_for(&catalog.labels, &track.label))),
                ("key_id", integer(id_for(&catalog.keys, &track.key))),
                ("djComment", text(&track.comment)),
                ("rating", integer(track.rating)),
                ("releaseYear", integer(track.year)),
                ("dateCreated", text(created_date)),
                ("dateAdded", text(created_date)),
                ("path", text(&track.device_path)),
                (
                    "fileName",
                    text(track.device_path.rsplit('/').next().unwrap()),
                ),
                ("fileSize", integer(media.size_bytes)),
                ("fileType", integer(media.format.device_code())),
                ("bitrate", integer(media.bitrate_kbps)),
                ("bitDepth", integer(media.bit_depth)),
                ("samplingRate", integer(media.sample_rate)),
                (
                    "isHotCueAutoLoadOn",
                    integer(i64::from(track.analysis.cues.iter().any(|c| c.hot_cue > 0))),
                ),
                ("masterDbId", integer(identity.master_database_id)),
                ("masterContentId", integer(track.id)),
                ("analysisDataFilePath", text(anlz::path_for_track(track)?)),
                ("analysedBits", integer(0x29)),
                (
                    "contentLink",
                    integer(devicesql::GENERATED_ANALYSIS_CONTENT_LINK),
                ),
            ],
        )?;
    }
    for playlist in &plan.playlists {
        insert(
            &tx,
            "playlist",
            &[
                ("playlist_id", integer(playlist.id)),
                ("sequenceNo", integer(playlist.position)),
                ("name", text(&playlist.name)),
                ("attribute", integer(i64::from(playlist.is_folder))),
                ("playlist_id_parent", integer(playlist.parent_id)),
            ],
        )?;
        for (i, track) in playlist.tracks.iter().enumerate() {
            insert(
                &tx,
                "playlist_content",
                &[
                    ("playlist_id", integer(playlist.id)),
                    ("content_id", integer(*track)),
                    ("sequenceNo", integer(i as i64 + 1)),
                ],
            )?;
        }
    }
    for item in browse::columns() {
        insert(
            &tx,
            "menuItem",
            &[
                ("menuItem_id", integer(item.id)),
                ("kind", integer(item.selector)),
                ("name", text(item.name)),
            ],
        )?;
    }
    for category in browse::categories() {
        insert(
            &tx,
            "category",
            &[
                ("category_id", integer(category.id)),
                ("menuItem_id", integer(category.column_id)),
                ("sequenceNo", integer(category.position)),
                ("isVisible", integer(i64::from(category.flags == 0))),
            ],
        )?;
    }
    for sort in browse::sorts() {
        insert(
            &tx,
            "sort",
            &[
                ("sort_id", integer(sort.id)),
                ("menuItem_id", integer(sort.column_id)),
                ("sequenceNo", integer(sort.position)),
                ("isVisible", integer(i64::from(sort.flags == 0))),
            ],
        )?;
    }
    insert(
        &tx,
        "property",
        &[
            ("deviceName", text(&identity.name)),
            ("dbVersion", text("1000")),
            ("numberOfContents", integer(plan.tracks.len() as i64)),
            ("createdDate", text(created_date)),
            ("myTagMasterDBID", integer(identity.master_database_id)),
        ],
    )?;
    // Native conversion leaves cue empty: per-track cues/loops are in ANLZ.
    // Hot-cue bank lists, histories, tags and recommendations are unsupported.
    tx.commit()?;
    db.close().map_err(|(_, e)| e)?;
    File::open(path)?.sync_all()?;
    let reread = inspect(path)?;
    compare(plan, audio, identity, &reread)?;
    if reread.created_date != created_date {
        return Err(invalid("OneLibrary read-back date differs from export"));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct InspectedTrack {
    pub id: u32,
    pub title: String,
    pub path: String,
    pub analysis_path: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    pub label: String,
    pub key: String,
    pub bpm_x100: u32,
    pub duration_seconds: u32,
    pub rating: u8,
    pub year: u16,
    pub track_number: u32,
    pub comment: String,
    pub size_bytes: u32,
    pub format_code: u16,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
    pub content_link: u32,
    pub analysed_bits: u16,
    pub master_content_id: u32,
    pub master_database_id: u32,
    pub hot_cue_auto_load: bool,
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub table_rows: BTreeMap<String, u64>,
    pub identity: LibraryIdentity,
    pub created_date: String,
    pub tracks: Vec<InspectedTrack>,
    pub playlists: Vec<super::ExportPlaylist>,
    pub entries: Vec<[u32; 3]>,
    pub browse_columns: Vec<devicesql::BrowseColumn>,
    pub browse_categories: Vec<devicesql::BrowseCategory>,
    pub browse_sorts: Vec<devicesql::BrowseSort>,
}

/// Bounded read-only inspection, without a Rekordbox installation or process.
pub fn inspect(path: &Path) -> Result<Inspection> {
    let size = path.metadata()?.len();
    if !(4096..=128 * 1024 * 1024).contains(&size) {
        return Err(invalid("OneLibrary file size is outside inspection limits"));
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    unlock(&db)?;
    db.pragma_update(None, "query_only", true)?;
    let check: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if check != "ok" {
        return Err(invalid("OneLibrary integrity check failed"));
    }
    let mut counts = BTreeMap::new();
    for (table, _) in TABLES {
        let count: u64 = db.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
            r.get(0)
        })?;
        if count > 1_000_000 {
            return Err(invalid("OneLibrary table exceeds inspection limits"));
        }
        counts.insert(table.to_string(), count);
    }
    if counts["content"] > 10_000 || counts["playlist"] > 10_000 || counts["property"] != 1 {
        return Err(invalid(
            "OneLibrary track/playlist/property counts are invalid",
        ));
    }
    let (name, master_database_id, version, declared_count, created_date): (
        String,
        u32,
        String,
        u64,
        String,
    ) = db.query_row(
        "SELECT deviceName,myTagMasterDBID,dbVersion,numberOfContents,createdDate FROM property",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    )?;
    if version != "1000"
        || declared_count != counts["content"]
        || chrono::NaiveDate::parse_from_str(&created_date, "%Y-%m-%d").is_err()
    {
        return Err(invalid("OneLibrary property version/count/date mismatch"));
    }
    let mut q = db.prepare("SELECT c.content_id,c.title,c.path,c.analysisDataFilePath,
        coalesce(a.name,''),coalesce(b.name,''),coalesce(g.name,''),coalesce(l.name,''),coalesce(k.name,''),
        c.bpmx100,c.length,c.rating,c.releaseYear,c.trackNo,c.djComment,c.fileSize,c.fileType,
        c.samplingRate,c.bitDepth,c.bitrate,c.contentLink,c.analysedBits,c.masterContentId,c.masterDbId,c.isHotCueAutoLoadOn
        FROM content c LEFT JOIN artist a ON a.artist_id=c.artist_id_artist
        LEFT JOIN album b ON b.album_id=c.album_id LEFT JOIN genre g ON g.genre_id=c.genre_id
        LEFT JOIN label l ON l.label_id=c.label_id LEFT JOIN key k ON k.key_id=c.key_id ORDER BY c.content_id")?;
    let tracks = q
        .query_map([], |r| {
            Ok(InspectedTrack {
                id: r.get(0)?,
                title: r.get(1)?,
                path: r.get(2)?,
                analysis_path: r.get(3)?,
                artist: r.get(4)?,
                album: r.get(5)?,
                genre: r.get(6)?,
                label: r.get(7)?,
                key: r.get(8)?,
                bpm_x100: r.get(9)?,
                duration_seconds: r.get(10)?,
                rating: r.get(11)?,
                year: r.get(12)?,
                track_number: r.get(13)?,
                comment: r.get(14)?,
                size_bytes: r.get(15)?,
                format_code: r.get(16)?,
                sample_rate: r.get(17)?,
                bit_depth: r.get(18)?,
                bitrate_kbps: r.get(19)?,
                content_link: r.get(20)?,
                analysed_bits: r.get(21)?,
                master_content_id: r.get(22)?,
                master_database_id: r.get(23)?,
                hot_cue_auto_load: r.get(24)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut q = db.prepare("SELECT playlist_id,playlist_id_parent,sequenceNo,name,attribute FROM playlist ORDER BY playlist_id")?;
    let mut playlists = q
        .query_map([], |r| {
            Ok(super::ExportPlaylist {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                position: r.get(2)?,
                name: r.get(3)?,
                is_folder: r.get::<_, i64>(4)? == 1,
                tracks: Vec::new(),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut q = db.prepare("SELECT sequenceNo,content_id,playlist_id FROM playlist_content ORDER BY playlist_id,sequenceNo")?;
    let entries = q
        .query_map([], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?]))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let ids = tracks.iter().map(|t| t.id).collect::<BTreeSet<_>>();
    let playlist_ids = playlists.iter().map(|p| p.id).collect::<BTreeSet<_>>();
    if entries
        .iter()
        .any(|e| !ids.contains(&e[1]) || !playlist_ids.contains(&e[2]))
    {
        return Err(invalid("OneLibrary has dangling playlist entries"));
    }
    for playlist in &mut playlists {
        playlist.tracks = entries
            .iter()
            .filter(|entry| entry[2] == playlist.id)
            .map(|entry| entry[1])
            .collect();
    }
    let mut q = db.prepare("SELECT menuItem_id,kind,name FROM menuItem ORDER BY menuItem_id")?;
    let browse_columns = q
        .query_map([], |r| {
            Ok(devicesql::BrowseColumn {
                id: r.get(0)?,
                selector: r.get(1)?,
                name: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut q = db.prepare(
        "SELECT category_id,menuItem_id,sequenceNo,isVisible FROM category ORDER BY category_id",
    )?;
    let browse_categories = q
        .query_map([], |r| {
            Ok(devicesql::BrowseCategory {
                id: r.get(0)?,
                column_id: r.get(1)?,
                position: r.get(2)?,
                flags: u8::from(!r.get::<_, bool>(3)?),
                kind: 0,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    // Category 'kind' is a classic-only field; restore its protocol value from
    // the shared vocabulary so both menu profiles can be compared semantically.
    let browse_categories = browse_categories
        .into_iter()
        .map(|mut c| {
            c.kind = browse::categories()
                .iter()
                .find(|p| p.id == c.id)
                .map(|p| p.kind)
                .unwrap_or(0);
            c
        })
        .collect::<Vec<_>>();
    let mut q =
        db.prepare("SELECT sort_id,menuItem_id,sequenceNo,isVisible FROM sort ORDER BY sort_id")?;
    let browse_sorts = q
        .query_map([], |r| {
            Ok(devicesql::BrowseSort {
                id: r.get(0)?,
                column_id: r.get(1)?,
                position: r.get(2)?,
                flags: u8::from(!r.get::<_, bool>(3)?),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    browse::validate_references(&browse_columns, &browse_categories, &browse_sorts)?;
    Ok(Inspection {
        table_rows: counts,
        identity: LibraryIdentity {
            name,
            master_database_id,
        },
        created_date,
        tracks,
        playlists,
        entries,
        browse_columns,
        browse_categories,
        browse_sorts,
    })
}

pub fn compare(
    plan: &ExportPlan,
    audio: &[AudioInfo],
    identity: &LibraryIdentity,
    found: &Inspection,
) -> Result<()> {
    browse::validate_defaults(
        &found.browse_columns,
        &found.browse_categories,
        &found.browse_sorts,
    )?;
    if found.identity != *identity
        || found.tracks.len() != plan.tracks.len()
        || audio.len() != plan.tracks.len()
        || found.playlists.len() != plan.playlists.len()
    {
        return Err(invalid("OneLibrary read-back identity/count mismatch"));
    }
    for (track, media) in plan.tracks.iter().zip(audio) {
        let row = found
            .tracks
            .iter()
            .find(|r| r.id == track.id)
            .ok_or_else(|| invalid("OneLibrary is missing a track"))?;
        if row.title != track.title
            || row.path != track.device_path
            || row.artist != track.artist
            || row.album != track.album
            || row.genre != track.genre
            || row.label != track.label
            || row.key != track.key
            || row.comment != track.comment
            || row.bpm_x100 != track.bpm_x100
            || row.rating != track.rating
            || row.year != track.year
            || row.track_number != track.track_number
            || row.duration_seconds != media.frames.div_ceil(u64::from(media.sample_rate)) as u32
            || row.size_bytes != media.size_bytes
            || row.format_code != media.format.device_code()
            || row.sample_rate != media.sample_rate
            || row.bit_depth != media.bit_depth
            || row.bitrate_kbps != media.bitrate_kbps
            || row.analysis_path != anlz::path_for_track(track)?
            || row.content_link != devicesql::GENERATED_ANALYSIS_CONTENT_LINK
            || row.analysed_bits != 0x29
            || row.master_content_id != track.id
            || row.master_database_id != identity.master_database_id
            || row.hot_cue_auto_load != track.analysis.cues.iter().any(|c| c.hot_cue > 0)
        {
            return Err(invalid(format!(
                "OneLibrary read-back differs for track {}",
                track.id
            )));
        }
    }
    let mut expected_entries = Vec::new();
    for playlist in &plan.playlists {
        let row = found
            .playlists
            .iter()
            .find(|r| r.id == playlist.id)
            .ok_or_else(|| invalid("OneLibrary is missing a playlist"))?;
        if row.parent_id != playlist.parent_id
            || row.position != playlist.position
            || row.name != playlist.name
            || row.is_folder != playlist.is_folder
        {
            return Err(invalid("OneLibrary playlist read-back differs from plan"));
        }
        expected_entries.extend(
            playlist
                .tracks
                .iter()
                .enumerate()
                .map(|(i, id)| [i as u32 + 1, *id, playlist.id]),
        );
    }
    expected_entries.sort_unstable();
    let mut actual = found.entries.clone();
    actual.sort_unstable();
    if actual != expected_entries {
        return Err(invalid(
            "OneLibrary playlist order/repeats differ from plan",
        ));
    }
    Ok(())
}

/// Cross-check the two formats before the export can be marked complete.
pub fn compare_classic(classic: &devicesql::DatabaseInspection, plus: &Inspection) -> Result<()> {
    if classic.tracks.len() != plus.tracks.len() || classic.playlists.len() != plus.playlists.len()
    {
        return Err(invalid("USB library counts disagree"));
    }
    for track in &classic.tracks {
        let p = plus
            .tracks
            .iter()
            .find(|p| p.id == track.id)
            .ok_or_else(|| invalid("USB libraries disagree on track IDs"))?;
        if track.title != p.title
            || track.path != p.path
            || track.analysis_path != p.analysis_path
            || track.content_link != p.content_link
            || track.master_content_id != p.master_content_id
            || track.master_database_id != p.master_database_id
            || track.size_bytes != p.size_bytes
            || track.format_code != p.format_code
        {
            return Err(invalid(
                "USB libraries disagree on track paths, identity or audio",
            ));
        }
    }
    if classic.browse_columns != plus.browse_columns
        || classic.browse_categories != plus.browse_categories
        || classic.browse_sorts != plus.browse_sorts
    {
        return Err(invalid("USB libraries disagree on browse settings"));
    }
    let mut classic_entries = classic.entries.clone();
    classic_entries.sort_unstable();
    let mut plus_entries = plus.entries.clone();
    plus_entries.sort_unstable();
    if classic_entries != plus_entries {
        return Err(invalid("USB libraries disagree on playlist entries"));
    }
    for p in &classic.playlists {
        let q = plus
            .playlists
            .iter()
            .find(|q| q.id == p.id)
            .ok_or_else(|| invalid("USB libraries disagree on playlist IDs"))?;
        if p.parent_id != q.parent_id
            || p.position != q.position
            || p.name != q.name
            || p.is_folder != q.is_folder
        {
            return Err(invalid("USB libraries disagree on playlist folders"));
        }
    }
    Ok(())
}
