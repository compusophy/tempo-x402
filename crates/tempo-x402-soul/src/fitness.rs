//! Fitness scoring for evolutionary selection pressure.
//!
//! Each agent computes a multi-dimensional fitness score every cycle from
//! observable metrics. Fitness drives:
//! - **Clone selection**: fitter agents are preferred for cloning
//! - **Evolution gradient**: trend over time shows if the collective is getting smarter
//! - **Peer comparison**: agents see each other's fitness and learn from the best
//!
//! The fitness function is the selection pressure that makes this actual evolution
//! rather than random mutation.

use serde::{Deserialize, Serialize};

use crate::db::SoulDatabase;
use crate::observer::NodeSnapshot;

/// Multi-dimensional fitness score for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitnessScore {
    /// Overall fitness [0.0, 1.0] — weighted combination of components.
    pub total: f64,
    /// Economic fitness: are your endpoints earning payments?
    /// (payments / max(endpoints, 1)), scaled by revenue growth.
    pub economic: f64,
    /// Economic trend.
    pub economic_trend: f64,
    /// Execution fitness: do your plans succeed?
    /// (completed_plans / max(total_plans, 1)), penalized by replan rate.
    pub execution: f64,
    /// Execution trend.
    pub execution_trend: f64,
    /// Evolution fitness: are you actually changing your code?
    /// Commits per 100 cycles, scaled by cargo-check pass rate.
    pub evolution: f64,
    /// Evolution trend.
    pub evolution_trend: f64,
    /// Coordination fitness: can you work with peers?
    /// Successful peer calls / max(attempted, 1).
    pub coordination: f64,
    /// Introspection fitness: do your beliefs match reality?
    /// Fraction of auto-beliefs that are accurate.
    pub introspection: f64,
    /// Trend: derivative of total fitness over last N measurements.
    /// Positive = improving, negative = declining. THIS is the gradient.
    pub trend: f64,
    /// Timestamp of this measurement.
    pub measured_at: i64,
    /// Which generation this agent is.
    pub generation: u32,
}

/// Component weights for the fitness function.
const W_ECONOMIC: f64 = 0.25;
const W_EXECUTION: f64 = 0.20;
const W_EVOLUTION: f64 = 0.25;
const W_COORDINATION: f64 = 0.15;
const W_INTROSPECTION: f64 = 0.15;

/// How many historical scores to keep for trend calculation.
const HISTORY_SIZE: usize = 50;

impl FitnessScore {
    /// Compute fitness from current state.
    pub fn compute(snapshot: &NodeSnapshot, db: &SoulDatabase) -> Self {
        let now = chrono::Utc::now().timestamp();

        let economic = compute_economic(snapshot);
        let execution = compute_execution(db);
        let evolution = compute_evolution(db);
        let coordination = compute_coordination(db);
        let introspection = compute_introspection(snapshot, db);

        let total = W_ECONOMIC * economic
            + W_EXECUTION * execution
            + W_EVOLUTION * evolution
            + W_COORDINATION * coordination
            + W_INTROSPECTION * introspection;

        // Compute trends from historical scores
        let history = load_history(db);
        let trend = compute_trend(&history, total, |s| s.total);
        let economic_trend = compute_trend(&history, economic, |s| s.economic);
        let execution_trend = compute_trend(&history, execution, |s| s.execution);
        let evolution_trend = compute_trend(&history, evolution, |s| s.evolution);

        Self {
            total,
            economic,
            economic_trend,
            execution,
            execution_trend,
            evolution,
            evolution_trend,
            coordination,
            introspection,
            trend,
            measured_at: now,
            generation: snapshot.generation,
        }
    }

    /// Store this score in the DB for historical tracking.
    pub fn store(&self, db: &SoulDatabase) {
        // Append to history (JSON array in soul_state)
        let mut history = load_history(db);
        history.push(self.clone());

        // Keep only the last HISTORY_SIZE entries
        if history.len() > HISTORY_SIZE {
            history.drain(..history.len() - HISTORY_SIZE);
        }

        if let Ok(json) = serde_json::to_string(&history) {
            let _ = db.set_state("fitness_history", &json);
        }

        // Also store current score for quick access
        if let Ok(json) = serde_json::to_string(self) {
            let _ = db.set_state("fitness_current", &json);
        }
    }

    /// Load the most recent fitness score.
    pub fn load_current(db: &SoulDatabase) -> Option<Self> {
        db.get_state("fitness_current")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Load fitness history for trend analysis.
    pub fn load_history(db: &SoulDatabase) -> Vec<Self> {
        load_history(db)
    }
}

fn load_history(db: &SoulDatabase) -> Vec<FitnessScore> {
    db.get_state("fitness_history")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Economic fitness: payments efficiency.
/// High score = endpoints are earning. Low score = lots of endpoints, no payments.
fn compute_economic(snapshot: &NodeSnapshot) -> f64 {
    let endpoints = snapshot.endpoint_count.max(1) as f64;
    let payments = snapshot.total_payments as f64;

    // Payments per endpoint, sigmoid-scaled so 1 payment/endpoint = ~0.7
    let efficiency = payments / endpoints;
    sigmoid(efficiency, 1.0)
}

/// Execution fitness: plan success rate.
fn compute_execution(db: &SoulDatabase) -> f64 {
    let completed = db.count_plans_by_status("Completed").unwrap_or(0).max(0) as f64;
    let failed = db.count_plans_by_status("Failed").unwrap_or(0).max(0) as f64;
    let total = completed + failed;
    if total < 1.0 {
        return 0.5; // no data yet — neutral
    }
    completed / total
}

/// Evolution fitness: are you changing your code?
fn compute_evolution(db: &SoulDatabase) -> f64 {
    let total_cycles: f64 = db
        .get_state("total_think_cycles")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0_f64)
        .max(1.0);

    let total_commits: f64 = db
        .get_state("total_commits")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    // Commits per 100 cycles, sigmoid-scaled
    let rate = (total_commits / total_cycles) * 100.0;
    sigmoid(rate, 2.0) // ~0.7 at 2 commits per 100 cycles
}

/// Coordination fitness: peer interaction success.
fn compute_coordination(db: &SoulDatabase) -> f64 {
    let peer_calls: f64 = db
        .get_state("peer_calls_attempted")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);
    let peer_successes: f64 = db
        .get_state("peer_calls_succeeded")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.0);

    if peer_calls < 1.0 {
        return 0.3; // hasn't tried yet — low but not zero
    }
    // Sanity: successes can't exceed attempts (prevent manipulation)
    let clamped = peer_successes.min(peer_calls);
    clamped / peer_calls
}

