# Smart Playlists

Smart Playlists covers the playlist intelligence features in Rau Studio:

- Rekordbox XML indexing into SQLite.
- Lexical and vector search.
- Artist and album browsing.
- Taxonomy visualizations.
- Playlist Copilot suggestions.
- Local draft playlists that can be exported back to Rekordbox XML.

The feature is designed for large DJ libraries where metadata, BPM, key, genre, artist, and playlist membership are useful signals for building new sets.

## Data Flow

1. Import a Rekordbox XML in **Playlists > Playlist Library**.
2. Rau Studio parses tracks, playlist folders, playlist memberships, source paths, and XML attributes.
3. The data is stored in local SQLite.
4. SQLite FTS is rebuilt for lexical search.
5. Optional OpenAI embeddings can be generated for indexed tracks.
6. Search, browse, taxonomy, and Copilot views read from SQLite.
7. Selected tracks can be added to local draft playlists.
8. Draft playlists can be exported as a Rekordbox-compatible XML.

Audio files are not uploaded and are not stored in the database. SQLite stores metadata, paths, index state, embeddings, and draft playlist membership.

## Playlist Library

Path: **Playlists > Playlist Library**

Playlist Library is the control center for importing XML, indexing selected playlists, searching tracks, generating embeddings, and managing local draft playlists.

### Index XML

The **Index XML** tab lets the user:

- choose a Rekordbox XML;
- preview playlists before importing;
- select specific playlists instead of indexing the whole library;
- index selected playlists with row-level status feedback;
- delete indexed libraries, playlists, or tracks;
- inspect whether tracks are indexed and whether vectors are ready.

Indexing selected playlists stores only the tracks referenced by those playlists. Indexing the whole XML stores the full collection from the XML.

### Search

The **Search** tab supports:

- normal lexical search through SQLite FTS;
- vector search when embeddings are available;
- row-level embedding status;
- embedding one track at a time;
- embedding selected tracks in batches;
- deleting selected tracks from the local index;
- opening the track detail sheet by clicking the title;
- playing and opening source folders from each row.

Vector search sends only text metadata to OpenAI to create a temporary query embedding. It compares that temporary embedding against vectors stored locally in SQLite.

### Playlist Drafts

The **Playlist** tab manages local draft playlists:

- create a new draft;
- add selected tracks from search, browser, taxonomy, or Copilot views;
- remove tracks from a draft;
- export the draft playlist as a Rekordbox XML.

Drafts do not modify the original XML until the user explicitly exports.

## Artist and Album Browser

Paths:

- **Playlists > Artists**
- **Playlists > Albums**

The browser views let users explore indexed tracks by artist or album. They include:

- fixed-height scrollable group lists;
- group-level search;
- track tables with covers, player, checkboxes, and metadata sheet;
- bulk selection;
- shared **Add to Playlist** dialog for existing or new draft playlists.

Clicking an artist or album opens its tracks. Track titles open the metadata sheet.

## Taxonomies

Path: **Playlists > Taxonomies**

Taxonomies summarize the indexed library from metadata. Current views include:

- overview metrics;
- genre distribution;
- BPM and key distribution;
- a graph connecting genre, BPM bucket, and key;
- track lists behind each taxonomy selection.

The graph intentionally avoids playlist nodes so the visualization focuses on musical metadata instead of folder structure. It helps discover clusters such as:

- genres that share BPM ranges;
- keys that dominate specific genres;
- tempo zones with enough tracks for a set.

Selected taxonomy tracks can be played, inspected, selected, and added to draft playlists.

## Playlist Copilot

Path: **Playlists > Playlist Copilot**

Playlist Copilot is a chat-like generator for playlist suggestions. The user writes a brief such as:

```text
Warm up house 118-124 BPM with soft vocals
```

or:

```text
Peak time melodic techno in Am or Em, no aggressive tracks
```

The Copilot:

1. reads the active indexed library from SQLite;
2. restores the structured intent for the active Copilot session;
3. applies a free-form message or a typed guided answer to that intent;
4. optionally asks OpenAI to update the intent, with a deterministic local fallback;
5. turns the brief into up to five focused probes for the complete brief, style, mood/energy, mix constraints, and adjacent discoveries;
6. embeds those probes in one batch when vectors are available, loads local embeddings once, and combines each focused top list with weighted reciprocal-rank fusion;
7. falls back to the same multi-probe flow over local metadata when embeddings are unavailable;
8. ranks local tracks with fused retrieval evidence, metadata, continuous BPM distance, source policy, discovery policy, recent suggestion history, and per-run exploration;
9. collapses duplicate Rekordbox IDs that represent the same artist, title, and duration into one musical identity;
10. applies artist diversity and, for broad briefs, a soft genre cap before sequencing by BPM, Camelot-compatible key transitions, and energy curve;
11. atomically stores raw messages, the current intent, score components, candidate order, reasoning, and coverage in SQLite;
12. returns candidate tracks with explanations and preselects them for a draft playlist.

### Structured Intent

The Copilot keeps an executable intent instead of relying on the accumulated chat transcript. It includes:

- genre, artist, key, BPM, mood, energy, and exclusion signals;
- `energy_curve`: flat, slow build, or ramp;
- `harmonic_policy`: ignore, soft, or strict;
- `discovery_mode`: known, balanced, or discovery;
- `tempo_policy`: flexible or tight;
- `source_policy`: prefer available, available only, or allow missing;
- maximum tracks per artist.

