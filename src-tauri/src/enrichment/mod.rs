mod domain;
mod planner;
mod providers;
mod registry;
mod resolver;

pub use domain::{
    CredentialRequirement, EnrichmentProvider, EnrichmentProviderDescriptor, EnrichmentTrack,
    ProviderCredentialDescriptor, ProviderCredentials, ProviderDefinition, ProviderError,
    ProviderErrorKind, ProviderSuggestion, ProviderTestResult,
};
pub use planner::planned_provider_ids;
#[cfg(test)]
pub use registry::load_provider_clients_for_test;
pub use registry::{
    load_provider_clients, normalize_provider_ids, provider_descriptor, provider_descriptors,
    ProviderClient,
};
pub use resolver::{resolve_fields, ResolutionInput, ResolvedField};

use crate::settings;
use tauri::AppHandle;

#[tauri::command]
pub fn enrichment_providers(app: AppHandle) -> Result<Vec<EnrichmentProviderDescriptor>, String> {
    provider_descriptors(&app)
}

#[tauri::command]
pub fn enrichment_save_provider_credential(
    app: AppHandle,
    provider_id: String,
    credential_id: String,
    value: String,
) -> Result<EnrichmentProviderDescriptor, String> {
    let provider_id = normalize_single_provider(&provider_id)?;
    validate_credential_id(&provider_id, &credential_id)?;
    settings::save_enrichment_credential(&app, &provider_id, &credential_id, Some(value))?;
    provider_descriptor(&app, &provider_id)
}

#[tauri::command]
pub fn enrichment_clear_provider_credential(
    app: AppHandle,
    provider_id: String,
    credential_id: String,
) -> Result<EnrichmentProviderDescriptor, String> {
    let provider_id = normalize_single_provider(&provider_id)?;
    validate_credential_id(&provider_id, &credential_id)?;
    settings::save_enrichment_credential(&app, &provider_id, &credential_id, None)?;
    provider_descriptor(&app, &provider_id)
}

#[tauri::command]
pub async fn enrichment_test_provider(
    app: AppHandle,
    provider_id: String,
) -> Result<ProviderTestResult, String> {
    let provider_id = normalize_single_provider(&provider_id)?;
    let app_for_task = app.clone();
    let id_for_task = provider_id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let clients = load_provider_clients(&app_for_task, std::slice::from_ref(&id_for_task))?;
        let client = clients
            .first()
            .ok_or_else(|| format!("Proveedor no encontrado: {id_for_task}."))?;
        client.test().map_err(|error| error.message)?;
        Ok(ProviderTestResult {
            provider_id: id_for_task,
            ok: true,
            message: "Conexion y credenciales validadas.".to_string(),
        })
    })
    .await
    .map_err(|error| format!("La validacion del proveedor fallo: {error}"))?
}

fn normalize_single_provider(provider_id: &str) -> Result<String, String> {
    if provider_id.trim().is_empty() {
        return Err("Proveedor requerido.".to_string());
    }
    normalize_provider_ids(vec![provider_id.to_string()])?
        .into_iter()
        .next()
        .ok_or_else(|| "Proveedor requerido.".to_string())
}

fn validate_credential_id(provider_id: &str, credential_id: &str) -> Result<(), String> {
    let descriptor = registry::definitions()
        .into_iter()
        .find(|definition| definition.id == provider_id)
        .ok_or_else(|| format!("Proveedor de enrichment no soportado: {provider_id}."))?;
    if descriptor
        .credentials
        .iter()
        .any(|credential| credential.id == credential_id)
    {
        Ok(())
    } else {
        Err(format!(
            "Credencial no soportada para {provider_id}: {credential_id}."
        ))
    }
}
