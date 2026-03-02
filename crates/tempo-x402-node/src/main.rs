//! x402-node: self-deploying x402 node with identity bootstrap + clone orchestration.
//!
//! Composes the x402-gateway (API proxy) with identity bootstrap and Railway
//! clone orchestration. Runs as a standalone binary that can spawn copies of itself.

use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{middleware::Logger, web, App, HttpServer};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use x402_agent::{CloneConfig, CloneOrchestrator, RailwayClient};
use x402_gateway::{
    config::GatewayConfig, db::Database, metrics::register_metrics, state::AppState as GatewayState,
};

mod db;
mod routes;
mod soul_observer;
mod state;

use state::NodeState;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Load .env file if present
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,actix_web=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // ── Identity bootstrap ──────────────────────────────────────────────
    // Must run BEFORE GatewayConfig::from_env() so that injected env vars
    // (EVM_ADDRESS, FACILITATOR_PRIVATE_KEY, FACILITATOR_SHARED_SECRET)
    // are visible to the gateway config loader.
    let auto_bootstrap = std::env::var("AUTO_BOOTSTRAP")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);

    let identity = if auto_bootstrap {
        let identity_path =
            std::env::var("IDENTITY_PATH").unwrap_or_else(|_| "/data/identity.json".to_string());
        let id = x402_identity::bootstrap(&identity_path).expect("Failed to bootstrap identity");
        tracing::info!("Instance identity: {:#x} ({})", id.address, id.instance_id);
        Some(id)
    } else {
        tracing::info!("AUTO_BOOTSTRAP not set — running without identity");
        None
    };

    // ── Gateway config (same as gateway main.rs) ────────────────────────
    let mut config = GatewayConfig::from_env().expect("Failed to load configuration");
    let port = config.port;
    let allowed_origins = config.allowed_origins.clone();
    let rate_limit_rpm = config.rate_limit_rpm;
    let spa_dir = config.spa_dir.clone();
    let rpc_url = config.rpc_url.clone();
    let db_path = config.db_path.clone();

    // Extract the private key early to minimize copies of key material in memory.
    let facilitator_private_key = config.facilitator_private_key.take();

    tracing::info!("Starting x402-node on port {}", port);
    tracing::info!("Platform address: {:#x}", config.platform_address);
    tracing::info!("Platform fee: {}", config.platform_fee);
    tracing::info!(
        "HMAC auth: {}",
        if config.hmac_secret.is_some() {
            "enabled"
        } else {
            "disabled (dev mode)"
        }
    );

    // ── Embedded facilitator bootstrap (same as gateway) ────────────────
    let facilitator_state = if let Some(ref key) = facilitator_private_key {
        if config.hmac_secret.is_none() {
            tracing::error!(
                "FACILITATOR_SHARED_SECRET is required when FACILITATOR_PRIVATE_KEY is set. \
                 Without HMAC, the embedded facilitator settlement endpoint is unauthenticated."
            );
            std::process::exit(1);
        }

        Some(x402_facilitator::bootstrap::bootstrap_embedded_facilitator(
            x402_facilitator::bootstrap::BootstrapConfig {
                private_key: key,
                rpc_url: &config.rpc_url,
                nonce_db_path: &config.nonce_db_path,
                hmac_secret: config
                    .hmac_secret
                    .clone()
                    .expect("HMAC secret must be set when embedded facilitator is enabled"),
                webhook_urls: config.webhook_urls.clone(),
                metrics_token: config.metrics_token.as_ref().map(|t| t.as_bytes().to_vec()),
            },
        ))
    } else {
        tracing::info!("Facilitator URL: {}", config.facilitator_url);
        None
    };

    // ── Database ────────────────────────────────────────────────────────
    let gateway_db = Database::new(&config.db_path).expect("Failed to initialize database");
    tracing::info!("Database initialized at: {}", config.db_path);

    let gateway_state = GatewayState::new(
        config.clone(),
        gateway_db.clone(),
        facilitator_state.clone().map(Arc::new),
    );

    match gateway_db.purge_stale_reservations(300) {
        Ok(0) => {}
        Ok(n) => tracing::info!("Purged {n} stale slug reservations from previous runs"),
        Err(e) => tracing::warn!("Failed to purge stale reservations: {e}"),
    }

    // Clean up leftover e2e test endpoints
    match gateway_db.purge_endpoints_by_prefix("e2e-test-") {
        Ok(0) => {}
        Ok(n) => tracing::info!("Purged {n} stale e2e-test endpoints"),
        Err(e) => tracing::warn!("Failed to purge e2e-test endpoints: {e}"),
    }

    // Clean up legacy demo endpoint
    if let Ok(_) = gateway_db.delete_endpoint("demo") {
        tracing::info!("Purged legacy demo endpoint");
    }

    // Initialize children table (node extension on top of gateway DB)
    db::init_children_schema(&gateway_db).expect("Failed to initialize children schema");
    tracing::info!("Children schema initialized");

    // Register Prometheus metrics
    register_metrics();

    // ── Clone orchestrator config ───────────────────────────────────────
    let railway_token = std::env::var("RAILWAY_TOKEN")
        .ok()
        .filter(|s| !s.is_empty());
    let railway_project_id = std::env::var("RAILWAY_PROJECT_ID")
        .ok()
        .filter(|s| !s.is_empty());
    let docker_image = std::env::var("DOCKER_IMAGE").ok().filter(|s| !s.is_empty());

    let clone_price = std::env::var("CLONE_PRICE").ok().filter(|s| !s.is_empty());
    let clone_price_amount = clone_price.as_ref().map(|p| {
        x402_gateway::config::parse_price_to_amount(p).expect("Failed to parse CLONE_PRICE")
    });
    let clone_max_children: u32 = std::env::var("CLONE_MAX_CHILDREN")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let self_url = std::env::var("RAILWAY_PUBLIC_DOMAIN")
        .ok()
        .map(|d| format!("https://{d}"))
        .or_else(|| std::env::var("SELF_URL").ok())
        .unwrap_or_else(|| format!("http://localhost:{port}"));

    if clone_price.is_some() {
        tracing::info!(
            "Clone price: {} (max children: {})",
            clone_price.as_deref().unwrap_or("?"),
            clone_max_children
        );
    }

    // ── Auto-register node endpoints ────────────────────────────────────
    let owner = std::env::var("EVM_ADDRESS").unwrap_or_default();
    if !owner.is_empty() {
        let default_clone_price = "$1.00".to_string();
        let default_clone_amount = "1000000".to_string();
        let endpoints: Vec<(String, String, String, String, String)> = vec![
            (
                "network-stats".to_string(),
                format!("{}/utils/network-stats", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Returns real-time blockchain status (block height, chain ID, gas prices).".to_string(),
            ),
            (
                "echo-ip".to_string(),
                format!("{}/utils/echo-ip", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Returns the public IP address of the caller. Useful for agent connectivity checks.".to_string(),
            ),
            (
                "headers".to_string(),
                format!("{}/utils/headers", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Returns the HTTP headers of the request as seen by the gateway.".to_string(),
            ),
            (
                "json-validator".to_string(),
                format!("{}/utils/json-validator", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Validates a JSON string. Returns 'valid: true' or an error message.".to_string(),
            ),
            (
                "hex-converter".to_string(),
                format!("{}/utils/hex-converter", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Encodes text to hex or decodes hex to text. Simple utility for agent data handling.".to_string(),
            ),
            (
                "estimate-gas".to_string(),
                format!("{}/utils/estimate-gas", self_url),
                "$0.001".to_string(),
                "1000".to_string(),
                "Estimates the gas required for a transaction. High-value for autonomous agents.".to_string(),
            ),
            (
                "chat".to_string(),
                format!("{}/soul/chat", self_url),
                "$0.01".to_string(),
                "10000".to_string(),
                "Interactive chat with the node's soul".to_string(),
            ),
            (
                "soul".to_string(),
                format!("{}/soul/status", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Soul status and recent thoughts".to_string(),
            ),
            (
                "get-nonce".to_string(),
                format!("{}/utils/get-nonce", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Retrieves the transaction count (nonce) for an address. Essential for transaction planning.".to_string(),
            ),
            (
                "get-balance".to_string(),
                format!("{}/utils/get-balance", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Retrieves the balance (native TEMPO or TIP-20) for an address. Critical for agent pre-flight checks.".to_string(),
            ),
            (
                "keccak256".to_string(),
                format!("{}/utils/keccak256", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Computes the Keccak-256 hash of the input (hex or text). A fundamental cryptographic primitive.".to_string(),
            ),
            (
                "verify-signature".to_string(),
                format!("{}/utils/verify-signature", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Verifies an Ethereum signature. Recovers the signer address and compares it to the provided address.".to_string(),
            ),
            (
                "get-transaction".to_string(),
                format!("{}/utils/get-transaction", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Retrieves transaction details by hash. Useful for verifying payment settlements and other on-chain events.".to_string(),
            ),
            (
                "get-allowance".to_string(),
                format!("{}/utils/get-allowance", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Retrieves the allowance for a spender on a token. Required before initiating token transfers.".to_string(),
            ),
            (
                "eth-call".to_string(),
                format!("{}/utils/eth-call", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Executes a read-only smart contract call (static call). Requires 'to' and 'data' (hex). Returns the execution result as a hex string.".to_string(),
            ),
            (
                "get-block".to_string(),
                format!("{}/utils/get-block", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Retrieves block details by number or hash. Returns the block header and transaction data.".to_string(),
            ),
            (
                "info".to_string(),
                format!("{}/instance/info", self_url),
                "$0.0001".to_string(),
                "100".to_string(),
                "Node identity, version, uptime".to_string(),
            ),
            (
                "clone".to_string(),
                format!("{}/clone", self_url),
                clone_price.as_deref().unwrap_or(&default_clone_price).to_string(),
                clone_price_amount.map(|a| a.to_string()).unwrap_or_else(|| default_clone_amount.clone()),
                "Orchestration service: spawns a new x402-node instance on Railway. Returns the URL of the new node.".to_string(),
            ),
        ];
        for (slug, target, price, amount, desc) in &endpoints {
            match gateway_db.create_endpoint(slug, &owner, target, price, amount, Some(desc)) {
                Ok(_) => tracing::info!(slug, "Auto-registered endpoint"),
                Err(_) => tracing::debug!(slug, "Endpoint already exists, skipping"),
            }
        }
    }

    // ── Clone orchestrator ──────────────────────────────────────────────
    let agent: Option<Arc<CloneOrchestrator>> = match (
        railway_token,
        railway_project_id,
        docker_image,
    ) {
        (Some(token), Some(project_id), Some(image)) => {
            tracing::info!("Clone orchestrator: enabled (image: {})", image);
            let railway = RailwayClient::new(token, project_id);
            let clone_config = CloneConfig {
                docker_image: image,
                rpc_url: rpc_url.clone(),
                self_url: self_url.clone(),
                max_children: clone_max_children,
            };
            Some(Arc::new(CloneOrchestrator::new(railway, clone_config)))
        }
        _ => {
            tracing::info!("Clone orchestrator: disabled (missing RAILWAY_TOKEN, RAILWAY_PROJECT_ID, or DOCKER_IMAGE)");
            None
        }
    };

    // ── Mind / Soul init (before NodeState so we can store the DB ref) ─
    let mind_enabled = x402_mind::MindConfig::is_enabled();

    // Either a Mind (dual-soul) or a single Soul
    enum SoulOrMind {
        Soul(Box<x402_soul::Soul>),
        Mind(Box<x402_mind::Mind>),
    }

    let (
        soul_db,
        soul_dormant,
        soul_or_mind,
        soul_generation,
        soul_config_for_state,
        mind_right_db,
    ) = if mind_enabled {
        match x402_mind::MindConfig::from_env() {
            Ok(mind_config) => {
                let dormant = mind_config.left.soul_config.llm_api_key.is_none();
                let generation = mind_config.left.soul_config.generation;
                let config_clone = mind_config.left.soul_config.clone();
                match x402_mind::Mind::new(mind_config) {
                    Ok(mind) => {
                        let db = mind.database().clone();
                        let right_db = mind.right_database().clone();
                        (
                            Some(db),
                            dormant,
                            Some(SoulOrMind::Mind(Box::new(mind))),
                            generation,
                            Some(config_clone),
                            Some(right_db),
                        )
                    }
                    Err(e) => {
                        tracing::warn!("Mind init failed (non-fatal): {e}");
                        (None, true, None, generation, None, None)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Mind config failed (non-fatal): {e}");
                (None, true, None, 0, None, None)
            }
        }
    } else {
        match x402_soul::SoulConfig::from_env() {
            Ok(soul_config) => {
                let dormant = soul_config.llm_api_key.is_none();
                let generation = soul_config.generation;
                let config_clone = soul_config.clone();
                match x402_soul::Soul::new(soul_config) {
                    Ok(soul) => {
                        let db = soul.database().clone();
                        (
                            Some(db),
                            dormant,
                            Some(SoulOrMind::Soul(Box::new(soul))),
                            generation,
                            Some(config_clone),
                            None,
                        )
                    }
                    Err(e) => {
                        tracing::warn!("Soul init failed (non-fatal): {e}");
                        (None, true, None, generation, None, None)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Soul config failed (non-fatal): {e}");
                (None, true, None, 0, None, None)
            }
        }
    };

    // ── Node state ──────────────────────────────────────────────────────
    let started_at = chrono::Utc::now();

    // Build observer early so we can share it between NodeState and soul spawn
    let soul_observer: Option<std::sync::Arc<dyn x402_soul::NodeObserver>> =
        if soul_or_mind.is_some() || soul_config_for_state.is_some() {
            Some(soul_observer::NodeObserverImpl::new(
                gateway_state.clone(),
                identity.clone(),
                soul_generation,
                started_at,
                db_path.clone(),
            ))
        } else {
            None
        };

    let node_state = NodeState {
        gateway: gateway_state,
        identity: identity.clone(),
        agent,
        started_at,
        db_path: db_path.clone(),
        clone_price,
        clone_price_amount,
        clone_max_children,
        soul_db,
        soul_dormant,
        soul_config: soul_config_for_state,
        soul_observer: soul_observer.clone(),
        mind_enabled,
        mind_right_db,
    };

    let node_data = web::Data::new(node_state.clone());
    let gateway_data = web::Data::new(node_state.gateway.clone());
    let facilitator_data = facilitator_state.map(web::Data::from);

    // ── Soul/Mind spawn (after NodeState so we can build the observer) ─
    if let Some(soul_or_mind) = soul_or_mind {
        if let Some(observer) = soul_observer {
            match soul_or_mind {
                SoulOrMind::Mind(mind) => {
                    (*mind).spawn(observer);
                    tracing::info!(
                        dormant = node_state.soul_dormant,
                        generation = soul_generation,
                        "Mind spawned (dual-soul: left + right + callosum)"
                    );
                }
                SoulOrMind::Soul(soul) => {
                    (*soul).spawn(observer);
                    tracing::info!(
                        dormant = node_state.soul_dormant,
                        generation = soul_generation,
                        "Soul spawned"
                    );
                }
            }
        }
    }

    // ── Background tasks ────────────────────────────────────────────────
    if let Some(ref id) = identity {
        let rpc = rpc_url.clone();
        let addr = id.address;
        // Faucet funding (best-effort)
        tokio::spawn(async move {
            if let Err(e) = x402_identity::request_faucet_funds(&rpc, addr).await {
                tracing::warn!("Faucet funding failed (non-fatal): {e}");
            }
        });

        // Parent registration (if PARENT_URL set)
        if let Some(ref parent_url) = id.parent_url {
            let parent = parent_url.clone();
            let id_clone = id.clone();
            let url = self_url.clone();
            tokio::spawn(async move {
                if let Err(e) = x402_identity::register_with_parent(&parent, &id_clone, &url).await
                {
                    tracing::warn!("Parent registration failed (non-fatal): {e}");
                }
            });
        }
    }

    // ── Background: health probe + version check + auto-redeploy ───────
    if node_state.agent.is_some() {
        let version_check_state = node_state.clone();
        tokio::spawn(async move {
            // Wait for children to finish booting
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            let probe_interval_secs: u64 = std::env::var("HEALTH_PROBE_INTERVAL_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300);
            let probe_interval = std::time::Duration::from_secs(probe_interval_secs);

            let parent_version = env!("CARGO_PKG_VERSION");
            let parent_build = {
                let compile_time = env!("GIT_SHA");
                if compile_time != "dev" {
                    compile_time.to_string()
                } else {
                    std::env::var("RAILWAY_GIT_COMMIT_SHA").unwrap_or_else(|_| "dev".to_string())
                }
            };

            let http = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default();

            let agent = match version_check_state.agent.as_ref() {
                Some(a) => a,
                None => {
                    tracing::warn!("Health probe: no agent available, exiting");
                    return;
                }
            };

            tracing::info!(
                interval_secs = probe_interval_secs,
                "Health probe loop started (parent v{parent_version} build={parent_build})"
            );

            loop {
                let children = match rusqlite::Connection::open(&version_check_state.db_path) {
                    Ok(conn) => db::query_children_active(&conn).unwrap_or_default(),
                    Err(e) => {
                        tracing::warn!("Version check: failed to open db: {e}");
                        tokio::time::sleep(probe_interval).await;
                        continue;
                    }
                };

                // Children with a URL that we can probe (running OR stuck deploying)
                let probeworthy: Vec<_> = children
                    .into_iter()
                    .filter(|c| {
                        c.url.is_some() && (c.status == "running" || c.status == "deploying")
                    })
                    .collect();

                if probeworthy.is_empty() {
                    tracing::debug!("Version check: no children to check");
                    tokio::time::sleep(probe_interval).await;
                    continue;
                }

                for child in &probeworthy {
                    let url = match child.url.as_ref() {
                        Some(u) => u,
                        None => continue,
                    };

                    // Probe /health to see if the child is actually alive
                    let health_url = format!("{url}/health");
                    let health_json = match http.get(&health_url).send().await {
                        Ok(resp) => match resp.json::<serde_json::Value>().await {
                            Ok(json) => json,
                            Err(e) => {
                                tracing::warn!(
                                    instance_id = %child.instance_id,
                                    error = %e,
                                    "Health probe: failed to parse response"
                                );

                                // Mark children returning bad health responses as failed after timeout
                                let age_secs = chrono::Utc::now().timestamp() - child.created_at;
                                let stale = match child.status.as_str() {
                                    "deploying" => age_secs > 600, // 10 min
                                    "running" => age_secs > 300,   // 5 min
                                    _ => false,
                                };
                                if stale {
                                    tracing::info!(
                                        instance_id = %child.instance_id,
                                        status = %child.status,
                                        age_secs = age_secs,
                                        "Marking child with bad health response as failed"
                                    );
                                    let _ = db::mark_child_failed(
                                        &version_check_state.gateway.db,
                                        &child.instance_id,
                                    );
                                }

                                continue;
                            }
                        },
                        Err(e) => {
                            tracing::warn!(
                                instance_id = %child.instance_id,
                                error = %e,
                                "Health probe: failed to reach child"
                            );

                            // Mark unreachable children as failed if they've been around long enough
                            let age_secs = chrono::Utc::now().timestamp() - child.created_at;
                            let stale = match child.status.as_str() {
                                "deploying" => age_secs > 600, // 10 min for deploying
                                "running" => age_secs > 300, // 5 min for running (was reachable before)
                                _ => false,
                            };

                            if stale {
                                tracing::info!(
                                    instance_id = %child.instance_id,
                                    status = %child.status,
                                    age_secs = age_secs,
                                    "Marking unreachable child as failed"
                                );
                                if let Err(mark_err) = db::mark_child_failed(
                                    &version_check_state.gateway.db,
                                    &child.instance_id,
                                ) {
                                    tracing::warn!(
                                        instance_id = %child.instance_id,
                                        error = %mark_err,
                                        "Failed to mark unreachable child as failed"
                                    );
                                }
                            }

                            continue;
                        }
                    };

                    let child_version = health_json
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let child_build = health_json
                        .get("build")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // ── Fix stuck "deploying" children ──────────────────────
                    // Child is alive but parent DB still says "deploying".
                    // Fetch its identity and promote to "running".
                    if child.status == "deploying" {
                        tracing::info!(
                            instance_id = %child.instance_id,
                            "Stuck deploying child is alive — recovering status"
                        );

                        // Try to get the child's address from its /instance/info
                        let mut child_address: Option<String> = None;
                        let info_url = format!("{url}/instance/info");
                        if let Ok(resp) = http.get(&info_url).send().await {
                            if let Ok(info) = resp.json::<serde_json::Value>().await {
                                child_address = info
                                    .get("identity")
                                    .and_then(|id| id.get("address"))
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                            }
                        }

                        if let Err(e) = db::update_child(
                            &version_check_state.gateway.db,
                            &child.instance_id,
                            child_address.as_deref(),
                            None, // keep existing URL
                            Some("running"),
                        ) {
                            tracing::warn!(
                                instance_id = %child.instance_id,
                                error = %e,
                                "Failed to recover stuck child status"
                            );
                        } else {
                            tracing::info!(
                                instance_id = %child.instance_id,
                                address = ?child_address,
                                "Child status recovered to running"
                            );
                        }
                    }

                    // ── Build hash check & auto-redeploy ────────────────────
                    // Compare build hashes (git SHA) for exact match. Fall back
                    // to semver if the child doesn't report a build hash yet
                    // (old image without the `build` field).
                    let up_to_date =
                        if !child_build.is_empty() && child_build != "dev" && parent_build != "dev"
                        {
                            child_build == parent_build
                        } else {
                            child_version == parent_version
                        };

                    if up_to_date {
                        tracing::debug!(
                            instance_id = %child.instance_id,
                            version = %child_version,
                            build = %child_build,
                            "Child is up to date"
                        );
                        continue;
                    }

                    tracing::info!(
                        instance_id = %child.instance_id,
                        child_version = %child_version,
                        child_build = %child_build,
                        parent_version = %parent_version,
                        parent_build = %parent_build,
                        "Child build mismatch — triggering redeploy"
                    );

                    let service_id = match child.railway_service_id.as_ref() {
                        Some(id) => id,
                        None => {
                            tracing::warn!(
                                instance_id = %child.instance_id,
                                "Cannot redeploy: no Railway service ID"
                            );
                            continue;
                        }
                    };

                    match agent.redeploy_clone(service_id).await {
                        Ok(_) => {
                            if let Err(e) = db::update_child_status(
                                &version_check_state.gateway.db,
                                &child.instance_id,
                                "deploying",
                            ) {
                                tracing::warn!(
                                    instance_id = %child.instance_id,
                                    error = %e,
                                    "Failed to update status after auto-redeploy"
                                );
                            }
                            tracing::info!(
                                instance_id = %child.instance_id,
                                "Auto-redeploy triggered"
                            );
                        }
                        Err(e) => {
                            let err_str = format!("{e}");
                            // If Railway says the service doesn't exist, mark child as failed
                            if err_str.contains("not found") || err_str.contains("Not Found") {
                                tracing::warn!(
                                    instance_id = %child.instance_id,
                                    error = %e,
                                    "Service not found on Railway — marking child as failed"
                                );
                                let _ = db::mark_child_failed(
                                    &version_check_state.gateway.db,
                                    &child.instance_id,
                                );
                            } else {
                                tracing::warn!(
                                    instance_id = %child.instance_id,
                                    error = %e,
                                    "Auto-redeploy failed (non-fatal)"
                                );
                            }
                        }
                    }
                }

                tracing::info!("Health probe cycle complete");
                tokio::time::sleep(probe_interval).await;
            } // end loop
        });
    }

    // ── Rate limiter ────────────────────────────────────────────────────
    let governor_conf = GovernorConfigBuilder::default()
        .requests_per_minute(rate_limit_rpm as u64)
        .finish()
        .expect("Failed to create rate limiter config");

    if let Some(ref dir) = spa_dir {
        tracing::info!("Serving SPA from: {}", dir);
    }

    // ── HTTP server ─────────────────────────────────────────────────────
    HttpServer::new(move || {
        let cors = x402_gateway::cors::build_cors(&allowed_origins);

        let mut app = App::new()
            .app_data(gateway_data.clone())
            .app_data(node_data.clone())
            .app_data(web::PayloadConfig::new(10 * 1024 * 1024))
            .wrap(Logger::default())
            .wrap(cors)
            .wrap(Governor::new(&governor_conf))
            // Gateway routes
            .configure(x402_gateway::routes::health::configure)
            .configure(x402_gateway::routes::register::configure)
            .configure(x402_gateway::routes::endpoints::configure)
            .configure(x402_gateway::routes::analytics::configure)
            .configure(x402_gateway::routes::gateway::configure)
            // Node routes (identity, clone, soul)
            .configure(crate::routes::instance::configure)
            .configure(crate::routes::clone::configure)
            .configure(crate::routes::soul::configure)
            .configure(crate::routes::utils::configure);

        // Mount facilitator HTTP routes if embedded
        if let Some(ref fac_data) = facilitator_data {
            app = app.service(
                web::scope("/facilitator")
                    .app_data(fac_data.clone())
                    .service(x402_facilitator::routes::supported)
                    .service(x402_facilitator::routes::verify_and_settle),
            );
        }

        // Serve SPA static files last (catch-all) if configured
        if let Some(ref dir) = spa_dir {
            let index_path = format!("{}/index.html", dir);
            app = app.service(
                actix_files::Files::new("/", dir)
                    .index_file("index.html")
                    .default_handler(web::to(move || {
                        let path = index_path.clone();
                        async move { actix_files::NamedFile::open_async(path).await }
                    })),
            );
        }

        app
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
