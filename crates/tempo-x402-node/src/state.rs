//! Node-specific state extending the gateway's AppState.

use std::sync::Arc;
use x402_agent::CloneOrchestrator;
use x402_gateway::state::AppState as GatewayState;
use x402_identity::InstanceIdentity;
use x402_soul::{NodeObserver, SoulConfig, SoulDatabase};

/// Node state wrapping gateway state with identity + agent capabilities.
#[derive(Clone)]
pub struct NodeState {
    /// The underlying gateway state (config, db, http_client, facilitator)
    pub gateway: GatewayState,
    /// Instance identity (when AUTO_BOOTSTRAP is set)
    pub identity: Option<InstanceIdentity>,
    /// Clone orchestrator (when Railway credentials are configured)
    pub agent: Option<Arc<CloneOrchestrator>>,
    /// When the node started
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Read connection to the database for children queries
    pub db_path: String,
    /// Clone price (e.g., "$0.10")
    pub clone_price: Option<String>,
    /// Clone price in token units
    pub clone_price_amount: Option<String>,
    /// Maximum children
    pub clone_max_children: u32,
    /// Soul database for querying thoughts/state (None if soul init failed)
    pub soul_db: Option<Arc<SoulDatabase>>,
    /// Whether the soul is dormant (no LLM API key)
    pub soul_dormant: bool,
    /// Soul config for chat handler (None if soul init failed)
    pub soul_config: Option<SoulConfig>,
    /// Soul observer for chat handler (None if soul init failed)
    pub soul_observer: Option<Arc<dyn NodeObserver>>,
}
