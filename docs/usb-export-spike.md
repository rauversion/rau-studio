# USB export: technical spike

**Status:** initial assessment recorded on October 21, 2026. Independent DeviceSQL generation was added on October 23 and encrypted OneLibrary generation on October 24. The user reports basic XDJ-1000MK2 operation with the earlier diagnostic and successful Rekordbox reading of the new dual-library export. Broader hardware coverage and physical RGB acceptance remain pending. See [the implementation and validation status](usb-export.md). The assessment below records the original spike scope.

## Decision

USB export is technically feasible in Rau Studio and fits its Rust/Tauri boundary. It is a new format-writing subsystem. The product target is both classic DeviceSQL (`export.pdb`) and OneLibrary (`exportLibrary.db`), covering the same broad player families as the reference project. Begin validation with the user's XDJ-1000, without making the exporter specific to that model. Do not promise broad player compatibility until real-device validation is complete.

This spike is an independent design assessment. No rbxport source code was copied or adapted. Its GPL-2.0-or-later implementation is not a dependency or a source for implementation code. Rau Studio is MIT; any later dependency or code contribution must be checked for license compatibility before adoption.

## What Rau Studio had at the initial assessment

- Rekordbox XML import with playlist order, track IDs, local paths, common metadata, and arbitrary XML attributes.
- Playlist selection, local draft playlists, ratings, and local file existence checks.
- Audio conversion to AIFF through the existing FFmpeg workflow.
- Rust/Tauri commands that own filesystem work and report progress.
- Existing XML export paths in `crates/aifficator-core/src/exporter.rs` and `src-tauri/src/playlist_index.rs`.

## Gaps identified at the initial assessment

- There is no USB volume discovery/selection or removable-volume identity tracking.
- There is no writer for the DeviceSQL `export.pdb` database or the newer `exportLibrary.db` format.
- The current track model has no first-class beat-grid, cue, loop, or waveform analysis data. XML attributes may contain values such as BPM and key, but this is not equivalent to the analysis files used by decks.
- There is no USB export manifest, reconciliation, capacity preflight, or recovery path.
- Audio conversion currently targets AIFF. A hardware export needs an explicit supported-format policy and a way to avoid needlessly converting compatible audio.
- No real exported USB fixture or hardware smoke-test evidence is present in this repository.

