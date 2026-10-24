# USB export prototype

Updated: October 24, 2026. Branch: `feature/usb-export`.

The target is independent USB export for both classic DeviceSQL players and
newer OneLibrary players. The implementation generates both classic DeviceSQL
and encrypted OneLibrary **directly in the selected folder or USB root**. The user's XDJ-1000MK2 is the first
available hardware target; the data model and writer do not depend on that model.
The user clarified that the device is an XDJ-1000MK2 and reports that the
diagnostic USB works but shows a monochrome waveform. The new complete export
sets the USB preference to RGB; physical RGB acceptance remains pending. On
October 24 the user confirmed that Rekordbox reads the new Rau-generated dual
library. Detailed cue/grid/loop checks and acceptance on OneLibrary hardware
remain pending.

## Implemented

- An app screen at **Playlists → Exportar a USB**, also accessible from the
  library header and a Rau-created playlist's toolbar. It selects existing
  indexed playlists and drafts, previews the selection and destination, and
  shows background progress and the result in Spanish or English.
- A common track/playlist export model with stable IDs within one plan, nested
  folders, Unicode metadata, ordering, and repeated playlist entries.
- Explicit XML playlist/folder selection, source availability checks, and a
  read-only plan command. Missing tracks stop the export instead of changing a set.
- Independent DeviceSQL page and row encoding, including table index pages,
  multi-page data, row masks, page-local row slots, metadata dictionaries,
  Unicode/long strings, and playlist references. Empty tables terminate at their
  index page; index entries preserve the reserved trailer.
- Independently encoded classic browse columns, categories and sort defaults,
  with reference checks during inspection. No captured table blobs are embedded.
- An independent SQLCipher 4 OneLibrary writer for `PIONEER/rekordbox/exportLibrary.db`.
  Both formats use a common metadata catalog, export identity, track/playlist IDs,
  audio paths and analysis references. OneLibrary creates all 22 protocol tables,
  including required empty tables, and preserves folder hierarchy, playlist order,
  repeated entries, Unicode names and 0–5 ratings. Menus share the classic profile.
  Both databases are checked against the plan and each other before completion.
- A minimal `PIONEER/DEVSETTING.DAT` writer with RGB waveform preference and
  CRC-16/XMODEM validation. It does not change desktop Rekordbox preferences or
  the player's other My Settings. RGB/3Band data are both retained in ANLZ.
- MP3 and AIFF (`.mp3`, `.aif`, `.aiff`, case-insensitive) copied byte-for-byte,
  preserving their tags and audio. FLAC, WAV and other formats are converted to
  PCM16 stereo AIFF at 44.1 kHz using FFmpeg, including mapped metadata/ID3 tags.
  FFprobe checks the actual exported container/codec and audio parameters; the
  database records MP3/AIFF format codes, size, sample rate, bit depth and bitrate.
  Duration comes from decoded samples, not XML or estimated VBR duration.
- Original source filenames (extension retained for copies, `.aiff` for conversions), grouped under
  `Contents/<artist>`. Missing artist metadata uses `Artista desconocido`.
  Names that collide after sanitization, case conversion or Unicode normalization
  receive a suffix such as ` (2)`. Artist folders also resolve sanitized-name
  collisions. Invalid filename characters become `_`; reserved names get an `_`
  prefix, and long names are shortened at a UTF-8 boundary. Artist folders fit
  within 80 bytes and complete device paths within 240 bytes.
- Preservation of XML `TEMPO` and `POSITION_MARK` attributes in the library,
  with a typed analysis model for selected tracks. Invalid grids or cues stop
  their export with a track-specific error.
- Independently written ANLZ DAT/EXT/2EX files: imported 4/4 grids, memory cues,
  hot cues and loops, Unicode cue names in EXT, and blue/RGB preview/detail waveforms
  generated from the actual decoded audio. DAT now includes PVBR framing even
  for PCM, using the zero seek table observed on the reference AIFF files.
  DeviceSQL points to each DAT and
  requests hot-cue auto-loading when the track has hot cues.
