# Crates Directory Mapping

This document outlines the crates directory structure and identifies accessible source files for research and logic improvements, based on the `guard.rs` protection rules.

## Protected Files & Directories
Any attempt to write to these will be blocked by the `guard.rs` safety layer.

- `Cargo.toml`, `Cargo.lock` (Anywhere)
- `.github/` (Anywhere)
- `crates/tempo-x402-identity/` (Entire crate)
- `crates/tempo-x402-gateway/src/` (Entire source directory)
- `crates/tempo-x402-node/src/main.rs`
- `crates/tempo-x402-node/src/routes/` (Entire directory)
- `crates/tempo-x402-soul/src/tools.rs`
- `crates/tempo-x402-soul/src/llm.rs`
- `crates/tempo-x402-soul/src/db.rs`
- `crates/tempo-x402-soul/src/error.rs`
- `crates/tempo-x402-soul/src/guard.rs`
- `crates/tempo-x402-soul/src/config.rs`
- `crates/tempo-x402-soul/src/tool_registry.rs`

## Accessible Source Files (Non-Protected)
These files can be modified to improve agent behavior and node logic.

### **tempo-x402-node**
*   `crates/tempo-x402-node/build.rs`
*   `crates/tempo-x402-node/src/soul_observer.rs`
*   `crates/tempo-x402-node/src/clone.rs`
*   `crates/tempo-x402-node/src/db.rs`
*   `crates/tempo-x402-node/src/railway.rs`
*   `crates/tempo-x402-node/src/state.rs`
*   `crates/tempo-x402-node/src/routes/mod.rs` (Note: individual files in `routes/` are protected, but `mod.rs` might be as well if the prefix match is greedy. `guard.rs` uses `crates/tempo-x402-node/src/routes/` as a prefix, so `mod.rs` IS protected.)

### **tempo-x402-soul**
*   `crates/tempo-x402-soul/src/lib.rs`
*   `crates/tempo-x402-soul/src/observer.rs`
*   `crates/tempo-x402-soul/src/git.rs`
*   `crates/tempo-x402-soul/src/chat.rs`
*   `crates/tempo-x402-soul/src/persistent_memory.rs`
*   `crates/tempo-x402-soul/src/coding.rs`
*   `crates/tempo-x402-soul/src/neuroplastic.rs`
*   `crates/tempo-x402-soul/src/memory.rs`
*   `crates/tempo-x402-soul/src/mode.rs`
*   `crates/tempo-x402-soul/src/vault.rs`
*   `crates/tempo-x402-soul/src/prompts.rs`
*   `crates/tempo-x402-soul/src/thinking.rs`
*   `crates/tempo-x402-soul/src/world_model.rs`
*   `crates/tempo-x402-soul/src/plan.rs`

### **tempo-x402 (Core Protocol Library)**
*All files in `crates/tempo-x402/src/` are accessible.*
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
*   `crates/tempo-x402/src/client/discovery.rs`
*   `crates/tempo-x402/src/client/payment.rs`
