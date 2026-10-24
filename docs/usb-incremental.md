# Incremental USB updates

Implemented October 24, 2026 on `feature/usb-export`. Compilation and frontend
packaging have been checked; this update path has not yet been exercised on a
real USB or player. No USB was accessed or changed during this implementation.

## Selection and preservation

Choose the existing export's root under **Playlists → Exportar a USB**. Rau reads
Device Library/OneLibrary and shows the existing playlists, including nested
paths and track counts. Matched local playlists are preselected. New receipts
persist the local playlist identity, so a local rename can still update the same
device playlist. Earlier receipts use an unambiguous exact-name match only.
Ambiguous matches are left separate rather than guessed.

- Selected playlists receive the current local name, track order and membership.
- Unselected and USB-only playlists remain on the device. Deselecting does not
  remove a playlist. Existing tracks also remain, even if no longer referenced.
- Tracks shared across selected playlists within a catalog library are prepared
  once. Distinct catalog identities are not deduplicated by content.
- Existing track/playlist IDs and the device library identity remain stable.
- Unchanged audio and analysis keep their paths and bytes. Changed audio gets
  a new filename; old audio is retained. A changed grid/cue set can receive new
  ANLZ files while retaining the existing audio.
- A missing local original can reuse its readable, validated copy on the USB.
  Missing new sources go through the explicit skip/cancel review.
- Supported metadata and grid/cue edits on the USB are retained when the local
  source has not changed. Simultaneous supported metadata edits prefer USB values
  with a review notice. Incompatible playlist edits between the two databases
  block the update instead of choosing one silently.

This is an additive update. It does not delete old media or provide a mirror,
purge, full conflict-resolution editor or cross-catalog deduplication.

## Compatibility boundary

Updates require a completed Rau export with a readable schema 5–7 receipt,
both databases, matching track identities/paths and valid media/analysis. Foreign
libraries can be displayed but are not rewritten. Exports with changed/unknown
track sets, unresolved playlist conflicts or nonempty unsupported history,
artwork, My Tags, cue-index/bank and recommendation tables are view-only.
The exporter does not promise preservation of every vendor-specific column.

Legacy receipts lack source hashes/signatures or stable playlist bindings.
The first update establishes them. Copied audio can be compared to its original
by hash. A legacy converted track with no trustworthy source fingerprint needs
one new conversion/export; subsequent unchanged updates reuse it.

The receipt is part of the export and should be retained. Schema 7 adds stable
playlist bindings, source/device fingerprints and last-observed source metadata
and preparation for subsequent merges. Moving to another machine or remounting
can invalidate fast filesystem signatures; hashes then establish identity again.

## Analysis reuse

There are two separate operations:

1. **Musical preparation**: BPM/grid/key proposals and explicitly saved corrections
   live in Rau's local catalog. Export reads the saved data without running the
   musical detector again. Stale saved preparation still requires review.
2. **Device analysis**: waveforms, grids and cues are serialized as DAT/EXT/2EX.
   Unchanged files in a compatible USB export are reused without decoding audio.

For another destination, the desktop exporter uses
`app_cache_dir()/usb-analysis-v1/`. Each cache entry stores three ANLZ companions,
audio properties and checksums. The key includes SHA-256 of the actual exported
audio, its device audio path, grid/cues and an explicit renderer version. It
does not include the USB volume name or ANLZ directory; summary paths are rebound
when needed. It does not store decoded PCM or copies of source audio.

A valid cache hit avoids FFmpeg waveform decoding. New/changed audio, grid/cue
changes, a different device audio path or renderer version, or invalid/missing
cached data require rendering. Exporting to another USB still copies or converts
the audio; this is an analysis cache, not a conversion cache. A cache failure is
logged as a warning and does not stop a valid export. The cache persists across
app restarts until cleared by the user or operating system. No eviction UI or
automatic size limit is implemented yet.

File size, modification time and, on Unix, inode/device/change-time signatures
avoid unnecessary repeated hashing. A changed signature triggers a hash check;
changed audio on the USB is rejected. Metadata revisions and prepared media
signatures are checked again before publication. This detects normal concurrent
changes; it is not an exclusive volume lock against other applications.

## Concurrency and terminal

Choose **Temas en paralelo**: 1, 2 (default), or 4. The core limits workers to four
and FFmpeg audio processing to one thread per worker. Each worker prepares a
separate track; a coordinator serializes progress messages. An error stops new
work and waits for already-running workers before failing.

Both databases are generated/validated and published serially after all track
work succeeds. The terminal reports source/destination paths, copy/conversion,
reuse, analysis, cache hits/stores/warnings, per-track completion, media additions,
database generation/publication and failures. The current job retains the latest
1,000 entries across navigation; logs are not persisted across app restarts.

## Publication, backups and recovery

Preparation happens in a temporary `.rau-stage-*` directory on the selected
volume. Existing media/analysis are never replaced. New files are created
exclusively, copied from staging, flushed and checked before publication. This
requires temporary space for staged additions and additional IO for those files.

The previous `export.pdb`, `exportLibrary.db` and `rau-export-report.json` are kept
under `.rau-backups/<timestamp-random>/`. A flushed `.rau-update-journal.json`
records their hashes before moving the metadata. Existing `DEVSETTING.DAT` is
retained. A normal publication error attempts restoration automatically.

An interrupted journal blocks further updates. **Restaurar biblioteca anterior**
verifies all available originals/backups before restoring them. This action is
available even if interruption left a database temporarily absent/unreadable.
Interrupted newer metadata are retained under the backup for diagnosis. The
receipt and both databases are restored as one recovery set.

Multiple file renames are not a whole-volume atomic transaction and do not
guarantee recovery from filesystem corruption or physical removal during writes.
A failed update can leave unreferenced new media or staging directories. They
are not automatically deleted; subsequent allocation avoids occupied audio
paths. Backups also remain until explicitly managed. There is no running-job
cancellation, automatic USB ejection or free-space preflight in this change.

## Remaining acceptance

No automated tests were added or run for this change. Rust compile checks,
TypeScript checking and a production frontend build cover build integration only.
Runtime checks on a separate test destination should cover repeated unchanged
updates, one added track, changed original audio, grid-only changes, missing
originals, renamed/empty/unchecked/USB-only playlists, parallel-worker failure,
cache reuse on another destination and interrupted publication/recovery.
Rekordbox/player acceptance and power-loss behavior remain to be validated.
MIKI RED remains outside this workflow and must stay unmounted.
