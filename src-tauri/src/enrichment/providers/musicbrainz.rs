use super::{clean_track_text, insert_json_string, response_json, string_match_score};
use crate::enrichment::{
    EnrichmentProvider, EnrichmentTrack, ProviderCredentials, ProviderDefinition, ProviderError,
    ProviderSuggestion,
};
use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

pub struct MusicBrainzProvider;

impl EnrichmentProvider for MusicBrainzProvider {
    fn definition(&self) -> ProviderDefinition {
        ProviderDefinition {
            id: "musicbrainz",
            label: "MusicBrainz",
            description: "Identidad canonica, ISRC, releases, fecha, label y generos.",
            capabilities: &[
                "genre",
                "year",
                "label",
                "isrc",
                "musicbrainz_recording_id",
                "musicbrainz_release_id",
            ],
            accepted_identifiers: &["isrc", "artist_title_album"],
            produced_identifiers: &["musicbrainz_recording_id", "musicbrainz_release_id", "isrc"],
            credentials: &[],
            min_interval_ms: 1_100,
            max_attempts: 3,
        }
    }

    fn enrich(
        &self,
        track: &EnrichmentTrack,
        _credentials: &ProviderCredentials,
    ) -> Result<ProviderSuggestion, ProviderError> {
        let Some(title) = clean_track_text(track.title.as_deref()) else {
            return Ok(ProviderSuggestion::no_match(
                "musicbrainz",
                format!("Track {} sin titulo para buscar.", track.track_id),
            ));
        };
        let Some(artist) = clean_track_text(track.artist.as_deref()) else {
            return Ok(ProviderSuggestion::no_match(
                "musicbrainz",
                format!("Track {} sin artista para buscar.", track.track_id),
            ));
        };

        let client = Client::builder()
            .timeout(Duration::from_secs(18))
            .user_agent(user_agent())
            .build()
            .map_err(|error| {
                ProviderError::configuration(format!(
                    "No se pudo crear cliente MusicBrainz: {error}"
                ))
            })?;
        let mut query = format!(
            "recording:\"{}\" AND artist:\"{}\"",
            escape_query(&title),
            escape_query(&artist)
        );
        if let Some(album) = clean_track_text(track.album.as_deref()) {
            query.push_str(&format!(" AND release:\"{}\"", escape_query(&album)));
        }
        let search_url = reqwest::Url::parse_with_params(
            "https://musicbrainz.org/ws/2/recording",
            &[("query", query.as_str()), ("fmt", "json"), ("limit", "5")],
        )
        .map_err(|error| {
            ProviderError::configuration(format!("No se pudo construir URL MusicBrainz: {error}"))
        })?;
        let search = response_json(client.get(search_url).send(), "MusicBrainz")?;
        let Some(recordings) = search.get("recordings").and_then(Value::as_array) else {
            return Ok(ProviderSuggestion::no_match(
                "musicbrainz",
                "MusicBrainz no retorno recordings.",
            ));
        };

        let best = recordings
            .iter()
            .map(|recording| (recording_confidence(track, recording), recording))
            .max_by(|left, right| left.0.total_cmp(&right.0));
        let Some((confidence, search_recording)) = best else {
            return Ok(ProviderSuggestion::no_match(
                "musicbrainz",
                "Sin candidatos MusicBrainz.",
            ));
        };
        let recording_id = search_recording
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let source_url = recording_id
            .as_ref()
            .map(|id| format!("https://musicbrainz.org/recording/{id}"));

        if confidence < 0.65 {
            return Ok(ProviderSuggestion {
                provider: "musicbrainz".to_string(),
                provider_key: recording_id,
                status: "no_match".to_string(),
                confidence,
                fields: recording_fields(search_recording),
                payload: search_recording.clone(),
                message: Some(
                    "MusicBrainz encontro candidatos, pero la confianza fue baja.".to_string(),
                ),
                source_url,
            });
        }

        let recording_id = recording_id.ok_or_else(|| {
            ProviderError::invalid_response("MusicBrainz retorno un candidato sin recording ID.")
        })?;
        thread::sleep(Duration::from_millis(1_100));
        let detail_url = reqwest::Url::parse_with_params(
            &format!("https://musicbrainz.org/ws/2/recording/{recording_id}"),
            &[
                ("fmt", "json"),
                ("inc", "artist-credits+isrcs+releases+genres"),
            ],
        )
        .map_err(|error| {
            ProviderError::configuration(format!(
                "No se pudo construir lookup MusicBrainz: {error}"
            ))
        })?;
        let detail = response_json(client.get(detail_url).send(), "MusicBrainz")?;
        let mut fields = recording_fields(&detail);
        let selected_release = select_release(&detail, track.album.as_deref());
        if let Some(release_id) = selected_release
            .and_then(|release| release.get("id"))
            .and_then(Value::as_str)
        {
            fields.insert("musicbrainz_release_id".to_string(), release_id.to_string());
            thread::sleep(Duration::from_millis(1_100));
            let release_url = reqwest::Url::parse_with_params(
                &format!("https://musicbrainz.org/ws/2/release/{release_id}"),
                &[("fmt", "json"), ("inc", "labels+release-groups")],
            )
            .map_err(|error| {
                ProviderError::configuration(format!(
                    "No se pudo construir release lookup MusicBrainz: {error}"
                ))
            })?;
            let release = response_json(client.get(release_url).send(), "MusicBrainz")?;
            merge_release_fields(&mut fields, &release);
        }

        Ok(ProviderSuggestion {
            provider: "musicbrainz".to_string(),
            provider_key: Some(recording_id),
            status: "matched".to_string(),
            confidence,
            fields,
            payload: detail,
            message: Some(format!(
                "Match MusicBrainz con confianza {:.0}%.",
                confidence * 100.0
            )),
            source_url,
        })
    }
}

