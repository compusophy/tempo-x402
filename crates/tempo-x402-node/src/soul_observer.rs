//! NodeObserver implementation for the x402 node.
//!
//! Reads analytics from the gateway database and identity info
//! to build a NodeSnapshot for the soul's thinking loop.

use std::sync::Arc;
use x402_gateway::state::AppState as GatewayState;
use x402_identity::InstanceIdentity;
use x402_soul::error::SoulError;
use x402_soul::observer::{NodeObserver, NodeSnapshot};

/// Observer that reads node state from the gateway database and identity.
pub struct NodeObserverImpl {
    gateway: GatewayState,
    identity: Option<InstanceIdentity>,
    generation: u32,
    started_at: chrono::DateTime<chrono::Utc>,
    db_path: String,
}

impl NodeObserverImpl {
    pub fn new(
        gateway: GatewayState,
        identity: Option<InstanceIdentity>,
        generation: u32,
        started_at: chrono::DateTime<chrono::Utc>,
        db_path: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            gateway,
            identity,
            generation,
            started_at,
            db_path,
        })
    }
}

impl NodeObserver for NodeObserverImpl {
    fn observe(&self) -> Result<NodeSnapshot, SoulError> {
        use x402_soul::observer::EndpointSummary;

        let uptime_secs = (chrono::Utc::now() - self.started_at).num_seconds().max(0) as u64;

        // Read endpoints + stats from gateway DB
        let endpoints = self
            .gateway
            .db
            .list_endpoints(500, 0)
            .map_err(|e| SoulError::Observer(format!("failed to list endpoints: {e}")))?;
        let endpoint_count = endpoints.len() as u32;

        let stats = self
            .gateway
            .db
            .list_endpoint_stats(500, 0)
            .map_err(|e| SoulError::Observer(format!("failed to list stats: {e}")))?;

        // Build stats lookup by slug
        let stats_by_slug: std::collections::HashMap<&str, _> =
            stats.iter().map(|s| (s.slug.as_str(), s)).collect();

        let mut total_revenue: u128 = 0;
        let mut total_payments: u64 = 0;
        let mut endpoint_summaries = Vec::new();

        for ep in &endpoints {
            let stat = stats_by_slug.get(ep.slug.as_str());
            let req_count = stat.map(|s| s.request_count).unwrap_or(0);
            let pay_count = stat.map(|s| s.payment_count).unwrap_or(0);
            let rev = stat
                .map(|s| s.revenue_total.clone())
                .unwrap_or_else(|| "0".to_string());

            total_revenue += rev.parse::<u128>().unwrap_or(0);
            total_payments += pay_count as u64;

            endpoint_summaries.push(EndpointSummary {
                slug: ep.slug.clone(),
                price: ep.price_usd.clone(),
                description: ep.description.clone(),
                request_count: req_count,
                payment_count: pay_count,
                revenue: rev,
            });
        }

        // Read children count from node DB
        let children_count = {
            match rusqlite::Connection::open(&self.db_path) {
                Ok(conn) => crate::db::query_children_active(&conn)
                    .map(|c| c.len() as u32)
                    .unwrap_or(0),
                Err(_) => 0,
            }
        };

        Ok(NodeSnapshot {
            uptime_secs,
            endpoint_count,
            total_revenue: total_revenue.to_string(),
            total_payments,
            children_count,
            wallet_address: self
                .identity
                .as_ref()
                .map(|id| format!("{:#x}", id.address))
                .or_else(|| {
                    Some(format!("{:#x}", self.gateway.config.platform_address))
                }),
            instance_id: self.identity.as_ref().map(|id| id.instance_id.clone()),
            generation: self.generation,
            endpoints: endpoint_summaries,
        })
    }
}
