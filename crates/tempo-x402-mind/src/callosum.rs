//! The callosum: active integration bus between hemispheres.
//!
//! Inspired by the corpus callosum (200M+ axons doing both excitatory sharing
//! AND inhibitory gating). Three operations:
//!
//! 1. **Share** (excitatory) — inject summaries of each hemisphere's thoughts into the other
//! 2. **Gate** (inhibitory) — resolve conflicts by domain authority
//! 3. **Escalate** — cross-wake when uncertainty or urgency is detected

use std::sync::Arc;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

use x402_soul::memory::{Thought, ThoughtType};
use x402_soul::SoulDatabase;

use crate::hemisphere::HemisphereRole;

/// The integration bus between hemispheres.
pub struct Callosum {
    left_db: Arc<SoulDatabase>,
    right_db: Arc<SoulDatabase>,
    integration_interval_secs: u64,
    escalation_threshold: f32,
    /// Notify the left hemisphere to wake up (triggered by right's [URGENT]).
    pub left_wake: Arc<Notify>,
    /// Notify the right hemisphere to wake up (triggered by left's [UNCERTAIN]).
    pub right_wake: Arc<Notify>,
}

impl Callosum {
    pub fn new(
        left_db: Arc<SoulDatabase>,
        right_db: Arc<SoulDatabase>,
        integration_interval_secs: u64,
        escalation_threshold: f32,
    ) -> Self {
        Self {
            left_db,
            right_db,
            integration_interval_secs,
            escalation_threshold,
            left_wake: Arc::new(Notify::new()),
            right_wake: Arc::new(Notify::new()),
        }
    }

    /// Spawn the callosum integration loop as a background task.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    /// Run the integration loop forever.
    async fn run(&self) {
        let interval = std::time::Duration::from_secs(self.integration_interval_secs);

        tracing::info!(
            interval_secs = self.integration_interval_secs,
            threshold = self.escalation_threshold,
            "Callosum integration loop started"
        );

        loop {
            if let Err(e) = self.integration_cycle() {
                tracing::warn!(error = %e, "Callosum integration cycle failed");
            }

            tokio::time::sleep(interval).await;
        }
    }

    /// Execute one integration cycle: share + check for escalation signals.
    fn integration_cycle(&self) -> Result<(), x402_soul::SoulError> {
        // Share: read recent thoughts from each hemisphere, inject summaries into the other
        self.share_thoughts()?;

        // Escalate: check for uncertainty/urgency signals in recent thoughts
        self.check_escalation_signals()?;

        Ok(())
    }

