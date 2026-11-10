# Incremental USB updates

November 9, 2026: schema 8 integrates `rekordbox-usb`. On macOS/Linux, analysis
from the previous renderer is regenerated from USB audio at fresh paths; later
unchanged updates reuse it. Both catalogs retain generated cue rows. See the
[engine integration](rekordbox-usb-engine.md) for platform and acceptance details.

Implemented October 24, 2026 on `feature/usb-export`. Compilation and frontend
packaging are the acceptance checks for this pipeline revision; no export was
run by the agent. MIKI TEST was inspected read-only during diagnosis. Runtime
validation of the new overlapping queues on a test USB/player remains pending.

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
  once. Existing USB tracks also match an unambiguous original file path across
  catalog libraries, with the usual source fingerprint/hash check before reuse.
  Source aliases are retained so later updates keep that identity. Ambiguous
  originals and unrelated files are not merged by title or filename.
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
purge, full conflict-resolution editor or global content deduplication.

## Compatibility boundary

Updates require a completed Rau export with a readable schema 5–8 receipt,
both databases, matching track identities/paths and valid media/analysis. Foreign
libraries can be displayed but are not rewritten. Exports with changed/unknown
track sets, unresolved playlist conflicts or nonempty unsupported history,
artwork, My Tags, cue-bank and recommendation tables are view-only. Schema 8
supports generated OneLibrary cue rows and preserves consistent cue edits.
Legacy reports with unexpected SQL cue rows remain view-only.
The exporter does not promise preservation of every vendor-specific column.

Legacy receipts lack source hashes/signatures or stable playlist bindings.
The first update establishes them. Copied audio can be compared to its original
by hash. A legacy converted track with no trustworthy source fingerprint needs
one new conversion/export; subsequent unchanged updates reuse it.

The receipt is part of the export and should be retained. Schema 7 adds stable
playlist bindings, source/device fingerprints and last-observed source metadata
and preparation for subsequent merges, plus optional source aliases. Moving to another machine or remounting
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

The metadata revision uses the canonical destination path, mounted device ID
on Unix, and hashes of the two databases, receipt and device settings. It does
not use the root directory's inode: MIKI TEST returned different synthetic root
inodes across read-only observations while all four metadata hashes remained
identical. A root identifier change alone must not invalidate the review.

## Concurrency and terminal

**Temas en paralelo** defaults to **Automático**, using Rust's available CPU
parallelism (the schedulable logical cores reported by the operating system).
Manual selection ranges from one to that count. The effective processing pool
also cannot exceed the number of tracks needing work. FFmpeg codec and filter
processing are each limited to one thread per worker.

CPU workers prepare changed/new audio and ANLZ in a local temporary directory.
A single dedicated USB writer starts copying as soon as a track is ready while
other tracks continue processing. A bounded queue allows two prepared tracks
to wait behind the active copy; workers also hold at most their current result.
Backpressure prevents preparing the whole library ahead of a slow USB.

Unchanged tracks bypass both queues. The processing panel counts only tracks
that need audio or analysis work and separately shows unchanged/reused tracks.
It displays the actual pool size and active tracks. The copy panel has its own
file counter, confirmed byte count, per-file copy/read-back progress and queued
track count. Its current track/phase remains independent of CPU activity. The
total byte count is shown only after all output sizes are known.

An error stops new processing/queue intake. In-flight threads are joined before
returning failure; new media may remain unreferenced, while the old databases
are retained. Both libraries are built and published serially only after both
queues succeed. Fresh exports retain their incomplete marker until the same
condition and final library publication succeed.

The terminal distinguishes processing counters from copied-file counters. It
reports paths, preparation/reuse, cache activity, flush/verification stages and
completion time. Frequent byte updates refresh the panels without flooding logs.
The current job retains 1,000 entries across navigation; logs are not persisted
across app restarts. Transfer speed has not been benchmarked for this revision.

## Publication, backups and recovery

Large media are prepared in a local `rau-usb-prepare-*` temporary directory,
then copied once to their final USB paths by the writer. This removes the former
second USB write from a USB staging folder to the final path. The source checksum
is accumulated during copying; the flushed USB file is independently read back.
Existing media/analysis are never replaced. Local temporary space is needed for
in-flight workers, queued output and decoded waveforms.

Only the small final database/report set uses `.rau-stage-*` on the destination,
so its publication renames and backups remain on the same filesystem. The old
databases continue referencing the old media until every new copy is verified.

The previous `export.pdb`, `exportLibrary.db` and `rau-export-report.json` are kept
under `.rau-backups/<timestamp-random>/`. A flushed `.rau-update-journal.json`
records their hashes before moving the metadata. Existing `DEVSETTING.DAT` is
retained. A normal publication error attempts restoration automatically.
During an update, the previous JSON receipt remains present until the final
metadata publication. The fresh-export incomplete marker is not used for
updates, so its absence does not establish that an update has finished.
Temporary cleanup is an explicit final phase; a cleanup error after successful
publication is reported as a warning while retaining the completed library.

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
