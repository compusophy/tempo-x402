# Tempo-X402 Crate Mapping

This document maps the `crates` directory structure and identifies accessible source files for research and logic improvements.

## 📦 Accessible Crates and Source Files

Source files listed below are considered non-protected and can be modified for research, logic improvements, or bug fixes.
*Note: `Cargo.toml`, `Cargo.lock`, and identity-related files are excluded.*

### **tempo-x402-node**
The core node execution environment. Handles railway scripts, peer management, and soul observation.
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
Gateway, proxying, validation, and facilitator logic for the x402 protocol.
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
The core cognitive and agency engine. Responsible for memory, LLM integration, and self-improvement loops.
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
*   `crates/tempo-x402-soul/src/fitness.rs`

### **tempo-x402 (Core Protocol Library)**
Implementation of the x402 payment protocol, Tip20 tokens, and general utilities.
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

### **tempo-x402-security-audit**
Security auditing utilities.
*   `crates/tempo-x402-security-audit/src/lib.rs`

### **tempo-x402-app**
Frontend application (Leptos/Trunk based).
*   `crates/tempo-x402-app/src/` (various frontend components)

## 🚫 Protected Files (Do Not Modify)

*   `Cargo.toml` / `Cargo.lock` (Workspace and crate levels)
*   `crates/tempo-x402-identity/` (Identity-related files)
*   Any file specifically marked as protected in the mission brief or system instructions.

## 🛠 Available Dependencies (Workspace)

These are already present in the workspace and should be used instead of adding new ones:
- `actix-web`: Web framework (HttpRequest, HttpResponse, web::Data, etc.)
- `serde` / `serde_json`: Serialization/Deserialization
- `tokio`: Async runtime and process management
- `alloy`: Ethereum types (Address, U256, FixedBytes) and signing
- `reqwest`: HTTP client
- `tracing`: Logging/instrumentation
- `chrono`: DateTime utilities
- `uuid`: UUID generation
- `sha2` / `hmac`: Hashing and message authentication
- `hex`: Hexadecimal encoding/decoding
- `rusqlite`: SQLite database

## 🏗 Rust Patterns for This Codebase

- **Error handling**: Prefer `Result<T, String>` for general logic or `Result<HttpResponse, actix_web::Error>` for web handlers.
- **Actix handlers**: Return `impl Responder` or `Result<HttpResponse, actix_web::Error>`.
- **JSON responses**: `HttpResponse::Ok().json(serde_json::json!({...}))`.
- **Shared state**: Passed via `web::Data<AppState>`.
- **Route registration**: Done in `configure` functions: `cfg.service(web::resource("/path").route(web::get().to(handler)))`.
po-x402-soul/src/fitness.rs`

### **tempo-x402 (Core Protocol Library)**
Implementation of the x402 payment protocol, Tip20 tokens, and general utilities.
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

### **tempo-x402-security-audit**
Security auditing utilities.
*   `crates/tempo-x402-security-audit/src/lib.rs`

### **tempo-x402-app**
Frontend application (Leptos/Trunk based).
*   `crates/tempo-x402-app/src/` (various frontend components)

## 🚫 Protected Files (Do Not Modify)

*   `Cargo.toml` / `Cargo.lock` (Workspace and crate levels)
*   `crates/tempo-x402-identity/` (Identity-related files)
*   Any file specifically marked as protected in the mission brief or system instructions.

## 🛠 Available Dependencies (Workspace)

These are already present in the workspace and should be used instead of adding new ones:
- `actix-web`: Web framework (HttpRequest, HttpResponse, web::Data, etc.)
- `serde` / `serde_json`: Serialization/Deserialization
- `tokio`: Async runtime and process management
- `alloy`: Ethereum types (Address, U256, FixedBytes) and signing
- `reqwest`: HTTP client
- `tracing`: Logging/instrumentation
- `chrono`: DateTime utilities
- `uuid`: UUID generation
- `sha2` / `hmac`: Hashing and message authentication
- `hex`: Hexadecimal encoding/decoding
- `rusqlite`: SQLite database

## 🏗 Rust Patterns for This Codebase

- **Error handling**: Prefer `Result<T, String>` for general logic or `Result<HttpResponse, actix_web::Error>` for web handlers.
- **Actix handlers**: Return `impl Responder` or `Result<HttpResponse, actix_web::Error>`.
- **JSON responses**: `HttpResponse::Ok().json(serde_json::json!({...}))`.
- **Shared state**: Passed via `web::Data<AppState>`.
- **Route registration**: Done in `configure` functions: `cfg.service(web::resource("/path").route(web::get().to(handler)))`.
