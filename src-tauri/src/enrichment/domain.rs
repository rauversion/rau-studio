use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct CredentialRequirement {
    pub id: &'static str,
    pub label: &'static str,
    pub required: bool,
    pub secret: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub capabilities: &'static [&'static str],
    pub accepted_identifiers: &'static [&'static str],
    pub produced_identifiers: &'static [&'static str],
    pub credentials: &'static [CredentialRequirement],
    pub min_interval_ms: u64,
    pub max_attempts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderCredentialDescriptor {
    pub id: String,
    pub label: String,
    pub required: bool,
    pub secret: bool,
    pub configured: bool,
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnrichmentProviderDescriptor {
    pub id: String,
    pub label: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub accepted_identifiers: Vec<String>,
    pub produced_identifiers: Vec<String>,
    pub credentials: Vec<ProviderCredentialDescriptor>,
    pub ready: bool,
    pub min_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderTestResult {
    pub provider_id: String,
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderCredentials {
    values: BTreeMap<String, String>,
}

impl ProviderCredentials {
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.values.insert(key.into(), value.into());
    }

    pub fn require(&self, key: &str, provider: &str) -> Result<&str, ProviderError> {
        self.values
            .get(key)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ProviderError::configuration(format!(
                    "Falta la credencial {key} para {provider}. Configurala en Settings."
                ))
            })
    }
}

#[derive(Debug, Clone, Default)]
pub struct EnrichmentTrack {
    pub track_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub total_time: Option<u64>,
    pub genre: Option<String>,
    pub comments: Option<String>,
    pub bpm: Option<String>,
    pub key: Option<String>,
    pub year: Option<String>,
    pub label: Option<String>,
    pub external_ids: BTreeMap<String, String>,
}

impl EnrichmentTrack {
    pub fn missing_fields(&self) -> BTreeSet<&'static str> {
        let mut fields = BTreeSet::new();
        insert_if_blank(&mut fields, "genre", self.genre.as_deref());
        insert_if_blank(&mut fields, "comments", self.comments.as_deref());
        insert_if_blank(&mut fields, "bpm", self.bpm.as_deref());
        insert_if_blank(&mut fields, "key", self.key.as_deref());
        insert_if_blank(&mut fields, "year", self.year.as_deref());
        insert_if_blank(&mut fields, "label", self.label.as_deref());
        fields
    }
}

fn insert_if_blank(fields: &mut BTreeSet<&'static str>, field: &'static str, value: Option<&str>) {
    if value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        fields.insert(field);
    }
}

#[derive(Debug, Clone)]
pub struct ProviderSuggestion {
    pub provider: String,
    pub provider_key: Option<String>,
    pub status: String,
    pub confidence: f64,
    pub fields: BTreeMap<String, String>,
    pub payload: Value,
    pub message: Option<String>,
    pub source_url: Option<String>,
}

impl ProviderSuggestion {
    pub fn no_match(provider: &str, message: impl Into<String>) -> Self {
        Self {
            provider: provider.to_string(),
            provider_key: None,
            status: "no_match".to_string(),
            confidence: 0.0,
            fields: BTreeMap::new(),
            payload: json!({}),
            message: Some(message.into()),
            source_url: None,
        }
    }

    pub fn failed(provider: &str, error: &ProviderError) -> Self {
        Self {
            provider: provider.to_string(),
            provider_key: None,
            status: "failed".to_string(),
            confidence: 0.0,
            fields: BTreeMap::new(),
            payload: json!({
                "error": error.message,
                "error_kind": error.kind.as_str(),
                "retryable": error.retryable,
            }),
            message: Some(error.message.clone()),
            source_url: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderErrorKind {
    Authentication,
    Configuration,
    Network,
    RateLimited,
    InvalidResponse,
}

impl ProviderErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Configuration => "configuration",
            Self::Network => "network",
            Self::RateLimited => "rate_limited",
            Self::InvalidResponse => "invalid_response",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ProviderError {
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Authentication, message, false)
    }

    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Configuration, message, false)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Network, message, true)
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::RateLimited, message, true)
    }

    pub fn invalid_response(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::InvalidResponse, message, false)
    }

    fn new(kind: ProviderErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }
}

pub trait EnrichmentProvider: Send + Sync {
    fn definition(&self) -> ProviderDefinition;

    fn enrich(
        &self,
        track: &EnrichmentTrack,
        credentials: &ProviderCredentials,
    ) -> Result<ProviderSuggestion, ProviderError>;

    fn test(&self, credentials: &ProviderCredentials) -> Result<(), ProviderError> {
        let sample = EnrichmentTrack {
            track_id: "provider-health-check".to_string(),
            title: Some("Believe".to_string()),
            artist: Some("Cher".to_string()),
            ..EnrichmentTrack::default()
        };
        self.enrich(&sample, credentials).map(|_| ())
    }
}