    /// Share (excitatory): read recent thoughts from each hemisphere
    /// and inject CrossHemisphere summaries into the other.
    ///
    /// When both hemispheres share the same DB (`shared_db=true`), sharing
    /// is skipped entirely — injecting from a DB into itself is nonsensical
    /// and creates an echo loop where the same thoughts get re-summarized
    /// every cycle.
    ///
    /// When DBs are separate, filters out CrossHemisphere and Escalation
    /// thoughts to prevent re-injection loops.
    fn share_thoughts(&self) -> Result<(), x402_soul::SoulError> {
        // If both hemispheres share the same DB, sharing is meaningless —
        // both would read the same thoughts and inject duplicates back.
        if Arc::ptr_eq(&self.left_db, &self.right_db) {
            tracing::trace!("Callosum: shared DB mode — skipping cross-hemisphere sharing");
            return Ok(());
        }

        // Get left's recent thoughts, excluding cross-hemisphere and escalation
        // to prevent infinite echo loops
        let left_recent: Vec<Thought> = self
            .left_db
            .recent_thoughts(5)?
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.thought_type,
                    ThoughtType::CrossHemisphere | ThoughtType::Escalation
                )
            })
            .take(3)
            .collect();
        // Get right's recent thoughts, same filtering
        let right_recent: Vec<Thought> = self
            .right_db
            .recent_thoughts(5)?
            .into_iter()
            .filter(|t| {
                !matches!(
                    t.thought_type,
                    ThoughtType::CrossHemisphere | ThoughtType::Escalation
                )
            })
            .take(3)
            .collect();

        // Inject left's thoughts into right's DB as CrossHemisphere
        if !left_recent.is_empty() {
            let summary = Self::summarize_thoughts(&left_recent, HemisphereRole::Left);
            let cross_thought = Thought {
                id: uuid::Uuid::new_v4().to_string(),
                thought_type: ThoughtType::CrossHemisphere,
                content: summary,
                context: Some(
                    serde_json::json!({
                        "source": "left",
                        "thought_count": left_recent.len(),
                    })
                    .to_string(),
                ),
                created_at: chrono::Utc::now().timestamp(),
                salience: None,
                memory_tier: None,
                strength: None,
            };
            self.right_db.insert_thought(&cross_thought)?;
            tracing::debug!(
                "Callosum: shared left→right ({} thoughts)",
                left_recent.len()
            );
        }

        // Inject right's thoughts into left's DB as CrossHemisphere
        if !right_recent.is_empty() {
            let summary = Self::summarize_thoughts(&right_recent, HemisphereRole::Right);
            let cross_thought = Thought {
                id: uuid::Uuid::new_v4().to_string(),
                thought_type: ThoughtType::CrossHemisphere,
                content: summary,
                context: Some(
                    serde_json::json!({
                        "source": "right",
                        "thought_count": right_recent.len(),
                    })
                    .to_string(),
                ),
                created_at: chrono::Utc::now().timestamp(),
                salience: None,
                memory_tier: None,
                strength: None,
            };
            self.left_db.insert_thought(&cross_thought)?;
            tracing::debug!(
                "Callosum: shared right→left ({} thoughts)",
                right_recent.len()
            );
        }

        Ok(())
    }

    /// Check for escalation signals in recent thoughts.
    ///
    /// Escalation still works in shared DB mode — it's about cross-waking
    /// hemispheres via Notify, not about injecting into a different DB.
    /// But we skip recording duplicate escalation thoughts when DBs are shared,
    /// since they'd just pollute the single DB.
    fn check_escalation_signals(&self) -> Result<(), x402_soul::SoulError> {
        let shared = Arc::ptr_eq(&self.left_db, &self.right_db);

        // Check left's recent thoughts for [UNCERTAIN]
        // Filter out Escalation thoughts so we don't re-escalate old escalations
        let left_recent = self.left_db.recent_thoughts(3)?;
        for thought in &left_recent {
            if thought.thought_type == ThoughtType::Escalation {
                continue;
            }
            if thought.content.contains("[UNCERTAIN]") {
                tracing::info!("Callosum: left signaled [UNCERTAIN] — waking right hemisphere");
                self.right_wake.notify_one();

                // Record the escalation (skip if shared DB to avoid noise)
                if !shared {
                    let escalation = Thought {
                        id: uuid::Uuid::new_v4().to_string(),
                        thought_type: ThoughtType::Escalation,
                        content: format!(
                            "Left hemisphere uncertainty detected — escalating to right for deeper analysis: {}",
                            thought.content.chars().take(200).collect::<String>()
                        ),
                        context: Some(
                            serde_json::json!({
                                "direction": "left_to_right",
                                "trigger": "uncertain",
                                "source_thought_id": thought.id,
                            })
                            .to_string(),
                        ),
                        created_at: chrono::Utc::now().timestamp(),
                        salience: None,
                        memory_tier: None,
                        strength: None,
                    };
                    self.right_db.insert_thought(&escalation)?;
                }
                break; // One escalation per cycle
            }
        }

        // Check right's recent thoughts for [URGENT]
        let right_recent = self.right_db.recent_thoughts(3)?;
        for thought in &right_recent {
            if thought.thought_type == ThoughtType::Escalation {
                continue;
            }
            if thought.content.contains("[URGENT]") {
                tracing::info!("Callosum: right signaled [URGENT] — waking left hemisphere");
                self.left_wake.notify_one();

                if !shared {
                    let escalation = Thought {
                        id: uuid::Uuid::new_v4().to_string(),
                        thought_type: ThoughtType::Escalation,
                        content: format!(
                            "Right hemisphere urgency detected — escalating to left for immediate action: {}",
                            thought.content.chars().take(200).collect::<String>()
                        ),
                        context: Some(
                            serde_json::json!({
                                "direction": "right_to_left",
                                "trigger": "urgent",
                                "source_thought_id": thought.id,
                            })
                            .to_string(),
                        ),
                        created_at: chrono::Utc::now().timestamp(),
                        salience: None,
                        memory_tier: None,
                        strength: None,
                    };
                    self.left_db.insert_thought(&escalation)?;
                }
                break;
            }
        }

        Ok(())
    }

    /// Summarize a list of thoughts from one hemisphere for injection into the other.
    fn summarize_thoughts(thoughts: &[Thought], source: HemisphereRole) -> String {
        let source_label = match source {
            HemisphereRole::Left => "Left hemisphere (analytical)",
            HemisphereRole::Right => "Right hemisphere (holistic)",
        };

        let entries: Vec<String> = thoughts
            .iter()
            .map(|t| {
                let truncated: String = t.content.chars().take(150).collect();
                format!("  - [{}] {}", t.thought_type.as_str(), truncated)
            })
            .collect();

        format!(
            "[Cross-hemisphere update from {}]\n{}",
            source_label,
            entries.join("\n")
        )
    }

    /// Resolve a conflict between two hemispheres' thoughts on the same topic.
    /// The hemisphere with domain authority wins.
    pub fn resolve_conflict(
        left_thought: &Thought,
        right_thought: &Thought,
        topic: &str,
    ) -> ConflictResolution {
        let left_authority = HemisphereRole::Left.is_authority_for(topic);
        let right_authority = HemisphereRole::Right.is_authority_for(topic);

        match (left_authority, right_authority) {
            (true, false) => ConflictResolution {
                winner: HemisphereRole::Left,
                winning_thought: left_thought.clone(),
                reason: format!("Left hemisphere has domain authority for: {topic}"),
            },
            (false, true) => ConflictResolution {
                winner: HemisphereRole::Right,
                winning_thought: right_thought.clone(),
                reason: format!("Right hemisphere has domain authority for: {topic}"),
            },
            _ => {
                // Both or neither have authority — default to left (System 1)
                ConflictResolution {
                    winner: HemisphereRole::Left,
                    winning_thought: left_thought.clone(),
                    reason: format!(
                        "No clear domain authority for {topic} — defaulting to left (System 1)"
                    ),
                }
            }
        }
    }
}

/// Result of resolving a conflict between hemispheres.
#[derive(Debug)]
pub struct ConflictResolution {
    pub winner: HemisphereRole,
    pub winning_thought: Thought,
    pub reason: String,
}