- A bounded read-only DeviceSQL inspector and semantic comparison against the
  export plan. It can also inspect a known-good `export.pdb` supplied locally.
- SHA-256 receipts, source-change detection during preparation, an incomplete
  marker on failure, and refusal to replace existing export files or folders.
- A reproducible two-track demo with generated quiet tones and a playlist ordered
  `1 / 2 / 1`, known timing markers, and a JSON report with audio/analysis hashes.

### OneLibrary generation

Normal UI and CLI exports now create `PIONEER/rekordbox/exportLibrary.db`
alongside `export.pdb`; no Rekordbox installation, conversion or process is
required. The historical Rust/CLI `write_classic_*`/`export-classic` names are
retained for callers but now produce both libraries.

The writer uses SQLCipher 4 through the same rusqlite/libsqlite3-sys engine as
the app's catalog, with vendored OpenSSL. Catalog connections remain unkeyed;
only the exported OneLibrary connection receives the publicly documented,
device-independent format key. The app does not read account credentials or
installation keys. The schema was checked against a read-only local copy of
the user's Rekordbox-created Plus library; its `length` field is in whole
seconds, despite an older public description calling it milliseconds.

A local transaction creates 22 tables and populates tracks, metadata, playlists,
browse settings and device properties. Required unsupported tables remain empty.
Per-track cues and loops remain in the common ANLZ companions; SQL cue-bank
indexes, hot-cue bank lists, My Tags, histories and recommendations are not
authored. The finished connection uses DELETE journal mode and is closed before
its encrypted bytes are copied to the USB; no WAL file is required for playback.

Receipt schema **5** adds `one_library_sha256`, `device_settings_sha256` and
`library_identity`. Both formats have the same nonzero export-scoped database
identity and track master IDs. These IDs are not persistent sync identities.
Read-back checks compare metadata, paths, format codes, ratings, playlist order,
duplicates and browse settings. Ordinary SQLite cannot read the generated Plus
file without its format key; the encrypted read-back passes integrity checks.

`usb_export inspect-onelibrary EXPORTLIBRARY.DB` performs read-only inspection.
`usb_export export-plan PLAN.JSON NEW_FOLDER` prepares both libraries from a
bounded serialized Rau plan. A full independent two-AIFF export, generated
without Rekordbox, is retained at
`target/usb-onelibrary-2026-10-24/export-final/` with hash verification evidence.
The UI reports that both libraries are generated and keeps OneLibrary hardware
acceptance explicitly pending.

The full export was installed on TEST after preserving its previous PIONEER
folder. The user confirmed that it works in Rekordbox. After opening, both
databases differed from Rau's published hashes; all six analysis companions,
both audio files and the RGB device settings remained unchanged. This confirms
user-observed software acceptance, but does not prove that Rekordbox accepted
every database field without rewriting it. RED remained unmounted.

## Current limits

This remains an experimental export, now with a UI in the app. It does not write
PQT2, phrase/vocal analysis, artwork, or a DJ's My Settings. Custom cue colors and active
or quantized-loop flags are not exported. Cue RGB values remain in the report.
The generated browse profile and analysis subset still need complete export
checks and hardware acceptance before a compatibility claim. A controlled
two-AIFF diagnostic now displays grids, a hot cue and a colored 3Band deck
overview in Rekordbox 7.2.3. After correcting the classic analysis profile and
regenerating Plus, both tracks also show colored browser previews without `?`
in the classic and Plus playlists. RGB display and physical-player acceptance
remain pending. The observed USB is a diagnostic composite that Rekordbox has also
updated, so this does not establish acceptance of a pristine full Rau export.

The [local musical preparation](music-analysis.md) editor can now propose BPM,
a constant-tempo 4/4 grid and key from audio. Only explicitly saved preparation
overrides XML grids/key in a USB plan; XML still supplies existing cues/loops.
Source hashes prevent stale saved timing from accompanying changed audio.
This initial detector has not yet been evaluated on music or hardware.
`AverageBpm` alone remains display metadata. Imported times use the decoded PCM
timeline without codec-delay correction. Alignment of converted MP3/AAC and
other compressed sources still needs verification against source playback.

