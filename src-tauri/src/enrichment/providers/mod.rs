mod lastfm;
mod musicbrainz;

pub use lastfm::LastFmProvider;
pub use musicbrainz::MusicBrainzProvider;

use crate::enrichment::{ProviderError, ProviderErrorKind};
use reqwest::blocking::Response;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn response_json(
    response: Result<Response, reqwest::Error>,
    provider: &str,
) -> Result<Value, ProviderError> {
    let response = response.map_err(|error| {
        let reason = if error.is_timeout() {
            "timeout"
        } else if error.is_connect() {
            "conexion"
        } else {
            "transporte"
        };
        ProviderError::network(format!("{provider} no respondio ({reason})."))
    })?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ProviderError::authentication(format!(
            "{provider} rechazo las credenciales ({status})."
        )));
    }
    if status.as_u16() == 429 || status.as_u16() == 503 {
        return Err(ProviderError::rate_limited(format!(
            "{provider} aplico rate limit ({status})."
        )));
    }
    if !status.is_success() {
        return Err(ProviderError {
            kind: ProviderErrorKind::Network,
            message: format!("{provider} retorno HTTP {status}."),
            retryable: status.is_server_error(),
        });
    }
    response.json::<Value>().map_err(|error| {
        ProviderError::invalid_response(format!("{provider} retorno JSON invalido: {error}"))
    })
}

pub(super) fn clean_track_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn string_match_score(left: &str, right: &str) -> f64 {
    let left = normalize_for_match(left);
    let right = normalize_for_match(right);
    if left.is_empty() || right.is_empty() {
        return 0.0;
    }
    if left == right {
        return 1.0;
    }
    if normalized_contains_phrase(&left, &right) || normalized_contains_phrase(&right, &left) {
        return 0.75;
    }
    let left_tokens = left.split_whitespace().collect::<BTreeSet<_>>();
    let right_tokens = right.split_whitespace().collect::<BTreeSet<_>>();
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }
    let intersection = left_tokens.intersection(&right_tokens).count() as f64;
    let union = left_tokens.union(&right_tokens).count() as f64;
    (intersection / union).clamp(0.0, 1.0)
}

pub(super) fn insert_json_string(
    fields: &mut BTreeMap<String, String>,
    key: &str,
    value: Option<&Value>,
) {
    if let Some(value) = value.and_then(json_scalar_to_string) {
        if !value.trim().is_empty() {
            fields.insert(key.to_string(), value);
        }
    }
}

fn json_scalar_to_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|value| value.to_string()))
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .or_else(|| value.as_f64().map(|value| value.to_string()))
}

fn normalize_for_match(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_contains_phrase(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    format!(" {haystack} ").contains(&format!(" {needle} "))
}
