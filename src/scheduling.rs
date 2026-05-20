//! Per-request scheduling profile resolution and weight calculation.
//!
//! The router computes per-share signals (quota_health, stability, headroom,
//! owner_penalty) once per `/v1/market/shares` sync. The market sorts by a
//! weighted base score over those signals. The *weights* depend on what the
//! caller is optimizing for — latency, cost, freshness, or diversification —
//! which is what `SchedulingProfile` selects.
//!
//! Profile resolution today reads `api_keys.scope_json["schedulingProfile"]`
//! (camelCase or snake_case). A future iteration will overlay user-level and
//! per-model preferences; right now only api-key scope is consulted.

use serde::{Deserialize, Serialize};

/// Catalog of supported scheduling profiles. Each variant maps to a tuple of
/// non-negative weights consumed by `select_share_candidates` in `proxy.rs`.
///
/// The numeric weights are *not* the user-facing API — callers refer to a
/// profile by its kebab-case string. Adding a profile means adding a variant
/// here plus the matching weight tuple in [`Self::weights`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchedulingProfile {
    /// Default — even mix across all four signals. Picked when no profile is
    /// configured or the configured value can't be parsed.
    Balanced,
    /// Squeeze unit cost: lean on cheaper shares (price_bias > 1.0) and
    /// quota_health (so quota-rich, cheap shares win).
    PriceFirst,
    /// Lean heavily on stability and headroom — for sessions where a stall
    /// hurts more than a slightly higher cost.
    StabilityFirst,
    /// Prefer shares whose quota windows are about to reset with low
    /// utilization — useful for batch jobs that can drain weekly quota.
    FreshQuota,
    /// Spread load — bias toward freshly-onboarded shares (`share_created_at`
    /// recency) and away from saturated ones.
    Diversify,
    /// Premium clients — quality-first, ignore price entirely. Pushes
    /// stability + headroom hard so premium traffic gets the healthiest shares.
    Premium,
    /// Budget-aware — like `PriceFirst` but also respects monthly spend cap
    /// and applies a stronger owner_penalty multiplier.
    BudgetAware,
}

impl Default for SchedulingProfile {
    fn default() -> Self {
        Self::Balanced
    }
}

/// Weights applied to the SQL base_score. Each component multiplies a value
/// already in `[0, 1]` (or close to it), so weights sum to roughly 1.0 to
/// keep the score in `[0, 1]` for easy reasoning. `price_bias` is a separate
/// multiplier — it scales the entire base_score before owner_penalty.
#[derive(Debug, Clone, Copy)]
pub struct ProfileWeights {
    pub stability: f64,
    pub quota_health: f64,
    pub headroom: f64,
    pub freshness: f64,
    /// Multiplier on the final base_score for cheaper shares. 1.0 = neutral.
    /// Today applied via `(1.0 + price_bias * (1.0 - sale_percent/100))` in
    /// the SQL expression, so a price_bias of 0 leaves cheap and expensive
    /// shares ranked equally on score alone.
    pub price_bias: f64,
}

impl SchedulingProfile {
    pub const fn weights(self) -> ProfileWeights {
        match self {
            Self::Balanced => ProfileWeights {
                stability: 0.35,
                quota_health: 0.30,
                headroom: 0.25,
                freshness: 0.10,
                price_bias: 0.0,
            },
            Self::PriceFirst => ProfileWeights {
                stability: 0.25,
                quota_health: 0.40,
                headroom: 0.25,
                freshness: 0.10,
                price_bias: 0.50,
            },
            Self::StabilityFirst => ProfileWeights {
                stability: 0.55,
                quota_health: 0.20,
                headroom: 0.20,
                freshness: 0.05,
                price_bias: 0.0,
            },
            Self::FreshQuota => ProfileWeights {
                stability: 0.20,
                quota_health: 0.55,
                headroom: 0.15,
                freshness: 0.10,
                price_bias: 0.0,
            },
            Self::Diversify => ProfileWeights {
                stability: 0.25,
                quota_health: 0.20,
                headroom: 0.30,
                freshness: 0.25,
                price_bias: 0.0,
            },
            Self::Premium => ProfileWeights {
                stability: 0.45,
                quota_health: 0.20,
                headroom: 0.30,
                freshness: 0.05,
                price_bias: -0.10, // bias *against* cheap shares for premium
            },
            Self::BudgetAware => ProfileWeights {
                stability: 0.25,
                quota_health: 0.35,
                headroom: 0.25,
                freshness: 0.15,
                price_bias: 0.40,
            },
        }
    }

