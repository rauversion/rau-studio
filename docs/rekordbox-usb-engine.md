# Rekordbox USB engine integration

The desktop export flow uses the private
[`rauversion/rekordbox-usb-rs`](https://github.com/rauversion/rekordbox-usb-rs)
repository, pinned to a full commit in the core manifest and `Cargo.lock`.
Rust 1.87 or newer is required by the dependency. Building Rau requires read
access over SSH; a sibling checkout is no longer required.

The installer and check workflows authenticate with the repository-scoped,
read-only `REKORDBOX_USB_DEPLOY_KEY` secret. `.cargo/config.toml` enables Git's
SSH authentication. The complete `reverse/` directory, private native corpus
and build output are excluded from the published repository. Synthetic fixtures
remain available for package CI on Linux, macOS and Windows.

`crates/aifficator-core/src/usb_export/engine.rs` adapts Rau's existing models:

- `Pdb`, `NewTrack`, `TrackProfile` and the row builders write classic catalogs,
  metadata references, nested playlists and repeated memberships. Rau supplies
  its existing browse settings, identities, paths and playback defaults explicitly.
  These defaults are application policy, not a captured universal native profile.
- On macOS and Linux, `media::decode_pcm` reads the actual exported PCM at its
  original rate/depth. `AnalysisSet` writes classic, RGB and three-band waveforms.
  `CueSet` packs memory/hot cues and loops, including the A–C/D–P DAT/EXT split,
  comments, explicit RGB, clocks and compressed seek fields.
- MP3 retains Rau's bundled FFmpeg decoder, at the stream's original rate, with
  gapless trimming disabled. `MpegFrames` supplies PVBR and cue seek from the
  original exported bytes. This decoder bridge is not claimed to be bit-identical
  to the optional native mpg123 path. MPEG2.5 remains unsupported by the scanner.
- OneLibrary retains Rau's bundled SQLCipher implementation and schema writer.
  Cue rows from the new crate are inserted in the same transaction. Global cue
  IDs are assigned during publication, so cached analysis is independent of track
  IDs. Both catalogs are checked with Rau's independent readers.

The high-level `source_export::build_snapshot` and `export::stage_snapshot` APIs
are not called: their native SQLCipher/profile configuration and whole-snapshot
flow do not match Rau's existing bundled SQLCipher, per-track cache, bounded
transfer queue and incremental backups. This integration composes the lower-level
APIs. It does not load or redistribute Rekordbox dylibs or copy a native USB profile.

Rau still owns source selection, omission consent, conversion, SHA-256 receipts,
progress, local caching, transfer workers and recovery. MP3/AIFF remain unchanged;
other formats become stereo PCM16 AIFF at 44.1 kHz before analysis. Windows uses
the new PDB writer but retains the legacy analysis path because the dependency's
combined cue API is currently Unix-only. The per-track renderer records that fact.

## Receipts, cache and updates

Receipt schema **8** adds an `engine` identifier and per-track analysis `renderer`
and `plus_cues`. Missing fields default for older receipts. The cache has a new
renderer namespace; old waveforms cannot be mistaken for new output. Increment
the renderer revision if changing the decoder or generated analysis contract.

On macOS/Linux, updating an older compatible Rau USB regenerates its old analysis
from existing USB audio, at fresh ANLZ paths. Audio bytes and track IDs remain;
previous companions stay available to the existing rollback protocol. Subsequent
unchanged exports reuse both USB analysis and the local cache. Schema 5–8 receipts
are accepted within the existing supported-table boundaries. A legacy library
with unexpected SQL cue rows remains read-only.

New cue rows are retained when reusing tracks. Consistent external edits to both
ANLZ and Plus can be preserved; inconsistent cue positions/seek are refused before
an update writes files. Unsupported histories, artwork, tags and cue-bank lists
still make the destination read-only. Publication and recovery continue to use
Rau's existing journal; this change does not claim whole-volume atomicity.

## Validation and remaining acceptance

`cargo test -p aifficator-core` includes an end-to-end AIFF export requiring no
Rekordbox/FFmpeg installation, both database readers, Unicode metadata/cue names,
loop/hot-cue distribution, cache reuse, incremental reuse and legacy migration.
It also verifies refusal of inconsistent external cue edits.

With FFmpeg available, run the mixed PCM/MP3 integration test as well:

```sh
cargo test -p aifficator-core --test rekordbox_usb_engine -- --include-ignored
```

It covers CBR/VBR byte preservation, PVBR/seek against the MPEG scanner, cue rows
with distinct track IDs and conversion from 24-bit/48 kHz WAV to AIFF.

Hardware acceptance of this integration remains pending. Previous Rau diagnostic
results do not validate the new writer or waveforms. BPM/key detection stays in
Rau's explicit preparation flow; no grid is invented from display BPM. Artwork,
PQT2, phrase/vocal analysis, inferred palette indices and XML active/quantized-loop
flags remain outside this integration.
