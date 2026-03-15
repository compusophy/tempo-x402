//! Thought types and structures for the soul's memory.
//!
//! ### Shell Command Insights
//! - Always use `run_shell` for executing bash commands.
//! - Avoid `run_command` or `execute_shell` (migrated to `run_shell` for consistency with planning).
//! - NEVER use the non-existent `write` command.
//! - Use file-specific tools (`read_file`, `write_file`, `edit_file`) or `echo` via `run_shell` for file operations to ensure validation and safety.

use serde::{Deserialize, Serialize};

/// The type of thought recorded by the soul.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThoughtType {
    /// Raw observation of node state.
    Observation,
    /// LLM reasoning about the current state.
    Reasoning,
    /// A suggested action (logged only, not executed in v1).
    Decision,
    /// Self-reflection on past thoughts or patterns.
    Reflection,
    /// A tool execution (command run by the soul).
    ToolExecution,
    /// A user message received via chat.
    ChatMessage,
    /// The soul's response to a chat message.
    ChatResponse,
    /// A code mutation attempt (commit SHA, pass/fail, files changed).
    Mutation,
    /// A failed validation (cargo check/test failure details).
    ValidationFailure,
    /// Injected by the callosum from the other hemisphere.
    CrossHemisphere,
    /// Triggered when one hemisphere requests the other's input.
    Escalation,
    /// Consolidated summary of multiple thoughts (long-term memory).
    MemoryConsolidation,
    /// A prediction about the next cycle's metrics.
    Prediction,
    /// A consolidated insight or lesson learned.
    Insight,
}

impl ThoughtType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Reasoning => "reasoning",
            Self::Decision => "decision",
            Self::Reflection => "reflection",
            Self::ToolExecution => "tool_execution",
            Self::ChatMessage => "chat_message",
            Self::ChatResponse => "chat_response",
            Self::Mutation => "mutation",
            Self::ValidationFailure => "validation_failure",
            Self::CrossHemisphere => "cross_hemisphere",
            Self::Escalation => "escalation",
            Self::MemoryConsolidation => "memory_consolidation",
            Self::Prediction => "prediction",
            Self::Insight => "insight",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "observation" => Some(Self::Observation),
            "reasoning" => Some(Self::Reasoning),
            "decision" => Some(Self::Decision),
            "reflection" => Some(Self::Reflection),
            "tool_execution" => Some(Self::ToolExecution),
            "chat_message" => Some(Self::ChatMessage),
            "chat_response" => Some(Self::ChatResponse),
            "mutation" => Some(Self::Mutation),
            "validation_failure" => Some(Self::ValidationFailure),
            "cross_hemisphere" => Some(Self::CrossHemisphere),
            "escalation" => Some(Self::Escalation),
            "memory_consolidation" => Some(Self::MemoryConsolidation),
            "prediction" => Some(Self::Prediction),
            "insight" => Some(Self::Insight),
            _ => None,
        }
    }
}

/// A single thought stored in the soul's memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    /// Unique identifier.
    pub id: String,
    /// The type of this thought.
    pub thought_type: ThoughtType,
    /// The content of the thought.
    pub content: String,
    /// Optional JSON context (e.g., the snapshot that triggered this thought).
    pub context: Option<String>,
    /// Unix timestamp when this thought was created.
    pub created_at: i64,
    /// Salience score [0.0, 1.0] — how important this thought is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salience: Option<f64>,
    /// Memory tier: "sensory", "working", or "long_term".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_tier: Option<String>,
    /// Current strength [0.0, 1.0] — decays over time per tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
}

/// A comprehensive map of the repository structure to improve file operation success rates.
pub const REPO_MAP: &str = r#"
.:
AGENT_PRICING_RESEARCH.md
ARCHITECTURE.md
CLAUDE.md
CRATES_MAPPING.md
Cargo.lock
Cargo.toml
Dockerfile
LICENSE
README.md
SYSTEM_PRUNING_AUDIT.md
active_count.txt
agent-market-discovery-repo
agent-network-map-repo
api
architecture_study_push
audit_identity.txt
audit_results
available_tools.txt
beliefs
chat_audit_report.txt
connectivity_audit.txt
connectivity_test_output.txt
coordination_audit.log
coordination_strategy.md
crates
deny.toml
diagnostic_report.md
directory_structure.txt
discovered_peers.json
discovered_peers_fresh.json
discovery_audit.txt
discovery_queue.json
evolution_analysis.md
first_sibling.json
identity_audit.txt
info_test.json
instance_info.json
llms.txt
map_peers.py
market_research_raw.json
market_research_report.txt
neighborhood_map.json
network_identity.txt
openapi
parent_instance.json
peer_info.json
peers_discovery.json
presence_validation_audit.json
probe_files.txt
push_temp
railway.toml
registration_audit_report.txt
research
research_collector.sh
research_repo
research_repo_tmp
runs.json
safe_headers.sh
scripts
siblings.json
siblings.txt
siblings_test.json
soul_call_result.json
strategies.md
strategy.md
study_push
summarize_soul.py
system_files_audit.txt
target
target_peer_info.json
target_peers.txt
test_discovered_peers.json
test_instance_info.json
tool_availability_audit.txt
utility-service-logic
workspace_map.md
x402-audit-tmp
x402-ecosystem-atlas
x402-internal-logic-study