fn recording_confidence(track: &EnrichmentTrack, recording: &Value) -> f64 {
    let score = recording
        .get("score")
        .and_then(|value| {
            value
                .as_i64()
                .map(|value| value as f64)
                .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
        })
        .unwrap_or(0.0)
        / 100.0;
    let title_score = string_match_score(
        track.title.as_deref().unwrap_or_default(),
        recording
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default(),
    );
    let artist_score = string_match_score(
        track.artist.as_deref().unwrap_or_default(),
        &artist_credit(recording).unwrap_or_default(),
    );
    let duration_score = duration_score(track, recording);
    (score * 0.55 + title_score * 0.2 + artist_score * 0.2 + duration_score * 0.05).clamp(0.0, 1.0)
}

fn duration_score(track: &EnrichmentTrack, recording: &Value) -> f64 {
    let Some(local_seconds) = track.total_time.map(|value| value as f64) else {
        return 0.5;
    };
    let Some(remote_ms) = recording.get("length").and_then(Value::as_f64) else {
        return 0.5;
    };
    match (local_seconds - remote_ms / 1000.0).abs() {
        diff if diff <= 4.0 => 1.0,
        diff if diff <= 12.0 => 0.75,
        diff if diff <= 30.0 => 0.35,
        _ => 0.0,
    }
}

fn recording_fields(recording: &Value) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    insert_json_string(&mut fields, "musicbrainz_recording_id", recording.get("id"));
    insert_json_string(&mut fields, "title", recording.get("title"));
    if let Some(artist) = artist_credit(recording) {
        fields.insert("artist".to_string(), artist);
    }
    if let Some(artist_id) = recording
        .get("artist-credit")
        .and_then(Value::as_array)
        .and_then(|credits| credits.first())
        .and_then(|credit| credit.get("artist"))
        .and_then(|artist| artist.get("id"))
        .and_then(Value::as_str)
    {
        fields.insert("musicbrainz_artist_id".to_string(), artist_id.to_string());
    }
    if let Some(isrc) = recording
        .get("isrcs")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
        .and_then(Value::as_str)
    {
        fields.insert("isrc".to_string(), isrc.to_string());
    }
    if let Some(genre) = top_genre(recording) {
        fields.insert("genre".to_string(), genre);
    }
    if let Some(release) = select_release(recording, None) {
        insert_json_string(&mut fields, "musicbrainz_release_id", release.get("id"));
        insert_json_string(&mut fields, "album", release.get("title"));
        merge_date(&mut fields, release.get("date").and_then(Value::as_str));
    }
    fields
}

