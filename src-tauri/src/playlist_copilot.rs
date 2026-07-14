use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct PlaylistIntent {
    pub(crate) genres: Vec<String>,
    pub(crate) artists: Vec<String>,
    pub(crate) keys: Vec<String>,
    pub(crate) bpm_min: Option<f64>,
    pub(crate) bpm_max: Option<f64>,
    pub(crate) mood: Option<String>,
    pub(crate) energy: Option<String>,
    pub(crate) exclude_terms: Vec<String>,
    pub(crate) target_count: Option<usize>,
    pub(crate) energy_curve: EnergyCurve,
    pub(crate) harmonic_policy: HarmonicPolicy,
    pub(crate) discovery_mode: DiscoveryMode,
    pub(crate) tempo_policy: TempoPolicy,
    pub(crate) source_policy: SourcePolicy,
    pub(crate) focus_policy: FocusPolicy,
    pub(crate) max_tracks_per_artist: usize,
}

impl Default for PlaylistIntent {
    fn default() -> Self {
        Self {
            genres: Vec::new(),
            artists: Vec::new(),
            keys: Vec::new(),
            bpm_min: None,
            bpm_max: None,
            mood: None,
            energy: None,
            exclude_terms: Vec::new(),
            target_count: None,
            energy_curve: EnergyCurve::Flat,
            harmonic_policy: HarmonicPolicy::Soft,
            discovery_mode: DiscoveryMode::Balanced,
            tempo_policy: TempoPolicy::Flexible,
            source_policy: SourcePolicy::PreferAvailable,
            focus_policy: FocusPolicy::Balanced,
            max_tracks_per_artist: 3,
        }
    }
}

