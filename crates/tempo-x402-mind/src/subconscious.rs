//! The subconscious loop: background maintenance that runs independently from the soul.
//!
//! Handles decay, pruning, promotion, belief decay, and periodic consolidation.
//! Shares the soul's database — no separate state.

use std::sync::Arc;

use x402_soul::db::SoulDatabase;
use x402_soul::llm::{ConversationMessage, ConversationPart, LlmClient, LlmResult};
use x402_soul::memory::{Thought, ThoughtType};

use crate::config::MindConfig;

/// Stats from a single subconscious cycle, exposed via `/mind/status`.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SubconsciousStats {
    pub total_cycles: u64,
    pub last_cycle_at: Option<i64>,
    pub last_thoughts_decayed: u32,
    pub last_thoughts_pruned: u32,
    pub last_thoughts_promoted: u32,
    pub last_beliefs_demoted: u32,
    pub last_beliefs_deactivated: u32,
    pub last_consolidation_at: Option<i64>,
}

/// The subconscious background processing loop.
pub struct SubconsciousLoop {
    db: Arc<SoulDatabase>,
    config: MindConfig,
}

impl SubconsciousLoop {
    pub fn new(db: Arc<SoulDatabase>, config: MindConfig) -> Self {
        Self { db, config }
    }

    /// Run the subconscious loop forever.
    pub async fn run(self) {
        let interval = std::time::Duration::from_secs(self.config.interval_secs);
        let mut cycle_count: u64 = 0;

        tracing::info!(
            interval_secs = self.config.interval_secs,
            consolidation_every = self.config.consolidation_every,
            "Subconscious loop started"
        );

        loop {
            tokio::time::sleep(interval).await;
            cycle_count += 1;

            let stats = self.cycle(cycle_count).await;

            // Persist stats for the /mind/status endpoint
            let _ = self
                .db
                .set_state("mind_total_cycles", &stats.total_cycles.to_string());
            if let Some(ts) = stats.last_cycle_at {
                let _ = self.db.set_state("mind_last_cycle_at", &ts.to_string());
            }
            if let Some(ts) = stats.last_consolidation_at {
                let _ = self
                    .db
                    .set_state("mind_last_consolidation_at", &ts.to_string());
            }

            tracing::info!(
                cycle = cycle_count,
                decayed = stats.last_thoughts_decayed,
                pruned = stats.last_thoughts_pruned,
                promoted = stats.last_thoughts_promoted,
                beliefs_demoted = stats.last_beliefs_demoted,
                beliefs_deactivated = stats.last_beliefs_deactivated,
                "Subconscious cycle complete"
            );
        }
    }

    /// Execute one subconscious cycle. No LLM needed except for consolidation.
    async fn cycle(&self, cycle_count: u64) -> SubconsciousStats {
        let now = chrono::Utc::now().timestamp();
        let mut stats = SubconsciousStats {
            total_cycles: cycle_count,
            last_cycle_at: Some(now),
            ..Default::default()
        };

        // 1. Decay — tier-based strength decay
        match self.db.run_decay_cycle(self.config.prune_threshold) {
            Ok((decayed, pruned)) => {
                stats.last_thoughts_decayed = decayed;
                stats.last_thoughts_pruned = pruned;
            }
            Err(e) => tracing::warn!(error = %e, "Subconscious: decay cycle failed"),
        }

        // 2. Promote — sensory with salience > 0.6 → working tier
        match self.db.promote_salient_sensory(0.6) {
            Ok(promoted) => {
                stats.last_thoughts_promoted = promoted;
            }
            Err(e) => tracing::warn!(error = %e, "Subconscious: promotion failed"),
        }

        // 3. Belief decay — unconfirmed beliefs: High→Medium→Low→inactive
        match self.db.decay_beliefs() {
            Ok((demoted_high, demoted_med, deactivated)) => {
                stats.last_beliefs_demoted = demoted_high + demoted_med;
                stats.last_beliefs_deactivated = deactivated;
            }
            Err(e) => tracing::warn!(error = %e, "Subconscious: belief decay failed"),
        }

        // 4. Consolidation — every N cycles, summarize recent thoughts (optional LLM)
        if self.config.consolidation_every > 0
            && cycle_count.is_multiple_of(self.config.consolidation_every as u64)
        {
            self.maybe_consolidate().await;
            stats.last_consolidation_at = Some(chrono::Utc::now().timestamp());
        }

        stats
    }

