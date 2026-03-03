//! Mind configuration from environment variables.

/// Configuration for the subconscious background processing loop.
#[derive(Debug, Clone)]
pub struct MindConfig {
    /// Master switch (env: MIND_ENABLED, default: true).
    pub enabled: bool,
    /// How often the subconscious loop runs in seconds (default: 3600).
    pub interval_secs: u64,
    /// Run consolidation every N subconscious cycles (default: 4).
    pub consolidation_every: u32,
    /// Strength threshold below which non-long-term thoughts are pruned (default: 0.01).
    pub prune_threshold: f64,
}

impl MindConfig {
    /// Check if mind mode is enabled via environment.
    pub fn is_enabled() -> bool {
        std::env::var("MIND_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(true)
    }

    /// Load mind configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = Self::is_enabled();

        let interval_secs: u64 = std::env::var("MIND_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);

        let consolidation_every: u32 = std::env::var("MIND_CONSOLIDATION_EVERY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(4);

        let prune_threshold: f64 = std::env::var("SOUL_PRUNE_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01);

        Self {
            enabled,
            interval_secs,
            consolidation_every,
            prune_threshold,
        }
    }
}
