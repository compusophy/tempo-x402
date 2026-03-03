# tempo-x402-node

Binary crate. Self-deploying x402 node: composes gateway + identity bootstrap + clone orchestration.

Runs a gateway, auto-generates its own wallet identity, and can clone itself onto Railway infrastructure. Extends gateway's SQLite DB with a `children` table.

Binary: `x402-node` on port 4023.

## Depends On

- `x402` (core)
- `x402-gateway` (AppState, Database, middleware, config, routes)
- `x402-identity` (bootstrap, faucet, registration)
- `x402-agent` (CloneOrchestrator)
- `x402-facilitator` (embedded facilitator state, routes)
- `x402-soul` (SoulDatabase for status queries)

## Non-Obvious Patterns

- Startup: identity bootstrap → gateway config → embedded facilitator → soul init → node state → soul spawn → server (order matters — soul DB must be cloned before spawn consumes it)
- `create_child_if_under_limit()` in `db.rs` is atomic (`BEGIN IMMEDIATE`) — don't replace with separate check+insert
- Input validation: UUID format for instance IDs, `0x` + 40 hex for addresses, HTTPS-only for URLs
- Extends gateway DB via `execute_schema()` — doesn't create a separate database
- Background tasks (faucet, parent registration) are best-effort, non-fatal
- Version check compares `build` (git SHA) from `/health`, falls back to semver if child lacks build field
- Background health probe is **periodic** (default 300s via `HEALTH_PROBE_INTERVAL_SECS`), recovers stuck "deploying" children by fetching `/instance/info` and promoting to "running"
- E2e test endpoints (`e2e-test-*` prefix) are purged on startup

## If You're Changing...

- **Clone logic**: Endpoint in `routes/clone.rs`, orchestration in `x402-agent` crate
- **Identity bootstrap**: `x402-identity` crate — node just calls `bootstrap()`
- **Database schema**: `db.rs` — uses gateway's `execute_schema()` pattern
- **Startup order**: `main.rs` — bootstrap must run before gateway config reads env vars; soul init must happen before NodeState
- **Soul status**: `routes/soul.rs` — `GET /soul/status` queries `NodeState.soul_db` (includes world model beliefs)