Copied MP3/AIFF files retain their original parameters. The current reader accepts
mono/stereo MP3 and 16/24-bit PCM AIFF; unsupported codecs or mismatched extensions
stop the export rather than silently changing a copied source. Acceptance of
original sample rates and other parameters still depends on the player.
MP3 compressed-frame seek offsets are not generated; its analysis report warns
that VBR seeking still requires validation. The empty PVBR compatibility record
is not a generated VBR seek index.
Other formats are resampled/downmixed for conversion to AIFF, and file size can
grow. Waveforms use a temporary local PCM WAV decode of each final audio file;
the decode is cleaned up after that track and is never published to the USB.
The UI estimates size from actual bytes for copies and catalog durations for
conversions; it does not check destination or temporary disk space, the physical
device's partition map, or its partition position. It has no media
discovery, ejection, cancellation, incremental sync, or recoverable replacement
of an existing USB library. Destinations may contain unrelated files, but must
not already contain `PIONEER`, `Contents`, `rau-export-report.json`, or
`RAU_EXPORT_INCOMPLETE.txt` (including symlinks and case variants). The writer
creates the marker and reserves new top-level directories exclusively. A failed
run retains its `RAU_EXPORT_INCOMPLETE.txt` marker and any partial output;
it is not a usable export. Publication is not a whole-volume atomic transaction.

Temporary IDs are scoped to one export. They are **not** suitable for sync across
changing selections; persistent identity storage belongs in the sync milestone.

## Export from the app

1. Open **Playlists → Exportar a USB**. A library-header shortcut preselects the
   active playlist; the draft toolbar preselects that draft.
2. Select one or more playlists, optionally filtering by playlist or library name.
   Empty playlists cannot be selected.
3. Choose the final destination folder (the **USB root** for a player), then click
   **Revisar y continuar**. The main button explains any missing requirement
   beside it. `PIONEER` and `Contents` will be created directly in that folder;
   there is no enclosing export folder or manual move afterward.
4. Review unique catalog-track count, source size, estimated output size,
   counts of tracks copied unchanged or converted to AIFF,
   imported-cue count, missing-grid count, and notices. The audio estimate excludes
   databases/analysis and is not a capacity guarantee. The same button now says
   **Preparar exportación**; click it to start. Changing the playlists or destination
   returns the button to review. Existing export folders/files are refused; this does
   not format a device or publish over an existing USB library.
5. Track progress through audio preparation, analysis and library publication.
   Navigation is allowed; returning to the screen reads the current job from the
   backend. The app must remain open. Jobs are not resumed after app restart.
6. On completion, use **Mostrar carpeta** or **Mostrar informe**. If exported
   to the USB root, eject it before trying it on the player. A failure retains
   partial output with the core's incomplete marker; a fresh attempt requires
   a destination without those previous export paths.

The desktop adapter snapshots the actual catalog membership/order and current
ratings in a database read transaction. Selected playlists appear at the device
root with their original names. Tracks shared within a catalog library are
prepared once. Separate catalog identities across libraries remain separate.
The reviewed snapshot is kept in the backend and identified by an opaque plan
ID and the canonical destination path; the export command cannot silently
substitute a new selection or destination. The destination is checked during
review, before starting, and when the writer reserves its paths. One export
runs at a time. Progress is measured by completed tracks, not elapsed time/bytes.

The existing catalog stores track attributes but does not persist XML tempo/cue
children. This adapter recovers them from known source XML files, newest source
first, and checks that the original track resolves to the same audio path.
Unavailable/changed source files or unmatched converted paths produce notices
and omit that source analysis. Local-only tracks also receive notices. The
report records these limits; the UI never implies that cues were recovered when
they were not. Source XML files and existing catalog records are not rewritten.

