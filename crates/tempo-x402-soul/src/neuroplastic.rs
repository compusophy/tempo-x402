//! Neuroplastic memory: salience scoring, tiered memory with decay, and prediction error.
//!
//! Three neuroscience-inspired systems that create a learning loop:
//! 1. **Salience** — not all thoughts matter equally. Novelty, prediction error, and reward
//!    determine how important a thought is.
//! 2. **Tiered memory** — sensory (fast decay), working (moderate), long-term (near-permanent).
//!    High-salience sensory memories get promoted to working memory.
//! 3. **Prediction error** — the soul predicts next-cycle metrics and learns from surprise
//!    when reality diverges from expectation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::memory::ThoughtType;
use crate::observer::NodeSnapshot;

/// Memory tier with characteristic decay rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Fast-decaying: ~2 cycles effective lifespan. Raw observations.
    Sensory,
    /// Moderate decay: ~90 cycles. Active reasoning and decisions.
    Working,
    /// Near-permanent: ~900 cycles. Consolidated insights, high-salience decisions.
    LongTerm,
}

impl MemoryTier {
    /// Decay multiplier applied to strength each cycle.
    pub fn decay_rate(&self) -> f64 {
        match self {
            Self::Sensory => 0.3,
            Self::Working => 0.95,
            Self::LongTerm => 0.995,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sensory => "sensory",
            Self::Working => "working",
            Self::LongTerm => "long_term",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "sensory" => Some(Self::Sensory),
            "working" => Some(Self::Working),
            "long_term" => Some(Self::LongTerm),
            _ => None,
        }
    }
}

/// Per-endpoint reward attribution: what changed and where.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardBreakdown {
    /// Total reward signal (0.0..=0.8).
    pub total_reward: f64,
    /// Endpoints that appeared since last snapshot.
    pub new_endpoints: Vec<String>,
    /// Endpoints that gained new payments since last snapshot.
    pub growing_endpoints: Vec<String>,
    /// Endpoints with zero payments (have never earned).
    pub stagnant_endpoints: Vec<String>,
}

/// Compute per-endpoint reward signal by diffing two snapshots.
///
/// - New endpoint → +0.3 reward
/// - Endpoint with new payments → +0.5 reward (split across all growing)
/// - Endpoint with zero payments → stagnant (no reward, but tracked)
/// - Total capped at 0.8
pub fn compute_reward_signal(
    snapshot: &NodeSnapshot,
    prev_snapshot: Option<&NodeSnapshot>,
) -> RewardBreakdown {
    let mut breakdown = RewardBreakdown {
        total_reward: 0.0,
        new_endpoints: vec![],
        growing_endpoints: vec![],
        stagnant_endpoints: vec![],
    };

    let Some(prev) = prev_snapshot else {
        // No previous snapshot — classify current endpoints but no reward
        for ep in &snapshot.endpoints {
            if ep.payment_count == 0 {
                breakdown.stagnant_endpoints.push(ep.slug.clone());
            }
        }
        return breakdown;
    };

    // Build lookup of previous endpoints by slug
    let prev_map: std::collections::HashMap<&str, &crate::observer::EndpointSummary> = prev
        .endpoints
        .iter()
        .map(|ep| (ep.slug.as_str(), ep))
        .collect();

    for ep in &snapshot.endpoints {
        match prev_map.get(ep.slug.as_str()) {
            None => {
                // New endpoint
                breakdown.new_endpoints.push(ep.slug.clone());
            }
            Some(prev_ep) => {
                if ep.payment_count > prev_ep.payment_count {
                    breakdown.growing_endpoints.push(ep.slug.clone());
                } else if ep.payment_count == 0 {
                    breakdown.stagnant_endpoints.push(ep.slug.clone());
                }
            }
        }
    }

    // Score: new endpoints contribute 0.3 each (capped), growing contribute 0.5 total
    let new_reward = (breakdown.new_endpoints.len() as f64 * 0.3).min(0.6);
    let growing_reward = if breakdown.growing_endpoints.is_empty() {
        0.0
    } else {
        0.5
    };

    breakdown.total_reward = (new_reward + growing_reward).min(0.8);
    breakdown
}