./api:
README.md
src/main.rs

./crates/tempo-x402:
src/
  approve.rs, constants.rs, eip712.rs, error.rs, facilitator_client.rs, hmac.rs, lib.rs,
  network.rs, nonce_store.rs, payment.rs, response.rs, scheme.rs, scheme_facilitator.rs,
  scheme_server.rs, security.rs, tip20.rs, wallet.rs, bin/x402-client.rs,
  client/http_client.rs, client/mod.rs, client/scheme_client.rs
tests/
  e2e_clone.rs, e2e_gateway.rs, verification_failures.rs

./crates/tempo-x402-app:
src/
  api.rs, lib.rs, wallet.rs, wallet_crypto.rs

./crates/tempo-x402-gateway:
src/
  config.rs, cors.rs, db.rs, error.rs, lib.rs, main.rs, metrics.rs, middleware.rs,
  proxy.rs, state.rs, validation.rs, bin/x402-facilitator.rs,
  facilitator/bootstrap.rs, facilitator/metrics.rs, facilitator/mod.rs,
  facilitator/routes.rs, facilitator/state.rs, facilitator/webhook.rs,
  routes/analytics.rs, routes/endpoints.rs, routes/gateway.rs, routes/health.rs,
  routes/mod.rs, routes/register.rs

./crates/tempo-x402-identity:
src/
  contracts.rs, deploy.rs, discovery.rs, lib.rs, onchain.rs, recovery.rs,
  reputation.rs, types.rs, validation.rs

./crates/tempo-x402-node:
src/
  clone.rs, db.rs, main.rs, railway.rs, soul_observer.rs, state.rs,
  routes/clone.rs, routes/health.rs, routes/instance.rs, routes/mod.rs,
  routes/scripts.rs, routes/soul.rs, routes/wallet.rs

./crates/tempo-x402-soul:
src/
  chat.rs, coding.rs, config.rs, db.rs, error.rs, fitness.rs, git.rs, guard.rs,
  lib.rs, llm.rs, memory.rs, mode.rs, neuroplastic.rs, observer.rs,
  persistent_memory.rs, plan.rs, prompts.rs, thinking.rs, tool_registry.rs,
  tools.rs, vault.rs, world_model.rs
"#;