Implementation: `src/UsbExportPage.tsx`, `src-tauri/src/usb_export.rs`, and
`src-tauri/src/playlist_index/usb_export_source.rs`. The app uses the existing
configured/bundled/system FFmpeg and FFprobe resolvers. CLI usage remains available below.

## Analysis contract

- Only explicit 4/4 XML grids are supported. Beat phase and tempo changes are
  preserved at each anchor; beat timestamps are calculated absolutely and rounded
  to milliseconds, avoiding accumulated rounding drift. Other meters, duplicate
  anchors and beats that overlap at millisecond precision are rejected.
- A track without `TEMPO` receives an empty grid and a report warning. Its actual
  audio still supplies a waveform. An average BPM never creates a grid.
- Up to ten memory cues/loops and sixteen distinct hot slots A–P are accepted.
  DAT carries A–C and memory points; EXT carries further legacy slots and all
  extended cues with names. Physical availability depends on the player.
  XML fade-in/fade-out/load markers are reported as unmapped, rather than converted
  to a different cue type. Out-of-range markers and zero-length loops are rejected.
- Legacy waveforms use maximum absolute PCM amplitude across channels and a
  square-root height curve. DAT retains 400-column and 100-column blue previews;
  EXT retains 150 monochrome detail columns per second, including the final
  partial column. Legacy preview floors remain unchanged.
- EXT also includes a 1,200-column RGB preview (PWV4) and RGB detail at 150
  columns per second (PWV5). Rau's independent visualization uses 20 Hz DC
  removal and one-pole crossovers at 250/2,500 Hz, measures low/mid/high energy
  per channel, and maps relative RMS to red/green/blue. Stereo channels are never
  summed, so opposite-phase content remains visible. RGB silence is zero.
  Colors are not calibrated to Rekordbox's analyzer.
- 2EX additionally carries 1,200-column PWV6 and 150-column/second PWV7 for
  Rekordbox's 3Band mode. Fields are mid/high/low energy in that wire order.
  Rau uses linear RMS heights for the stacked preview (combined range capped
  at 127) and square-root RMS for detail, with separate channel filtering and
  zero silence. No vocal/phrase metadata is synthesized.
- Analysis folders follow the final audio path's documented UTF-16 hash. The
  planner resolves collisions by adding a suffix to the audio filename
  before encoding the database and analysis.
  All companions embed the same final path in `PPTH`. Manually supplied plans
  with colliding hashes are still rejected.
- The ANLZ reader checks framing, section lengths, UTF-16, beat order and cue
  payload bounds. Export compares decoded paths/grids/cues with the input model
  and reads written files back before publishing the database. This establishes
  internal consistency, not independent player acceptance.
- Report schema version 3 includes per-track copy/conversion intent, actual audio
  format/bitrate, analysis counts, DAT/EXT/2EX paths and hashes, plus explicit warnings
  and limitations. Copied files are compared with the source SHA-256 as well as
  checked for source changes. `hardware_validated` stays `false`.

## Run locally

Run from the repository root. FFmpeg must be on PATH, or set `RAU_FFMPEG` to its
executable path. FFprobe defaults to the adjacent executable (or PATH for a bare
FFmpeg name); set `RAU_FFPROBE` to override it. The app UI uses the configured,
bundled or system tool resolver for both executables.
Commands never format or erase a USB. The CLI wrappers below require a new output
directory; the app uses the direct writer for an existing destination.

Generate a small sample in a directory that does not already exist:

```sh
cargo run -p aifficator-core --example usb_export -- demo target/usb-export-analysis-demo
```

List XML playlist paths and inspect a plan:

```sh
cargo run -p aifficator-core --example usb_export -- list /path/library.xml
cargo run -p aifficator-core --example usb_export -- plan /path/library.xml "ROOT/Set"
```

Prepare selected playlists or folders (multiple paths are accepted):

```sh
cargo run -p aifficator-core --example usb_export -- export-classic /path/library.xml /path/new-output "ROOT/Set"
```

Inspect a generated or reference database without changing it:

