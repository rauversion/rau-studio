# Exact-file duplicates

Open **Duplicates** in the Catalog toolbar. Select a library and start a scan. The scanner checks the audio paths already indexed in that library; it does not discover new folders or compare different libraries against each other.

## Scan and progress

The backend scans on a blocking worker, independent of navigation. The view restores its progress when reopened and shows preparation, completed files, bytes read, elapsed time, estimated remaining time, cached hashes, unique sizes, missing files, and skipped/unreadable files. One scan or merge can run at a time.

1. Read current filesystem metadata, refreshing catalog availability.
2. Exclude missing, empty, non-regular, and unreadable files. An empty checksum is never a match.
3. Group candidate files by current byte size. Unique sizes need no content read; several catalog records pointing to the same path still count as candidates.
4. Reuse SHA-256 hashes only when the file signature matches. The signature includes size and nanosecond modification time; on Unix it also includes device, inode, and change time.
5. Read remaining candidates sequentially in 1 MiB blocks. This bounds memory and avoids concurrent seeks on a hard drive. Check signatures before and after reading so a changing file never yields accepted evidence.
6. Group only valid, non-null SHA-256 hashes of the same size. The results view checks cached matches against current filesystem metadata again.

**Skip file** omits the active file; **Cancel** stops the scan while retaining finished checksums. Both take effect between read blocks (an OS read on an unresponsive drive must return first). A subsequent scan retries missing/skipped files and reuses completed, unchanged hashes. Closing the application stops the worker; reopening reports the interrupted scan and allows a new scan using the saved cache. Cancelled and interrupted scans may show partial results.

All estimates are approximate. Track count alone does not determine time. If 6,000 tracks average 100 MB and every file needs hashing, that is about 600 GB to read: roughly 100 minutes at an effective 100 MB/s. At 10 MB per file it is roughly 10 minutes at that same speed. Size filtering and cached hashes can reduce the work substantially. These are arithmetic examples, not measurements of the user's drive. The view estimates remaining time from measured throughput after reading starts.

SHA-256 identifies byte-for-byte copies. Different tags, artwork, or encodings produce different hashes, even if the song sounds identical. Acoustic fingerprints are outside this feature.

## Merge

Choose the record to keep in an exact-match group, then review and confirm the merge. The app reads all group files again before making changes. Missing, changed, or unverified files block the merge. Database identities and file signatures are checked again inside the write transaction.

**Approve all** reviews every group in the report, across all pages, in one confirmation. It respects the record selected in each group (the first record is selected by default). The backend processes groups sequentially with per-group progress and continues after individual failures. **Stop** takes effect after the active group; completed merges remain committed. Navigation does not interrupt the batch. Failed groups are reported with their errors; stopping leaves unprocessed groups available for another attempt. Closing the app stops pending work without undoing committed groups.

After each committed merge, redundant records are removed from the displayed groups. Responses from report requests started before a merge are discarded, so a delayed refresh cannot put those records back. The report and library counts refresh as a bulk merge advances.

The transaction:

- keeps the chosen record's identity, file path, existing text tags, and rating, filling empty tags/rating from the other records;
- unions indexed playlists, local playlist additions, source XML memberships, and drafts, keeping the earliest position when references overlap;
- redirects import provenance and Copilot references, preserving enrichment observations and non-conflicting enrichment records;
- updates Broadcast catalog IDs while retaining Broadcast's existing file snapshots;
- records an audit snapshot of track metadata, enrichment and source membership data before removing redundant catalog records;
- persists import aliases, including across successive merges, to avoid recreating the merged records when the same files are imported again; a changed file or changed path releases its alias;
- refreshes counts, saved searches, and full-text search, and invalidates embeddings affected by merged metadata.

Conflicting enrichment provider records remain in the merge audit snapshot; the chosen record's current provider result wins. A failed transaction rolls back all changes. The generated local-library XML is refreshed after a successful local merge; any XML refresh error is reported separately from the committed merge.

Audio files and original imported XMLs are never deleted or rewritten. A merge removes redundant **catalog records**, not physical copies. There is no undo action in the UI.

## Storage and code

The existing SQLite database holds `playlist_file_checksums`, `playlist_duplicate_scans`, `playlist_track_merge_aliases`, and `playlist_track_merge_history`. Hashes are evidence, not track IDs. The cache is keyed by path, so reading a physical path is shared across records; matching is scoped to the selected library.

- `src-tauri/src/playlist_index/duplicates.rs`: scan worker, cache, progress, validation, merge transaction, and regression tests.
- `src/DuplicatesPage.tsx`: progress, paginated groups/omissions, survivor selection, and merge review.
- `src-tauri/src/playlist_index.rs`: schema setup and reimport alias handling.