/// Introspection fitness: do beliefs match reality?
fn compute_introspection(snapshot: &NodeSnapshot, db: &SoulDatabase) -> f64 {
    let beliefs = db.get_all_active_beliefs().unwrap_or_default();
    if beliefs.is_empty() {
        return 0.3; // no beliefs yet — low
    }

    let mut correct = 0u32;
    let mut total = 0u32;

    for belief in &beliefs {
        // Check auto-beliefs against snapshot reality
        if !belief.evidence.starts_with("auto:") {
            continue;
        }

        total += 1;
        match (belief.subject.as_str(), belief.predicate.as_str()) {
            ("node", "endpoint_count") => {
                if belief.value == snapshot.endpoint_count.to_string() {
                    correct += 1;
                }
            }
            ("node", "total_payments") => {
                if belief.value == snapshot.total_payments.to_string() {
                    correct += 1;
                }
            }
            ("node", "children_count") => {
                if belief.value == snapshot.children_count.to_string() {
                    correct += 1;
                }
            }
            _ => {
                // Can't verify — assume correct (benefit of the doubt)
                correct += 1;
            }
        }
    }

    if total == 0 {
        return 0.5;
    }
    correct as f64 / total as f64
}

/// Compute trend (gradient) from historical fitness scores.
/// Returns the slope of a simple linear regression over recent scores.
fn compute_trend(history: &[FitnessScore], current_val: f64, selector: impl Fn(&FitnessScore) -> f64) -> f64 {
    if history.len() < 3 {
        return 0.0; // not enough data
    }

    // Use last 10 scores + current for trend
    let n = history.len().min(10);
    let recent: Vec<f64> = history[history.len() - n..]
        .iter()
        .map(selector)
        .chain(std::iter::once(current_val))
        .collect();

    // Simple linear regression: slope of fitness over time
    let len = recent.len() as f64;
    let x_mean = (len - 1.0) / 2.0;
    let y_mean: f64 = recent.iter().sum::<f64>() / len;

    let mut numerator = 0.0;
    let mut denominator = 0.0;
    for (i, y) in recent.iter().enumerate() {
        let x = i as f64;
        numerator += (x - x_mean) * (y - y_mean);
        denominator += (x - x_mean) * (x - x_mean);
    }

    if denominator.abs() < 1e-10 {
        return 0.0;
    }

    let slope = numerator / denominator;
    // Guard against NaN/Inf propagation
    if slope.is_finite() {
        slope
    } else {
        0.0
    }
}

/// Sigmoid scaling: maps [0, ∞) to [0, 1), with midpoint at `midpoint`.
fn sigmoid(x: f64, midpoint: f64) -> f64 {
    x / (x + midpoint)
}

/// Collective fitness: aggregate score across all visible peers + self.
/// This is the swarm-level gradient.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectiveFitness {
    /// Average fitness across all agents in the swarm.
    pub mean: f64,
    /// Best individual fitness in the swarm.
    pub best: f64,
    /// Worst individual fitness in the swarm.
    pub worst: f64,
    /// Number of agents measured.
    pub agent_count: u32,
    /// Trend of the mean (swarm-level gradient).
    pub swarm_trend: f64,
    pub measured_at: i64,
}

impl CollectiveFitness {
    /// Compute from own score + peer scores (fetched via discover_peers).
    pub fn compute(own_score: &FitnessScore, peer_scores: &[f64]) -> Self {
        let mut all_scores = vec![own_score.total];
        all_scores.extend_from_slice(peer_scores);

        let n = all_scores.len() as f64;
        let mean = all_scores.iter().sum::<f64>() / n;
        let best = all_scores.iter().cloned().fold(f64::MIN, f64::max);
        let worst = all_scores.iter().cloned().fold(f64::MAX, f64::min);

        Self {
            mean,
            best,
            worst,
            agent_count: all_scores.len() as u32,
            swarm_trend: own_score.trend, // approximation until we have swarm history
            measured_at: chrono::Utc::now().timestamp(),
        }
    }

    /// Store collective fitness for historical tracking.
    pub fn store(&self, db: &SoulDatabase) {
        if let Ok(json) = serde_json::to_string(self) {
            let _ = db.set_state("collective_fitness", &json);
        }
    }

    /// Load most recent collective fitness.
    pub fn load(db: &SoulDatabase) -> Option<Self> {
        db.get_state("collective_fitness")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
    }
}
