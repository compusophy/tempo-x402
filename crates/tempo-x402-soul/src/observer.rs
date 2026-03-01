//! Node observer trait and snapshot types.

use serde::{Deserialize, Serialize};

use crate::error::SoulError;

/// Per-endpoint summary for the soul's context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointSummary {
    pub slug: String,
    pub price: String,
    pub description: Option<String>,
    pub request_count: i64,
    pub payment_count: i64,
    pub revenue: String,
}

/// A snapshot of the node's current state, captured by the observer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSnapshot {
    /// How long the node has been running (seconds).
    pub uptime_secs: u64,
    /// Number of registered endpoints.
    pub endpoint_count: u32,
    /// Total revenue across all endpoints (token units as string).
    pub total_revenue: String,
    /// Total payment count across all endpoints.
    pub total_payments: u64,
    /// Number of active child nodes.
    pub children_count: u32,
    /// The node's wallet address (if identity is bootstrapped).
    pub wallet_address: Option<String>,
    /// The node's instance ID (if identity is bootstrapped).
    pub instance_id: Option<String>,
    /// The node's generation in the lineage.
    pub generation: u32,
    /// Per-endpoint details (slug, price, traffic).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub endpoints: Vec<EndpointSummary>,
}

/// Trait for observing node state. Implemented by the node crate.
pub trait NodeObserver: Send + Sync + 'static {
    /// Capture a snapshot of the current node state.
    fn observe(&self) -> Result<NodeSnapshot, SoulError>;
}