/// Breakdown of salience factors for a thought.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceFactors {
    /// How novel is this content? (0.0 = seen many times, 1.0 = never seen)
    pub novelty: f64,
    /// How much did reality diverge from prediction? (0.0 = perfect, 1.0 = total surprise)
    pub prediction_error: f64,
    /// Was there a positive change in payments/revenue? (0.0 = no, up to 0.8)
    pub reward_signal: f64,
    /// Constant small boost for being recent (0.1).
    pub recency_boost: f64,
    /// How often has this pattern been seen before? Logarithmic reinforcement.
    pub reinforcement: f64,
}

/// A prediction about the next cycle's metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prediction {
    pub expected_payments: u64,
    pub expected_revenue: f64,
    pub expected_endpoint_count: u32,
    pub expected_children_count: u32,
    /// How confident the prediction is (0.0 = no basis, 1.0 = strong trend).
    pub confidence: f64,
    /// Human-readable basis for the prediction.
    pub basis: String,
}

/// Compute salience score and factor breakdown for a thought.
///
/// Weights: novelty 30%, prediction_error 25%, reward_signal 25%, recency 10%, reinforcement 10%.
pub fn compute_salience(
    _thought_type: &ThoughtType,
    content: &str,
    snapshot: &NodeSnapshot,
    prev_snapshot: Option<&NodeSnapshot>,
    prediction_error: f64,
    pattern_counts: &HashMap<String, u64>,
) -> (f64, SalienceFactors) {
    let fp = content_fingerprint(content);
    let count = pattern_counts.get(&fp).copied().unwrap_or(0);

    // Novelty: never seen = 1.0, otherwise diminishing
    let novelty = if count == 0 {
        1.0
    } else {
        (1.0 / (count as f64 + 1.0)).min(0.5)
    };

    // Prediction error (already computed, pass through)
    let pred_error = prediction_error.clamp(0.0, 1.0);

    // Reward signal: per-endpoint attribution
    let reward_breakdown = compute_reward_signal(snapshot, prev_snapshot);
    let reward = reward_breakdown.total_reward;

    // Recency: constant small boost
    let recency = 0.1;

    // Reinforcement: logarithmic growth for repeated patterns
    let reinforcement = if count > 1 {
        (0.1 * (count as f64).ln()).min(0.5)
    } else {
        0.0
    };

    let salience =
        (novelty * 0.3 + pred_error * 0.25 + reward * 0.25 + recency * 0.1 + reinforcement * 0.1)
            .clamp(0.0, 1.0);

    let factors = SalienceFactors {
        novelty,
        prediction_error: pred_error,
        reward_signal: reward,
        recency_boost: recency,
        reinforcement,
    };

    (salience, factors)
}

/// Determine the initial memory tier for a thought based on type and salience.
pub fn initial_tier(thought_type: &ThoughtType, salience: f64) -> MemoryTier {
    match thought_type {
        ThoughtType::Observation => MemoryTier::Sensory,
        ThoughtType::Reasoning | ThoughtType::Decision | ThoughtType::Reflection => {
            if salience > 0.7 {
                MemoryTier::LongTerm
            } else {
                MemoryTier::Working
            }
        }
        ThoughtType::MemoryConsolidation => MemoryTier::LongTerm,
        ThoughtType::Prediction => MemoryTier::Working,
        // Everything else (tool executions, chat, mutations) → working
        _ => MemoryTier::Working,
    }
}