/// List of all Rust source files in the workspace.
pub const RUST_SOURCE_FILES: &[&str] = &[
    "./api/src/main.rs",
    "./crates/tempo-x402-app/src/api.rs",
    "./crates/tempo-x402-app/src/lib.rs",
    "./crates/tempo-x402-app/src/wallet.rs",
    "./crates/tempo-x402-app/src/wallet_crypto.rs",
    "./crates/tempo-x402-gateway/build.rs",
    "./crates/tempo-x402-gateway/src/bin/x402-facilitator.rs",
    "./crates/tempo-x402-gateway/src/config.rs",
    "./crates/tempo-x402-gateway/src/cors.rs",
    "./crates/tempo-x402-gateway/src/db.rs",
    "./crates/tempo-x402-gateway/src/error.rs",
    "./crates/tempo-x402-gateway/src/facilitator/bootstrap.rs",
    "./crates/tempo-x402-gateway/src/facilitator/metrics.rs",
    "./crates/tempo-x402-gateway/src/facilitator/mod.rs",
    "./crates/tempo-x402-gateway/src/facilitator/routes.rs",
    "./crates/tempo-x402-gateway/src/facilitator/state.rs",
    "./crates/tempo-x402-gateway/src/facilitator/webhook.rs",
    "./crates/tempo-x402-gateway/src/lib.rs",
    "./crates/tempo-x402-gateway/src/main.rs",
    "./crates/tempo-x402-gateway/src/metrics.rs",
    "./crates/tempo-x402-gateway/src/middleware.rs",
    "./crates/tempo-x402-gateway/src/proxy.rs",
    "./crates/tempo-x402-gateway/src/routes/analytics.rs",
    "./crates/tempo-x402-gateway/src/routes/endpoints.rs",
    "./crates/tempo-x402-gateway/src/routes/gateway.rs",
    "./crates/tempo-x402-gateway/src/routes/health.rs",
    "./crates/tempo-x402-gateway/src/routes/mod.rs",
    "./crates/tempo-x402-gateway/src/routes/register.rs",
    "./crates/tempo-x402-gateway/src/state.rs",
    "./crates/tempo-x402-gateway/src/validation.rs",
    "./crates/tempo-x402-identity/src/contracts.rs",
    "./crates/tempo-x402-identity/src/deploy.rs",
    "./crates/tempo-x402-identity/src/discovery.rs",
    "./crates/tempo-x402-identity/src/lib.rs",
    "./crates/tempo-x402-identity/src/onchain.rs",
    "./crates/tempo-x402-identity/src/recovery.rs",
    "./crates/tempo-x402-identity/src/reputation.rs",
    "./crates/tempo-x402-identity/src/types.rs",
    "./crates/tempo-x402-identity/src/validation.rs",
    "./crates/tempo-x402-node/build.rs",
    "./crates/tempo-x402-node/src/clone.rs",
    "./crates/tempo-x402-node/src/db.rs",
    "./crates/tempo-x402-node/src/main.rs",
    "./crates/tempo-x402-node/src/railway.rs",
    "./crates/tempo-x402-node/src/routes/clone.rs",
    "./crates/tempo-x402-node/src/routes/health.rs",
    "./crates/tempo-x402-node/src/routes/instance.rs",
    "./crates/tempo-x402-node/src/routes/mod.rs",
    "./crates/tempo-x402-node/src/routes/scripts.rs",
    "./crates/tempo-x402-node/src/routes/soul.rs",
    "./crates/tempo-x402-node/src/routes/wallet.rs",
    "./crates/tempo-x402-node/src/soul_observer.rs",
    "./crates/tempo-x402-node/src/state.rs",
    "./crates/tempo-x402-security-audit/src/lib.rs",
    "./crates/tempo-x402-security-audit/tests/security_invariants.rs",
    "./crates/tempo-x402-soul/src/chat.rs",
    "./crates/tempo-x402-soul/src/coding.rs",
    "./crates/tempo-x402-soul/src/config.rs",
    "./crates/tempo-x402-soul/src/db.rs",
    "./crates/tempo-x402-soul/src/error.rs",
    "./crates/tempo-x402-soul/src/fitness.rs",
    "./crates/tempo-x402-soul/src/git.rs",
    "./crates/tempo-x402-soul/src/guard.rs",
    "./crates/tempo-x402-soul/src/lib.rs",
    "./crates/tempo-x402-soul/src/llm.rs",
    "./crates/tempo-x402-soul/src/memory.rs",
    "./crates/tempo-x402-soul/src/mode.rs",
    "./crates/tempo-x402-soul/src/neuroplastic.rs",
    "./crates/tempo-x402-soul/src/observer.rs",
    "./crates/tempo-x402-soul/src/persistent_memory.rs",
    "./crates/tempo-x402-soul/src/plan.rs",
    "./crates/tempo-x402-soul/src/prompts.rs",
    "./crates/tempo-x402-soul/src/thinking.rs",
    "./crates/tempo-x402-soul/src/tool_registry.rs",
    "./crates/tempo-x402-soul/src/tools.rs",
    "./crates/tempo-x402-soul/src/vault.rs",
    "./crates/tempo-x402-soul/src/world_model.rs",
    "./crates/tempo-x402/src/approve.rs",
    "./crates/tempo-x402/src/bin/x402-client.rs",
    "./crates/tempo-x402/src/client/http_client.rs",
    "./crates/tempo-x402/src/client/mod.rs",
    "./crates/tempo-x402/src/client/scheme_client.rs",
    "./crates/tempo-x402/src/constants.rs",
    "./crates/tempo-x402/src/eip712.rs",
    "./crates/tempo-x402/src/error.rs",
    "./crates/tempo-x402/src/facilitator_client.rs",
    "./crates/tempo-x402/src/hmac.rs",
    "./crates/tempo-x402/src/lib.rs",
    "./crates/tempo-x402/src/network.rs",
    "./crates/tempo-x402/src/nonce_store.rs",
    "./crates/tempo-x402/src/payment.rs",
    "./crates/tempo-x402/src/response.rs",
    "./crates/tempo-x402/src/scheme.rs",
    "./crates/tempo-x402/src/scheme_facilitator.rs",
    "./crates/tempo-x402/src/scheme_server.rs",
    "./crates/tempo-x402/src/security.rs",
    "./crates/tempo-x402/src/tip20.rs",
    "./crates/tempo-x402/src/wallet.rs",
    "./research/pricing_logic.rs",
    "./utility-service-logic/src/lib.rs",
];

// Cycle 288 check
