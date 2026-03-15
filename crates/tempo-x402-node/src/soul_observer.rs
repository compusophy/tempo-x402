//! NodeObserver implementation for the x402 node.
//!
//! Standardized retry logic for peer discovery.
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
    /// Cached peers from last fetch (refreshed periodically).
    peers_cache: std::sync::Mutex<(Vec<x402_soul::observer::PeerInfo>, std::time::Instant)>,
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
            peers_cache: std::sync::Mutex::new((Vec::new(), std::time::Instant::now())),
        })
    }
}

impl NodeObserverImpl {
    /// Refresh the peers cache by querying parent's /instance/siblings and each peer's /instance/info.
    /// Called periodically from the thinking loop context (async).
    /// Updated for robust handling with exponential backoff.
    pub async fn refresh_peers(&self) {
        use x402_soul::observer::PeerInfo;

        let parent_url = std::env::var("PARENT_URL").ok();
        let self_instance_id = self.identity.as_ref().map(|id| id.instance_id.as_str());

        // Also include local children as peers (for parent nodes)
        let mut peers = Vec::new();

        // Local children (parent perspective)
        if let Ok(conn) = rusqlite::Connection::open(&self.db_path) {
            if let Ok(children) = crate::db::query_children_active(&conn) {
                for child in children {
                    if child.status != "running" {
                        continue;
                    }
                    if let Some(url) = &child.url {
                        let mut peer = PeerInfo {
                            instance_id: child.instance_id.clone(),
                            url: url.clone(),
                            address: Some(child.address.clone()),
                            version: None,
                            endpoints: Vec::new(),
                        };
                        // Try to fetch /instance/info for richer data
                        if let Ok(info) = Self::fetch_peer_info(url).await {
                            peer.version = info.version;
                            peer.endpoints = info.endpoints;
                        }
                        peers.push(peer);
                    }
                }
            }
        }

        // Siblings (child perspective — ask parent)
        if let Some(ref parent) = parent_url {
            let siblings_url = format!("{}/instance/siblings", parent.trim_end_matches('/'));
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build();
            if let Ok(client) = client {
                let mut last_err = None;
                for attempt in 0..3 {
                    if attempt > 0 {
                        let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt as u32));
                        tokio::time::sleep(delay).await;
                    }
                    match client.get(&siblings_url).send().await {
                        Ok(resp) => {
                            let status = resp.status();
                            if status.is_success() {
                                if let Ok(json) = resp.json::<serde_json::Value>().await {
                                    if let Some(siblings) = json.get("siblings").and_then(|v| v.as_array()) {
                                        for sib in siblings {
                                            let inst_id = sib
                                                .get("instance_id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or_default();
                                            // Skip self
                                            if self_instance_id == Some(inst_id) {
                                                continue;
                                            }
                                            let url = match sib.get("url").and_then(|v| v.as_str()) {
                                                Some(u) => u.to_string(),
                                                None => continue,
                                            };
                                            let mut peer = PeerInfo {
                                                instance_id: inst_id.to_string(),
                                                url: url.clone(),
                                                address: sib
                                                    .get("address")
                                                    .and_then(|v| v.as_str())
                                                    .map(String::from),
                                                version: None,
                                                endpoints: Vec::new(),
                                            };
                                            // Fetch peer's /instance/info for endpoints
                                            if let Ok(info) = Self::fetch_peer_info(&url).await {
                                                peer.version = info.version;
                                                peer.endpoints = info.endpoints;
                                            }
                                            peers.push(peer);
                                        }
                                    }
                                }
                                last_err = None;
                                break;
                            } else if status.as_u16() == 429 || status.is_server_error() {
                                last_err = Some(format!("HTTP {}", status));
                                continue;
                            } else {
                                break;
                            }
                        }
                        Err(e) => {
                            last_err = Some(e.to_string());
                            continue;
                        }
                    }
                }
                if let Some(err) = last_err {
                    tracing::warn!(error = %err, url = %siblings_url, "Failed to fetch siblings after retries");
                }
            }
        }

        if let Ok(mut cache) = self.peers_cache.lock() {
            *cache = (peers, std::time::Instant::now());
        }
    }

    /// Fetch a peer's /instance/info and extract version + endpoints.
    async fn fetch_peer_info(
        peer_url: &str,
    ) -> Result<PeerInfoResult, Box<dyn std::error::Error + Send + Sync>> {
        use x402_soul::observer::PeerEndpoint;
        use std::time::Duration;

        let url = format!("{}/instance/info", peer_url.trim_end_matches('/'));
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        let mut last_err = None;
        for attempt in 0..3 {
            if attempt > 0 {
                let delay = Duration::from_millis(500 * 2u64.pow(attempt as u32));
                tokio::time::sleep(delay).await;
            }

            match client.get(&url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let json: serde_json::Value = resp.json().await?;
                        let version = json
                            .get("version")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let mut endpoints = Vec::new();
                        if let Some(eps) = json.get("endpoints").and_then(|v| v.as_array()) {
                            for ep in eps {
                                endpoints.push(PeerEndpoint {
                                    slug: ep
                                        .get("slug")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string(),
                                    price: ep
                                        .get("price")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("0")
                                        .to_string(),
                                    description: ep
                                        .get("description")
                                        .and_then(|v| v.as_str())
                                        .map(String::from),
                                });
                            }
                        }
                        return Ok(PeerInfoResult { version, endpoints });
                    } else if status.as_u16() == 429 || status.is_server_error() {
                        last_err = Some(format!("HTTP {}: {}", status, url).into());
                        continue;
                    } else {
                        return Err(format!("HTTP {}: {}", status, url).into());
                    }
                }
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| "Failed after retries".into()))
    }
}

/// Extracted peer info from /instance/info.
struct PeerInfoResult {
    version: Option<String>,
    endpoints: Vec<x402_soul::observer::PeerEndpoint>,
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

        // Use cached peers (refreshed async in thinking loop)
        let peers = self
            .peers_cache
            .lock()
            .map(|cache| cache.0.clone())
            .unwrap_or_default();

        Ok(NodeSnapshot {
            uptime_secs,
            endpoint_count,
            total_revenue: total_revenue.to_string(),
            total_payments,
            children_count,
            wallet_address: self
                .identity
                .as_ref()
                .map(|id| format!("{:#x}", id.address)),
            instance_id: self.identity.as_ref().map(|id| id.instance_id.clone()),
            generation: self.generation,
            endpoints: endpoint_summaries,
            peers,
        })
    }
}