    /// Parse a profile from its kebab-case wire string. Returns `None` for
    /// unknown values; callers fall back to [`Self::default`].
    pub fn from_kebab(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "balanced" => Some(Self::Balanced),
            "price-first" | "price_first" => Some(Self::PriceFirst),
            "stability-first" | "stability_first" => Some(Self::StabilityFirst),
            "fresh-quota" | "fresh_quota" => Some(Self::FreshQuota),
            "diversify" => Some(Self::Diversify),
            "premium" => Some(Self::Premium),
            "budget-aware" | "budget_aware" => Some(Self::BudgetAware),
            _ => None,
        }
    }
}

/// Resolve the active profile from an api_key's optional scope_json.
///
/// Accepts both `schedulingProfile` and `scheduling_profile` keys. Unknown
/// values and missing scopes both collapse to `Balanced` — the caller never
/// has to special-case a missing profile.
pub fn resolve_profile(scope_json: Option<&serde_json::Value>) -> SchedulingProfile {
    let Some(scope) = scope_json else {
        return SchedulingProfile::default();
    };
    let raw = scope
        .get("schedulingProfile")
        .or_else(|| scope.get("scheduling_profile"))
        .and_then(|v| v.as_str());
    raw.and_then(SchedulingProfile::from_kebab)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_scope_collapses_to_balanced() {
        assert_eq!(resolve_profile(None), SchedulingProfile::Balanced);
    }

    #[test]
    fn unknown_profile_collapses_to_balanced() {
        let scope = json!({ "schedulingProfile": "ludicrous-speed" });
        assert_eq!(resolve_profile(Some(&scope)), SchedulingProfile::Balanced);
    }

    #[test]
    fn parses_camel_case_key() {
        let scope = json!({ "schedulingProfile": "stability-first" });
        assert_eq!(
            resolve_profile(Some(&scope)),
            SchedulingProfile::StabilityFirst
        );
    }

    #[test]
    fn parses_snake_case_key_and_value() {
        let scope = json!({ "scheduling_profile": "fresh_quota" });
        assert_eq!(resolve_profile(Some(&scope)), SchedulingProfile::FreshQuota);
    }

    #[test]
    fn camel_takes_precedence_over_snake() {
        // Both present, kebab/camel wins because it's checked first.
        let scope = json!({
            "schedulingProfile": "premium",
            "scheduling_profile": "balanced",
        });
        assert_eq!(resolve_profile(Some(&scope)), SchedulingProfile::Premium);
    }

    #[test]
    fn all_profiles_have_nonneg_normalized_signal_weights() {
        let all = [
            SchedulingProfile::Balanced,
            SchedulingProfile::PriceFirst,
            SchedulingProfile::StabilityFirst,
            SchedulingProfile::FreshQuota,
            SchedulingProfile::Diversify,
            SchedulingProfile::Premium,
            SchedulingProfile::BudgetAware,
        ];
        for p in all {
            let w = p.weights();
            assert!(w.stability >= 0.0, "{p:?} stability");
            assert!(w.quota_health >= 0.0, "{p:?} quota_health");
            assert!(w.headroom >= 0.0, "{p:?} headroom");
            assert!(w.freshness >= 0.0, "{p:?} freshness");
            let sum = w.stability + w.quota_health + w.headroom + w.freshness;
            // Weights should be in a sane band (0.9..=1.1) so the score
            // stays interpretable. Drift outside that band probably means a
            // typo when adding a profile.
            assert!(
                (sum - 1.0).abs() <= 0.1,
                "{p:?} weights sum {sum} drifted outside [0.9, 1.1]"
            );
        }
    }

    #[test]
    fn premium_has_negative_price_bias() {
        // Premium should *avoid* the cheapest shares so the highest-quality
        // upstream wins. Regression guard: a future refactor that drops the
        // negative sign would silently turn premium into budget-aware.
        assert!(SchedulingProfile::Premium.weights().price_bias < 0.0);
    }

    #[test]
    fn price_first_and_budget_aware_have_positive_price_bias() {
        assert!(SchedulingProfile::PriceFirst.weights().price_bias > 0.0);
        assert!(SchedulingProfile::BudgetAware.weights().price_bias > 0.0);
    }
}
