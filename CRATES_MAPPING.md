# Crates Mapping

This document maps the `crates` directory structure and identifies non-protected source files that can be modified for research and self-improvement.

## Protected Files (DO NOT MODIFY)
- `Cargo.toml` (in any directory)
- `Cargo.lock`
- Files in `crates/tempo-x402-identity/`
- Soul core files (as defined in system constraints)

## Accessible Source Files

### **tempo-x402-node**
Main node implementation, manages soul state and execution.
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

### **tempo-x402-gateway**
API gateway and facilitator implementation.
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

### **tempo-x402-soul**
The "thinking" engine of the agent.
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
*   `crates/tempo-x402-soul/src/thinking.rs`
*   `crates/tempo-x402-soul/src/error.rs`
*   `crates/tempo-x402-soul/src/world_model.rs`
*   `crates/tempo-x402-soul/src/plan.rs`

### **tempo-x402 (Core Protocol Library)**
Core types and logic for the x402 protocol.
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

### **tempo-x402-app**
Frontend web application.
*   `crates/tempo-x402-app/src/lib.rs`
*   `crates/tempo-x402-app/src/api.rs`
*   `crates/tempo-x402-app/src/wallet.rs`
*   `crates/tempo-x402-app/src/wallet_crypto.rs`
*   `crates/tempo-x402-app/index.html`
*   `crates/tempo-x402-app/style.css`

### **tempo-x402-security-audit**
Security audit tools and tests.
*   `crates/tempo-x402-security-audit/src/lib.rs`
