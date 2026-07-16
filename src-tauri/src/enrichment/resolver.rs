use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ResolutionInput {
    pub provider: String,
    pub confidence: f64,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedField {
    pub value: String,
    pub provider: String,
    pub confidence: f64,
}

pub fn resolve_fields(inputs: &[ResolutionInput]) -> BTreeMap<String, ResolvedField> {
    let mut candidates = BTreeMap::<String, Vec<ResolvedField>>::new();
    for input in inputs {
        for field in [
            "genre",
            "year",
            "label",
            "isrc",
            "musicbrainz_recording_id",
            "musicbrainz_release_id",
            "bpm",
            "key",
        ] {
            if let Some(value) = input
                .fields
                .get(field)
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidates
                    .entry(field.to_string())
                    .or_default()
                    .push(ResolvedField {
                        value: value.to_string(),
                        provider: input.provider.clone(),
                        confidence: input.confidence,
                    });
            }
        }

        let comment = input
            .fields
            .get("tags")
            .map(|tags| format!("Tags: {}", tags.trim()))
            .or_else(|| {
                input
                    .fields
                    .get("lastfm_url")
                    .map(|url| format!("Last.fm: {}", url.trim()))
            });
        if let Some(comment) = comment.filter(|value| !value.trim().is_empty()) {
            candidates
                .entry("comments".to_string())
                .or_default()
                .push(ResolvedField {
                    value: comment,
                    provider: input.provider.clone(),
                    confidence: input.confidence,
                });
        }
    }

    candidates
        .into_iter()
        .filter_map(|(field, values)| {
            values
                .into_iter()
                .max_by(|left, right| {
                    resolution_score(&field, left).total_cmp(&resolution_score(&field, right))
                })
                .map(|value| (field, value))
        })
        .collect()
}

fn resolution_score(field: &str, candidate: &ResolvedField) -> f64 {
    candidate.confidence + provider_priority(field, &candidate.provider)
}

fn provider_priority(field: &str, provider: &str) -> f64 {
    match (field, provider) {
        ("genre" | "comments", "lastfm") => 0.25,
        (
            "year" | "label" | "isrc" | "musicbrainz_recording_id" | "musicbrainz_release_id",
            "musicbrainz",
        ) => 0.25,
        ("bpm" | "key", "local_audio") => 0.25,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_uses_field_specific_source_priority() {
        let inputs = vec![
            ResolutionInput {
                provider: "musicbrainz".to_string(),
                confidence: 0.94,
                fields: BTreeMap::from([
                    ("genre".to_string(), "Electronic".to_string()),
                    ("year".to_string(), "2022".to_string()),
                ]),
            },
            ResolutionInput {
                provider: "lastfm".to_string(),
                confidence: 0.8,
                fields: BTreeMap::from([("genre".to_string(), "Deep House".to_string())]),
            },
        ];

        let resolved = resolve_fields(&inputs);
        assert_eq!(resolved["genre"].value, "Deep House");
        assert_eq!(resolved["year"].value, "2022");
    }

    #[test]
    fn resolver_builds_comments_from_structured_tags() {
        let resolved = resolve_fields(&[ResolutionInput {
            provider: "lastfm".to_string(),
            confidence: 0.9,
            fields: BTreeMap::from([("tags".to_string(), "house, club".to_string())]),
        }]);

        assert_eq!(resolved["comments"].value, "Tags: house, club");
    }
}
