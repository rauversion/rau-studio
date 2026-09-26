# File Importer

File Importer converts local audio files to AIFF without requiring a Rekordbox XML. It is useful for preparing folders, external drives, or manual selections before using them in other workflows.

## Goals

- Open a folder or select individual files.
- Show the current import without mixing it with global history.
- Save each import as a browsable group.
- Convert only when the user explicitly starts conversion.
- Keep file references, statuses, and events in SQLite.
- Show `ffmpeg` progress and logs in realtime.

## Workflow

1. Open **File Conversion > File Conversion**.
2. Use **Folder**, **Files**, or drag a mixed batch of files and folders onto the import area.
3. Review the **Current Import** tab.
4. Select files or use the header checkbox.
5. Adjust concurrency if needed.
6. Run **Convert Selected** or convert a single row.
   To recreate previous conversions with the current profile, use **Regenerate selected**
   or the **Regenerate AIFF** action on a row.
7. Use **Add to playlist** to create a playlist or add the checked files to an existing local playlist.
8. Watch the bottom terminal for events and errors.
9. Open **Groups** to revisit a previous import.
10. Open **All** to inspect the global file history.

Choosing a folder or files does not enqueue conversion automatically. Items stay `pending` until the user starts a conversion action.

Consecutive drops accumulate in the active persisted drop batch and select its compatible audio files. Opening a different import group starts a new drop batch. Dropped folders follow the current **Recursive** setting, and one drop can mix multiple files and folders. Duplicate paths are collapsed; `converted/` folders, unsupported files, and macOS `._` sidecars are skipped.

## Local Playlists

The checkboxes also enable **Add to playlist**. Before opening the playlist dialog, Rau Studio indexes the selected files in a local **File Conversion** library. When `ffprobe` is available, it reads title, artist, album, genre, comments, year, duration, sample rate, and bitrate; otherwise the file name and filesystem metadata provide a safe fallback.

If a converted AIFF exists, the playlist points to that file. Otherwise it points to the original. Re-adding the same conversion item refreshes its indexed path and metadata without duplicating the track. Local playlists can also be exported as Rekordbox XML from Playlist Library.

macOS AppleDouble sidecars whose names start with `._` are ignored during scanning and removed from saved File Conversion playlists; they contain filesystem metadata, not playable audio.

## Output

AIFF files are written inside a `converted/` folder next to the source file.

```text
/Music/Artist/Track.flac
/Music/Artist/converted/Track.aiff
```

If the source is already AIFF/AIF, it is marked `already_aiff` and not duplicated. If the target AIFF already exists, **Convert** checks for missing text metadata and recovers it from the original before marking it `already_converted`. Existing nonempty tags take precedence, and audio and existing artwork streams are copied without re-encoding. A temporary file is verified before replacing the existing AIFF; a failure leaves the existing file intact. Files with complete metadata are reused without rewriting.

This also repairs AIFF files created by older versions that omitted ID3 tags. Select the original files again and click **Convert Selected**. To refresh metadata already indexed in Playlist Library, use **Add to playlist** again after recovery; this refreshes the same track IDs without duplicating them.

### Regeneration

**Regenerate AIFF** rebuilds the audio from the original, keeping the existing output
path, conversion item ID and group memberships. It is available for an existing output
or a previously completed conversion whose output has disappeared. Original AIFF/AIF
files and missing originals are excluded; selections show the eligible count.

The original's nonempty text tags take priority. Tags found only in the previous AIFF
and its attached artwork are retained. The new file is generated in a temporary file
beside the destination and checked for the audio profile, duration, tags, artwork and
successful decoding before publication. A failure before publication leaves the
previous AIFF untouched. Regeneration never modifies the original.

The backend reserves each destination across conversion requests, checks known source
collisions and limits active writers to four. A persisted publication checkpoint lets
the next refresh reconcile interrupted regenerations. Files already indexed in Playlist
Library are refreshed without recreating their IDs, memberships or ratings; failed
catalog refreshes can be retried with **Refresh**, without re-encoding. Regenerating a
file stops its local playback; the next play loads the new version.

Rekordbox Convert does not expose regeneration yet. An external application may need
to reload metadata even though the output path is unchanged.

New conversions preserve embedded text tags such as title, artist, album, album artist, genre, date, track/disc numbers, comments, composer, BPM and key using ID3v2.3. Metadata must exist in the source file; tags stored only in another application's database are not available to File Importer. New conversions remain audio-only and do not copy cover art.

## Supported Formats

- FLAC
- MP3
- WAV / WAVE
- M4A
- ALAC
- AAC
- AIFF / AIF

AIFF/AIF is considered a final format.

## Conversion Command

The conversion uses the same core profile:

```sh
ffmpeg \
  -hide_banner \
  -nostdin \
  -n \
  -i input \
  -map 0:a:0 \
  -map_metadata 0 \
  -vn \
  -ac 2 \
  -ar 44100 \
  -c:a pcm_s16be \
  -write_id3v2 1 \
  -id3v2_version 3 \
  -progress pipe:1 \
  -nostats \
  output.aiff
```

The `-n` flag prevents conversion from overwriting existing files. Metadata recovery uses a separate stream-copy operation as described above and requires both `ffmpeg` and `ffprobe`.

## Tabs

### Current Import

Shows only the latest folder or manual selection. This view refreshes whenever a new folder is imported or a saved group is opened.

### All

Shows every file reference stored in SQLite. This is the global history.

### Groups

Lists persisted import groups:

- `folder`: a scanned folder, recursive or non-recursive;
- `files`: a manual file selection.

Opening a group makes its files the current import.

## Statuses

| Status | Meaning |
| --- | --- |
| `pending` | Registered but not sent to conversion |
| `queued` | Waiting for `ffmpeg` |
| `running` | Conversion in progress |
| `converted` | AIFF generated successfully |
| `already_converted` | Target AIFF already existed |
| `already_aiff` | Source was already AIFF/AIF |
| `failed` | Conversion or validation failed |

## SQLite Persistence

Everything is stored in the local SQLite database, currently the legacy `aifficator.sqlite3` file inside the app data directory.

Main tables:

- `local_conversion_items`: one unique reference per `source_path`.
- `local_conversion_groups`: folder or manual-selection groups.
- `local_conversion_group_items`: many-to-many relation between groups and files.
- `local_conversion_events`: logs and conversion events.

Separating items from groups lets a file exist once in global history while appearing in multiple imports.

## Terminal and Events

The fixed bottom terminal shows:

- batch start and finish;
- missing-file errors;
- relevant `ffmpeg` lines;
- per-file progress;
- reuse of existing AIFF files;
- write and permission failures.

The terminal starts collapsed and can be expanded for detailed inspection.

## Concurrency

The UI proposes a default concurrency based on logical CPU cores:

```text
default = min(4, max(1, floor(logical_cores / 2)))
```

The backend also clamps concurrency between `1` and `4`.

## Tauri Commands

- `local_conversion_list_items`
- `local_conversion_list_groups`
- `local_conversion_group_items`
- `local_conversion_add_files`
- `local_conversion_scan_folder`
- `local_conversion_convert_items`
- `local_conversion_delete_item`

## Relevant Files

- `src/FileConversionPage.tsx`
- `src-tauri/src/local_conversion.rs`
- `src-tauri/src/local_conversion/regeneration.rs`
- `src-tauri/src/local_conversion/jobs.rs`
- `src-tauri/src/playlist_index/conversion_refresh.rs`
- `crates/aifficator-core/src/conversion.rs`
