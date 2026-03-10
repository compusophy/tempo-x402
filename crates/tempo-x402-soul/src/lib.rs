//! x402-soul: agentic soul for x402 nodes. (updated)
//!
//! Provides a periodic observe-think-record loop powered by an LLM (currently Gemini).
//! The soul observes node state via the [`NodeObserver`] trait, reasons
//! about it, and records thoughts to a dedicated SQLite database.
//!
//! Without an LLM API key, the soul runs in dormant mode: it still
//! observes and records snapshots, but skips LLM calls.

pub mod chat;
pub mod coding;
pub mod config;
pub mod db;
pub mod error;
pub mod fitness;
pub mod git;
pub mod guard;
pub mod llm;
pub mod memory;
pub mod mode;
pub mod neuroplastic;
pub mod observer;
pub mod persistent_memory;
pub mod plan;
pub mod prompts;
pub mod thinking;
pub mod tool_registry;
pub mod tools;
pub mod world_model;

pub use chat::{handle_chat, ChatReply};
pub use config::SoulConfig;
pub use db::{ChatMessage, ChatSession, Nudge, SoulDatabase};
pub use error::SoulError;
pub use memory::{Thought, ThoughtType};
pub use observer::{NodeObserver, NodeSnapshot};
pub use plan::{Plan, PlanStatus, PlanStep};
pub use thinking::ThinkingLoop;
pub use tools::ToolExecutor;
pub use world_model::{Goal, GoalStatus};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// The Soul: owns the database and thinking loop, spawns as a background task.
pub struct Soul {
    db: Arc<SoulDatabase>,
    config: SoulConfig,
}

impl Soul {
    /// Create a new Soul from config. Opens the soul database.
    pub fn new(config: SoulConfig) -> Result<Self, SoulError> {
        let db = Arc::new(SoulDatabase::new(&config.db_path)?);
        Ok(Self { db, config })
    }

    /// Spawn the thinking loop as a background tokio task.
    /// The `alive` flag is set to `true` while the soul is running, `false` during restart.
    /// The loop automatically restarts after panics (with a 30s cooldown).
    pub fn spawn(self, observer: Arc<dyn NodeObserver>, alive: Arc<AtomicBool>) -> JoinHandle<()> {
        let alive_for_task = alive;
        let config = self.config;
        let db = self.db;

        let handle = tokio::spawn(async move {
            loop {
                alive_for_task.store(true, Ordering::Relaxed);
                let alive_for_loop = alive_for_task.clone();
                let loop_instance = ThinkingLoop::new(config.clone(), db.clone(), observer.clone());

                // Spawn the thinking loop in an inner task so panics become JoinErrors
                let inner = tokio::spawn(async move {
                    loop_instance.run(alive_for_loop).await;
                });

                match inner.await {
                    Ok(()) => break, // clean exit (shouldn't happen — run() loops forever)
                    Err(e) if e.is_panic() => {
                        alive_for_task.store(false, Ordering::Relaxed);
                        tracing::error!("Soul thinking loop panicked — restarting in 30s: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                        // loop continues → fresh ThinkingLoop created
                    }
                    Err(e) => {
                        alive_for_task.store(false, Ordering::Relaxed);
                        tracing::error!("Soul thinking loop failed: {:?}", e);
                        break;
                    }
                }
            }
        });

        handle
    }

    /// Get a reference to the soul database (for external queries).
    pub fn database(&self) -> &Arc<SoulDatabase> {
        &self.db
    }
}
