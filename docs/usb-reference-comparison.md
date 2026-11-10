# USB reference comparison — October 23, 2026

## Initial comparison result

This section records the first comparison. Later experiments and the RED
automatic-update incident are recorded below; initial unchanged-file statements
apply only to that first inspection.

The user reports that **MIKI RED works on their XDJ-1000**, while the library and
playlists from Rau's **MIKI TEST do not appear**. Firmware remains unknown. The
agent inspected both mounted volumes read-only; this was not a new hardware test.

The strongest actionable difference is the **partition map**: RED uses MBR with
one FAT32 partition; TEST uses GPT/GUID, with an EFI partition before its FAT32
music partition. A controlled test on MBR/FAT32 is needed before attributing the
failure solely to the database writer. The inspection also identified missing
browse defaults and analysis extensions in Rau; hardware acceptance is still
unconfirmed.

## Media and library comparison

| Item | MIKI RED (working reference) | MIKI TEST (Rau) |
| --- | --- | --- |
| Partition scheme | MBR (`FDisk_partition_scheme`) | GPT (`GUID_partition_scheme`) |
| Partitions | One FAT32 partition | EFI, then FAT32 |
| Music partition | First | Second |
| Physical capacity | 123,111,211,008 bytes | 62,914,560,000 bytes |
| Export location | `PIONEER` and `Contents` at root | `PIONEER` and `Contents` at root |
| Classic database | `PIONEER/rekordbox/export.pdb` | Same path |
| Database size | 1,388,544 bytes | 167,936 bytes |
| Page size / table count | 4,096 bytes / 20 | 4,096 bytes / 20 |
| Referenced tracks | 1,231 | 7 |
| Playlist/folder rows | 53 | 1 |
| Playlist entries | 1,426 | 7 |
| Track format codes | AIFF: 1,109; MP3: 49; FLAC: 73 | AIFF: 7 |
| Browse-related tables 16 / 17 / 18 | 27 / 22 / 17 rows | Empty |
| Analysis companions | DAT, EXT and 2EX | DAT and EXT |

The reference's track inventory is not a statement that every track or codec has
been played successfully on this XDJ. Playlist/folder counts include folders.

The current TEST export is already directly at the USB root. Its playlist is
`2019 - All My Life`, with seven AceMo tracks under `Contents/AceMo/`, retaining
the source filenames with invalid path characters replaced. It is the schema-3
export dated October 23 at 21:42 local time, not the earlier enclosing-folder WAV
export shown in the screenshot.

## Checks completed

- Read both partition maps and FAT32 volume metadata without modifying them.
- Parsed local snapshots of both classic databases with Rau's inspector. Both
  passed its structural and reference checks. This is our own parser, so a pass
  does not establish independent firmware compatibility.
- Checked all **1,238 database track references** across both volumes: each audio
  and referenced DAT exists, and every audio size matches its database row.
- Checked all seven TEST AIFF files with FFprobe: PCM signed 16-bit big-endian,
  stereo, 44.1 kHz. Two reference AIFF samples have the same parameters; one
  reference MP3 sample was also inspected.
- Checked framing and embedded audio paths for all seven TEST DAT/EXT pairs and
  three reference tracks' DAT/EXT/2EX sets. Their paths match the database.
- All **21 TEST audio/DAT/EXT SHA-256 hashes**, plus its database hash, match the
  export receipt. This found no discrepancy between the completed export and
  the files currently on the drive; it does not prove player acceptance.
- Rechecked SHA-256 and modification time for RED's `export.pdb`,
  `exportExt.pdb`, `MYSETTING.DAT`, `DEVSETTING.DAT`, and `RBFLTR.DAT`: unchanged.
  TEST's database and report were also unchanged. This is a check of those files,
  not a full-volume hash of every file on RED.

All generated diagnostics and local reference snapshots are under the ignored
`target/usb-comparison-2026-10-23/` directory. Nothing was written to either USB.
No reference binary, audio file, or upstream implementation was added to the
repository's source or fixtures.

## Interpretation and remaining differences

### 1. Partition map: first variable to isolate

