# Metadata Enrichment

Rau Studio enriches indexed tracks without modifying the source Rekordbox XML or the original audio files. Provider output is stored in local SQLite, reviewed in the Enrichment page, and applied to the indexed metadata only after approval.

## Architecture

The backend uses a provider registry instead of branching on provider names in the Tauri command.

```text
missing fields -> planner -> provider clients -> observations -> resolver -> apply
                         ^
                         |
                 encrypted credentials
```

Each provider declares:

- the fields it can provide;
- accepted and produced external identifiers;
- required credentials;
- minimum request interval and retry policy.

The planner skips providers that cannot fill any missing field. Explicitly selected tracks force the selected providers, which is useful for refreshing provenance or diagnosing a source.

## Providers

### MusicBrainz

MusicBrainz requires no API key. The adapter:

1. searches by normalized title, artist, and optional album;
2. scores candidates using the remote score, local text similarity, and duration;
3. looks up the chosen recording by MBID with releases, ISRCs, and genres;
4. looks up the selected release with label information.

The provider uses a descriptive User-Agent, observes a minimum interval of 1.1 seconds, and retries retryable network or rate-limit errors.

### Last.fm

Last.fm requires an API key configured under **Settings → Enrichment sources**. The key is encrypted in the same local settings database used by the OpenAI key and is never sent from the Enrichment page.

When MusicBrainz resolves an MBID in the same run, Last.fm receives the stable identifier. Artist and title are the fallback.

## Persistence

The current result for each `library + track + provider` remains in `playlist_track_enrichments` for the review UI. Runs also produce append-only operational history:

- `playlist_enrichment_runs`: request, capabilities, status, and counters;
- `playlist_enrichment_tasks`: one provider execution per track;
- `playlist_enrichment_observations`: one value per field with source and confidence.

Refreshing a provider no longer destroys the historical observations from earlier runs.

## Applying Results

Applying multiple provider results is resolved per field rather than by result ID order. Source preferences are field-specific:

- Last.fm is preferred for social genre tags and comments;
- MusicBrainz is preferred for year, label, ISRC, and MusicBrainz IDs;
- future local audio analysis is preferred for BPM and key.

Existing canonical Rekordbox metadata is preserved. All provider values are also stored under source-specific `Enrichment...` attributes for provenance.

## Adding a Provider

1. Implement `EnrichmentProvider` under `src-tauri/src/enrichment/providers/`.
2. Declare credentials, capabilities, identifiers, and rate policy.
3. Register it in `src-tauri/src/enrichment/registry.rs`.
4. Add fixture-driven adapter tests and planner/resolver coverage.

Unknown provider IDs are rejected as configuration errors; they are never converted to `no_match` results.
