use super::{EnrichmentTrack, ProviderClient};

pub fn planned_provider_ids(
    track: &EnrichmentTrack,
    providers: &[ProviderClient],
    force_selected: bool,
) -> Vec<String> {
    if force_selected {
        return providers
            .iter()
            .map(|provider| provider.id().to_string())
            .collect();
    }

    let missing = track.missing_fields();
    providers
        .iter()
        .filter(|provider| {
            provider
                .definition()
                .capabilities
                .iter()
                .any(|field| missing.contains(field))
        })
        .map(|provider| provider.id().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::{load_provider_clients_for_test, ProviderCredentials};

    #[test]
    fn planner_skips_remote_sources_for_audio_only_gaps() {
        let providers = load_provider_clients_for_test(
            &["musicbrainz".to_string(), "lastfm".to_string()],
            ProviderCredentials::default(),
        )
        .expect("providers");
        let track = EnrichmentTrack {
            genre: Some("House".to_string()),
            comments: Some("ok".to_string()),
            year: Some("2024".to_string()),
            label: Some("Label".to_string()),
            ..EnrichmentTrack::default()
        };

        assert!(planned_provider_ids(&track, &providers, false).is_empty());
    }

    #[test]
    fn planner_routes_social_tags_only_to_lastfm() {
        let providers = load_provider_clients_for_test(
            &["musicbrainz".to_string(), "lastfm".to_string()],
            ProviderCredentials::default(),
        )
        .expect("providers");
        let track = EnrichmentTrack {
            genre: Some("House".to_string()),
            year: Some("2024".to_string()),
            label: Some("Label".to_string()),
            bpm: Some("124".to_string()),
            key: Some("8A".to_string()),
            ..EnrichmentTrack::default()
        };

        assert_eq!(
            planned_provider_ids(&track, &providers, false),
            vec!["lastfm"]
        );
    }
}