```sh
cargo run -p aifficator-core --example usb_export -- inspect /path/new-output/PIONEER/rekordbox/export.pdb
cargo run -p aifficator-core --example usb_export -- inspect-analysis /path/from/rau-export-report/ANLZ0000.DAT
```

Example layout (analysis hashes depend on the final audio paths; consult the report):

```text
Contents/Artist name/Original track name.mp3
Contents/Artist name/Another track.aiff
Contents/Artista desconocido/Track without artist.aiff
PIONEER/rekordbox/export.pdb
PIONEER/USBANLZ/<shard>/<hash>/ANLZ0000.DAT
PIONEER/USBANLZ/<shard>/<hash>/ANLZ0000.EXT
PIONEER/USBANLZ/<shard>/<hash>/ANLZ0000.2EX
rau-export-report.json
```

The report records exactly which format and features were produced. Its
`hardware_validated: false` is intentional. For the generated demo, source paths
in the report refer to temporary synthesis inputs that are deleted after the run;
the AIFF files under `Contents` are self-contained. Previously generated sample
archives retain the earlier numeric WAV layout and have not been regenerated.

## XDJ-1000MK2 validation

The user initially reported an XDJ-1000 and later clarified XDJ-1000MK2;
firmware remains unknown. The first read-only
comparison found MBR/FAT32 on the working MIKI RED and GPT/FAT32 on MIKI TEST.
The user reformatted TEST as MBR/FAT32 and exported two AIFF tracks, but the
player still reported `NO USB`. **The user then confirmed that TEST is recognized
when Rau's `PIONEER` library is disabled.** This isolates a library-loading problem;
reformatting alone was not sufficient.

The current writer fixes index-table reserved bytes, generates browse defaults,
and stops allocating empty data pages in empty-table chains. The original export
and successive diagnostic revisions are preserved locally. Rekordbox 7.2.3 now shows the playlist and both tracks without the previous
corruption symptom. The initial analysis-load failure (deck BPM 0.00, empty
cues/previews) was resolved in the controlled PVBR diagnostic below: the deck
now displays 126 BPM, beat-grid markers, hot cue A and monochrome previews for
both tracks. After loading, DAT/EXT and audio hashes still match Rau's files;
Rekordbox did not regenerate them. Rekordbox did rewrite the database, so this
does not establish acceptance of a pristine end-to-end Rau export. The `?`
status remains; exact cue recall, grid alignment, RGB display and player
acceptance remain unverified. See the
[reference comparison](usb-reference-comparison.md) for the full sequence and the
Rekordbox automatic-history-update incident on RED. Keep RED unmounted during
software checks; merely opening Rekordbox is not a read-only operation.

