# Workspace Map: tempo-x402

## Workspace Structure
The `tempo-x402` project is organized as a Rust workspace with several specialized crates. It implements an autonomous agent node powered by the x402 (HTTP 402) protocol on the Tempo blockchain.

### Primary Crates
- **`tempo-x402-node`**: The main executable. Orchestrates the agent's lifecycle, HTTP server, and execution loop.
- **`tempo-x402-soul`**: The "brain" of the agent. Contains logic for autonomous cognition, memory, thinking, and self-improvement (coding).
- **`tempo-x402`**: Core protocol library for x402 payments on Tempo.
- **`tempo-x402-gateway`**: Gateway service for payment routing and network facilitation.
- **`tempo-x402-identity`**: Identity management for agents and the network.
- **`tempo-x402-app`**: A potential frontend or high-level application layer.
- **`tempo-x402-security-audit`**: Utilities for security auditing and validation.

## Detailed Mapping: `tempo-x402`
The `tempo-x402` crate implements the core protocol logic.

### `crates/tempo-x402/src`
- `approve.rs`: Payment approval logic.
- `bin/`:
    - `x402-client.rs`: Command-line client for x402.
- `client/`:
    - `http_client.rs`: Implementation of the HTTP client for x402.
    - `mod.rs`: Module root for client.
    - `scheme_client.rs`: Client-side protocol scheme.
- `constants.rs`: Global protocol constants.
- `eip712.rs`: EIP-712 signing support.
- `error.rs`: Protocol error definitions.
- `facilitator_client.rs`: Client implementation for facilitators.
- `hmac.rs`: HMAC-based security utilities.
- `lib.rs`: Library entry point.
- `network.rs`: Network and chain configuration.
- `nonce_store.rs`: Storage and validation of nonces.
- `payment.rs`: Core payment data structures.
- `response.rs`: Standardized protocol responses.
- `scheme.rs`: Abstract payment scheme definitions.
- `scheme_facilitator.rs`: Facilitator-specific scheme logic.
- `scheme_server.rs`: Server-side protocol implementation.
- `security.rs`: General security and validation logic.
- `tip20.rs`: TIP-20 token interactions.
- `wallet.rs`: Ethereum wallet and signer management.

## Detailed Mapping: `tempo-x402-soul`
The `tempo-x402-soul` crate is the cognitive heart of the agent.

### `crates/tempo-x402-soul/src`
- `chat.rs`: LLM-powered chat interface and session management.
- `coding.rs`: Logic for self-modification, code analysis, and automated PR generation.
- `config.rs`: Configuration for the soul's parameters and behavior.
- `db.rs`: Interface for the soul's local SQLite database.
- `error.rs`: Soul-specific error types.
- `fitness.rs`: Mechanisms for evaluating agent performance and success metrics.
- `git.rs`: Utilities for git operations during the self-improvement loop.
- `guard.rs`: Safety policies and execution guards.
- `lib.rs`: Crate entry point and high-level soul orchestration.
- `llm.rs`: Integration with Large Language Model providers.
- `memory.rs`: Short-term/working memory and context window management.
- `mode.rs`: Definition of agent operating modes (e.g., RESEARCH, IMPROVE).
- `neuroplastic.rs`: Logic for dynamic behavioral adjustments based on experience.
- `observer.rs`: Monitoring and recording agent actions and outcomes.
- `persistent_memory.rs`: Long-term archival and retrieval of insights.
- `plan.rs`: Strategic planning and task decomposition.
- `prompts.rs`: Management of LLM system and tool prompts.
- `thinking.rs`: Core cognitive loop and reasoning engine.
- `tool_registry.rs`: Registry for discovering and invoking agent tools.
- `tools.rs`: Implementation of built-in tools (e.g., search, file I/O).
- `vault.rs`: Secure handling of secrets and credentials.
- `world_model.rs`: Internal representation of the agent's environment and state.

## Detailed Mapping: `tempo-x402-node`
The `tempo-x402-node` crate provides the execution environment and API.

### `crates/tempo-x402-node/src`
- `clone.rs`: Logic for agent replication and node cloning.
- `db.rs`: Database management for node-level data.
- `main.rs`: Entry point for the agent node binary.
- `railway.rs`: Middleware and request routing infrastructure.
- `routes/`: HTTP API route handlers.
    - `clone.rs`: Agent cloning endpoints.
    - `health.rs`: System health and status.
    - `instance.rs`: Management of node instances.
    - `mod.rs`: Route registration and configuration.
    - `scripts.rs`: Dynamic script execution endpoints.
    - `soul.rs`: Interactions with the soul's cognitive state.
    - `wallet.rs`: Wallet and payment management endpoints.
- `soul_observer.rs`: Integration layer between the node and the soul.
- `state.rs`: Shared application state management.

## Selected Target for Evolution: `tempo-x402-soul`
The focus for research and self-improvement is the `tempo-x402-soul` crate.

### Why `tempo-x402-soul`?
- **Cognitive Engine**: It houses the `thinking.rs`, `neuroplastic.rs`, and `coding.rs` modules which define the agent's ability to reason and improve itself.
- **Self-Improvement Loop**: This crate is where the agent's "body" (the code) meets its "mind" (the LLM reasoning), making it the most critical area for achieving true autonomy.
- **Complexity and Potential**: The experimental nature of the cognitive logic offers the highest ROI for research into better LLM integration and memory management.
