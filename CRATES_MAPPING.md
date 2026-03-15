# Crates Mapping

This document outlines the directory structure of the workspace and identifies source files accessible for logic improvements and research.

## **Root**
*   `Cargo.toml`
*   `CRATES_MAPPING.md`

## **tempo-x402-node**
Primary node implementation responsible for managing the agent's lifecycle, routes, and state.
*   `crates/tempo-x402-node/Cargo.toml`
*   `crates/tempo-x402-node/build.rs`
*   `crates/tempo-x402-node/src/main.rs`
*   `crates/tempo-x402-node/src/soul_observer.rs`
*   `crates/tempo-x402-node/src/clone.rs`
*   `crates/tempo-x402-node/src/db.rs`
*   `crates/tempo-x402-node/src/railway.rs`
*   `crates/tempo-x402-node/src/state.rs`
*   `crates/tempo-x402-node/src/routes/mod.rs`
*   `crates/tempo-x402-node/src/routes/instance.rs`
*   `crates/tempo-x402-node/src/routes/scripts.rs`
*   `crates/tempo-x402-node/src/routes/clone.rs`
*   `crates/tempo-x402-node/src/routes/soul.rs`
*   `crates/tempo-x402-node/src/routes/wallet.rs`
*   `crates/tempo-x402-node/src/routes/health.rs`

## **tempo-x402-gateway**
Gateway and facilitator logic for handling requests, proxying, and metrics.
*   `crates/tempo-x402-gateway/Cargo.toml`
*   `crates/tempo-x402-gateway/build.rs`
*   `crates/tempo-x402-gateway/src/main.rs`
*   `crates/tempo-x402-gateway/src/lib.rs`
*   `crates/tempo-x402-gateway/src/cors.rs`
*   `crates/tempo-x402-gateway/src/validation.rs`
*   `crates/tempo-x402-gateway/src/proxy.rs`
*   `crates/tempo-x402-gateway/src/config.rs`
*   `crates/tempo-x402-gateway/src/db.rs`
*   `crates/tempo-x402-gateway/src/metrics.rs`
*   `crates/tempo-x402-gateway/src/error.rs`
*   `crates/tempo-x402-gateway/src/state.rs`
*   `crates/tempo-x402-gateway/src/middleware.rs`
*   `crates/tempo-x402-gateway/src/bin/x402-facilitator.rs`
*   `crates/tempo-x402-gateway/src/routes/mod.rs`
*   `crates/tempo-x402-gateway/src/routes/analytics.rs`
*   `crates/tempo-x402-gateway/src/routes/gateway.rs`
*   `crates/tempo-x402-gateway/src/routes/register.rs`
*   `crates/tempo-x402-gateway/src/routes/health.rs`
*   `crates/tempo-x402-gateway/src/routes/endpoints.rs`
*   `crates/tempo-x402-gateway/src/facilitator/mod.rs`
*   `crates/tempo-x402-gateway/src/facilitator/webhook.rs`
*   `crates/tempo-x402-gateway/src/facilitator/bootstrap.rs`
*   `crates/tempo-x402-gateway/src/facilitator/metrics.rs`
*   `crates/tempo-x402-gateway/src/facilitator/routes.rs`
*   `crates/tempo-x402-gateway/src/facilitator/state.rs`

