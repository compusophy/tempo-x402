//! x402-mind: subconscious background processing for autonomous nodes.
//!
//! The mind handles maintenance tasks that don't need conscious attention:
//! memory decay, pruning, promotion, belief decay, and periodic consolidation.
//!
//! It shares the soul's database and runs independently — the soul thinks,
//! the mind maintains.

pub mod config;
pub mod subconscious;

pub use config::MindConfig;
pub use subconscious::SubconsciousStats;

use std::sync::Arc;
use tokio::task::JoinHandle;

use x402_soul::SoulDatabase;

/// Handle to the running mind — wraps the subconscious loop's join handle.
pub struct MindHandle {
    pub handle: JoinHandle<()>,
}

/// The Mind: owns the subconscious background loop.
pub struct Mind {
    subconscious: subconscious::SubconsciousLoop,
}

impl Mind {
    /// Create a new Mind that shares the soul's database.
    pub fn new(db: Arc<SoulDatabase>, config: MindConfig) -> Self {
        Self {
            subconscious: subconscious::SubconsciousLoop::new(db, config),
        }
    }

    /// Spawn the subconscious loop as a background tokio task.
    pub fn spawn(self) -> MindHandle {
        let handle = tokio::spawn(async move {
            self.subconscious.run().await;
        });

        tracing::info!("Mind spawned: subconscious background loop");

        MindHandle { handle }
    }
}
