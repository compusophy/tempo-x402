# Crates Directory Mapping

This document outlines the crates directory structure and identifies accessible source files for research and logic improvements.

## Protected Files & Directories
- `Cargo.toml`, `Cargo.lock`
- `crates/tempo-x402-identity/` (Identity-related)
- Soul Core and Identity files

## Accessible Source Files (Non-Protected)

### **tempo-x402-node**
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
*   `crates/tempo-x402-app/src/` (Web application source files)

### **tempo-x402-security-audit**
*   `crates/tempo-x402-security-audit/src/`
*   `crates/tempo-x402-security-audit/tests/`

---

## Development Guidelines

### Available Dependencies (already in Cargo.toml — do NOT add new ones)
- **actix-web**: HttpRequest, HttpResponse, web::{Data, Json, Path, Query, ServiceConfig}
- **serde / serde_json**: Serialize, Deserialize, serde_json::{json, Value}
- **tokio**: async runtime, tokio::process::Command, tokio::time
- **alloy**: Ethereum types (Address, U256, FixedBytes), providers, signers
- **reqwest**: HTTP client
- **tracing**: tracing::info!, tracing::warn!, tracing::error!
- **chrono**: Utc, DateTime, NaiveDateTime
- **uuid**: Uuid::new_v4()
- **sha2 / hmac**: for hashing
- **hex**: hex::encode, hex::decode
- **rusqlite**: SQLite (used via SoulDatabase wrapper)

### Rust Patterns for This Codebase
- Error handling: use `Result<T, String>` or `Result<T, actix_web::Error>` for handlers
- Actix handlers return `impl Responder` or `Result<HttpResponse, actix_web::Error>`
- Route registration: `cfg.service(web::resource("/path").route(web::get().to(handler)))`
- JSON responses: `HttpResponse::Ok().json(serde_json::json!({...}))`
- Shared state: `web::Data<AppState>` passed to handlers
- String → &str: use `.as_str()` or `&*string_var`
- `async fn handler(req: HttpRequest) -> impl Responder { ... }`

### Rules
- Use `edit_file` for existing files (provide unique `old_string` and `new_string`)
- Use `write_file` only for brand new files
- Keep changes minimal and focused — add code at the right location
- For actix-web endpoints: add the handler function AND update the configure fn
- Ensure all imports are at the top of the file
- Do NOT rewrite the entire file — only add/change what's needed
- If unsure about an import path, use `search_files` or `read_file` to check
- After editing, run `cargo check --workspace` to verify