    /// Consolidate recent thoughts into a MemoryConsolidation summary.
    /// Uses LLM if a Gemini API key is available, otherwise does a simple concatenation.
    async fn maybe_consolidate(&self) {
        // Fetch last 20 substantive thoughts
        let thoughts = match self.db.recent_thoughts_by_type(
            &[
                ThoughtType::Reasoning,
                ThoughtType::Decision,
                ThoughtType::Observation,
                ThoughtType::Reflection,
                ThoughtType::Prediction,
            ],
            20,
        ) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "Subconscious: failed to fetch thoughts for consolidation");
                return;
            }
        };

        if thoughts.len() < 5 {
            return;
        }

        // Check if we have an LLM key for smart consolidation
        let api_key = std::env::var("GEMINI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());

        let summary = if let Some(key) = api_key {
            // LLM-powered consolidation
            match self.llm_consolidate(&key, &thoughts).await {
                Some(s) => s,
                None => self.simple_consolidate(&thoughts),
            }
        } else {
            self.simple_consolidate(&thoughts)
        };

        let consolidation = Thought {
            id: uuid::Uuid::new_v4().to_string(),
            thought_type: ThoughtType::MemoryConsolidation,
            content: summary,
            context: None,
            created_at: chrono::Utc::now().timestamp(),
            salience: None,
            memory_tier: None,
            strength: None,
        };

        // Insert as high-salience long-term
        let result = self.db.insert_thought_with_salience(
            &consolidation,
            0.9,
            r#"{"novelty":0.8,"prediction_error":0.0,"reward_signal":0.0,"recency_boost":0.1,"reinforcement":0.0}"#,
            "long_term",
            1.0,
            None,
        );

        match result {
            Ok(()) => tracing::info!("Subconscious: memory consolidation recorded"),
            Err(e) => tracing::warn!(error = %e, "Subconscious: failed to insert consolidation"),
        }
    }

    /// Simple consolidation without LLM — just concatenate and truncate.
    fn simple_consolidate(&self, thoughts: &[Thought]) -> String {
        let entries: Vec<String> = thoughts
            .iter()
            .map(|t| {
                let truncated: String = t.content.chars().take(100).collect();
                format!("[{}] {}", t.thought_type.as_str(), truncated)
            })
            .collect();

        format!(
            "[Memory consolidation — subconscious ({} thoughts)]\n{}",
            thoughts.len(),
            entries.join("\n")
        )
    }

    /// LLM-powered consolidation using Gemini.
    async fn llm_consolidate(&self, api_key: &str, thoughts: &[Thought]) -> Option<String> {
        let model = std::env::var("GEMINI_MODEL_FAST")
            .unwrap_or_else(|_| "gemini-3-flash-preview".to_string());

        let llm = LlmClient::new(api_key.to_string(), model.clone(), model);

        let thought_text: String = thoughts
            .iter()
            .map(|t| {
                format!(
                    "[{}] {}",
                    t.thought_type.as_str(),
                    t.content.chars().take(400).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Summarize these recent thoughts into a concise 2-3 sentence consolidation. \
             Focus on key patterns, decisions made, and current state of understanding. \
             Be specific and factual.\n\n{thought_text}"
        );

        let conversation = vec![ConversationMessage {
            role: "user".to_string(),
            parts: vec![ConversationPart::Text(prompt)],
        }];

        match llm
            .think_with_tools(
                "You are a memory consolidation system. Produce brief, factual summaries.",
                &conversation,
                &[],
            )
            .await
        {
            Ok(LlmResult::Text(summary)) => Some(summary),
            Ok(_) => None,
            Err(e) => {
                tracing::warn!(error = %e, "Subconscious: consolidation LLM call failed");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_consolidate() {
        let db = Arc::new(SoulDatabase::new(":memory:").unwrap());
        let config = MindConfig {
            enabled: true,
            interval_secs: 60,
            consolidation_every: 4,
            prune_threshold: 0.01,
        };
        let loop_ = SubconsciousLoop::new(db, config);

        let thoughts: Vec<Thought> = (0..5)
            .map(|i| Thought {
                id: format!("t{i}"),
                thought_type: ThoughtType::Reasoning,
                content: format!("Thought number {i}"),
                context: None,
                created_at: 1000 + i as i64,
                salience: None,
                memory_tier: None,
                strength: None,
            })
            .collect();

        let summary = loop_.simple_consolidate(&thoughts);
        assert!(summary.contains("Memory consolidation"));
        assert!(summary.contains("5 thoughts"));
    }

    #[tokio::test]
    async fn test_cycle_on_empty_db() {
        let db = Arc::new(SoulDatabase::new(":memory:").unwrap());
        let config = MindConfig {
            enabled: true,
            interval_secs: 60,
            consolidation_every: 4,
            prune_threshold: 0.01,
        };
        let loop_ = SubconsciousLoop::new(db, config);

        // Should not panic on empty DB
        let stats = loop_.cycle(1).await;
        assert_eq!(stats.total_cycles, 1);
        assert_eq!(stats.last_thoughts_pruned, 0);
    }

    #[tokio::test]
    async fn test_consolidation_cycle() {
        let db = Arc::new(SoulDatabase::new(":memory:").unwrap());

        // Insert enough thoughts for consolidation
        for i in 0..10 {
            let thought = Thought {
                id: format!("t{i}"),
                thought_type: ThoughtType::Reasoning,
                content: format!("Reasoning about topic {i}"),
                context: None,
                created_at: 1000 + i as i64,
                salience: None,
                memory_tier: None,
                strength: None,
            };
            db.insert_thought(&thought).unwrap();
        }

        let config = MindConfig {
            enabled: true,
            interval_secs: 60,
            consolidation_every: 1, // consolidate every cycle for test
            prune_threshold: 0.01,
        };
        let loop_ = SubconsciousLoop::new(db.clone(), config);

        // Run cycle 1 (triggers consolidation since consolidation_every=1)
        let stats = loop_.cycle(1).await;
        assert!(stats.last_consolidation_at.is_some());

        // Check that a consolidation thought was inserted
        let recent = db.recent_thoughts(1).unwrap();
        assert_eq!(recent[0].thought_type, ThoughtType::MemoryConsolidation);
    }
}
