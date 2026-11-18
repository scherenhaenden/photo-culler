//! Versioned, explainable photo scores built only from metrics that ran.
//!
//! This module is locally provisional until the core crate registers it from
//! `lib.rs` during integration. Metric names and units follow the Rust image
//! engine (`*.ratio` values are in `[0, 1]`). Legacy Python keys are accepted
//! through explicit aliases and remain named as supplied in explanations.

use std::collections::BTreeMap;

pub const SCORING_CONTRACT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileId {
    Fast,
    Technical,
    Concert,
}

impl ProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Technical => "technical",
            Self::Concert => "concert",
        }
    }

    pub const fn version(self) -> u32 { 1 }

    const fn weights(self) -> [f64; 4] {
        match self {
            Self::Fast => [0.65, 0.35, 0.0, 0.0],
            Self::Technical => [0.40, 0.25, 0.20, 0.15],
            Self::Concert => [0.35, 0.15, 0.30, 0.20],
        }
    }
}

/// Versioned analyzer output. A cache entry is compatible only when both the
/// metric name and its producer version match the requested analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct Metric {
    pub name: String,
    pub version: u32,
    pub value: f64,
    pub confidence: f64,
}

impl Metric {
    pub fn new(name: impl Into<String>, version: u32, value: f64, confidence: f64) -> Self {
        Self { name: name.into(), version, value, confidence }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentExplanation {
    /// Exact metric keys consumed by this component.
    pub metrics: Vec<String>,
    pub metric_versions: Vec<u32>,
    pub normalized_value: f64,
    pub weight: f64,
    pub contribution: f64,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreExplanation {
    pub profile: ProfileId,
    pub profile_version: u32,
    pub contract_version: u32,
    pub score: f64,
    pub confidence: f64,
    pub quality_tier: &'static str,
    pub components: Vec<ComponentExplanation>,
    /// Optional evidence is reported separately and never fabricated as a
    /// scoring component when the corresponding analyzer did not run.
    pub raw_recoverability: Option<f64>,
    pub corruption: Option<String>,
}

/// Metric versions are included in deterministic order so analyzer upgrades
/// invalidate old cache keys even when the numeric values happen to coincide.
pub fn cache_key(profile: ProfileId, metrics: &[Metric]) -> String {
    let mut entries: Vec<_> = metrics.iter().map(|m| (m.name.as_str(), m.version)).collect();
    entries.sort_unstable();
    let versions = entries.into_iter().map(|(n, v)| format!("{n}@{v}")).collect::<Vec<_>>().join(",");
    format!("score-v{}:{}-v{}:[{}]", SCORING_CONTRACT_VERSION, profile.as_str(), profile.version(), versions)
}

/// Computes a score from available measurements; absent or invalid metrics
/// are omitted and remaining weights are renormalized.
pub fn score(profile: ProfileId, metrics: &[Metric], raw_recoverability: Option<f64>) -> ScoreExplanation {
    let mut by_name = BTreeMap::new();
    for metric in metrics {
        if metric.value.is_finite() && metric.confidence.is_finite() {
            by_name.insert(metric.name.as_str(), metric);
        }
    }

    if let Some(corruption) = by_name.get("corruption_status") {
        if corruption.value != 1.0 {
            return ScoreExplanation {
                profile, profile_version: profile.version(), contract_version: SCORING_CONTRACT_VERSION,
                score: 0.0, confidence: corruption.confidence.clamp(0.0, 1.0),
                quality_tier: "corrupted", components: vec![],
                raw_recoverability: valid_recoverability(raw_recoverability),
                corruption: Some("File is corrupted or unreadable".into()),
            };
        }
    }

    let weights = profile.weights();
    let names = ["sharpness.global_score", "exposure.score", "noise.estimated_level"];
    let aliases = ["global_sharpness", "exposure_score", "estimated_noise_level"];
    let mut components = Vec::new();
    for (name, legacy, index) in [(names[0], aliases[0], 0), (names[1], aliases[1], 1), (names[2], aliases[2], 3)] {
        let Some(metric) = by_name.get(name).or_else(|| by_name.get(legacy)) else { continue };
        let Some(value) = normalize(&metric.name, metric.value) else { continue };
        if weights[index] <= 0.0 { continue; }
        components.push(ComponentExplanation {
            metrics: vec![metric.name.clone()], metric_versions: vec![metric.version], normalized_value: round(value),
            weight: weights[index], contribution: round(value * weights[index]),
            confidence: metric.confidence.clamp(0.0, 1.0),
        });
    }
    // Use the exact metric keys emitted by the legacy clipping analyzer.
    if weights[2] > 0.0 {
        let hi = by_name.get("clipping.highlight_ratio").or_else(|| by_name.get("highlight_clipping_pct"));
        let center = by_name.get("clipping.center_highlight_ratio").or_else(|| by_name.get("center_highlight_clipping_pct")).or(hi);
        let shadows = by_name.get("clipping.shadow_ratio").or_else(|| by_name.get("shadow_clipping_pct"));
        if hi.is_some() || center.is_some() || shadows.is_some() {
            let hi_value = hi.map(|m| m.value).unwrap_or(0.0);
            let center_value = center.map(|m| m.value).unwrap_or(hi_value);
            let shadow_value = shadows.map(|m| m.value).unwrap_or(0.0);
            let penalty = if profile == ProfileId::Concert {
                center_value * 3.0 + hi_value * 0.5 + shadow_value
            } else { hi_value * 2.0 + shadow_value };
            let used: Vec<_> = [hi, center, shadows].into_iter().flatten().collect();
            let value = (1.0 - penalty).clamp(0.0, 1.0);
            components.push(ComponentExplanation {
                metrics: used.iter().map(|m| m.name.clone()).collect(),
                metric_versions: used.iter().map(|m| m.version).collect(),
                normalized_value: round(value), weight: weights[2], contribution: round(value * weights[2]),
                confidence: used.iter().map(|m| m.confidence.clamp(0.0, 1.0)).sum::<f64>() / used.len() as f64,
            });
        }
    }
    let active_weight: f64 = components.iter().map(|c| c.weight).sum();
    let score = if active_weight == 0.0 { 0.0 } else {
        components.iter().map(|c| c.contribution).sum::<f64>() / active_weight
    }.clamp(0.0, 1.0);
    let confidence = if components.is_empty() { 0.0 } else {
        components.iter().map(|c| c.confidence).sum::<f64>() / components.len() as f64
    };
    let tier = if score >= 0.80 { "excellent" } else if score >= 0.60 { "good" } else if score >= 0.40 { "fair" } else { "poor" };
    ScoreExplanation {
        profile, profile_version: profile.version(), contract_version: SCORING_CONTRACT_VERSION,
        score: round(score), confidence: round(confidence), quality_tier: tier, components,
        raw_recoverability: valid_recoverability(raw_recoverability), corruption: None,
    }
}

fn normalize(name: &str, value: f64) -> Option<f64> {
    let normalized = match name {
        "sharpness.global_score" | "global_sharpness" | "exposure.score" | "exposure_score" => value,
        // Rust clipping ratios are [0, 1]; the legacy percentages require /100.
        "clipping.highlight_ratio" | "clipping.shadow_ratio" | "clipping.center_highlight_ratio" => value,
        "highlight_clipping_pct" | "shadow_clipping_pct" | "center_highlight_clipping_pct" => value / 100.0,
        "noise.estimated_level" | "estimated_noise_level" => 1.0 - value,
        "corruption_status" => return None,
        _ => return None,
    };
    if normalized.is_finite() { Some(normalized.clamp(0.0, 1.0)) } else { None }
}

fn valid_recoverability(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite()).map(|v| round(v.clamp(0.0, 1.0)))
}
fn round(v: f64) -> f64 { (v * 10_000.0).round() / 10_000.0 }

#[cfg(test)]
mod tests {
    use super::*;
    fn metric(name: &str, value: f64) -> Metric { Metric::new(name, 1, value, 0.9) }
    fn full(sharpness: f64, exposure: f64, clipping: f64, noise: f64) -> Vec<Metric> {
        vec![metric("sharpness.global_score", sharpness), metric("exposure.score", exposure),
             metric("clipping.highlight_ratio", clipping), metric("clipping.center_highlight_ratio", clipping),
             metric("clipping.shadow_ratio", 0.0), metric("noise.estimated_level", noise)]
    }

    #[test]
    fn golden_profile_orderings_and_concert_light_tolerance() {
        let clean = full(0.9, 0.8, 0.0, 0.1);
        let soft = full(0.4, 0.8, 0.0, 0.1);
        let mut stage_clipped = full(0.9, 0.8, 0.8, 0.1);
        stage_clipped.iter_mut().find(|m| m.name == "clipping.center_highlight_ratio").unwrap().value = 0.0;
        assert!(score(ProfileId::Fast, &clean, None).score > score(ProfileId::Fast, &soft, None).score);
        assert!(score(ProfileId::Technical, &clean, None).score > score(ProfileId::Technical, &stage_clipped, None).score);
        assert!(score(ProfileId::Concert, &stage_clipped, None).score > score(ProfileId::Technical, &stage_clipped, None).score);
        assert_eq!(score(ProfileId::Concert, &clean, None).profile_version, 1);
    }

    #[test]
    fn explanation_only_names_available_metrics_and_reports_raw_separately() {
        let metrics = vec![metric("sharpness.global_score", 0.8), metric("exposure.score", 0.7), metric("unavailable_horizon", 0.2)];
        let result = score(ProfileId::Technical, &metrics, Some(0.75));
        assert_eq!(result.components.iter().flat_map(|c| c.metrics.iter().map(String::as_str)).collect::<Vec<_>>(), vec!["sharpness.global_score", "exposure.score"]);
        assert_eq!(result.raw_recoverability, Some(0.75));
        assert!((result.score - ((0.8*0.4 + 0.7*0.25)/0.65)).abs() < 0.0001);
    }

    #[test]
    fn cache_key_tracks_profile_and_metric_versions_independent_of_input_order() {
        let a = vec![Metric::new("sharpness.global_score", 2, 0.8, 1.0), metric("exposure.score", 0.6)];
        let b = vec![a[1].clone(), a[0].clone()];
        assert_eq!(cache_key(ProfileId::Fast, &a), cache_key(ProfileId::Fast, &b));
        assert_ne!(cache_key(ProfileId::Fast, &a), cache_key(ProfileId::Technical, &a));
        let mut upgraded = a.clone(); upgraded[0].version = 3;
        assert_ne!(cache_key(ProfileId::Fast, &a), cache_key(ProfileId::Fast, &upgraded));
    }

    #[test]
    fn corrupted_image_is_hard_failure() {
        let result = score(ProfileId::Technical, &[metric("corruption_status", 0.0)], Some(0.4));
        assert_eq!(result.score, 0.0);
        assert_eq!(result.quality_tier, "corrupted");
        assert!(result.components.is_empty());
    }
}