/// Generate a prediction for the next cycle based on current and previous snapshots.
/// Uses simple linear extrapolation — no LLM needed.
pub fn generate_prediction(current: &NodeSnapshot, prev: Option<&NodeSnapshot>) -> Prediction {
    match prev {
        Some(prev) => {
            // Extrapolate from delta
            let payment_delta = current.total_payments as i64 - prev.total_payments as i64;
            let expected_payments = (current.total_payments as i64 + payment_delta).max(0) as u64;

            let cur_rev: f64 = current.total_revenue.parse().unwrap_or(0.0);
            let prev_rev: f64 = prev.total_revenue.parse().unwrap_or(0.0);
            let rev_delta = cur_rev - prev_rev;
            let expected_revenue = (cur_rev + rev_delta).max(0.0);

            let ep_delta = current.endpoint_count as i32 - prev.endpoint_count as i32;
            let expected_endpoint_count = (current.endpoint_count as i32 + ep_delta).max(0) as u32;

            let ch_delta = current.children_count as i32 - prev.children_count as i32;
            let expected_children_count = (current.children_count as i32 + ch_delta).max(0) as u32;

            // Confidence based on whether we have a meaningful delta
            let has_change = payment_delta != 0
                || rev_delta.abs() > f64::EPSILON
                || ep_delta != 0
                || ch_delta != 0;
            let confidence = if has_change { 0.6 } else { 0.3 };

            Prediction {
                expected_payments,
                expected_revenue,
                expected_endpoint_count,
                expected_children_count,
                confidence,
                basis: format!(
                    "Linear extrapolation: payments delta {payment_delta}, revenue delta {rev_delta:.2}, endpoints delta {ep_delta}, children delta {ch_delta}"
                ),
            }
        }
        None => {
            // No previous snapshot — predict same as current
            Prediction {
                expected_payments: current.total_payments,
                expected_revenue: current.total_revenue.parse().unwrap_or(0.0),
                expected_endpoint_count: current.endpoint_count,
                expected_children_count: current.children_count,
                confidence: 0.1,
                basis: "No previous snapshot — baseline prediction".to_string(),
            }
        }
    }
}

/// Compute prediction error: normalized diff between prediction and actual snapshot.
/// Returns a value in [0.0, 1.0].
pub fn compute_prediction_error(prediction: &Prediction, actual: &NodeSnapshot) -> f64 {
    let mut errors = Vec::new();

    // Payments: relative error
    let pred_pay = prediction.expected_payments as f64;
    let act_pay = actual.total_payments as f64;
    if pred_pay > 0.0 || act_pay > 0.0 {
        let max_val = pred_pay.max(act_pay).max(1.0);
        errors.push(((pred_pay - act_pay).abs() / max_val).min(1.0));
    }

    // Revenue: relative error
    let act_rev: f64 = actual.total_revenue.parse().unwrap_or(0.0);
    if prediction.expected_revenue > 0.0 || act_rev > 0.0 {
        let max_val = prediction.expected_revenue.max(act_rev).max(1.0);
        errors.push(((prediction.expected_revenue - act_rev).abs() / max_val).min(1.0));
    }

    // Endpoint count: relative error
    let pred_ep = prediction.expected_endpoint_count as f64;
    let act_ep = actual.endpoint_count as f64;
    if pred_ep > 0.0 || act_ep > 0.0 {
        let max_val = pred_ep.max(act_ep).max(1.0);
        errors.push(((pred_ep - act_ep).abs() / max_val).min(1.0));
    }

    // Children count: relative error
    let pred_ch = prediction.expected_children_count as f64;
    let act_ch = actual.children_count as f64;
    if pred_ch > 0.0 || act_ch > 0.0 {
        let max_val = pred_ch.max(act_ch).max(1.0);
        errors.push(((pred_ch - act_ch).abs() / max_val).min(1.0));
    }

    if errors.is_empty() {
        0.0
    } else {
        let sum: f64 = errors.iter().sum();
        (sum / errors.len() as f64).clamp(0.0, 1.0)
    }
}