fn merge_release_fields(fields: &mut BTreeMap<String, String>, release: &Value) {
    insert_json_string(fields, "musicbrainz_release_id", release.get("id"));
    insert_json_string(fields, "album", release.get("title"));
    merge_date(fields, release.get("date").and_then(Value::as_str));
    if let Some(label) = release
        .get("label-info")
        .and_then(Value::as_array)
        .and_then(|labels| labels.first())
        .and_then(|label| label.get("label"))
        .and_then(|label| label.get("name"))
        .and_then(Value::as_str)
    {
        fields.insert("label".to_string(), label.to_string());
    }
}

fn merge_date(fields: &mut BTreeMap<String, String>, date: Option<&str>) {
    let Some(date) = date else { return };
    fields.insert("release_date".to_string(), date.to_string());
    if let Some(year) = date
        .get(0..4)
        .filter(|year| year.chars().all(|character| character.is_ascii_digit()))
    {
        fields.insert("year".to_string(), year.to_string());
    }
}

fn select_release<'a>(recording: &'a Value, album: Option<&str>) -> Option<&'a Value> {
    let releases = recording.get("releases").and_then(Value::as_array)?;
    if let Some(album) = album.map(str::trim).filter(|album| !album.is_empty()) {
        if let Some(release) = releases.iter().max_by(|left, right| {
            let left_score = string_match_score(
                album,
                left.get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            let right_score = string_match_score(
                album,
                right
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            left_score.total_cmp(&right_score)
        }) {
            return Some(release);
        }
    }
    releases.first()
}

fn artist_credit(recording: &Value) -> Option<String> {
    let credits = recording.get("artist-credit")?.as_array()?;
    let names = credits
        .iter()
        .filter_map(|credit| {
            credit
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| credit.get("artist")?.get("name").and_then(Value::as_str))
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!names.is_empty()).then(|| names.join(", "))
}

fn top_genre(recording: &Value) -> Option<String> {
    let genres = recording
        .get("genres")
        .and_then(Value::as_array)
        .or_else(|| recording.get("tags").and_then(Value::as_array))?;
    genres
        .iter()
        .filter_map(|genre| {
            let name = genre.get("name").and_then(Value::as_str)?;
            let count = genre
                .get("count")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            Some((count, name.trim()))
        })
        .filter(|(_, name)| !name.is_empty())
        .max_by_key(|(count, _)| *count)
        .map(|(_, name)| name.to_string())
}

fn escape_query(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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
    fn detailed_lookups_map_recording_and_release_fields() {
        let recording = json!({
            "id": "recording-id",
            "title": "Track",
            "artist-credit": [{
                "name": "Artist",
                "artist": { "id": "artist-id", "name": "Artist" }
            }],
            "isrcs": ["CLAAA2400001"],
            "genres": [{ "name": "Electronic", "count": 7 }],
            "releases": [{ "id": "release-id", "title": "Album", "date": "2024-06-01" }]
        });
        let release = json!({
            "id": "release-id",
            "title": "Album",
            "date": "2024-06-01",
            "label-info": [{ "label": { "name": "Rau Records" } }]
        });

        let mut fields = recording_fields(&recording);
        merge_release_fields(&mut fields, &release);
        assert_eq!(fields["musicbrainz_recording_id"], "recording-id");
        assert_eq!(fields["musicbrainz_release_id"], "release-id");
        assert_eq!(fields["isrc"], "CLAAA2400001");
        assert_eq!(fields["year"], "2024");
        assert_eq!(fields["label"], "Rau Records");
        assert_eq!(fields["genre"], "Electronic");
    }

    #[test]
    fn release_selection_prefers_the_local_album() {
        let recording = json!({
            "releases": [
                { "id": "wrong", "title": "Unrelated Compilation" },
                { "id": "right", "title": "Selected Album" }
            ]
        });

        let selected = select_release(&recording, Some("Selected Album")).unwrap();
        assert_eq!(selected["id"], "right");
    }
}
