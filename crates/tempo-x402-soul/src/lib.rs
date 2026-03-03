//! x402-soul: agentic soul for x402 nodes.
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
pub mod git;
pub mod guard;
pub mod llm;
pub mod memory;
pub mod mode;
pub mod neuroplastic;
pub mod observer;
pub mod persistent_memory;
pub mod prompts;
pub mod thinking;
pub mod tool_registry;
pub mod tools;
pub mod world_model;

pub use chat::{handle_chat, ChatReply};
pub use config::SoulConfig;
pub use db::SoulDatabase;
pub use error::SoulError;
pub use memory::{Thought, ThoughtType};
pub use observer::{NodeObserver, NodeSnapshot};
pub use thinking::ThinkingLoop;
pub use tools::ToolExecutor;

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
    /// Returns the JoinHandle so the caller can optionally await or abort it.
    pub fn spawn(self, observer: Arc<dyn NodeObserver>) -> JoinHandle<()> {
        let thinking_loop = ThinkingLoop::new(self.config, self.db, observer);
        tokio::spawn(async move {
            thinking_loop.run().await;
        })
    }

    /// Get a reference to the soul database (for external queries).
    pub fn database(&self) -> &Arc<SoulDatabase> {
        &self.db
    }
}
