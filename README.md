<p align="center">
  <h1 align="center">tempo-x402</h1>
  <p align="center">Security-hardened, ultra-fast pay-per-request APIs on the Tempo blockchain &mdash; one HTTP header, one on-chain transfer, zero custodial risk.</p>
</p>

<p align="center">
  <a href="https://crates.io/crates/tempo-x402"><img src="https://img.shields.io/crates/v/tempo-x402.svg" alt="crates.io"></a>
  <a href="https://docs.rs/tempo-x402"><img src="https://docs.rs/tempo-x402/badge.svg" alt="docs.rs"></a>
  <a href="https://github.com/compusophy/tempo-x402/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://docs.rs/tempo-x402">Documentation</a> &middot; <a href="https://x402-gateway-production-5018.up.railway.app">Live Demo</a> &middot; <a href="https://crates.io/crates/tempo-x402">crates.io</a> &middot; <a href="https://github.com/compusophy/tempo-x402">GitHub</a>
</p>

<p align="center">
  <a href="https://railway.com/template/tempo-x402?referralCode=tempo"><img src="https://railway.com/button.svg" alt="Deploy on Railway"></a>
</p>

---

**tempo-x402** implements [HTTP 402 Payment Required](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Status/402) for the Tempo blockchain. Clients sign EIP-712 payment authorizations, servers gate content behind 402 responses, and a facilitator settles payments on-chain via `transferFrom` &mdash; all in a single request/response cycle.

The facilitator holds no user funds. It only has token approval to call `transferFrom` on behalf of clients who have explicitly approved it.

## How it works

```
Client                     Server                    Facilitator               Chain
  |                          |                            |                      |
  |  GET /resource           |                            |                      |
  |------------------------->|                            |                      |
  |                          |                            |                      |
  |  402 + price/token/to    |                            |                      |
  |<-------------------------|                            |                      |
  |                          |                            |                      |
  |  [sign EIP-712]          |                            |                      |
  |                          |                            |                      |
  |  GET /resource           |                            |                      |
  |  + PAYMENT-SIGNATURE     |                            |                      |
  |------------------------->|                            |                      |
  |                          |  POST /verify-and-settle   |                      |
  |                          |--------------------------->|                      |
  |                          |                            |  transferFrom()      |
  |                          |                            |--------------------->|
  |                          |                            |              tx hash |
  |                          |                            |<---------------------|
  |                          |         settlement result  |                      |
  |                          |<---------------------------|                      |
  |                          |                            |                      |
  |  200 + content + tx hash |                            |                      |
  |<-------------------------|                            |                      |
```

1. Client requests a protected endpoint
2. Server responds **402** with `PaymentRequirements` (price, token, recipient)
3. Client signs an **EIP-712 `PaymentAuthorization`**, retries with `PAYMENT-SIGNATURE` header
4. Server forwards to facilitator's `/verify-and-settle`
5. Facilitator **atomically** verifies signature, checks balance/allowance/nonce, calls `transferFrom`
6. Server returns the content + settlement transaction hash

## Quick start

### Install

```bash
cargo add tempo-x402          # core types + traits
cargo add tempo-x402-client   # client SDK (optional)
```

### Make a paid request

```rust
use alloy::signers::local::PrivateKeySigner;
use x402_client::{TempoSchemeClient, X402Client};

#[tokio::main]
async fn main() {
    let signer: PrivateKeySigner = "0xYOUR_PRIVATE_KEY".parse().unwrap();
    let client = X402Client::new(TempoSchemeClient::new(signer));

    let (response, settlement) = client
        .fetch("https://x402-gateway.example.com/g/my-api/data", reqwest::Method::GET)
        .await
        .unwrap();

    println!("{}", response.text().await.unwrap());
    if let Some(s) = settlement {
        println!("tx: {}", s.transaction.unwrap_or_default());
    }
}
```

### Gate an endpoint (server-side)

```rust
use x402_server::config::PaymentConfigBuilder;

let config = PaymentConfigBuilder::new(facilitator_url, recipient_address)
    .route("GET", "/premium", "$0.01", "Premium content")
    .route("POST", "/generate", "$0.05", "AI generation")
    .build();
```

### Monetize any API via the gateway

No code changes to the upstream API &mdash; the gateway proxies requests and handles payment.

```bash
# Register an endpoint (pays a small platform fee)
curl -X POST https://x402-gateway.example.com/register \
  -H "Content-Type: application/json" \
  -H "PAYMENT-SIGNATURE: <base64-payment>" \
  -d '{"slug": "my-api", "target_url": "https://api.example.com", "price": "$0.05"}'

# Clients call through the gateway
curl https://x402-gateway.example.com/g/my-api/users/123 \
  -H "PAYMENT-SIGNATURE: <base64-payment>"
```

Target APIs receive verification headers: `X-X402-Verified`, `X-X402-Payer`, `X-X402-Amount`, `X-X402-TxHash`.

## Architecture

