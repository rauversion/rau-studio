# Local musical preparation

Updated: October 24, 2026. Initial implementation; detection accuracy and
player alignment have not yet been evaluated on music or hardware.

This implements the first version of **step 6, Incorporar análisis musical propio**,
from the agreed USB roadmap: BPM/beats/downbeat proposals, manual grid correction
and key detection. Phrase and vocal analysis remain later work.

## Use

1. Open a local track's details from the catalog, playlists, or artist/album browser.
2. Under **Preparación musical**, choose a BPM range and **Analizar pista**.
3. Listen, seek with the overview, and inspect the numbered grid on the zoomed
   waveform. Orange lines indicate beat 1; the green line is the playback position.
4. Correct BPM directly, tap the tempo, or use half/double tempo. Set any known
   beat's time and number, or mark the listening position as beat 1. Shift the
   grid by 10 ms and check it near both the start and the end.
5. Review the key. Leave **Mantener tonalidad actual** to preserve existing metadata.
   Disable **Aplicar grilla de Rau** to retain an existing saved/imported grid.
6. Choose **Guardar preparación**. Future USB exports use the saved preparation
   for both Device Library and OneLibrary. Analysis proposals alone are not exported.

Reanalysis creates a separate proposal and preserves previously saved
preparation. **Ver preparación anterior** restores that version in the editor.
Closing the sheet while work is running does not cancel the backend worker;
reopen it after completion to retrieve the proposal. Processing is limited to
one analysis at a time. Reopening the app retains completed proposals and saved
preparation; in-flight work does not survive closing the app.

Saving updates Rau's catalog metadata and search index. It does not write
audio tags, source XML, an already exported USB, or a Rekordbox installation.
Imported XML continues to supply existing cues and loops. The new analyzer
does not automatically invent cue points.

## Implementation

| Layer | Responsibility |
| --- | --- |
| `crates/aifficator-core/src/music_analysis.rs` | Independent streaming DSP, grid/key validation, time normalization |
| `src-tauri/src/playlist_index/music_analysis.rs` | Read-only decoding, file identity, proposals, explicit save, catalog/search updates |
| `src/components/tracks/MusicAnalysisPanel.tsx` | Listening, numbered waveforms, tempo/grid/key review |
| `src-tauri/src/playlist_index/usb_export_source.rs` | Apply saved preparation to the common USB plan, preserve XML cues |

FFmpeg streams mono float PCM at 11,025 Hz to Rust; audio is not loaded as a
whole-track sample array or stored in SQLite. The limit is two hours, with a
two-second minimum for waveform/key preparation. A tempo proposal additionally
requires at least eight seconds and sufficient transient events.

The independently implemented radix-2 FFT measures positive spectral change.
Locally centered novelty peaks are folded over candidate beat periods, with
a fine search around the best candidate and a mild octave tie preference.
Short RMS rises refine beat phase on the decoded timeline. Low-frequency accent
averages suggest a 4/4 bar position. Spectral pitch peaks accumulate a normalized
chroma histogram, scored against Rau's own major/minor scale templates.
Insufficiently distinct results remain unknown. Clarity labels are heuristic
scores, not calibrated probabilities or measured detection accuracy.

The grid is constant-tempo 4/4. It cannot follow tempo ramps, variable meter,
or expressive timing. Half/double-tempo mistakes, offbeats, incorrect bar phase,
and relative-key confusion remain possible. Reviewing the first beat and
checking drift at the end are essential to this initial workflow. There is no
dynamic-grid editor, metronome, phrase labeling, vocal detection, or batch queue
in this version. Those are separate follow-ups.

`playlist_track_analysis` stores proposals and saved preparation separately,
keyed by catalog library/track ID with cascading deletion. Each record has an
analyzer version, revision, source path, size, modification timestamp and SHA-256.
Loading, saving and USB preparation reject stale results. The common export
plan also carries the source hash so a later source change stops publication
instead of applying an old grid to different audio. Source identity checks do
not correct codec delay or prove alignment after lossy decoding/conversion.

## Reference review and provenance

The public `main` branch of rbxport was reviewed on October 24, 2026. Its
[`rbl-analysis` documentation](https://github.com/chrisle/rbxport/blob/main/crates/rbl-analysis/README.md)
and [entry point](https://github.com/chrisle/rbxport/blob/main/crates/rbl-analysis/src/lib.rs)
confirm audio-derived tempo, beat grids, downbeats, key and waveforms, with
constant and variable tempo modes. Phrase labels and vocal detection are
unimplemented. Its [analysis UI documentation](https://github.com/chrisle/rbxport/blob/main/docs/user/analysis-settings.md)
describes independent stage selection, retained cues and saved per-batch options.
These are behavioral references, not dependencies or imported implementation.
Rau uses its own streaming design and a smaller, constant-tempo first version.
No rbxport source files, tuned models or analyzer routines were copied or adapted.

General background on onset-based tempo estimation is available in
[Ellis, Beat Tracking by Dynamic Programming (2007)](https://www.ee.columbia.edu/~dpwe/pubs/Ellis07-beattrack.pdf).
Rau's current periodic-folding search is independently authored and does not
implement that paper's dynamic-programming tracker.

## Checks and remaining acceptance

Rust and TypeScript compile checks and the frontend production build pass.
No new automated tests were added or run for this implementation. Musical
accuracy, manual correction with real audio, and exported grid/key behavior
still require listening and player acceptance. No USB was modified for this step;
MIKI RED remains outside the workflow.
