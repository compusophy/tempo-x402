# System Map: Tempo-x402 Workspace

This `system_map.md` provides a comprehensive overview of the `tempo-x402` workspace, documenting the directory structure, each crate's purpose, and its primary dependencies.

## Directory Structure

```text
.
├── ARCHITECTURE.md
├── CLAUDE.md
├── Cargo.lock
├── Cargo.toml
├── Dockerfile
├── EVOLUTION_ARCHITECTURE.md
├── INITIAL_DIAGNOSTIC.md
├── LICENSE
├── NEW_REPO_STRATEGY.md
├── README.md
├── RESEARCH.md
├── ROADMAP.md
├── STRATEGY.md
├── SYSTEM_PRUNING_AUDIT.md
├── active_count.txt
├── api
│   ├── README.md
│   └── src
│       └── main.rs
├── audit_identity.txt
├── audit_results
│   ├── deep_system_audit.md
│   └── endpoint_audit.md
├── available_tools.txt
├── benchmarking_request.txt
├── chat_audit_report.txt
├── connectivity_audit.txt
├── connectivity_test_output.txt
├── crates
│   ├── tempo-x402
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── src
│   │   │   ├── approve.rs
│   │   │   ├── bin
│   │   │   │   └── x402-client.rs
│   │   │   ├── client
│   │   │   │   ├── http_client.rs
│   │   │   │   ├── mod.rs
│   │   │   │   └── scheme_client.rs
│   │   │   ├── constants.rs
│   │   │   ├── eip712.rs
│   │   │   ├── error.rs
│   │   │   ├── facilitator_client.rs
│   │   │   ├── hmac.rs
│   │   │   ├── lib.rs
│   │   │   ├── network.rs
│   │   │   ├── nonce_store.rs
│   │   │   ├── payment.rs
│   │   │   ├── response.rs
│   │   │   ├── scheme.rs
│   │   │   ├── scheme_facilitator.rs
│   │   │   ├── scheme_server.rs
│   │   │   ├── security.rs
│   │   │   ├── tip20.rs
│   │   │   └── wallet.rs
│   │   └── tests
│   │       ├── e2e_clone.rs
│   │       ├── e2e_gateway.rs
│   │       └── verification_failures.rs
│   ├── tempo-x402-app
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── Trunk.toml
│   │   ├── index.html
│   │   ├── src
│   │   │   ├── api.rs
│   │   │   ├── lib.rs
│   │   │   ├── wallet.rs
│   │   │   └── wallet_crypto.rs
│   │   ├── style.css
│   │   └── vercel.json
│   ├── tempo-x402-gateway
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── README.md
│   │   ├── build.rs
│   │   └── src
│   │       ├── bin
│   │       │   └── x402-facilitator.rs
│   │       ├── config.rs
│   │       ├── cors.rs
│   │       ├── db.rs
│   │       ├── error.rs
│   │       ├── facilitator
│   │       │   ├── bootstrap.rs
│   │       │   ├── metrics.rs
│   │       │   ├── mod.rs
│   │       │   ├── routes.rs
│   │       │   ├── state.rs
│   │       │   └── webhook.rs
│   │       ├── lib.rs
│   │       ├── main.rs
│   │       ├── metrics.rs
│   │       ├── middleware.rs
│   │       ├── proxy.rs
│   │       ├── routes
│   │       │   ├── analytics.rs
│   │       │   ├── endpoints.rs
│   │       │   ├── gateway.rs
│   │       │   ├── health.rs
│   │       │   ├── mod.rs
│   │       │   └── register.rs
│   │       ├── state.rs
│   │       └── validation.rs
│   ├── tempo-x402-identity
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   └── src
│   │       ├── contracts.rs
│   │       ├── deploy.rs
│   │       ├── discovery.rs
│   │       ├── lib.rs
│   │       ├── onchain.rs
│   │       ├── recovery.rs
│   │       ├── reputation.rs
│   │       ├── types.rs
│   │       └── validation.rs
│   ├── tempo-x402-node
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── build.rs
│   │   └── src
│   │       ├── clone.rs
│   │       ├── db.rs
│   │       ├── main.rs
│   │       ├── railway.rs
│   │       ├── routes
│   │       │   ├── clone.rs
│   │       │   ├── health.rs
│   │       │   ├── instance.rs
│   │       │   ├── mod.rs
│   │       │   ├── scripts.rs
│   │       │   ├── soul.rs
│   │       │   └── wallet.rs
│   │       ├── soul_observer.rs
│   │       └── state.rs
│   ├── tempo-x402-security-audit
│   │   ├── CLAUDE.md
│   │   ├── Cargo.toml
│   │   ├── src
│   │   │   └── lib.rs
│   │   └── tests
│   │       └── security_invariants.rs
│   └── tempo-x402-soul
│       ├── CLAUDE.md
│       ├── Cargo.toml
│       └── src
│           ├── chat.rs
│           ├── coding.rs
│           ├── config.rs
│           ├── db.rs
│           ├── error.rs
│           ├── fitness.rs
│           ├── git.rs
│           ├── guard.rs
│           ├── lib.rs
│           ├── llm.rs
│           ├── memory.rs
│           ├── mode.rs
│           ├── neuroplastic.rs
│           ├── observer.rs
│           ├── persistent_memory.rs
│           ├── plan.rs
│           ├── prompts.rs
│           ├── thinking.rs
│           ├── tool_registry.rs
│           ├── tools.rs
│           ├── vault.rs
│           └── world_model.rs
├── deny.toml
├── diagnostic_report.md
├── directory_structure.txt
├── discovered_peers.json
├── discovered_peers_fresh.json
├── discovery_audit.txt
├── discovery_queue.json
├── first_sibling.json
├── identity_audit.txt
├── info_test.json
├── instance_info.json
├── llms.txt
├── map_peers.py
├── market_research_raw.json
├── market_research_report.txt
├── neighborhood_map.json
├── network_identity.txt
├── openapi
│   ├── facilitator.yaml
│   ├── gateway.yaml
│   └── openapi.yaml
├── parent_instance.json
├── peer_info.json
├── peers_discovery.json
├── peers_raw.json
├── performance_report_final.md
├── presence_validation_audit.json
├── probe_files.txt
├── railway.toml
├── registration_audit_report.txt
├── registry.json
├── research_collector.sh
├── research_hub
├── research_metadata.sh
├── runs.json
├── safe_headers.sh
├── scripts
├── siblings.json
├── siblings.txt
├── siblings_test.json
├── summarize_soul.py
├── system_files_audit.txt
├── system_map.md
├── target
├── target_peers.txt
├── temp_audit_repo
├── temp_market_strategy
├── temp_registry
├── temp_registry_repo
├── temp_system_design
├── test_discovered_peers.json
├── test_file.txt
├── test_instance_info.json
├── tool_availability_audit.txt
├── x402-diagnostic-standard
├── x402-discovery-mesh
├── x402-economic-growth-research
├── x402-network-analyzer
├── x402-network-health
├── x402-protocol-research
├── x402_economic_protocol_spec.md
├── x402_growth_strategy_backup.md
└── x402_protocol_readme.md
```