```
crates/
├── tempo-x402/                # Core: types, EIP-712, TIP-20, nonce store, HMAC, traits
├── tempo-x402-client/         # Client SDK + CLI for making paid requests
├── tempo-x402-server/         # Resource server with payment middleware (actix-web)
├── tempo-x402-facilitator/    # Payment verification + on-chain settlement (actix-web)
├── tempo-x402-gateway/        # API proxy: register endpoints, pay-per-request routing
├── tempo-x402-wallet/         # WASM-compatible wallet: key gen + EIP-712 signing
├── tempo-x402-node/           # Self-deploying node: gateway + identity + clone orchestration
├── tempo-x402-identity/       # Wallet generation, persistence, faucet, parent registration
├── tempo-x402-agent/          # Railway API client + clone lifecycle management
├── tempo-x402-soul/           # Agentic thinking loop with persistent memory + agent-to-agent economy tools
├── tempo-x402-mind/           # Lateralized dual-soul: left (analytical) + right (holistic) + callosum
├── tempo-x402-app/            # Leptos WASM demo SPA (not published)
└── tempo-x402-security-audit/ # 15 security invariant tests enforced on every build
```

| Crate | What it does | Install |
|-------|-------------|---------|
| [`tempo-x402`](https://docs.rs/tempo-x402) | Core library &mdash; types, EIP-712, TIP-20, nonce store, HMAC | `cargo add tempo-x402` |
| [`tempo-x402-client`](https://docs.rs/tempo-x402-client) | Client SDK for making paid API requests | `cargo add tempo-x402-client` |
| [`tempo-x402-server`](https://docs.rs/tempo-x402-server) | Resource server with payment middleware | `cargo add tempo-x402-server` |
| [`tempo-x402-facilitator`](https://docs.rs/tempo-x402-facilitator) | Payment verification and on-chain settlement | `cargo add tempo-x402-facilitator` |
| [`tempo-x402-gateway`](https://docs.rs/tempo-x402-gateway) | API gateway &mdash; proxy any HTTP API with payment rails | `cargo add tempo-x402-gateway` |
| [`tempo-x402-wallet`](https://docs.rs/tempo-x402-wallet) | WASM-compatible wallet for browsers and edge runtimes | `cargo add tempo-x402-wallet` |
| [`tempo-x402-node`](https://docs.rs/tempo-x402-node) | Self-deploying node with clone orchestration | `cargo add tempo-x402-node` |
| [`tempo-x402-identity`](https://docs.rs/tempo-x402-identity) | Wallet generation, persistence, faucet funding | `cargo add tempo-x402-identity` |
| [`tempo-x402-agent`](https://docs.rs/tempo-x402-agent) | Railway API client + clone spawning | `cargo add tempo-x402-agent` |
| [`tempo-x402-soul`](https://docs.rs/tempo-x402-soul) | Agentic thinking loop with persistent memory + agent economy tools | `cargo add tempo-x402-soul` |
| [`tempo-x402-mind`](https://docs.rs/tempo-x402-mind) | Lateralized dual-soul architecture (left/right hemispheres + callosum) | `cargo add tempo-x402-mind` |

## Gateway API

The gateway lets you monetize any HTTP API without modifying its source code.

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `POST` | `/register` | Platform fee | Register a new endpoint |
| `GET` | `/endpoints` | Free | List all active endpoints |
| `GET` | `/endpoints/:slug` | Free | Get endpoint details |
| `PATCH` | `/endpoints/:slug` | Platform fee | Update endpoint (owner only) |
| `DELETE` | `/endpoints/:slug` | Platform fee | Deactivate endpoint (owner only) |
| `GET` | `/analytics` | Free | Per-endpoint payment stats and revenue |
| `GET` | `/analytics/:slug` | Free | Stats for a single endpoint |
| `ANY` | `/g/:slug/*` | Endpoint price | Proxy to target API |
| `GET` | `/g/network-stats` | $0.0001 | Real-time blockchain status (block, gas, etc.) |
| `GET` | `/g/echo-ip` | $0.0001 | Public IP address of the caller |
| `GET` | `/g/headers` | $0.0001 | Request headers as seen by the gateway |
| `POST` | `/g/json-validator` | $0.0001 | Validates a JSON string |
| `POST` | `/g/hex-converter` | $0.0001 | Text to hex / hex to text conversion |
| `POST` | `/g/estimate-gas` | $0.001 | Estimate gas limit for a transaction |
| `POST` | `/g/get-nonce` | $0.0001 | Get transaction count (nonce) for an address |
| `POST` | `/g/get-balance` | $0.0001 | Get balance (native or TIP-20) for an address |
| `GET` | `/soul/status` | Free | Soul thinking loop status (node only) |
| `POST` | `/soul/chat` | Free | Interactive chat with the soul (node only) |
| `GET` | `/mind/status` | Free | Dual-soul status — both hemispheres (node only, `MIND_ENABLED`) |
| `POST` | `/mind/chat` | Free | Chat routed to left hemisphere (node only, `MIND_ENABLED`) |
| `GET` | `/health` | Free | Health check |
| `GET` | `/metrics` | Bearer token | Prometheus metrics |

## Network

| | |
|-|-|
| **Chain** | Tempo Moderato (testnet) |
| **Chain ID** | `42431` ([CAIP-2](https://github.com/ChainAgnostic/CAIPs/blob/main/CAIPs/caip-2.md): `eip155:42431`) |
| **Token** | pathUSD &mdash; `0x20c0000000000000000000000000000000000000` (6 decimals) |
| **Scheme** | `tempo-tip20` |
| **RPC** | `https://rpc.moderato.tempo.xyz` |
| **Explorer** | `https://explore.moderato.tempo.xyz` |

## Prerequisites

Before making payments:

```bash
# 1. Fund your wallet with testnet pathUSD
cast rpc tempo_fundAddress 0xYOUR_ADDRESS --rpc-url https://rpc.moderato.tempo.xyz

# 2. Approve the facilitator to spend your tokens
cargo run --bin x402-approve
```

Or programmatically:

```rust
use x402::tip20;
tip20::approve(&provider, token, facilitator_address, amount).await?;
```

## Environment variables

| Variable | Used by | Description |
|----------|---------|-------------|
| `EVM_ADDRESS` | server | Payment recipient address |
| `EVM_PRIVATE_KEY` | client | Client wallet private key |
| `FACILITATOR_URL` | server | Facilitator endpoint (default: `localhost:4022`) |
| `FACILITATOR_PRIVATE_KEY` | facilitator, gateway, node | Facilitator wallet key |
| `FACILITATOR_ADDRESS` | approve | Facilitator address for token approval |
| `FACILITATOR_SHARED_SECRET` | server, facilitator | HMAC shared secret for request auth |
| `RESOURCE_SERVER_URL` | client | Server endpoint (default: `localhost:4021`) |
| `RPC_URL` | all | Tempo RPC endpoint |
| `ALLOWED_ORIGINS` | server, facilitator | Comma-separated CORS origins |
| `RATE_LIMIT_RPM` | server, facilitator | Rate limit per minute per IP |
| `METRICS_TOKEN` | server, facilitator, gateway | Bearer token for `/metrics` endpoint |
| `WEBHOOK_URLS` | facilitator, gateway | Comma-separated settlement webhook URLs |
| `GEMINI_API_KEY` | node | Gemini API key for soul thinking (dormant without it) |
| `SOUL_DB_PATH` | node | Soul database path (default: `./soul.db`) |
| `SOUL_CODING_ENABLED` | node | Enable soul file write/edit/commit tools (default: `false`) |
| `SOUL_DYNAMIC_TOOLS_ENABLED` | node | Enable dynamic tool registry (default: `false`) |
| `HEALTH_PROBE_INTERVAL_SECS` | node | Health probe loop interval in seconds (default: `300`) |
| `SOUL_MEMORY_FILE` | node | Path to persistent memory file (default: `/data/soul_memory.md`) |
| `GATEWAY_URL` | node | Gateway URL for soul endpoint registration (default: `http://localhost:4023`) |
| `MIND_ENABLED` | node | Enable dual-soul architecture (default: `false`) |

## Security

This project enforces security invariants through automated testing. The `tempo-x402-security-audit` crate runs 15 tests on every build that scan all production source code for:

- No hardcoded private keys in production code
- HMAC verification reaches constant-time comparison on all paths (via `subtle` crate)
- All `reqwest` HTTP clients disable redirects (SSRF protection)
- Webhook URLs require HTTPS with private IP blocking and DNS rebinding prevention
- HTTP error responses never leak internal details (balances, allowances, stack traces)
- SQLite nonce store is mandatory in production (no in-memory fallback)
- Parameterized SQL queries only (no string formatting)
- HMAC shared secret is mandatory (not `Option`)
- Private keys never appear in tracing/logging macros

Additional hardening:

- **EIP-2 high-s rejection** prevents signature malleability
- **Per-payer mutex locks** prevent TOCTOU races during settlement
- **Nonces claimed before `transferFrom`**, never released on failure
- **Integer-only arithmetic** for all token amounts (never `f64`)
- **SSRF protection** on proxy and webhooks: HTTPS-only, private IP blocking, DNS resolution validation, CRLF rejection
- **Atomic slug reservation** with `BEGIN IMMEDIATE` to prevent race conditions

## Deployed services

| Service | URL |
|---------|-----|
| Server | https://x402-server-production.up.railway.app |
| Facilitator | https://x402-facilitator-production-ec87.up.railway.app |
| Gateway | https://x402-gateway-production-5018.up.railway.app |

Health check: `GET /health` on any service.

## Development

```bash
cargo build --workspace            # build everything
cargo test --workspace             # run all tests (including security audit)
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

OpenAPI 3.1 specs are available in the `openapi/` directory for the facilitator, server, and gateway.

## License

MIT