Pioneer DJ's support response to an XDJ-1000 USB-recognition problem specifically
recommends FAT32 with MBR rather than GUID. That thread concerns firmware loading,
so it supports the media-format hypothesis without proving the cause of this
playlist failure:
[Pioneer DJ Community — XDJ-1000 firmware update issue](https://community.pioneerdj.com/hc/en-us/community/posts/22980146122521-Firmware-Update-1-13-Issue-XDJ-1000).

Both current drives are FAT32; checking only the filesystem would miss the
difference. Rau currently checks destination ownership/conflicts but does not
check the physical device's partition map or partition position.

### 2. Browse defaults: unresolved writer compatibility

Rau leaves tables 16, 17 and 18 empty; the reference populates them. The reference
also contains `RBFLTR.DAT` and player settings. Their absence needs a targeted
compatibility check if the library is still invisible on correctly prepared
media. Do not transplant captured rows or settings as unexplained binary blobs.

### 3. Analysis: a smaller implemented subset

The sampled RED DAT files include PVBR; Rau's DAT files omit it. Both contain
PPTH, PQTZ, PWAV, PWV2 and PCOB. The sampled RED EXT files additionally contain
PQT2, PWV4 and PWV5; Rau currently writes PPTH, PWV3, PCOB and PCO2. RED also has
2EX files, which Rau does not generate. These are observed differences, not
confirmed explanations for a completely missing library.

The reference also includes artwork and additional databases. Presence in a
reference does not by itself establish a requirement for classic DeviceSQL.

### 4. Page allocation differences are not sufficient evidence of corruption

Both databases have index pages. A fresh export's empty free/deleted-row indexes
and candidate page numbers beyond the current file length are consistent with
the documented allocation model. They should not be changed just to resemble an
older, repeatedly updated reference database. See
[Deep Symmetry — DeviceSQL export structure](https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html).

## Next controlled experiment

1. Keep RED as the unchanged reference.
2. With explicit authorization for erasing **TEST only**, back up its current
   export locally and prepare that physical drive with **MBR and one FAT32
   partition**. Reconfirm the physical disk identity at that time; mounted disk
   identifiers are not stable across reconnections.
3. Restore the same export at TEST's root and compare its hashes. Keeping the
   content identical isolates the partition-map change from writer changes.
4. Eject and test on the XDJ: record USB detection, library/playlist visibility,
   track loading and playback. Then check waveforms, grids and cues.
5. If the library remains invisible, investigate browse defaults and remaining
   DeviceSQL differences on TEST, with each change independently generated and
   documented. Add destination-media validation to Rau's export review so this
   mismatch is visible before a future export.

No formatting, restoration, or new export was performed during this comparison.

## Follow-up: `NO USB` after MBR formatting

Later on October 23, the user formatted TEST themselves. Read-only checks confirmed
MBR and a single FAT32 partition. The user then reported **`NO USB`** on the XDJ.
The reconnected TEST still has that layout and contains a completed Rau export
from 22:05 local time with **two AIFF tracks**, not the previous seven-track set.
Changing to MBR therefore did not resolve recognition in this attempt; this was
not an identical-content comparison. The message alone does not establish whether
the remaining problem is the USB device, its filesystem, or the experimental
library.

To isolate library loading, the agent prepared a reversible diagnostic on **TEST
only**:

- Copied all eight non-AppleDouble export files (92,935,390 bytes) locally to
  `target/usb-no-usb-2026-10-23/original-export/` and verified their hashes. Audio,
  analysis and database hashes match the current export receipt.
- Confirmed TEST's volume UUID, then renamed its `PIONEER` directory to
  `PIONEER.RAU-DIAGNOSTICO`. Audio under `Contents` is unchanged. Checked the
  relocated library files against their original hashes.
- Recorded the operation and backup location in
  `target/usb-no-usb-2026-10-23/diagnostic-state.json`.

**User result:** TEST is recognized without the library. This implicates loading
Rau's library rather than basic USB/media recognition. The backed-up directory
was retained. Rekordbox subsequently created a fresh empty `PIONEER` library;
that directory was preserved as `PIONEER.REKORDBOX-AUTO` before installing a
separately generated diagnostic database. Do not blindly rename directories over
one another.

## Writer corrections and software validation

Three concrete format issues were identified independently from protocol notes
and local reference observations:

- The index entry array has 1,004 entries starting at byte 60; the last 20 bytes
  are reserved. The original writer filled those reserved bytes with sentinels.
- Tables 16–18 lacked browse columns, categories and sorts. They are now generated
  field-by-field with Rau's own visible category/sort order, not copied blobs.
- Fresh empty tables must end at their index page, with a zeroed allocation
  candidate outside the chain. The original writer allocated an empty data page
  and included it in the chain. A fresh Rekordbox database confirms the distinction.

The revised inspector checks these conditions, cross-references browse columns,
and still accepts used reference tables whose allocated rows have been deleted.
Regression checks cover index bounds, empty-table layout and browse encoding.

The database-only diagnostic rebuild uses the original two-track receipt. Audio
and DAT/EXT files are retained. The v2 attempt did not make TEST visible in
Rekordbox, so its conversion prompt was not considered a successful read. The v3
attempt additionally corrects empty tables. Rekordbox 7.2.3 now displays the
`abril-2026` playlist and both tracks, and the user confirms the library no longer
appears corrupt. Playback and analysis display are not yet verified; the user
reports no visible analysis. Both original DAT/EXT pairs remain byte-identical,
with grids of 448/648 beats, 124/126 BPM, one hot cue each and monochrome waves.
PWV4/PWV5 color waveforms used by Rekordbox are absent, a relevant unresolved
difference. The visible screenshot had Device Library Plus selected; a direct
classic-library load is still required.
Rekordbox may rewrite a database on opening; retain before/after snapshots and
never confuse a repaired database with acceptance of the pristine writer output.

## RED automatic-update incident during Rekordbox testing

During software testing on October 23, RED became mounted while Rekordbox was
running. The UI showed `Importing Histories`. At 22:28:48 local time, RED's
`export.pdb` and `exportExt.pdb` were automatically modified. No agent file-write
operation targeted RED, but launching/using Rekordbox did not preserve the user's
read-only requirement. The user was informed, RED was unmounted, and a subsequent
audit used an explicit read-only mount. RED remains unmounted for further tests.

Original database snapshots from the first comparison remain under
`target/usb-comparison-2026-10-23/`. Post-update snapshots and the audit are under
`target/usb-no-usb-2026-10-23/red-readonly-audit/`. The audit found:

- The inspected track fields, playlist nodes and playlist entries are unchanged.
- History playlist/entry counts changed from 1/42 to 0/0.
- Thirty-nine track rows changed play-count bytes; other raw differences include
  padding and row relocation. Unknown fields have not all been decoded.
- The extended database changed four bytes. The three checked settings/filter
  files retain their original hashes and timestamps.

This is consistent with the visible history import, not proof of every side
effect. No restoration has been attempted; writing the original databases back
would require the user's authorization and could discard newer history state.

### User screenshot: track loading works, analysis acceptance fails

The user's screenshot shows `Rising (Joe Smooth Afro House Remix)` loaded in
PERFORMANCE mode with elapsed time 00:17.1 and a detailed waveform. This confirms
track loading and a running playback timeline; audible playback was not checked
by the agent. It does not identify which waveform source Rekordbox used. The
classic Device Library playlist is selected in the screenshot.

The metadata header shows 126 BPM, but the deck shows 0.00 BPM, the hot-cue pads
are empty, and both browser rows have `?` with blank preview columns. Rekordbox's
[official manual, page 14](https://cdn.rekordbox.com/files/20260409151936/rekordbox7.214_manual_EN.pdf)
explains `?` as insufficient or older analysis information. The symbol alone is
not proof that all analysis is absent. In this case the UI evidence and the
verified DAT/EXT contents establish a remaining interoperability problem.

Do not mark the export accepted or infer XDJ compatibility from playlist
visibility. Missing color waveform tags are one known difference, but have not
been established as the cause of missing grids/cues. A controlled companion-file
experiment is needed. Do not silently run Rekordbox's analysis and then present
its regenerated output as successful analysis from Rau.

Rekordbox remains open for the user's inspection. Its rewritten classic database
is saved locally as `browse-fix-v3/after-rekordbox-open.pdb`; inspected track and
playlist fields remain unchanged. TEST now also contains files automatically
created by Rekordbox, including its Plus database. It is not a pristine Rau
export and must not be used to claim independently generated OneLibrary support.
The original receipt database hash describes the generated v3 file before these
Rekordbox edits; current diagnostics track the distinction separately.

## Controlled follow-up: missing PVBR (October 23)

The user requested a direct code review of rbxport. At revision
`b739d369665b492534dc2eae0733322c040a26d5`, its ANLZ generation and export repair
paths explicitly identify missing PVBR as a Rekordbox 6/7 DAT rejection case.
Rau's two DAT files omit it. The saved local RED AIFF samples contain the empty
record, confirming the shape independently: 16-byte header, total 1,620 bytes,
zero data. The sampled MP3 has a nonzero trailing word, so the PCM diagnostic
must not be generalized into a claim that empty records supply VBR seeking.

An independently encoded PVBR record was inserted after PPTH in separate local
copies under `target/usb-no-usb-2026-10-23/pvbr-v4/`. Removing that record and
restoring the container length reproduces each original DAT byte-for-byte.
Grids (448/648 beats), cues, paths and waveform payloads are unchanged. The
experiment changes no audio, EXT or database bytes. Both corrected DAT files
were published to TEST after Rekordbox closed, with volume UUID checks and a
local pre-write backup. Hashes confirm that audio, EXT and the existing
Rekordbox-rewritten database did not change during publication. The report
records the diagnostic revision and hashes at publication.

**Observed result:** Rekordbox 7.2.3 displays 126 BPM in the loaded deck,
beat-grid markers, hot cue A (`1.1Bars`) and monochrome browser previews for
both tracks. The user's screenshot confirms this. Post-load hashes match for
both corrected DAT files, both original EXT files and both audio files:
Rekordbox did not regenerate the analysis. The classic database was rewritten
and saved as `pvbr-v4/after-load-export.pdb`; the hash audit is retained beside
it. RED remained unmounted. This establishes partial software acceptance of
Rau's analysis, while pristine full-export acceptance, cue recall/alignment and
physical player testing remain pending. The `?` status remains and is not used
as a claim of complete analysis support.

The user then noted that the previews are all the same color. This is expected
for the installed v4 files: they contain only Rau's monochrome waveform tags.
RGB generation was added independently to the production encoder; the
controlled PVBR result above did not depend on RGB.

The production encoder also writes the framing record; MP3 exports explicitly
warn that compressed-frame offsets are not generated. Eighteen core tests pass
(one FFmpeg integration test ignored), including the new seek-table regression;
`cargo check -p rau-studio --lib` passes. Those code checks are separate from
the consumer observation above. Upstream source was read, not imported or
adapted into the codebase.

## RGB-only follow-up (v5)

The encoder now supplies PWV4/PWV5 using Rau's own filtered-energy visualization
of decoded audio. Three equal-amplitude tone fixtures establish distinct
frequency-driven colors; silence, stereo phase, PCM depth and sample-rate
cases pass. Twenty core tests pass, with one FFmpeg integration test ignored;
the Tauri library check also passes.

Two new EXT files were prepared from the local original audio backup. The
diagnostic helper requires the decoded audio to reproduce the existing blue
detail waveform before appending RGB. Every pre-existing section byte remains
unchanged, as do the DAT grid/cues. The files and manifest are saved locally
under `target/usb-no-usb-2026-10-23/rgb-v5/`.

After Rekordbox finished its normal shutdown, the two EXT files and diagnostic
receipt were updated on TEST with a volume UUID check and pre-write backup.
Audio, DAT and database hashes remained unchanged during publication; RED was
unmounted. After reopening, previews remained blue. Preferences > View > Color
shows **3Band** selected; its required 2EX files were absent. The user explicitly
chose to retain 3Band, so the preference was not changed. RGB display acceptance
therefore remains unverified. All updated EXT and existing DAT/audio hashes
remain unchanged after opening. Rekordbox rewrote the database again.

## 3Band follow-up (v6)

The same independent DSP now also generates PWV6/PWV7 in a 2EX companion.
Layout is documented publicly and corroborated against the saved local
reference samples: 1,200 three-byte preview entries with a 20-byte header;
150 three-byte detail entries per second with a 24-byte header. Entries store
mid/high/low, using Rau's own energy-to-height curves. PWVC/vocal and phrase
metadata are not synthesized. DAT and EXT remain intact.

Tests check tone separation, band order, silence, PCM16/24, sample rates and
partial columns. The production export path writes/reads back/hashes all three
companions. A full local two-track demo confirms all six analysis-file hashes
against its schema-4 report. Both real-track 2EX files are prepared locally in
`target/usb-no-usb-2026-10-23/three-band-v6/`. After normal Rekordbox shutdown,
both new 2EX companions were added to TEST with volume UUID, existing-file and
hash checks. Audio, DAT, EXT and the current database remained unchanged during
publication. Rekordbox reopened with 3Band retained. Consumer display acceptance
and the `?` status cause remained pending at publication.

The user subsequently reported that color was still absent. A fresh observation
showed blue previews and `?` for both tracks in Device Library Plus. All six
DAT/EXT/2EX hashes still match the published Rau files; the current classic
database points at those exact analysis paths. The 3Band preference is retained.
At that point v6 had no confirmed visual acceptance; passing our parser and
tests alone was insufficient. The later deck observation below supersedes
that initial result for the deck overview, while the browser Preview remains
blue.

## Controlled metadata experiment (v7)

A read-only comparison of the saved RED database found `CONTENTLINK` at track
row offset `0x04` equal to `0x000c0700` on all 1,231 live tracks. TEST uses zero
on both tracks. The `0x56` word is 41 for both TEST tracks and 1,230 RED tracks
(the other RED track is 57), so that word does not explain this difference.
DEVSETTING body/checksum bytes also match between TEST and the saved reference.

A local candidate changes only track 2 (Rising) CONTENTLINK to the observed
reference value, leaving track 1 as a control. No waveform/control sections,
analysis status word, audio, cues or paths change. This is an experiment, not a
claim that CONTENTLINK semantics are known. The production writer remains
unchanged until consumer evidence supports a specific correction. Prepared
bytes, original database and exact byte offset are retained under
`target/usb-no-usb-2026-10-23/content-link-v7/`.

After normal Rekordbox shutdown, the experiment was published only to TEST
classic `export.pdb`. The publication used the latest post-shutdown database
as its baseline and retained a local backup. Byte comparison permits changes
only within track 2 CONTENTLINK; hashes verify unchanged audio, DAT/EXT/2EX
and their macOS auxiliary files. Device Library Plus was not modified.
Rekordbox was reopened. Both our observation and the user screenshot confirm
that Rising lost its `?`, while the control track retained it. Both previews
remain blue. Post-open hashes confirm no audio or ANLZ bytes were regenerated.
This isolates an effect of CONTENTLINK on the warning, but does not yet show
which bits matter or establish acceptance of color waveform data.

## Deck color accepted; browser preview still blue

The user clarified the result with a screenshot: the deck overview shows the
blue/orange/white 3Band waveform for Je Suis Music, while the browser Preview
column remains monochrome. All six DAT/EXT/2EX and both audio files still match
their published Rau hashes after this observation. No PWVC diagnostic was
published and no re-analysis was requested. This establishes partial 3Band
consumer acceptance, including the deck overview, independent of the v7
CONTENTLINK change (Je Suis Music was the unchanged metadata control). Browser
preview rendering remains under investigation; it must not be claimed fixed
or assumed to prove rejection of all color data.

The user subsequently reported `?` on both tracks again. Direct UI inspection
confirmed Device Library Plus > All Tracks was selected. Switching to classic
Device Library > All Tracks showed `?` only on Je Suis Music; Rising remained
clear. A read-only database check still found CONTENTLINK values 0 and
0x000c0700 respectively, with the same post-open database hash. This observation
is a difference between the two library views, not a reversal of the classic
metadata experiment. Both deck overviews visibly showed 3Band color during
this check, while both browser previews remained blue. RED stayed unmounted.

## Exporter analysis profile and TEST follow-up (v9)

The writer now serializes the observed `0x000c0700` CONTENTLINK profile for its
DAT/EXT/2EX exports and verifies it on read-back. Its individual bit meanings
remain unknown. Fourteen export tests pass, including serialization and a
read-back comparison that rejects a missing profile. A fresh two-track local
demo contains the profile on both tracks and matching hashes for all six
analysis companions. Rebuilding databases from pre-schema-4 receipts is refused
because those receipts do not describe the complete analysis profile.

After normal Rekordbox shutdown, TEST track 1 received the same word. The
production demo supplied the expected value; the diagnostic changed no other
classic database bytes. All eight audio/analysis hashes remain unchanged.
The previous Plus database was copied to the local diagnostic backup and parked
at `PIONEER.RAU-PLUS-BEFORE-V9/exportLibrary.db` on TEST for recovery. Publication evidence is under
`target/usb-no-usb-2026-10-23/content-link-v9/`.

The user explicitly chose to keep RED unmounted after automatic review rejected
a proposed read-only mount. A separate temporary image contains only the saved
local reference database, settings and three ANLZ sets. Its initial 14 file
hashes match the snapshots. The image has no reference audio and cannot validate
playback; it is only a browser Preview comparison control.

After reopening Rekordbox, a new Plus database was present and differed from
the parked backup. Direct inspection of both the Plus and classic `abril-2026`
playlists showed colored blue/orange/white previews for both tracks, with `CUE`
instead of `?` in the status column. The user confirmed the visible success.
The current classic database retains `0x000c0700` on both tracks. All six ANLZ
companions and both AIFF files still match the pre-publication hashes; the
receipt is `content-link-v9/after-open-hashes.json`. Rekordbox did not replace
the generated analysis files. Fourteen export tests and the Tauri library check
pass.

This establishes the observed browser Preview/status correction after updating
the classic profile and regenerating Plus. It does not isolate the contribution
of Plus regeneration versus cache invalidation to browser color. The temporary
reference image was not listed by Rekordbox and supplied no visual evidence;
the successful TEST observation made further reference browsing unnecessary.
RED remained unmounted throughout. Hardware acceptance and a pristine full
export validation remain pending.

## First user-reported hardware success and MK2 clarification — October 24

The user reports that the USB now works, with a monochrome waveform, and then
clarified that the player is an **XDJ-1000MK2**. This is an initial functional hardware observation, not a completed
compatibility matrix: firmware, individual cue/loop checks, timing accuracy and
whether this was a fresh full export have not been established.

The initial explanation based on the original XDJ-1000 was inapplicable to the
user's actual model. The official
[rekordbox waveform compatibility table](https://rekordbox.com/en/support/faq/v7/)
lists XDJ-1000 under BLUE only and XDJ-1000MK2 under BLUE/RGB. RGB acceptance on
this MK2 remains open; the desktop 3Band observation does not validate RGB.

A subsequent read-only inspection of TEST found `DEVSETTING.DAT` is 140 bytes,
has a valid CRC-16/XMODEM over its 32-byte body, and has waveform-color byte
`0x01` (BLUE) at file offset `0x72`. Public
[rekordcrate format documentation](https://docs.rs/rekordcrate/latest/rekordcrate/setting/enum.WaveformColor.html)
maps RGB to `0x03` and 3Band to `0x04`. Both EXT files still match the published
v5 hashes and contain PWV4/PWV5. This identifies a concrete display-setting
candidate, not yet a confirmed resolution. No device settings were changed;
the user was asked to inspect WAVEFORM COLOR on the MK2. The desktop preference
remains 3Band as requested, and RED remains unmounted.

## Rau-generated dual-library export — October 24

Rau now independently creates classic `export.pdb`, encrypted SQLCipher 4
`exportLibrary.db`, and RGB `DEVSETTING.DAT` in one export. No Rekordbox process
or conversion is required to generate them. The OneLibrary writer creates all
22 tables and shares metadata, IDs, paths, playlist hierarchy and ordering with
the classic writer. Cues and loops remain in the common ANLZ files; advanced
SQL cue-bank indexes and other unsupported tables remain empty.

The complete two-track export is retained at
`target/usb-onelibrary-2026-10-24/export-final/`. Its schema-5 report records
the shared nonzero database identity and hashes of both libraries, analysis,
audio and device settings. It was installed on TEST after checking the volume
UUID, confirming that RED was unmounted and Rekordbox was closed, and verifying
the unchanged audio. The previous PIONEER folder and receipt were preserved at
`/Volumes/MIKI TEST/PIONEER.RAU-BEFORE-ONELIBRARY`.

The user then confirmed that the export works in Rekordbox. Direct app
inspection was unavailable because the UI control timed out, so this acceptance
is explicitly user-reported. After opening, **both database hashes changed**.
All six ANLZ files, both AIFF files and RGB device settings still match Rau's
published hashes. Generation and initial publication are recorded separately
in `publication.json`; post-open checks are in `after-open-verification.json`
under the same task directory. This does not establish acceptance without
database rewriting. RED remained unmounted throughout.

The core checks passed (24 passed, one FFmpeg conversion test ignored), the app
catalog tests passed (36), and the Tauri library check and frontend production
build passed. Physical acceptance on hardware that uses OneLibrary exclusively,
and RGB on the XDJ-1000MK2 with these new settings, remain pending.