/// Content fingerprint: first 60 chars lowercased and trimmed, for pattern matching.
pub fn content_fingerprint(content: &str) -> String {
    content.trim().to_lowercase().chars().take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_snapshot(payments: u64, revenue: &str, endpoints: u32, children: u32) -> NodeSnapshot {
        NodeSnapshot {
            uptime_secs: 3600,
            endpoint_count: endpoints,
            total_revenue: revenue.to_string(),
            total_payments: payments,
            children_count: children,
            wallet_address: None,
            instance_id: None,
            generation: 0,
            endpoints: vec![],
        }
    }

    #[test]
    fn test_content_fingerprint() {
        let fp = content_fingerprint(
            "  Hello World, this is a test of the fingerprinting system that should truncate  ",
        );
        assert!(fp.len() <= 60);
        assert!(fp.starts_with("hello world"));
    }

    #[test]
    fn test_compute_salience_novel() {
        let snap = test_snapshot(10, "100.0", 3, 0);
        let pattern_counts = HashMap::new();
        let (salience, factors) = compute_salience(
            &ThoughtType::Observation,
            "brand new observation",
            &snap,
            None,
            0.0,
            &pattern_counts,
        );
        assert!(salience > 0.0);
        assert!((factors.novelty - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_salience_repeated() {
        let snap = test_snapshot(10, "100.0", 3, 0);
        let mut pattern_counts = HashMap::new();
        let fp = content_fingerprint("same observation again");
        pattern_counts.insert(fp, 5);
        let (_, factors) = compute_salience(
            &ThoughtType::Observation,
            "same observation again",
            &snap,
            None,
            0.0,
            &pattern_counts,
        );
        assert!(factors.novelty < 0.5);
        assert!(factors.reinforcement > 0.0);
    }

    #[test]
    fn test_compute_salience_with_reward() {
        let prev = NodeSnapshot {
            endpoints: vec![test_endpoint("weather", 10, "100.0")],
            ..test_snapshot(10, "100.0", 1, 0)
        };
        let snap = NodeSnapshot {
            endpoints: vec![test_endpoint("weather", 15, "150.0")],
            ..test_snapshot(15, "150.0", 1, 0)
        };
        let (salience, factors) = compute_salience(
            &ThoughtType::Observation,
            "new observation",
            &snap,
            Some(&prev),
            0.0,
            &HashMap::new(),
        );
        assert!(factors.reward_signal > 0.0);
        assert!(salience > 0.3); // novelty + reward
    }

    #[test]
    fn test_initial_tier() {
        assert_eq!(
            initial_tier(&ThoughtType::Observation, 0.5),
            MemoryTier::Sensory
        );
        assert_eq!(
            initial_tier(&ThoughtType::Reasoning, 0.3),
            MemoryTier::Working
        );
        assert_eq!(
            initial_tier(&ThoughtType::Decision, 0.8),
            MemoryTier::LongTerm
        );
        assert_eq!(
            initial_tier(&ThoughtType::MemoryConsolidation, 0.1),
            MemoryTier::LongTerm
        );
    }

    #[test]
    fn test_generate_prediction_no_prev() {
        let snap = test_snapshot(10, "100.0", 3, 0);
        let pred = generate_prediction(&snap, None);
        assert_eq!(pred.expected_payments, 10);
        assert!((pred.expected_revenue - 100.0).abs() < f64::EPSILON);
        assert!(pred.confidence < 0.2);
    }

    #[test]
    fn test_generate_prediction_with_delta() {
        let prev = test_snapshot(10, "100.0", 3, 0);
        let curr = test_snapshot(15, "150.0", 4, 0);
        let pred = generate_prediction(&curr, Some(&prev));
        assert_eq!(pred.expected_payments, 20); // 15 + (15-10)
        assert!((pred.expected_revenue - 200.0).abs() < f64::EPSILON);
        assert_eq!(pred.expected_endpoint_count, 5);
        assert!(pred.confidence > 0.3);
    }

    #[test]
    fn test_compute_prediction_error_perfect() {
        let pred = Prediction {
            expected_payments: 10,
            expected_revenue: 100.0,
            expected_endpoint_count: 3,
            expected_children_count: 0,
            confidence: 0.5,
            basis: "test".to_string(),
        };
        let snap = test_snapshot(10, "100.0", 3, 0);
        let error = compute_prediction_error(&pred, &snap);
        assert!(error < f64::EPSILON);
    }

    #[test]
    fn test_compute_prediction_error_divergent() {
        let pred = Prediction {
            expected_payments: 10,
            expected_revenue: 100.0,
            expected_endpoint_count: 3,
            expected_children_count: 0,
            confidence: 0.5,
            basis: "test".to_string(),
        };
        let snap = test_snapshot(20, "200.0", 6, 2);
        let error = compute_prediction_error(&pred, &snap);
        assert!(error > 0.3);
    }

    #[test]
    fn test_memory_tier_decay_rates() {
        assert!((MemoryTier::Sensory.decay_rate() - 0.3).abs() < f64::EPSILON);
        assert!((MemoryTier::Working.decay_rate() - 0.95).abs() < f64::EPSILON);
        assert!((MemoryTier::LongTerm.decay_rate() - 0.995).abs() < f64::EPSILON);
    }

    fn test_endpoint(slug: &str, payments: i64, revenue: &str) -> crate::observer::EndpointSummary {
        crate::observer::EndpointSummary {
            slug: slug.to_string(),
            price: "$0.01".to_string(),
            description: None,
            request_count: payments * 2,
            payment_count: payments,
            revenue: revenue.to_string(),
        }
    }

    #[test]
    fn test_compute_reward_signal_no_prev() {
        let snap = NodeSnapshot {
            endpoints: vec![
                test_endpoint("weather", 0, "0"),
                test_endpoint("translate", 5, "0.05"),
            ],
            ..test_snapshot(5, "0.05", 2, 0)
        };
        let breakdown = compute_reward_signal(&snap, None);
        assert!((breakdown.total_reward - 0.0).abs() < f64::EPSILON);
        assert_eq!(breakdown.stagnant_endpoints, vec!["weather"]);
    }

    #[test]
    fn test_compute_reward_signal_new_endpoint() {
        let prev = NodeSnapshot {
            endpoints: vec![test_endpoint("weather", 5, "0.05")],
            ..test_snapshot(5, "0.05", 1, 0)
        };
        let snap = NodeSnapshot {
            endpoints: vec![
                test_endpoint("weather", 5, "0.05"),
                test_endpoint("translate", 0, "0"),
            ],
            ..test_snapshot(5, "0.05", 2, 0)
        };
        let breakdown = compute_reward_signal(&snap, Some(&prev));
        assert_eq!(breakdown.new_endpoints, vec!["translate"]);
        assert!((breakdown.total_reward - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_reward_signal_growing_endpoint() {
        let prev = NodeSnapshot {
            endpoints: vec![test_endpoint("weather", 5, "0.05")],
            ..test_snapshot(5, "0.05", 1, 0)
        };
        let snap = NodeSnapshot {
            endpoints: vec![test_endpoint("weather", 10, "0.10")],
            ..test_snapshot(10, "0.10", 1, 0)
        };
        let breakdown = compute_reward_signal(&snap, Some(&prev));
        assert_eq!(breakdown.growing_endpoints, vec!["weather"]);
        assert!((breakdown.total_reward - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_compute_reward_signal_capped() {
        let prev = NodeSnapshot {
            endpoints: vec![],
            ..test_snapshot(0, "0", 0, 0)
        };
        // 3 new endpoints (3 * 0.3 = 0.9 but capped at 0.6) + no growing = 0.6
        let snap = NodeSnapshot {
            endpoints: vec![
                test_endpoint("a", 0, "0"),
                test_endpoint("b", 0, "0"),
                test_endpoint("c", 0, "0"),
            ],
            ..test_snapshot(0, "0", 3, 0)
        };
        let breakdown = compute_reward_signal(&snap, Some(&prev));
        assert_eq!(breakdown.new_endpoints.len(), 3);
        assert!(breakdown.total_reward <= 0.8);
    }
}