## **tempo-x402-soul**
The "Soul" of the agent, containing logic for thinking, planning, memory, and tool usage.
*   `crates/tempo-x402-soul/Cargo.toml`
*   `crates/tempo-x402-soul/src/lib.rs`
*   `crates/tempo-x402-soul/src/observer.rs`
*   `crates/tempo-x402-soul/src/git.rs`
*   `crates/tempo-x402-soul/src/chat.rs`
*   `crates/tempo-x402-soul/src/tools.rs`
*   `crates/tempo-x402-soul/src/persistent_memory.rs`
*   `crates/tempo-x402-soul/src/coding.rs`
*   `crates/tempo-x402-soul/src/guard.rs`
*   `crates/tempo-x402-soul/src/config.rs`
*   `crates/tempo-x402-soul/src/neuroplastic.rs`
*   `crates/tempo-x402-soul/src/db.rs`
*   `crates/tempo-x402-soul/src/memory.rs`
*   `crates/tempo-x402-soul/src/mode.rs`
*   `crates/tempo-x402-soul/src/vault.rs`
*   `crates/tempo-x402-soul/src/tool_registry.rs`
*   `crates/tempo-x402-soul/src/llm.rs`
*   `crates/tempo-x402-soul/src/prompts.rs`
*   `crates/tempo-x402-soul/src/retry_utils.rs`
*   `crates/tempo-x402-soul/src/thinking.rs`
*   `crates/tempo-x402-soul/src/error.rs`
*   `crates/tempo-x402-soul/src/world_model.rs`
*   `crates/tempo-x402-soul/src/plan.rs`
*   `crates/tempo-x402-soul/src/fitness.rs`

## **tempo-x402** (Core Protocol Library)
Core library implementing the x402 protocol, HMAC signing, and communication schemes.
*   `crates/tempo-x402/Cargo.toml`
*   `crates/tempo-x402/src/lib.rs`
*   `crates/tempo-x402/src/hmac.rs`
*   `crates/tempo-x402/src/response.rs`
*   `crates/tempo-x402/src/scheme.rs`
*   `crates/tempo-x402/src/scheme_server.rs`
*   `crates/tempo-x402/src/security.rs`
*   `crates/tempo-x402/src/eip712.rs`
*   `crates/tempo-x402/src/constants.rs`
*   `crates/tempo-x402/src/approve.rs`
*   `crates/tempo-x402/src/nonce_store.rs`
*   `crates/tempo-x402/src/facilitator_client.rs`
*   `crates/tempo-x402/src/tip20.rs`
*   `crates/tempo-x402/src/payment.rs`
*   `crates/tempo-x402/src/scheme_facilitator.rs`
*   `crates/tempo-x402/src/network.rs`
*   `crates/tempo-x402/src/error.rs`
*   `crates/tempo-x402/src/wallet.rs`
*   `crates/tempo-x402/src/bin/x402-client.rs`
*   `crates/tempo-x402/src/client/mod.rs`
*   `crates/tempo-x402/src/client/http_client.rs`
*   `crates/tempo-x402/src/client/scheme_client.rs`

## **tempo-x402-identity**
Identity management, on-chain contracts, and reputation.
*   `crates/tempo-x402-identity/Cargo.toml`
*   `crates/tempo-x402-identity/src/contracts.rs`
*   `crates/tempo-x402-identity/src/deploy.rs`
*   `crates/tempo-x402-identity/src/discovery.rs`
*   `crates/tempo-x402-identity/src/lib.rs`
*   `crates/tempo-x402-identity/src/onchain.rs`
*   `crates/tempo-x402-identity/src/recovery.rs`
*   `crates/tempo-x402-identity/src/reputation.rs`
*   `crates/tempo-x402-identity/src/types.rs`
*   `crates/tempo-x402-identity/src/validation.rs`

## **tempo-x402-app**
Frontend application and wallet interface.
*   `crates/tempo-x402-app/Cargo.toml`
*   `crates/tempo-x402-app/Trunk.toml`
*   `crates/tempo-x402-app/src/api.rs`
*   `crates/tempo-x402-app/src/lib.rs`
*   `crates/tempo-x402-app/src/wallet.rs`
*   `crates/tempo-x402-app/src/wallet_crypto.rs`
*   `crates/tempo-x402-app/index.html`

## **tempo-x402-security-audit**
Security invariants and audit testing.
*   `crates/tempo-x402-security-audit/Cargo.toml`
*   `crates/tempo-x402-security-audit/src/lib.rs`
*   `crates/tempo-x402-security-audit/tests/security_invariants.rs`