impl PlaylistIntent {
    pub(crate) fn has_musical_signals(&self) -> bool {
        !self.genres.is_empty()
            || !self.artists.is_empty()
            || !self.keys.is_empty()
            || self.bpm_min.is_some()
            || self.bpm_max.is_some()
            || self.mood.is_some()
            || self.energy.is_some()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EnergyCurve {
    #[default]
    Flat,
    SlowBuild,
    Ramp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HarmonicPolicy {
    Ignore,
    #[default]
    Soft,
    Strict,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiscoveryMode {
    Known,
    #[default]
    Balanced,
    Discovery,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TempoPolicy {
    Tight,
    #[default]
    Flexible,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SourcePolicy {
    AvailableOnly,
    #[default]
    PreferAvailable,
    AllowMissing,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FocusPolicy {
    Genre,
    Mood,
    #[default]
    Balanced,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GuidedAnswer {
    pub(crate) question_id: String,
    pub(crate) value: String,
}

pub(crate) fn apply_guided_answer(
    intent: &mut PlaylistIntent,
    answer: &GuidedAnswer,
    tempo_anchor: Option<f64>,
) {
    match answer.question_id.as_str() {
        "style_focus" => {
            if let Some(genre) = answer.value.strip_prefix("genre:") {
                if !genre.trim().is_empty()
                    && !intent
                        .genres
                        .iter()
                        .any(|item| normalize(item) == normalize(genre))
                {
                    intent.genres.push(genre.trim().to_string());
                }
                intent.focus_policy = FocusPolicy::Genre;
            } else if answer.value == "mood_first" {
                intent.focus_policy = FocusPolicy::Mood;
            }
        }
        "set_shape" => {
            intent.energy_curve = match answer.value.as_str() {
                "slow_build" => EnergyCurve::SlowBuild,
                "energy_ramp" => EnergyCurve::Ramp,
                _ => EnergyCurve::Flat,
            };
        }
        "harmony" => {
            intent.harmonic_policy = match answer.value.as_str() {
                "strict" => HarmonicPolicy::Strict,
                "ignore" => {
                    intent.keys.clear();
                    HarmonicPolicy::Ignore
                }
                _ => HarmonicPolicy::Soft,
            };
        }
        "discovery" => {
            intent.discovery_mode = match answer.value.as_str() {
                "known" => DiscoveryMode::Known,
                "discovery" => DiscoveryMode::Discovery,
                _ => DiscoveryMode::Balanced,
            };
        }
        "tempo" => {
            intent.tempo_policy = if answer.value == "tight" {
                if intent.bpm_min.is_none() && intent.bpm_max.is_none() {
                    if let Some(anchor) = tempo_anchor {
                        intent.bpm_min = Some((anchor - 4.0).max(50.0));
                        intent.bpm_max = Some((anchor + 4.0).min(220.0));
                    }
                }
                TempoPolicy::Tight
            } else {
                TempoPolicy::Flexible
            };
        }
        _ => {}
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TrackFeatures {
    pub(crate) track_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) genre: String,
    pub(crate) key: String,
    pub(crate) bpm: Option<f64>,
    pub(crate) duration_seconds: Option<u64>,
    pub(crate) source_exists: bool,
    pub(crate) search_text: String,
    pub(crate) metadata_quality: usize,
    pub(crate) semantic_score: Option<f64>,
    pub(crate) semantic_probes: Vec<String>,
    pub(crate) prior_suggestion_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct RankedTrack {
    pub(crate) track_id: String,
    pub(crate) score: f64,
    pub(crate) reasons: Vec<String>,
    pub(crate) components: BTreeMap<String, f64>,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn rank_and_sequence(
    tracks: &[TrackFeatures],
    intent: &PlaylistIntent,
    prompt: &str,
    target_count: usize,
) -> Vec<RankedTrack> {
    rank_and_sequence_with_seed(tracks, intent, prompt, target_count, 0)
}

pub(crate) fn rank_and_sequence_with_seed(
    tracks: &[TrackFeatures],
    intent: &PlaylistIntent,
    prompt: &str,
    target_count: usize,
    exploration_seed: u64,
) -> Vec<RankedTrack> {
    let prompt_terms = prompt_terms(prompt);
    let exclusions = intent
        .exclude_terms
        .iter()
        .map(|term| normalize(term))
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    let artist_counts = artist_counts(tracks);
    let identity_history = identity_history_counts(tracks);
    let max_artist_count = artist_counts.values().copied().max().unwrap_or(1) as f64;
    let semantic_values = semantic_values(tracks);
    let has_retrieval_signal = intent.has_musical_signals() || !prompt_terms.is_empty();

    let candidates = tracks
        .iter()
        .filter_map(|track| {
            let normalized_text = normalize(&track.search_text);
            if exclusions
                .iter()
                .any(|term| contains_phrase(&normalized_text, term))
            {
                return None;
            }
            if intent.source_policy == SourcePolicy::AvailableOnly && !track.source_exists {
                return None;
            }
            if intent.tempo_policy == TempoPolicy::Tight
                && !bpm_within_range(track.bpm, intent.bpm_min, intent.bpm_max)
            {
                return None;
            }
            if intent.harmonic_policy == HarmonicPolicy::Strict
                && !intent.keys.is_empty()
                && !matches_any(&track.key, &intent.keys)
            {
                return None;
            }

            let mut reasons = Vec::new();
            let mut components = BTreeMap::new();
            let mut relevance = 0.0;

            if let Some(score) = track.semantic_score {
                let percentile = percentile(score, &semantic_values);
                let points = 8.0 + percentile * 28.0;
                relevance += points;
                components.insert("semantic".to_string(), points);
                if track.semantic_probes.is_empty() {
                    reasons.push("Match semantico con el brief".to_string());
                } else {
                    reasons.push(format!(
                        "Encontrado por: {}",
                        track
                            .semantic_probes
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }

            let genre_weight = match intent.focus_policy {
                FocusPolicy::Genre => 38.0,
                FocusPolicy::Mood => 18.0,
                FocusPolicy::Balanced => 32.0,
            };
            if matches_any(&track.genre, &intent.genres) {
                relevance += genre_weight;
                components.insert("genre".to_string(), genre_weight);
                reasons.push(format!("Genero: {}", track.genre));
            }
            if matches_any(&track.artist, &intent.artists) {
                relevance += 26.0;
                components.insert("artist".to_string(), 26.0);
                reasons.push(format!("Artista: {}", track.artist));
            }
            if matches_any(&track.key, &intent.keys) {
                relevance += 16.0;
                components.insert("key".to_string(), 16.0);
                reasons.push(format!("Key: {}", track.key));
            }

            let bpm_points = bpm_score(track.bpm, intent.bpm_min, intent.bpm_max);
            if bpm_points.abs() > f64::EPSILON {
                relevance += bpm_points;
                components.insert("bpm".to_string(), bpm_points);
                if bpm_points > 12.0 {
                    if let Some(bpm) = track.bpm {
                        reasons.push(format!("BPM {bpm:.0} dentro del rango"));
                    }
                }
            }

            let matched_prompt_terms = prompt_terms
                .iter()
                .filter(|term| contains_phrase(&normalized_text, term))
                .take(8)
                .cloned()
                .collect::<Vec<_>>();
            if !matched_prompt_terms.is_empty() {
                let points = (matched_prompt_terms.len() as f64 * 1.75).min(12.0);
                relevance += points;
                components.insert("lexical".to_string(), points);
                reasons.push(format!("Coincide con: {}", matched_prompt_terms.join(", ")));
            }

            if let Some(mood) = intent.mood.as_deref() {
                let mood_weight = if intent.focus_policy == FocusPolicy::Mood {
                    18.0
                } else {
                    8.0
                };
                if contains_phrase(&normalized_text, &normalize(mood)) {
                    relevance += mood_weight;
                    components.insert("mood".to_string(), mood_weight);
                    reasons.push(format!("Mood: {mood}"));
                }
            }
            if let Some(energy) = intent.energy.as_deref() {
                let energy_points = energy_score(track.bpm, energy);
                if energy_points > 0.0 {
                    relevance += energy_points;
                    components.insert("energy".to_string(), energy_points);
                    reasons.push(format!("Energia: {energy}"));
                }
            }

            if has_retrieval_signal && relevance < 5.0 {
                return None;
            }

            let artist_key = normalize(&track.artist);
            let artist_frequency = artist_counts.get(&artist_key).copied().unwrap_or(1) as f64;
            let popularity = artist_frequency / max_artist_count;
            let discovery_points = match intent.discovery_mode {
                DiscoveryMode::Known => popularity * 8.0,
                DiscoveryMode::Discovery => (1.0 - popularity) * 8.0,
                DiscoveryMode::Balanced => (1.0 - (popularity - 0.5).abs() * 2.0) * 3.0,
            };
            components.insert("discovery".to_string(), discovery_points);

            let prior_suggestions = identity_history
                .get(&track_identity(track))
                .copied()
                .unwrap_or_default();
            let history_points = match intent.discovery_mode {
                DiscoveryMode::Known => 0.0,
                DiscoveryMode::Balanced => -(prior_suggestions.min(8) as f64).sqrt() * 8.0,
                DiscoveryMode::Discovery => -(prior_suggestions.min(8) as f64).sqrt() * 12.0,
            };
            if history_points < 0.0 {
                components.insert("recent_history".to_string(), history_points);
                reasons.push(format!(
                    "Sugerido recientemente {} vez/veces",
                    prior_suggestions
                ));
            }

            let metadata_points = (track.metadata_quality.min(6) as f64) * 0.4;
            components.insert("metadata".to_string(), metadata_points);
            let source_points =
                if !track.source_exists && intent.source_policy == SourcePolicy::PreferAvailable {
                    reasons.push("Archivo no encontrado".to_string());
                    -28.0
                } else {
                    0.0
                };
            if source_points != 0.0 {
                components.insert("source".to_string(), source_points);
            }

            let exploration_weight = match intent.discovery_mode {
                DiscoveryMode::Known => 1.5,
                DiscoveryMode::Balanced => 7.0,
                DiscoveryMode::Discovery => 12.0,
            };
            let exploration_points = (exploration_unit(exploration_seed, &track_identity(track))
                - 0.5)
                * exploration_weight;
            components.insert("exploration".to_string(), exploration_points);

            let score = relevance
                + discovery_points
                + history_points
                + metadata_points
                + source_points
                + exploration_points;
            if reasons.is_empty() {
                reasons.push("Fit general por metadata disponible".to_string());
            }

            Some(RankedTrack {
                track_id: track.track_id.clone(),
                score: round_score(score),
                reasons,
                components,
            })
        })
        .collect::<Vec<_>>();

    let mut candidates = deduplicate_candidates(candidates, tracks);
    candidates.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.track_id.cmp(&right.track_id))
    });

    let selected = diversify(candidates, tracks, intent, target_count);
    sequence(selected, tracks, intent)
}

fn diversify(
    candidates: Vec<RankedTrack>,
    tracks: &[TrackFeatures],
    intent: &PlaylistIntent,
    target_count: usize,
) -> Vec<RankedTrack> {
    let features = tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let artist_limit = intent.max_tracks_per_artist.max(1);
    let mut artist_counts = HashMap::<String, usize>::new();
    let distinct_genres = candidates
        .iter()
        .filter_map(|candidate| features.get(candidate.track_id.as_str()))
        .map(|track| normalize(&track.genre))
        .filter(|genre| !genre.is_empty())
        .collect::<HashSet<_>>();
    let genre_limit = (intent.genres.len() != 1 && distinct_genres.len() >= 3)
        .then(|| ((target_count as f64 * 0.45).ceil() as usize).max(1));
    let mut genre_counts = HashMap::<String, usize>::new();
    let mut selected = Vec::new();
    let mut selected_ids = HashSet::<String>::new();
    let mut deferred = Vec::new();

    if genre_limit.is_some() {
        for candidate in &candidates {
            if selected.len() >= target_count || genre_counts.len() >= 3 {
                break;
            }
            let track = features.get(candidate.track_id.as_str()).copied();
            let artist = track
                .map(|track| normalize(&track.artist))
                .unwrap_or_default();
            let genre = track
                .map(|track| normalize(&track.genre))
                .unwrap_or_default();
            if genre.is_empty()
                || genre_counts.contains_key(&genre)
                || (!artist.is_empty()
                    && artist_counts.get(&artist).copied().unwrap_or_default() >= artist_limit)
            {
                continue;
            }
            *artist_counts.entry(artist).or_default() += 1;
            *genre_counts.entry(genre).or_default() += 1;
            selected_ids.insert(candidate.track_id.clone());
            selected.push(candidate.clone());
        }
    }

    for candidate in candidates {
        if selected_ids.contains(&candidate.track_id) {
            continue;
        }
        let track = features.get(candidate.track_id.as_str()).copied();
        let artist = track
            .map(|track| normalize(&track.artist))
            .unwrap_or_default();
        let genre = track
            .map(|track| normalize(&track.genre))
            .unwrap_or_default();
        let artist_count = artist_counts.get(&artist).copied().unwrap_or_default();
        let genre_count = genre_counts.get(&genre).copied().unwrap_or_default();
        let genre_available =
            genre_limit.is_none_or(|limit| genre.is_empty() || genre_count < limit);
        if selected.len() < target_count
            && (artist.is_empty() || artist_count < artist_limit)
            && genre_available
        {
            *artist_counts.entry(artist).or_default() += 1;
            *genre_counts.entry(genre).or_default() += 1;
            selected.push(candidate);
        } else {
            deferred.push(candidate);
        }
    }

    let mut artist_deferred = Vec::new();
    for candidate in deferred {
        if selected.len() >= target_count {
            break;
        }
        let artist = features
            .get(candidate.track_id.as_str())
            .map(|track| normalize(&track.artist))
            .unwrap_or_default();
        let count = artist_counts.get(&artist).copied().unwrap_or_default();
        if artist.is_empty() || count < artist_limit {
            *artist_counts.entry(artist).or_default() += 1;
            selected.push(candidate);
        } else {
            artist_deferred.push(candidate);
        }
    }

    for candidate in artist_deferred {
        if selected.len() >= target_count {
            break;
        }
        selected.push(candidate);
    }

    selected
}

fn sequence(
    selected: Vec<RankedTrack>,
    tracks: &[TrackFeatures],
    intent: &PlaylistIntent,
) -> Vec<RankedTrack> {
    if selected.len() <= 1 {
        return selected;
    }

    let features = tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let bpms = selected
        .iter()
        .filter_map(|candidate| {
            features
                .get(candidate.track_id.as_str())
                .and_then(|track| track.bpm)
        })
        .collect::<Vec<_>>();
    let bpm_min = intent
        .bpm_min
        .or_else(|| bpms.iter().copied().reduce(f64::min));
    let bpm_max = intent
        .bpm_max
        .or_else(|| bpms.iter().copied().reduce(f64::max));
    let score_min = selected
        .iter()
        .map(|candidate| candidate.score)
        .reduce(f64::min)
        .unwrap_or(0.0);
    let score_max = selected
        .iter()
        .map(|candidate| candidate.score)
        .reduce(f64::max)
        .unwrap_or(score_min);

    let mut remaining = selected;
    let mut ordered = Vec::with_capacity(remaining.len());
    while !remaining.is_empty() {
        let position = ordered.len();
        let progress = if position == 0 {
            0.0
        } else {
            position as f64 / (position + remaining.len()).saturating_sub(1).max(1) as f64
        };
        let target_bpm = target_bpm(intent.energy_curve, bpm_min, bpm_max, progress);
        let previous = ordered
            .last()
            .and_then(|candidate: &RankedTrack| features.get(candidate.track_id.as_str()).copied());

        let best_index = remaining
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let track = features.get(candidate.track_id.as_str()).copied();
                let relevance_weight = match intent.energy_curve {
                    EnergyCurve::Flat => 30.0,
                    EnergyCurve::SlowBuild => 14.0,
                    EnergyCurve::Ramp => 8.0,
                };
                let relevance =
                    normalize_score(candidate.score, score_min, score_max) * relevance_weight;
                let curve = track
                    .and_then(|track| track.bpm.zip(target_bpm))
                    .map(|(bpm, target)| curve_score(intent.energy_curve, bpm, target))
                    .unwrap_or(0.0);
                let transition = match (previous, track) {
                    (Some(previous), Some(track)) => transition_score(previous, track, intent),
                    _ => 0.0,
                };
                (index, relevance + curve + transition)
            })
            .max_by(|left, right| {
                left.1
                    .total_cmp(&right.1)
                    .then_with(|| right.0.cmp(&left.0))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);

        let mut next = remaining.remove(best_index);
        if let (Some(previous), Some(track)) =
            (previous, features.get(next.track_id.as_str()).copied())
        {
            if harmonic_compatible(&previous.key, &track.key) {
                next.reasons
                    .push("Transicion armonica compatible".to_string());
            }
            if let (Some(left), Some(right)) = (previous.bpm, track.bpm) {
                if (left - right).abs() <= 3.0 {
                    next.reasons.push("Transicion BPM cercana".to_string());
                }
            }
        }
        ordered.push(next);
    }

    ordered
}

fn artist_counts(tracks: &[TrackFeatures]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    let mut seen = HashSet::new();
    for track in tracks {
        if !seen.insert(track_identity(track)) {
            continue;
        }
        *counts.entry(normalize(&track.artist)).or_default() += 1;
    }
    counts
}

fn identity_history_counts(tracks: &[TrackFeatures]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for track in tracks {
        *counts.entry(track_identity(track)).or_default() += track.prior_suggestion_count;
    }
    counts
}

fn deduplicate_candidates(
    candidates: Vec<RankedTrack>,
    tracks: &[TrackFeatures],
) -> Vec<RankedTrack> {
    let features = tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let mut by_identity = HashMap::<String, RankedTrack>::new();
    for candidate in candidates {
        let identity = features
            .get(candidate.track_id.as_str())
            .map(|track| track_identity(track))
            .unwrap_or_else(|| format!("track:{}", candidate.track_id));
        match by_identity.get(&identity) {
            Some(current) if current.score > candidate.score => {}
            Some(current)
                if current.score == candidate.score && current.track_id <= candidate.track_id => {}
            _ => {
                by_identity.insert(identity, candidate);
            }
        }
    }
    by_identity.into_values().collect()
}

fn track_identity(track: &TrackFeatures) -> String {
    let title = normalize(&track.title);
    let artist = normalize(&track.artist);
    if title.is_empty() {
        return format!("track:{}", track.track_id);
    }
    let duration = track
        .duration_seconds
        .map(|seconds| format!("duration:{}", (seconds + 2) / 5))
        .unwrap_or_else(|| "duration:unknown".to_string());
    format!("artist:{artist}|title:{title}|{duration}")
}

fn semantic_values(tracks: &[TrackFeatures]) -> Vec<f64> {
    let mut values = tracks
        .iter()
        .filter_map(|track| track.semantic_score)
        .filter(|score| score.is_finite())
        .collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values
}

fn percentile(value: f64, sorted: &[f64]) -> f64 {
    if sorted.len() <= 1 {
        return 1.0;
    }
    let rank = sorted.partition_point(|candidate| *candidate <= value);
    rank.saturating_sub(1) as f64 / (sorted.len() - 1) as f64
}

fn exploration_unit(seed: u64, identity: &str) -> f64 {
    let mut hash = 0xcbf29ce484222325_u64 ^ seed;
    for byte in identity.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as f64 / u64::MAX as f64
}

fn bpm_within_range(bpm: Option<f64>, min: Option<f64>, max: Option<f64>) -> bool {
    if min.is_none() && max.is_none() {
        return true;
    }
    bpm.is_some_and(|value| value >= min.unwrap_or(50.0) && value <= max.unwrap_or(220.0))
}

fn bpm_score(bpm: Option<f64>, min: Option<f64>, max: Option<f64>) -> f64 {
    if min.is_none() && max.is_none() {
        return 0.0;
    }
    let Some(bpm) = bpm else {
        return -4.0;
    };
    let min = min.unwrap_or(50.0);
    let max = max.unwrap_or(220.0);
    if bpm >= min && bpm <= max {
        let center = (min + max) / 2.0;
        let half_width = ((max - min) / 2.0).max(1.0);
        return 20.0 + (1.0 - ((bpm - center).abs() / half_width).min(1.0)) * 4.0;
    }
    let distance = if bpm < min { min - bpm } else { bpm - max };
    (12.0 - distance * 1.5).max(-10.0)
}

fn energy_score(bpm: Option<f64>, energy: &str) -> f64 {
    let Some(bpm) = bpm else {
        return 0.0;
    };
    match normalize(energy).as_str() {
        "peak" if bpm >= 124.0 => 6.0,
        "warmup" if bpm <= 124.0 => 6.0,
        "closing" if bpm <= 128.0 => 4.0,
        _ => 0.0,
    }
}

fn matches_any(value: &str, terms: &[String]) -> bool {
    if value.trim().is_empty() || terms.is_empty() {
        return false;
    }
    let value = normalize(value);
    terms
        .iter()
        .map(|term| normalize(term))
        .any(|term| contains_phrase(&value, &term) || contains_phrase(&term, &value))
}

fn prompt_terms(prompt: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "para", "con", "sin", "que", "una", "uno", "los", "las", "the", "and", "for", "from",
        "playlist", "lista", "temas", "tracks", "quiero", "generar", "crear", "algo", "entre",
        "bpm", "del", "por", "como", "more", "keep", "same", "pero", "mantener",
    ];
    let mut terms = Vec::new();
    for term in normalize(prompt).split_whitespace() {
        if term.len() >= 3 && !STOPWORDS.contains(&term) && !terms.iter().any(|item| item == term) {
            terms.push(term.to_string());
            if terms.len() >= 24 {
                break;
            }
        }
    }
    terms
}

fn target_bpm(
    curve: EnergyCurve,
    min: Option<f64>,
    max: Option<f64>,
    progress: f64,
) -> Option<f64> {
    let (Some(min), Some(max)) = (min, max) else {
        return None;
    };
    let progress = progress.clamp(0.0, 1.0);
    Some(match curve {
        EnergyCurve::Flat => (min + max) / 2.0,
        EnergyCurve::SlowBuild => min + (max - min) * (0.15 + progress * 0.7),
        EnergyCurve::Ramp => min + (max - min) * progress,
    })
}

fn curve_score(curve: EnergyCurve, bpm: f64, target: f64) -> f64 {
    let (base, weight) = match curve {
        EnergyCurve::Flat => (4.0, 0.25),
        EnergyCurve::SlowBuild => (20.0, 1.25),
        EnergyCurve::Ramp => (28.0, 2.0),
    };
    base - (bpm - target).abs() * weight
}

fn transition_score(
    previous: &TrackFeatures,
    current: &TrackFeatures,
    intent: &PlaylistIntent,
) -> f64 {
    let bpm_score = match (previous.bpm, current.bpm) {
        (Some(left), Some(right)) => (10.0 - (left - right).abs()).max(-8.0),
        _ => 0.0,
    };
    let harmonic_score = match intent.harmonic_policy {
        HarmonicPolicy::Ignore => 0.0,
        HarmonicPolicy::Soft if harmonic_compatible(&previous.key, &current.key) => 8.0,
        HarmonicPolicy::Soft => -3.0,
        HarmonicPolicy::Strict if harmonic_compatible(&previous.key, &current.key) => 12.0,
        HarmonicPolicy::Strict => -18.0,
    };
    let artist_penalty = if !previous.artist.trim().is_empty()
        && normalize(&previous.artist) == normalize(&current.artist)
    {
        -10.0
    } else {
        0.0
    };
    bpm_score + harmonic_score + artist_penalty
}

fn harmonic_compatible(left: &str, right: &str) -> bool {
    let (Some(left), Some(right)) = (camelot_key(left), camelot_key(right)) else {
        return false;
    };
    left == right
        || (left.number == right.number && left.mode != right.mode)
        || (left.mode == right.mode
            && (left.number % 12 + 1 == right.number || right.number % 12 + 1 == left.number))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CamelotKey {
    number: u8,
    mode: char,
}

fn camelot_key(value: &str) -> Option<CamelotKey> {
    let compact = normalize_key(value);
    if compact.len() >= 2 {
        let (number, mode) = compact.split_at(compact.len() - 1);
        if matches!(mode, "A" | "B") {
            if let Ok(number) = number.parse::<u8>() {
                if (1..=12).contains(&number) {
                    return Some(CamelotKey {
                        number,
                        mode: mode.chars().next()?,
                    });
                }
            }
        }
    }

    let (number, mode) = match compact.as_str() {
        "ABM" | "G#M" => (1, 'A'),
        "EBM" | "D#M" => (2, 'A'),
        "BBM" | "A#M" => (3, 'A'),
        "FM" => (4, 'A'),
        "CM" => (5, 'A'),
        "GM" => (6, 'A'),
        "DM" => (7, 'A'),
        "AM" => (8, 'A'),
        "EM" => (9, 'A'),
        "BM" => (10, 'A'),
        "F#M" | "GBM" => (11, 'A'),
        "C#M" | "DBM" => (12, 'A'),
        "B" => (1, 'B'),
        "F#" | "GB" => (2, 'B'),
        "DB" | "C#" => (3, 'B'),
        "AB" | "G#" => (4, 'B'),
        "EB" | "D#" => (5, 'B'),
        "BB" | "A#" => (6, 'B'),
        "F" => (7, 'B'),
        "C" => (8, 'B'),
        "G" => (9, 'B'),
        "D" => (10, 'B'),
        "A" => (11, 'B'),
        "E" => (12, 'B'),
        _ => return None,
    };
    Some(CamelotKey { number, mode })
}

fn normalize_key(value: &str) -> String {
    value
        .trim()
        .replace('♯', "#")
        .replace('♭', "B")
        .replace("MINOR", "M")
        .replace("minor", "M")
        .replace("MIN", "M")
        .replace("min", "M")
        .replace(' ', "")
        .to_uppercase()
}

fn normalize_score(value: f64, min: f64, max: f64) -> f64 {
    if (max - min).abs() <= f64::EPSILON {
        1.0
    } else {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    }
}

fn round_score(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn contains_phrase(haystack: &str, needle: &str) -> bool {
    let needle = needle.trim();
    if needle.is_empty() {
        return false;
    }
    if needle.len() <= 2 {
        return haystack.split_whitespace().any(|token| token == needle);
    }
    haystack.contains(needle)
}

fn normalize(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| match character {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            character if character.is_alphanumeric() => character,
            _ => ' ',
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, artist: &str, genre: &str, bpm: f64, key: &str) -> TrackFeatures {
        TrackFeatures {
            track_id: id.to_string(),
            title: format!("Track {id}"),
            artist: artist.to_string(),
            genre: genre.to_string(),
            key: key.to_string(),
            bpm: Some(bpm),
            duration_seconds: Some(300),
            source_exists: true,
            search_text: format!("Track {id} {artist} {genre} {bpm} {key}"),
            metadata_quality: 6,
            semantic_score: Some(0.7 + id.len() as f64 / 100.0),
            semantic_probes: Vec::new(),
            prior_suggestion_count: 0,
        }
    }

    #[test]
    fn guided_answers_update_executable_intent() {
        let mut intent = PlaylistIntent::default();
        apply_guided_answer(
            &mut intent,
            &GuidedAnswer {
                question_id: "set_shape".to_string(),
                value: "energy_ramp".to_string(),
            },
            None,
        );
        apply_guided_answer(
            &mut intent,
            &GuidedAnswer {
                question_id: "harmony".to_string(),
                value: "strict".to_string(),
            },
            None,
        );
        apply_guided_answer(
            &mut intent,
            &GuidedAnswer {
                question_id: "tempo".to_string(),
                value: "tight".to_string(),
            },
            Some(124.0),
        );

        assert_eq!(intent.energy_curve, EnergyCurve::Ramp);
        assert_eq!(intent.harmonic_policy, HarmonicPolicy::Strict);
        assert_eq!(intent.tempo_policy, TempoPolicy::Tight);
        assert_eq!(intent.bpm_min, Some(120.0));
        assert_eq!(intent.bpm_max, Some(128.0));
    }

    #[test]
    fn hard_constraints_remove_missing_sources_and_out_of_range_tracks() {
        let mut missing = track("missing", "Artist A", "House", 122.0, "8A");
        missing.source_exists = false;
        let tracks = vec![
            missing,
            track("slow", "Artist B", "House", 110.0, "8A"),
            track("fit", "Artist C", "House", 123.0, "8A"),
        ];
        let intent = PlaylistIntent {
            genres: vec!["House".to_string()],
            bpm_min: Some(120.0),
            bpm_max: Some(126.0),
            tempo_policy: TempoPolicy::Tight,
            source_policy: SourcePolicy::AvailableOnly,
            ..PlaylistIntent::default()
        };

        let ranked = rank_and_sequence(&tracks, &intent, "house", 10);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].track_id, "fit");
    }

    #[test]
    fn semantic_percentiles_produce_continuous_ordering() {
        let mut low = track("low", "Artist A", "House", 122.0, "8A");
        low.semantic_score = Some(0.51);
        let mut mid = track("mid", "Artist B", "House", 122.0, "8A");
        mid.semantic_score = Some(0.64);
        let mut high = track("high", "Artist C", "House", 122.0, "8A");
        high.semantic_score = Some(0.83);
        let ranked = rank_and_sequence(&[low, mid, high], &PlaylistIntent::default(), "house", 3);

        let scores = ranked
            .iter()
            .map(|item| (item.track_id.as_str(), item.score))
            .collect::<HashMap<_, _>>();
        assert!(scores["high"] > scores["mid"]);
        assert!(scores["mid"] > scores["low"]);
    }

    #[test]
    fn ramp_sequences_tracks_from_lower_to_higher_bpm() {
        let tracks = vec![
            track("fast", "Artist A", "House", 128.0, "9A"),
            track("slow", "Artist B", "House", 118.0, "8A"),
            track("middle", "Artist C", "House", 123.0, "8B"),
        ];
        let intent = PlaylistIntent {
            genres: vec!["House".to_string()],
            energy_curve: EnergyCurve::Ramp,
            harmonic_policy: HarmonicPolicy::Soft,
            ..PlaylistIntent::default()
        };
        let ranked = rank_and_sequence(&tracks, &intent, "house energy", 3);
        let by_id = tracks
            .iter()
            .map(|track| (track.track_id.as_str(), track.bpm.unwrap()))
            .collect::<HashMap<_, _>>();
        let bpms = ranked
            .iter()
            .map(|item| by_id[item.track_id.as_str()])
            .collect::<Vec<_>>();

        assert!(bpms[0] <= bpms[1]);
        assert!(bpms[1] <= bpms[2]);
    }

    #[test]
    fn duplicate_track_ids_collapse_to_one_musical_identity() {
        let mut first = track("copy-a", "Todd Terry Project, The", "House", 122.86, "2A");
        first.title = "Just Wanna Dance".to_string();
        first.duration_seconds = Some(349);
        let mut second = first.clone();
        second.track_id = "copy-b".to_string();
        second.semantic_score = Some(0.91);

        let ranked = rank_and_sequence(
            &[first, second],
            &PlaylistIntent::default(),
            "deep house",
            10,
        );

        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].track_id, "copy-b");
    }

    #[test]
    fn balanced_mode_penalizes_recently_repeated_tracks() {
        let mut repeated = track("repeated", "Artist A", "House", 123.0, "8A");
        repeated.prior_suggestion_count = 4;
        repeated.semantic_score = Some(0.8);
        let mut fresh = track("fresh", "Artist B", "House", 123.0, "8A");
        fresh.semantic_score = Some(0.8);

        let ranked = rank_and_sequence(&[repeated, fresh], &PlaylistIntent::default(), "house", 1);

        assert_eq!(ranked[0].track_id, "fresh");
        assert!(!ranked[0].components.contains_key("recent_history"));
    }

    #[test]
    fn exploration_seed_rotates_close_matches_without_losing_strong_anchor() {
        let mut tracks = (0..12)
            .map(|index| {
                let mut item = track(
                    &format!("track-{index}"),
                    &format!("Artist {index}"),
                    "House",
                    123.0,
                    "8A",
                );
                item.semantic_score = Some(0.75);
                item
            })
            .collect::<Vec<_>>();
        tracks[0].artist = "Reference Artist".to_string();
        let intent = PlaylistIntent {
            artists: vec!["Reference Artist".to_string()],
            ..PlaylistIntent::default()
        };

        let first = rank_and_sequence_with_seed(&tracks, &intent, "house set", 5, 11);
        let second = rank_and_sequence_with_seed(&tracks, &intent, "house set", 5, 98_711);
        let first_ids = first
            .iter()
            .map(|candidate| candidate.track_id.as_str())
            .collect::<HashSet<_>>();
        let second_ids = second
            .iter()
            .map(|candidate| candidate.track_id.as_str())
            .collect::<HashSet<_>>();

        assert!(first_ids.contains("track-0"));
        assert!(second_ids.contains("track-0"));
        assert_ne!(first_ids, second_ids);
    }

    #[test]
    fn broad_brief_soft_caps_a_dominant_genre() {
        let mut tracks = Vec::new();
        for index in 0..10 {
            let mut item = track(
                &format!("house-{index}"),
                &format!("House Artist {index}"),
                "House",
                123.0,
                "8A",
            );
            item.semantic_score = Some(0.9);
            tracks.push(item);
        }
        for (genre, score) in [("Techno", 0.78), ("Disco", 0.72)] {
            for index in 0..5 {
                let mut item = track(
                    &format!("{}-{index}", genre.to_lowercase()),
                    &format!("{genre} Artist {index}"),
                    genre,
                    124.0,
                    "9A",
                );
                item.semantic_score = Some(score);
                tracks.push(item);
            }
        }

        let ranked = rank_and_sequence_with_seed(
            &tracks,
            &PlaylistIntent::default(),
            "dancefloor selections",
            9,
            42,
        );
        let genres_by_id = tracks
            .iter()
            .map(|track| (track.track_id.as_str(), track.genre.as_str()))
            .collect::<HashMap<_, _>>();
        let mut genre_counts = HashMap::<&str, usize>::new();
        for candidate in ranked {
            *genre_counts
                .entry(genres_by_id[candidate.track_id.as_str()])
                .or_default() += 1;
        }

        assert!(genre_counts.len() >= 3);
        assert!(genre_counts.values().copied().max().unwrap_or_default() <= 5);
    }

    #[test]
    fn semantic_probe_labels_are_exposed_as_track_evidence() {
        let mut item = track("probe", "Artist", "House", 123.0, "8A");
        item.semantic_probes = vec!["Mood y energia".to_string(), "Brief completo".to_string()];

        let ranked = rank_and_sequence(&[item], &PlaylistIntent::default(), "house", 1);

        assert!(ranked[0]
            .reasons
            .iter()
            .any(|reason| reason.contains("Mood y energia") && reason.contains("Brief completo")));
    }

    #[test]
    fn camelot_and_musical_keys_are_compatible() {
        assert!(harmonic_compatible("8A", "9A"));
        assert!(harmonic_compatible("Am", "8B"));
        assert!(harmonic_compatible("F#m", "11A"));
        assert!(!harmonic_compatible("8A", "2B"));
    }
}