The public format reference describes a device export as related audio, library-database, and per-track analysis files. The classic path centers on `Contents/`, `PIONEER/rekordbox/export.pdb`, and `PIONEER/USBANLZ/.../ANLZ0000.DAT`; richer analysis and optional library files add more work. See the [format overview](https://github.com/chrisle/rbxport/blob/main/docs/reference/usb-export-db.md) and its [file requirements and limits](https://github.com/chrisle/rbxport/blob/main/docs/reference/usb-export-db.md#the-short-version).

## Independent implementation shape

Keep USB export separate from XML serialization. Proposed Rust boundary:

```text
selected Rau playlist(s) + source library metadata + destination volume
    -> export preflight / immutable export plan
    -> audio placement or compatible conversion
    -> DeviceSQL library writer
    -> optional analysis-file writer
    -> staged-volume verification
    -> publication and export report
```

Possible internal modules, subject to refinement during implementation:

- `usb_export::model`: the app-owned track, playlist, identity, and export-profile inputs.
- `usb_export::plan`: validate paths and formats, calculate required space, and produce a deterministic write plan without touching the destination.
- `usb_export::paths`: generate device-safe relative paths and stable IDs.
- `usb_export::devicesql`: independently implement the supported subset of `export.pdb` records.
- `usb_export::analysis`: later, write only analysis data Rau can actually provide or generate.
- `usb_export::publish`: stage, verify, and publish files with clear interruption recovery.
- `usb_export::verify`: reopen the produced database and check every playlist, track reference, file path, size, and generated companion.

The export plan should take Rau's indexed playlist and media records as inputs; it must not mutate the imported XML or the source library. Reuse existing audio conversion only behind an explicit export profile, rather than changing the current conversion defaults.

## Suggested first milestone

Build a **new-stick, selected-playlist, DeviceSQL-only** exporter:

1. Let the user choose a mounted destination and playlist(s).
2. Refuse unsupported, missing, unreadable, or unsafe-named source files during preflight; show the exact files and approximate required space.
3. Copy compatible audio into a device-safe `Contents/` tree. Conversion stays opt-in and profile-driven.
4. Write a minimal compatible `export.pdb` with track rows, playlist rows, and references that agree with the copied paths.
5. If Rau has no valid analysis for a track, report it plainly. Do not invent cues, grids, or waveforms. Treat the initial output as a metadata/listing export until tested otherwise.
6. Build the complete candidate tree in a temporary directory on the destination volume, verify its structure and database references, then publish. Preserve unrelated files and refuse to overwrite an existing rekordbox library in this first milestone.
7. Keep a human-readable report of exported, skipped, converted, and analysis-missing tracks.

This deliberately leaves incremental sync, deleting device content, playlist editing on USB, OneLibrary, artwork, device settings, and player-to-library import out of the first milestone.

## Risks and validation gates

| Risk | Spike finding | Gate before release |
| --- | --- | --- |
| Player compatibility | Format layout alone does not establish firmware compatibility. | Test on a disposable USB with named player models/firmware; confirm library appears, playlists load, and each track loads/plays. |
| Incomplete analysis | Rau's current import model does not preserve beat grids, cues, loops, or waveform payloads as typed data. | Decide whether bare-list export is useful, or add an independent analysis acquisition/generation path first. |
| Database correctness | One broken reference or malformed record can make a device library unreadable. | Structural read-back verification plus Rekordbox import/read and hardware smoke tests. |
| Unplug or process failure | Removable drives can disappear during writes; whole-volume atomic replacement is not available. | Stage on the same volume, detect destination identity changes, maintain a recoverable publication record, and test interruption scenarios. |
| Filesystem and naming | FAT-family filesystems and player firmware impose path and filename limits. | Define a deterministic sanitization/collision policy and test non-ASCII names, long names, and duplicate filenames. |
| Licensing | rbxport is GPL-2.0-or-later while Rau Studio is MIT. | Keep implementation independently authored; review every dependency's license and get counsel for any uncertainty before distribution. |

## Hardware validation protocol

Before testing a Rau-generated export, capture this baseline:

- Player make/model and exact firmware version.
- Rekordbox version and operating system used to make the reference export.
- USB filesystem, capacity, and whether the target player reads that media format.
- A **separate test USB** and a known-good Rekordbox-authored export from the same device class. Keep the original export unchanged as a reference.

Use a tiny library with a few locally available tracks and one playlist. Include at least one non-ASCII title and one long filename only after the plain ASCII case passes. Preserve an untouched copy of the known-good export for comparison. Run each Rau build against a disposable copy; do not test first on a working performance USB.

Record each result separately:

1. The player recognizes the USB and shows the library without a corruption warning.
2. The exported playlist appears with the expected name, track count, and order.
3. Every listed track can be loaded and played from start to finish.
4. BPM/key, waveform, beat grid, and cues are checked individually; mark unavailable features as unsupported rather than treating them as a pass.
5. Remove the USB safely, reconnect it, and confirm the export still opens.

For a first prototype, passing only steps 1–3 supports a basic-listing claim for that model/firmware. It does not establish cue, grid, waveform, OneLibrary, or general device compatibility.

## Go/no-go recommendation

**Go** for a bounded prototype of the common plan model and classic DeviceSQL writer, using independently authored code and generated fixtures. Full sync and OneLibrary are later implementation milestones within the agreed product target. The first prototype establishes database and playlist behavior; it is not a complete DJ-preparation release. Analysis acquisition/generation and dual-library consistency must be addressed before claiming coverage equivalent to the reference project.

## References consulted

- [rbxport project overview and license](https://github.com/chrisle/rbxport)
- [Public USB format reference](https://github.com/chrisle/rbxport/blob/main/docs/reference/usb-export-db.md)
- [Public USB export behavior guide](https://github.com/chrisle/rbxport/blob/main/docs/user/usb-export.md)
- [Rau Studio Rekordbox conversion workflow](rekordbox-convert.md)
- [Rau Studio architecture](architecture.md)
