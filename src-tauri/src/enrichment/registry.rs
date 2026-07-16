use super::{
    providers::{LastFmProvider, MusicBrainzProvider},
    EnrichmentProvider, EnrichmentProviderDescriptor, EnrichmentTrack,
    ProviderCredentialDescriptor, ProviderCredentials, ProviderDefinition, ProviderError,
    ProviderSuggestion,
};
use crate::settings;
use std::collections::BTreeSet;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

pub struct ProviderClient {
    provider: Box<dyn EnrichmentProvider>,
    credentials: ProviderCredentials,
}

impl ProviderClient {
    pub fn id(&self) -> &'static str {
        self.definition().id
    }

    pub fn definition(&self) -> ProviderDefinition {
        self.provider.definition()
    }

    pub fn enrich(&self, track: &EnrichmentTrack) -> ProviderSuggestion {
        let definition = self.definition();
        let mut attempt = 0_usize;
        loop {
            attempt += 1;
            match self.provider.enrich(track, &self.credentials) {
                Ok(suggestion) => return suggestion,
                Err(error) if error.retryable && attempt < definition.max_attempts => {
                    let retry_delay = (500 * attempt as u64).max(definition.min_interval_ms);
                    thread::sleep(Duration::from_millis(retry_delay));
                }
                Err(error) => return ProviderSuggestion::failed(definition.id, &error),
            }
        }
    }

    pub fn test(&self) -> Result<(), ProviderError> {
        self.provider.test(&self.credentials)
    }
}

pub fn definitions() -> Vec<ProviderDefinition> {
    vec![
        MusicBrainzProvider.definition(),
        LastFmProvider.definition(),
    ]
}

pub fn normalize_provider_ids(provider_ids: Vec<String>) -> Result<Vec<String>, String> {
    let supported = definitions()
        .into_iter()
        .map(|definition| definition.id)
        .collect::<BTreeSet<_>>();
    let mut normalized = Vec::new();
    for provider_id in provider_ids {
        let provider_id = provider_id.trim().to_ascii_lowercase();
        if provider_id.is_empty() || normalized.contains(&provider_id) {
            continue;
        }
        if !supported.contains(provider_id.as_str()) {
            return Err(format!(
                "Proveedor de enrichment no soportado: {provider_id}."
            ));
        }
        normalized.push(provider_id);
    }
    if normalized.is_empty() {
        normalized.push("musicbrainz".to_string());
    }
    Ok(normalized)
}

pub fn load_provider_clients(
    app: &AppHandle,
    provider_ids: &[String],
) -> Result<Vec<ProviderClient>, String> {
    provider_ids
        .iter()
        .map(|provider_id| {
            let provider = provider_by_id(provider_id)
                .ok_or_else(|| format!("Proveedor de enrichment no soportado: {provider_id}."))?;
            let definition = provider.definition();
            let credentials = load_credentials(app, &definition)?;
            validate_required_credentials(&definition, &credentials)?;
            Ok(ProviderClient {
                provider,
                credentials,
            })
        })
        .collect()
}

#[cfg(test)]
pub fn load_provider_clients_for_test(
    provider_ids: &[String],
    credentials: ProviderCredentials,
) -> Result<Vec<ProviderClient>, String> {
    provider_ids
        .iter()
        .map(|provider_id| {
            let provider = provider_by_id(provider_id)
                .ok_or_else(|| format!("Proveedor de enrichment no soportado: {provider_id}."))?;
            Ok(ProviderClient {
                provider,
                credentials: credentials.clone(),
            })
        })
        .collect()
}

pub fn provider_descriptors(app: &AppHandle) -> Result<Vec<EnrichmentProviderDescriptor>, String> {
    definitions()
        .into_iter()
        .map(|definition| descriptor(app, &definition))
        .collect()
}

pub fn provider_descriptor(
    app: &AppHandle,
    provider_id: &str,
) -> Result<EnrichmentProviderDescriptor, String> {
    let definition = definitions()
        .into_iter()
        .find(|definition| definition.id == provider_id)
        .ok_or_else(|| format!("Proveedor de enrichment no soportado: {provider_id}."))?;
    descriptor(app, &definition)
}

fn descriptor(
    app: &AppHandle,
    definition: &ProviderDefinition,
) -> Result<EnrichmentProviderDescriptor, String> {
    let credentials = definition
        .credentials
        .iter()
        .map(|requirement| {
            let value = settings::load_enrichment_credential(app, definition.id, requirement.id)?;
            Ok(ProviderCredentialDescriptor {
                id: requirement.id.to_string(),
                label: requirement.label.to_string(),
                required: requirement.required,
                secret: requirement.secret,
                configured: value.is_some(),
                preview: value.as_deref().map(settings::masked_secret),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let ready = credentials
        .iter()
        .all(|credential| !credential.required || credential.configured);
    Ok(EnrichmentProviderDescriptor {
        id: definition.id.to_string(),
        label: definition.label.to_string(),
        description: definition.description.to_string(),
        capabilities: definition
            .capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        accepted_identifiers: definition
            .accepted_identifiers
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        produced_identifiers: definition
            .produced_identifiers
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        credentials,
        ready,
        min_interval_ms: definition.min_interval_ms,
    })
}

fn provider_by_id(provider_id: &str) -> Option<Box<dyn EnrichmentProvider>> {
    match provider_id {
        "musicbrainz" => Some(Box::new(MusicBrainzProvider)),
        "lastfm" => Some(Box::new(LastFmProvider)),
        _ => None,
    }
}

fn load_credentials(
    app: &AppHandle,
    definition: &ProviderDefinition,
) -> Result<ProviderCredentials, String> {
    let mut credentials = ProviderCredentials::default();
    for requirement in definition.credentials {
        if let Some(value) =
            settings::load_enrichment_credential(app, definition.id, requirement.id)?
        {
            credentials.insert(requirement.id, value);
        }
    }
    Ok(credentials)
}

fn validate_required_credentials(
    definition: &ProviderDefinition,
    credentials: &ProviderCredentials,
) -> Result<(), String> {
    for requirement in definition
        .credentials
        .iter()
        .filter(|requirement| requirement.required)
    {
        credentials
            .require(requirement.id, definition.label)
            .map_err(|error| error.message)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rejects_unknown_providers() {
        let error = normalize_provider_ids(vec!["unknown".to_string()]).unwrap_err();
        assert!(error.contains("no soportado"));
    }

    #[test]
    fn registry_deduplicates_provider_ids() {
        assert_eq!(
            normalize_provider_ids(vec![
                " MusicBrainz ".to_string(),
                "musicbrainz".to_string(),
                "LASTFM".to_string(),
            ])
            .unwrap(),
            vec!["musicbrainz", "lastfm"]
        );
    }
}
