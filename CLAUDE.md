# tempo-x402 — Project Context

## What This Is

x402 (HTTP 402 Payment Required) implementation for the **Tempo blockchain** using TIP-20 tokens (pathUSD). Pay-per-request API monetization where clients sign EIP-712 payment authorizations and a facilitator settles them on-chain.

Rust workspace. Published as `tempo-x402`, `tempo-x402-server`, `tempo-x402-facilitator`, `tempo-x402-gateway`, `tempo-x402-wallet`, `tempo-x402-node`, `tempo-x402-identity`, `tempo-x402-agent`, `tempo-x402-soul` on crates.io.

## Architecture

```
Client (x402-client) --> Resource Server (x402-server:4021) --> Facilitator (x402-facilitator:4022) --> Tempo Chain (42431)
                \--- or uses x402-wallet (WASM) for signing ---/
```

Three-party model: **Client** signs + pays, **Server** gates endpoints, **Facilitator** verifies + settles on-chain. The **Wallet** crate provides a lightweight, WASM-compatible alternative for signing (key generation + EIP-712) without network dependencies.

## Workspace

```
crates/
├── tempo-x402/               # core lib: types, traits, EIP-712, nonce store, TIP-20
├── tempo-x402-client/        # Rust SDK + CLI for making paid requests
├── tempo-x402-server/        # resource server + payment middleware (actix-web)
├── tempo-x402-facilitator/   # payment verification + on-chain settlement (actix-web)
├── tempo-x402-gateway/       # API proxy with endpoint registration + embedded facilitator
├── tempo-x402-wallet/        # WASM-compatible wallet: key gen, EIP-712 signing
├── tempo-x402-node/          # self-deploying node: gateway + identity + clone orchestration
├── tempo-x402-identity/      # wallet generation, persistence, faucet, parent registration
├── tempo-x402-agent/         # Railway API client + clone orchestration
├── tempo-x402-soul/          # agentic thinking loop powered by Gemini 3 Flash + nudge queue + stagnation detection
├── tempo-x402-app/           # Leptos WASM demo SPA (not published)
└── tempo-x402-security-audit/# test-only: 15 security invariant checks (not published)
```

Package names use `tempo-` prefix for crates.io. Library names stay `x402`, `x402_server`, `x402_facilitator`, `x402_wallet` in code.

Each crate has its own `CLAUDE.md` with local context. Read that first when working in a crate.

## Deployments

- **Server**: https://x402-server-production.up.railway.app (port 4021)
- **Facilitator**: https://x402-facilitator-production-ec87.up.railway.app (port 4022)
- **Gateway**: https://x402-gateway-production-5018.up.railway.app (port 4023)
- **crates.io**: https://crates.io/crates/tempo-x402
- **GitHub**: https://github.com/compusophy/tempo-x402

## Chain

- **Chain**: Tempo Moderato, Chain ID `42431`, CAIP-2 `eip155:42431`
- **Token**: pathUSD `0x20c0000000000000000000000000000000000000` (6 decimals)
- **Scheme**: `tempo-tip20`
- **RPC**: `https://rpc.moderato.tempo.xyz`
- **Explorer**: `https://explore.moderato.tempo.xyz`

## Payment Flow

1. Client GET protected endpoint -> Server responds 402 with price/token/recipient
2. Client signs EIP-712 `PaymentAuthorization`, retries with `PAYMENT-SIGNATURE` header
3. Server forwards to facilitator `/verify-and-settle`
4. Facilitator atomically: verify signature, check balance/allowance/nonce, `transferFrom`
5. Server returns content + tx hash

## Environment Variables

| Var | Used By | Purpose |
|-----|---------|---------|
| `EVM_ADDRESS` | server | Payment recipient address |
| `EVM_PRIVATE_KEY` | client | Client wallet private key |
| `FACILITATOR_URL` | server | Facilitator endpoint (default: localhost:4022) |
| `FACILITATOR_PRIVATE_KEY` | facilitator | Facilitator wallet key |
| `FACILITATOR_ADDRESS` | approve | Facilitator address for token approval |
| `FACILITATOR_SHARED_SECRET` | server, facilitator | HMAC shared secret |
| `RESOURCE_SERVER_URL` | client | Server endpoint (default: localhost:4021) |
| `RPC_URL` | all | Tempo RPC endpoint |
| `ALLOWED_ORIGINS` | server, facilitator | Comma-separated CORS origins |
| `RATE_LIMIT_RPM` | server, facilitator | Rate limit per minute |
| `GEMINI_API_KEY` | node | Gemini API key for soul (dormant without it) |
| `SOUL_CODING_ENABLED` | node | Enable soul write/edit/commit tools (default: false) |
| `SOUL_DYNAMIC_TOOLS_ENABLED` | node | Enable dynamic tool registry (default: false) |
| `HEALTH_PROBE_INTERVAL_SECS` | node | Health probe interval in seconds (default: 300) |
| `SOUL_FORK_REPO` | node | Fork repo for soul push (e.g. `compusophy-bot/tempo-x402`) |
| `SOUL_UPSTREAM_REPO` | node | Upstream repo for soul PRs/issues (e.g. `compusophy/tempo-x402`) |
| `SOUL_MEMORY_FILE` | soul | Path to persistent memory file (default: `/data/soul_memory.md`) |
| `GATEWAY_URL` | soul | Gateway URL for register_endpoint tool (default: `http://localhost:4023`) |

## Commands

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all                 # CI enforces formatting
```

## Docs Maintenance

- Each crate has a `CLAUDE.md` — keep it **structural, not detailed**
- CLAUDE.md covers: what it does, what it depends on, cross-crate impacts, non-obvious patterns
- Do NOT duplicate type fields, env var defaults, or flow details that are readable from code
- Update a crate's CLAUDE.md when: dependencies change, public API changes, or cross-crate impacts change
- When adding a new crate: add a CLAUDE.md (security-audit CI verifies this)
- The `tempo-x402-security-audit` crate enforces security invariants via file scanning — new crates are auto-included