The confirmed XDJ-1000MK2 supports BLUE/RGB waveform display, as listed in
[rekordbox's official compatibility table](https://rekordbox.com/en/support/faq/v7/).
Its monochrome result therefore requires checking the selected USB waveform
color and RGB rendering; acceptance of 3Band in desktop Rekordbox is separate.
Software acceptance is a prerequisite for this diagnostic, not proof of firmware
compatibility. The controlled test USB contains `abril-2026` with two actual
tracks; the generated demo described below is a separate fixture.

1. Confirm the exact model/firmware. Keep RED unmounted and use TEST only.
   TEST already has MBR/FAT32; no further formatting is indicated by this test.
2. Inspect the reference database and compare its table/page structure and browse
   defaults with the generated sample. Resolve structural differences before
   attempting player acceptance.
3. Once ready, choose the separate test USB's **root** in the app and export
   directly there. For a CLI-generated sample, copy its **contents** to that root
   instead. `PIONEER` and `Contents` must be at the root. Do not merge this
   prototype with an existing performance library.
4. Record USB recognition, playlist visibility, names, `1 / 2 / 1` ordering, both
   tracks loading, playback, and reconnect behavior. The two demo tracks each
   contain 12 seconds of quiet stereo pulses, with different pitches by channel.
5. For **each** track, record whether the preview/detail waveform is present on
   load, whether the grid shows 120 BPM with beats every 0.5 seconds, and whether
   hot A jumps to 0 s, B recalls the 4–6 s loop, C jumps to 8 s, and the memory
   cue recalls 10 s. There are 24 beats in each 12-second track. Name display is
   player-dependent; the extended memory name is `Memoria ñ`.
6. Check grid and cue timing audibly; file structure alone cannot establish
   alignment. Record quantization behavior separately and repeat after reconnect.
   A pass on this XDJ does not establish compatibility on other players.

| Target | Format | Firmware | Status |
| --- | --- | --- | --- |
| User's XDJ-1000MK2 | DeviceSQL | Pending | Earlier diagnostic works according to user; fresh export sets RGB; physical RGB and detailed checks pending |
| Other classic players | DeviceSQL | Per device | Target coverage; not tested |
| Newer OneLibrary players | OneLibrary | Per device | Rau writer implemented; Rekordbox reading confirmed by user; physical-device acceptance pending |

## Next implementation milestones

1. Establish software and classic-player acceptance of the corrected library,
   including browse defaults. Add fixtures generated
   from our own exports and record their origin.
2. Reproduce the accepted 3Band browser/deck display with a pristine full export,
   validate RGB and physical-player waveforms, and extend the analysis subset with
   cue colors and reference-driven compatibility details. Add focused analysis
   regression coverage when validating this milestone, including the new
   destination and filename planning behavior.
3. Extend the user-confirmed OneLibrary software acceptance to hardware, including players that
   use OneLibrary exclusively. Track any required SQL cue-index or advanced
   feature coverage separately from the existing ANLZ cue/loop export.
4. Extend the implemented Tauri/UI flow with device profiles, partition-map and disk-capacity checks,
   staged volume publication and cancellation. Folder export, selection review,
   background track progress and compatibility notices are now implemented.
5. Stable persistent IDs, recovery, incremental sync, and preservation of edits
   made by players. These require their own interruption and conflict checks.

## Implementation provenance

Rau's encoder, reader, planner, and tests were written independently. No rbxport
Rust/TypeScript source, libraries, captured default-row blobs, or media fixtures
were imported. The public descriptions below were consulted for interoperability
facts such as field offsets, endian order and required format constants:

- [Deep Symmetry: DeviceSQL export structure](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html)
- [Pioneer DJ: XML format for playlist sharing](https://cdn.rekordbox.com/files/20200410160904/xml_format_list.pdf)
- [Deep Symmetry: analysis files](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html)
- [Early ANLZ format observations by CodeKiller](https://reverseengineering.stackexchange.com/questions/4311/help-reversing-a-edb-database-file-for-pioneers-rekordbox-software)
- [rbxport: USB format observations](https://github.com/chrisle/rbxport/blob/main/docs/reference/usb-export-db.md)
- [FFmpeg command options](https://ffmpeg.org/ffmpeg.html) and
  [FFprobe structured format/stream metadata](https://ffmpeg.org/ffprobe.html)

For the ambiguous constants in older prose, the written format values are
`0x00080000` in PQTZ, `0x00010000` in PWAV/PWV2 and PCPT, and `0x00960000`
in PWV3. No captured ANLZ blob or implementation from those projects was copied.

Changing names or rephrasing GPL code would not by itself establish independent
authorship. This work uses format descriptions and newly authored logic; it is
not claimed to be a formally audited clean-room implementation or a legal opinion.

## Verification

`cargo test -p aifficator-core` covers multi-page encoding, Unicode/long strings,
repeated entries, documented binary fields, malformed input, missing references,
output preservation, and PCM validation. The CLI demo additionally exercises real
FFmpeg conversion and database read-back. These checks establish internal
consistency; reference/Rekordbox/player acceptance is still pending.

For the analysis milestone, `cargo check -p aifficator-core --examples` and
`cargo clippy -p aifficator-core --all-targets -- -D warnings` succeeded. The new
demo was generated with real FFmpeg conversion and built-in ANLZ/grid/cue
read-back. Both tracks contain 24 beats, 400/100 preview columns, 1800 detail
columns, three hot cues and one memory cue. The unit-test suite was not rerun
in this step, and analysis-specific regression tests have not been added yet.

Implementation: `crates/aifficator-core/src/usb_export/`, runner:
`crates/aifficator-core/examples/usb_export.rs`, tests:
`crates/aifficator-core/tests/usb_export.rs`.

For the UI milestone, TypeScript type checking, the Vite production build and
`cargo check -p rau-studio --lib` succeeded. The Vite build reports a
bundle-size warning for chunks larger than 500 kB. No unit/UI tests or hardware export were run in this
step; accepting the generated USB on a player remains a separate validation.

For the direct-destination, artist-folder and mixed-format update, TypeScript
type checking and `cargo check -p rau-studio --lib -p aifficator-core --examples`
succeeded. Existing test fixtures were adapted to the expanded audio model;
no tests or exports were run for this update. MP3/AIFF copying, conversion to
AIFF, and the new paths still require interactive and hardware validation.

### Database-only diagnostic rebuild

`usb_export rebuild-database REPORT NEW_EXPORT.PDB` independently generates a new
DeviceSQL database from a saved Rau receipt, validates it, and refuses to replace
an existing output. It does not copy audio, change the receipt, or publish to a
USB. This allows a controlled database-only comparison with unchanged audio and
ANLZ. Publication to TEST is a separate, backed-up diagnostic operation.

The current correction passed `cargo test -p aifficator-core`: 17 passed and one
FFmpeg integration test ignored. `cargo check -p rau-studio --lib` also passed.
The inspector accepts the original and post-history-import RED snapshots and a
fresh empty database created by Rekordbox. These are structural checks, not an
independent demonstration that the writer's output works in Rekordbox or a player.

### Analysis-load correction after reviewing rbxport

At the user's request, the implementation at rbxport revision
`b739d369665b492534dc2eae0733322c040a26d5` was read for interoperability diagnosis.
Its ANLZ author and export repair path identify a missing PVBR section as a cause
of Rekordbox 6/7 rejecting DAT analysis. Rau omitted this section. Independent
local inspection of the saved RED AIFF samples confirms a 16-byte PVBR header,
1,620-byte total section, and zero payload. Source files remain in ignored local
research storage; no upstream implementation, dependency, or captured payload
was incorporated into Rau.

The independently written Rau encoder now emits the zero record. The bounded
reader checks PVBR framing and duplicate sections. A PCM-only diagnostic helper
inserts the section after PPTH and preserves every prior section byte-for-byte;
`usb_export repair-pcm-analysis INPUT.DAT NEW.DAT` writes a new file and refuses
an existing output, a DAT already containing PVBR, or a compressed-audio path.
A regression test covers encoding, repair equivalence, malformed/duplicate tags,
grid/cue preservation, and the MP3 warning. Eleven USB tests pass.

This was a controlled DAT-only experiment: two AIFF tracks in TEST, with the
existing database, audio and EXT untouched during publication. Rekordbox 7.2.3
then displayed 126 BPM in the deck, beat-grid markers, hot cue A (`1.1Bars`) and
browser previews for both tracks. The user supplied a matching screenshot.
Post-load SHA-256 checks confirm both DAT files, both EXT files and both audio
files are unchanged. The database was rewritten by Rekordbox and its snapshot
is retained separately. Audible playback, exact timing/recall, RGB and hardware
acceptance are still pending; the `?` status remains. The reviewed
writer itself omits PQT2 when generating a new grid, so PQT2 absence alone is
not established as the blocker.

Code evidence:
- [ANLZ author: required PVBR record](https://github.com/chrisle/rbxport/blob/b739d369665b492534dc2eae0733322c040a26d5/crates/rbl-anlz/src/encode.rs#L447)
- [Export: repair missing PVBR](https://github.com/chrisle/rbxport/blob/b739d369665b492534dc2eae0733322c040a26d5/crates/rbl-export/src/lib.rs#L1536)
- [Zero record format](https://github.com/chrisle/rbxport/blob/b739d369665b492534dc2eae0733322c040a26d5/crates/rbl-anlz/src/write.rs#L91)

### RGB preview follow-up

The user correctly observed that the accepted v4 preview has one color. The
installed files only contain blue PWAV/PWV2/PWV3 data. The production encoder
now additionally writes PWV4 and PWV5, generated from decoded audio using the
independent filter/energy model described above. Wire dimensions and bit fields
come from the public [ANLZ description](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html)
and [protocol observations](https://djl-analysis.deepsymmetry.org/djl-analysis/track_metadata.html).
Header lengths/constants were corroborated against saved local reference
files. No upstream DSP implementation or waveform payload was incorporated.

Tests use equal-amplitude 80 Hz, 1 kHz and 8 kHz tones to establish distinct
dominant RGB channels, plus silence, opposite-phase stereo, PCM16/24,
44.1/48 kHz, a final partial detail column and a one-frame file. Malformed
headers, counts and duplicate RGB sections are rejected. Twenty core tests
pass (one FFmpeg integration test ignored).

`usb_export add-rgb-analysis INPUT.EXT DECODED.WAV NEW.EXT` prepares a separate
diagnostic EXT. It requires the decoded PCM to reproduce the existing Rau
monochrome detail, refuses already-colored files and writes only a new output.
Appending its RGB tags leaves every earlier section byte-identical. The
production export receipt is now schema 4 with separate RGB column counts.
Rekordbox display and physical-player acceptance of RGB remain pending.

The consumer was configured to **3Band** (confirmed in Preferences > View >
Color), and the user explicitly chose to retain that setting. RGB data does
not supply its distinct 2EX waveform requirements. The encoder now also emits
PWV6/PWV7 in 2EX, and the inspector validates their framing/counts. The same
tone tests check their mid/high/low field ordering and silence. The diagnostic
command `create-three-band-analysis INPUT.EXT DECODED.WAV NEW.2EX` prepares a
new companion after confirming the existing monochrome detail matches the
decoded audio. It does not rewrite DAT/EXT. The user subsequently confirmed a
colored 3Band deck overview for Je Suis Music while the browser Preview stayed
blue. All six published DAT/EXT/2EX files and both audio files retained their
hashes after this observation. Deck overview acceptance is therefore recorded
separately from the unresolved browser preview. See
[the reference comparison](usb-reference-comparison.md#deck-color-accepted-browser-preview-still-blue)
for the diagnostic sequence and the independent metadata experiment affecting
the `?` indicator.

### Classic analysis status

The classic writer now emits `CONTENTLINK = 0x000c0700` at track-row offset
`0x04`, after the export pipeline has written and checked all DAT/EXT/2EX
companions. This opaque profile was observed on all 1,231 saved native reference
tracks. A controlled comparison in Rekordbox 7.2.3 removed the `?` from Rising
when only this word changed; audio and all analysis bytes remained identical.
Individual bit meanings are still unknown. This value does not establish
phrase/vocal analysis, grid accuracy or hardware compatibility.

The inspector exposes `content_link`, and export read-back checks it against
the generated profile. Diagnostic `rebuild-database` now requires a schema-4
or newer receipt, which records the complete DAT/EXT/2EX profile; older exports
must be regenerated rather than given a new analysis declaration alone.

Device Library Plus is a separate database. An existing Plus library can retain
the previous `?` indicators even after the classic library is corrected. Rau
now generates both formats itself. The earlier diagnostics below used conversion
by Rekordbox and remain distinct from validation of the new dual-library writer.

The v9 follow-up updated both TEST tracks to this profile and allowed Rekordbox
to regenerate Plus from the corrected classic library, preserving the old Plus
database as a backup. Both classic and Plus playlists now display colored
3Band previews and neither track shows `?`. All six ANLZ companions and both
audio files retain their Rau publication hashes. This resolves the observed
Preview problem in the two-track diagnostic; it does not isolate whether
Plus regeneration or invalidation of cached display state caused the color
transition. No waveform bytes or display preferences were changed.
