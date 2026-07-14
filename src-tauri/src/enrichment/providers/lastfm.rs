use super::{clean_track_text, insert_json_string, response_json, string_match_score};
use crate::enrichment::{
    CredentialRequirement, EnrichmentProvider, EnrichmentTrack, ProviderCredentials,
    ProviderDefinition, ProviderError, ProviderSuggestion,
};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Duration;

const CREDENTIALS: &[CredentialRequirement] = &[CredentialRequirement {
    id: "api_key",
    label: "API key",
    required: true,
    secret: true,
}];

pub struct LastFmProvider;

impl EnrichmentProvider for LastFmProvider {
    fn definition(&self) -> ProviderDefinition {
        ProviderDefinition {
            id: "lastfm",
            label: "Last.fm",
            description: "Tags sociales, genero, listeners y popularidad.",
            capabilities: &["genre", "comments", "tags", "listeners", "playcount"],
            accepted_identifiers: &["musicbrainz_recording_id", "artist_title"],
            produced_identifiers: &["lastfm_url"],
            credentials: CREDENTIALS,
            min_interval_ms: 250,
            max_attempts: 2,
        }
    }

    fn enrich(
        &self,
        track: &EnrichmentTrack,
        credentials: &ProviderCredentials,
    ) -> Result<ProviderSuggestion, ProviderError> {
        let api_key = credentials.require("api_key", "Last.fm")?;
        let title = clean_track_text(track.title.as_deref());
        let artist = clean_track_text(track.artist.as_deref());
        let mbid = track
            .external_ids
            .get("musicbrainz_recording_id")
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if mbid.is_none() && (title.is_none() || artist.is_none()) {
            return Ok(ProviderSuggestion::no_match(
                "lastfm",
                format!(
                    "Track {} sin MBID ni artista/titulo para buscar.",
                    track.track_id
                ),
            ));
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(18))
            .user_agent(user_agent())
            .build()
            .map_err(|error| {
                ProviderError::configuration(format!("No se pudo crear cliente Last.fm: {error}"))
            })?;
        let mut params = vec![
            ("method", "track.getInfo"),
            ("api_key", api_key),
            ("autocorrect", "1"),
            ("format", "json"),
        ];
        if let Some(mbid) = mbid {
            params.push(("mbid", mbid));
        } else {
            params.push(("artist", artist.as_deref().unwrap_or_default()));
            params.push(("track", title.as_deref().unwrap_or_default()));
        }
        let url = reqwest::Url::parse_with_params("https://ws.audioscrobbler.com/2.0/", params)
            .map_err(|error| {
                ProviderError::configuration(format!("No se pudo construir URL Last.fm: {error}"))
            })?;
        let response = response_json(client.get(url).send(), "Last.fm")?;

        if let Some(error_code) = response.get("error").and_then(Value::as_i64) {
            let message = response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Last.fm no encontro metadata.");
            return match error_code {
                6 | 7 => Ok(ProviderSuggestion::no_match("lastfm", message)),
                10 | 26 => Err(ProviderError::authentication(format!(
                    "Last.fm rechazo la API key: {message}"
                ))),
                29 => Err(ProviderError::rate_limited(format!(
                    "Last.fm aplico rate limit: {message}"
                ))),
                _ => Err(ProviderError::invalid_response(format!(
                    "Last.fm error {error_code}: {message}"
                ))),
            };
        }

        let Some(track_payload) = response.get("track") else {
            return Ok(ProviderSuggestion::no_match(
                "lastfm",
                "Last.fm no retorno track.",
            ));
        };
        let fields = lastfm_track_fields(track_payload);
        let provider_key = fields
            .get("lastfm_url")
            .cloned()
            .or_else(|| fields.get("title").cloned());
        let source_url = fields.get("lastfm_url").cloned();
        let confidence = lastfm_confidence(track, track_payload);

        Ok(ProviderSuggestion {
            provider: "lastfm".to_string(),
            provider_key,
            status: "matched".to_string(),
            confidence,
            fields,
            payload: track_payload.clone(),
            message: Some(format!(
                "Tags Last.fm con confianza {:.0}%.",
                confidence * 100.0
            )),
            source_url,
        })
    }
}

fn lastfm_track_fields(track_payload: &Value) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    insert_json_string(&mut fields, "title", track_payload.get("name"));
    insert_json_string(&mut fields, "lastfm_url", track_payload.get("url"));
    insert_json_string(&mut fields, "listeners", track_payload.get("listeners"));
    insert_json_string(&mut fields, "playcount", track_payload.get("playcount"));
    if let Some(artist) = track_payload
        .get("artist")
        .and_then(|artist| artist.get("name"))
        .and_then(Value::as_str)
    {
        fields.insert("artist".to_string(), artist.to_string());
    }
    let tags = track_payload
        .get("toptags")
        .and_then(|tags| tags.get("tag"))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|tag| tag.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .take(8)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(first_tag) = tags.first() {
        fields.insert("genre".to_string(), first_tag.clone());
    }
    if !tags.is_empty() {
        fields.insert("tags".to_string(), tags.join(", "));
    }
    fields
}

fn lastfm_confidence(track: &EnrichmentTrack, track_payload: &Value) -> f64 {
    let title_score = string_match_score(
        track.title.as_deref().unwrap_or_default(),
        track_payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let artist_score = string_match_score(
        track.artist.as_deref().unwrap_or_default(),
        track_payload
            .get("artist")
            .and_then(|artist| artist.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    (0.55 + title_score * 0.25 + artist_score * 0.2).clamp(0.0, 1.0)
}

fn user_agent() -> String {
    format!(
        "RauStudio/{} (https://rauversion.com)",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_lastfm_tags_without_flattening_the_raw_payload() {
        let payload = json!({
            "name": "Track",
            "url": "https://last.fm/music/artist/_/track",
            "listeners": "42",
            "playcount": "100",
            "artist": { "name": "Artist" },
            "toptags": { "tag": [
                { "name": "Deep House" },
                { "name": "Club" }
            ] }
        });

        let fields = lastfm_track_fields(&payload);
        assert_eq!(fields["genre"], "Deep House");
        assert_eq!(fields["tags"], "Deep House, Club");
        assert_eq!(fields["listeners"], "42");
    }
}