Guided option values are machine-readable patches such as `harmony=strict` or `set_shape=energy_ramp`. The visible label is stored as the user message, while the patch changes the intent deterministically.

Hard constraints are applied before scoring. The ranker can return fewer tracks than requested when the indexed library does not contain enough valid matches.

In balanced and discovery modes, tracks present in the eight most recent candidate sets receive a bounded recency penalty. Close candidates also receive a deterministic exploration value that changes per library run. Known mode minimizes exploration and disables the history penalty. This rotates the result set without overriding hard constraints or strong artist, genre, BPM, and key matches.

Genre diversity is conditional. A brief with one explicit genre is allowed to stay focused. A broad brief with at least three available genre clusters initially limits one genre to roughly 45% of the requested result set, then relaxes that cap only when needed to fill the playlist.

### Guided Sessions

Each Copilot run belongs to a local session. A session can receive follow-up prompts such as:

```text
Make it darker and keep the last third more energetic.
```

or:

```text
Use loose key flow, but avoid tracks with missing source files.
```

The UI shows:

- chat history for the current working session;
- a structured interpretation card;
- the implicit changes applied to the persisted brief after each message;
- a decision trace with each planning step;
- live progress events and the candidate count from every focused search probe;
- one guided question at a time, with clickable answer options;
- suggested playlist titles;
- coverage metrics for BPM, genre, key, format, artists, and missing source files;
- candidate tracks with row-level reasons.

The decision trace is a user-facing summary of how the result was built. It is not raw model chain-of-thought.

In guided mode, the assistant does not ask every possible question at once. It asks one question, waits for the user's answer, uses that answer as context, and then continues with the next useful decision.

Intermediate guided turns update the session intent but do not create empty candidate sets. A candidate set is persisted only after ranking has run.

### What Goes to OpenAI

When an API key is configured, Playlist Copilot sends:

- the latest user message;
- the previous structured intent, when the session already exists;
- a compact profile of the indexed library;
- target track count.

It does not send audio files. It does not upload the full collection. Track selection and scoring happen locally against SQLite.

If no API key is configured, Copilot still works with local parsing and ranking.

### Candidate Review

The candidate table uses the shared track-list components:

- cover extraction/cache;
- row-level play/stop;
- clickable title for the metadata sheet;
- checkbox selection;
- open source folder action;
- metadata columns for artist, album, genre, BPM, key, and format.

A reason panel explains why the top tracks were suggested.

### Adding to Playlists

Selected Copilot candidates use the same **Add to Playlist** dialog as browser and taxonomy views:

- add to an existing draft playlist;
- create a new draft playlist and add the selected tracks;
- show the selected track count before confirming.

## Privacy and Network Behavior

| Feature | Network use |
| --- | --- |
| XML indexing | None |
| Lexical search | None |
| Artist/album browsing | None |
| Taxonomies | None |
| Vector indexing | Sends text metadata to OpenAI embeddings |
| Vector search | Sends search query text to OpenAI embeddings |
| Playlist Copilot | Sends prompt + compact library profile to OpenAI chat and a small batch of focused text probes to embeddings, if configured |

Audio files stay local.

## SQLite Tables

Main playlist-intelligence tables:

- `playlist_index_libraries`
- `playlist_index_tracks`
- `playlist_index_playlists`
- `playlist_index_memberships`
- `playlist_track_fts`
- `playlist_track_embeddings`
- `playlist_drafts`
- `playlist_draft_tracks`
- `playlist_copilot_sessions`
- `playlist_copilot_messages`
- `playlist_copilot_candidate_sets`
- `playlist_copilot_candidate_tracks`

`playlist_copilot_sessions.intent_json` stores the latest executable intent. Candidate tracks also store `score_components_json` so ranking decisions can be inspected and compared across ranker versions.

## Tauri Commands

Indexing and search:

- `playlist_index_libraries`
- `playlist_index_preview_xml`
- `playlist_index_import_xml`
- `playlist_index_library_playlists`
- `playlist_index_playlist_tracks`
- `playlist_index_search_tracks`
- `playlist_index_generate_embeddings`
- `playlist_index_delete_library`
- `playlist_index_delete_playlists`
- `playlist_index_delete_tracks`
- `playlist_index_clean_missing_files`

Browsing and taxonomies:

- `playlist_index_track_groups`
- `playlist_index_group_tracks`
- `playlist_index_taxonomy_overview`
- `playlist_index_taxonomy_graph`
- `playlist_index_taxonomy_tracks`
- `playlist_index_track_cover`

Draft playlists:

- `playlist_index_drafts`
- `playlist_index_create_draft`
- `playlist_index_add_tracks_to_draft`
- `playlist_index_remove_draft_track`
- `playlist_index_delete_draft`
- `playlist_index_draft_tracks`
- `playlist_index_export_draft_xml`

Copilot:

- `playlist_copilot_generate`

## Relevant Files

- `src/PlaylistIndexPage.tsx`
- `src/PlaylistBrowserPage.tsx`
- `src/TaxonomyPage.tsx`
- `src/PlaylistCopilotPage.tsx`
- `src/components/tracks/TrackList.tsx`
- `src/components/tracks/TrackCover.tsx`
- `src/components/tracks/TrackDetailSheet.tsx`
- `src/components/tracks/PlaylistAddDialog.tsx`
- `src-tauri/src/playlist_copilot.rs`
- `src-tauri/src/playlist_index.rs`