## Workspace Overview

The workspace consists of seven primary crates, ranging from core protocol libraries to user-facing applications and autonomous agent components.

### 1. `tempo-x402` (Core Library)
*   **Path:** `crates/tempo-x402`
*   **Purpose:** The foundation of the system. It implements the x402 payment protocol, including EIP-712 signing, TIP-20 token support, wallet management, and client-side request handling.
*   **Key Dependencies:**
    *   `alloy`: Ethereum/Tempo blockchain interactions.
    *   `serde`, `serde_json`: Serialization.
    *   `reqwest`: HTTP client for network requests.
    *   `rusqlite`: Local persistence for state management.
    *   `tokio`: Async runtime.

### 2. `tempo-x402-app` (Web Application)
*   **Path:** `crates/tempo-x402-app`
*   **Purpose:** A "kitchen sink" web application built with Leptos. It serves as a demo for all x402 payment features, designed to run in the browser via WebAssembly (WASM).
*   **Key Dependencies:**
    *   `leptos`: Full-stack web framework (CSR mode).
    *   `wasm-bindgen`: Rust/JS interoperability.
    *   `web-sys`: Browser API bindings.
    *   `tempo-x402`: Core library with WASM and demo features enabled.

### 3. `tempo-x402-gateway` (API Gateway & Facilitator)
*   **Path:** `crates/tempo-x402-gateway`
*   **Purpose:** An API gateway that acts as a proxy with per-request payment rails. It includes an embedded "facilitator" for handling on-chain settlement and request verification.
*   **Key Dependencies:**
    *   `actix-web`: High-performance web server.
    *   `actix-governor`: Rate limiting.
    *   `tempo-x402`: Core protocol implementation.
    *   `prometheus`: Metrics and monitoring.
    *   `alloy`: On-chain verification.

### 4. `tempo-x402-identity` (Identity Management)
*   **Path:** `crates/tempo-x402-identity`
*   **Purpose:** Manages identities for x402 instances. This includes wallet generation, persistent storage of credentials, faucet funding, and ERC-8004 agent identity management.
*   **Key Dependencies:**
    *   `alloy`: Smart contract interactions for ERC-8004.
    *   `uuid`: Unique identifier generation for agent instances.
    *   `tempo-x402`: Protocol-level identity support.

### 5. `tempo-x402-node` (Node Orchestrator)
*   **Path:** `crates/tempo-x402-node`
*   **Purpose:** The top-level orchestrator for self-deploying x402 nodes. It bootstraps the gateway, manages identity, and handles autonomous node orchestration.
*   **Key Dependencies:**
    *   `tempo-x402-gateway`: Core proxy functionality.
    *   `tempo-x402-identity`: Identity bootstrapping.
    *   `tempo-x402-soul`: (Optional) Agentic logic integration.
    *   `actix-web`: Control plane API.

### 6. `tempo-x402-security-audit` (Security Invariants)
*   **Path:** `crates/tempo-x402-security-audit`
*   **Purpose:** A test-only crate dedicated to enforcing security invariants across the workspace. It runs static analysis and regex-based checks to ensure sensitive patterns (like private key handling) follow safety protocols.
*   **Key Dependencies:**
    *   `walkdir`: File system traversal for scanning code.
    *   `regex`: Pattern matching for security audits.

### 7. `tempo-x402-soul` (Agentic Intelligence)
*   **Path:** `crates/tempo-x402-soul`
*   **Purpose:** Provides an "agentic soul" for x402 nodes, implementing an observe-think-record loop powered by LLMs (Gemini). It allows nodes to act autonomously based on environmental feedback.
*   **Key Dependencies:**
    *   `tempo-x402-identity`: For signing agent actions.
    *   `reqwest`: For communicating with LLM APIs.
    *   `rusqlite`: Persistent memory for the soul's thoughts and actions.
    *   `alloy`: Interaction with blockchain identity and payments.
