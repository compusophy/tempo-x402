# tempo-x402-soul

Library crate. Agentic "soul" for x402 nodes: a periodic observe-think-record loop powered by Gemini with full coding agent capabilities.

Observes node state via `NodeObserver` trait, reasons via Gemini API, can read/write/edit files and execute shell commands via Gemini function calling, records thoughts and mutations to SQLite. Operates on `vm/<instance-id>` branches with pre-commit validation. Runs dormant (observe-only) without a Gemini API key.

## Depends On

- `x402` (core types only)
- `x402-wallet` (EIP-712 signing for `register_endpoint` tool)

No dependency on gateway/identity/agent/node. Communicates via `NodeObserver` trait — the node crate implements it.

## Module Overview

| Module | Purpose |
|--------|---------|
| `guard.rs` | Hardcoded protected file list — prevents self-bricking |
| `tools.rs` | Tool executor: shell, file read/write/edit, search, commit, PR + dynamic tool dispatch |
| `tool_registry.rs` | Dynamic tool registry: register/list/unregister tools at runtime, shell execution |
| `git.rs` | Branch-per-VM git workflow (ensure_branch, commit, push, PR, issues) with fork support |
| `coding.rs` | Pre-commit validation pipeline (cargo check → test → commit) |
| `mode.rs` | Agent modes (Observe, Chat, Code, Review) with per-mode tool sets |
| `prompts.rs` | System prompts per mode |
| `llm.rs` | Gemini API client with thought_signature support |
| `thinking.rs` | The main observe → think → tool loop |
| `chat.rs` | Interactive chat handler with mode detection |
| `db.rs` | SQLite: thoughts, soul_state, mutations, tools, pattern_counts, beliefs tables + neuroplastic queries |
| `memory.rs` | Thought types (Observation, Reasoning, Decision, Prediction, etc.) + salience/tier/strength fields |
| `neuroplastic.rs` | Salience scoring, tiered memory decay, prediction error — the learning loop |
| `persistent_memory.rs` | Persistent markdown memory file — read/seed/update, 4KB cap |
| `world_model.rs` | Structured belief system: Belief, BeliefDomain, Confidence, ModelUpdate types + formatters |

## Safety Layers (7 deep)

1. **Rust guard** — hardcoded protected file list in `guard.rs`
2. **Shell heuristic** — guard checks on write/edit tool args
3. **System prompt** — instructs LLM to use file tools, not shell for file ops
4. **Pre-commit validation** — `cargo check` + `cargo test` before any commit
5. **Branch isolation** — changes on `vm/<instance-id>`, never on `main`
6. **Rollback** — `reset_to_last_good()` on health check failure
7. **Human gate** — cross-pollination to main requires PR review

## Non-Obvious Patterns

- Separate SQLite DB (`soul.db`) — does NOT share the gateway DB
- On Railway, `SOUL_DB_PATH` must point to persistent volume (`/data/soul.db`)
- Dormant mode: without `GEMINI_API_KEY`, still observes and records, skips LLM calls
- Default model: `gemini-3-flash-preview` (configurable via `GEMINI_MODEL_FAST` env var)
- Gemini 3+ requires `thoughtSignature` passback on function calls — handled in `llm.rs`
- Tool output truncated to 4KB per stream to stay within Gemini context limits
- Tools disabled via `SOUL_TOOLS_ENABLED=false`
- Coding disabled by default — requires `SOUL_CODING_ENABLED=true` + `INSTANCE_ID`
- Protected paths are hardcoded (not env-var) so the soul cannot bypass via shell
- Dynamic tool registry: `SOUL_DYNAMIC_TOOLS_ENABLED=false` by default, max 20 tools, meta-tools only in Code mode
- Dynamic tools execute via shell with `TOOL_ARGS` JSON + `TOOL_PARAM_{NAME}` env vars, respects existing timeouts
- `tool_registry.rs` is in PROTECTED_PREFIXES — soul cannot modify its own tool registry code
- Fork workflow: `SOUL_FORK_REPO` + `SOUL_UPSTREAM_REPO` enable push-to-fork + cross-fork PRs + issue creation
- Fork remote named "fork" is auto-configured on first push; origin stays as upstream reference
- Direct push mode (`SOUL_DIRECT_PUSH=true`): pushes to fork's main branch directly, triggering auto-deploy. Used for self-editing instances.
- Deep model: `SOUL_DIRECT_PUSH` + `SOUL_AUTONOMOUS_CODING` together use Gemini Pro (think model) instead of Flash for deeper reasoning
- Persistent memory: soul reads `/data/soul_memory.md` every cycle, can update via `update_memory` tool, seeded on first boot, 4KB cap
- Structured thought retrieval: salience-weighted when neuroplastic enabled (most important thoughts first), falls back to recency — Decision, Reasoning, Observation, Reflection, MemoryConsolidation (no ToolExecution)
- Tool calls are ephemeral — NOT recorded as thoughts, NOT fed back into context
- Memory consolidation: every 10 cycles, summarize last 20 substantive thoughts (including Reflection and Prediction) into a MemoryConsolidation thought (LongTerm tier)
- `register_endpoint` tool: full x402 payment flow (402 → sign → retry), Code mode only, requires `EVM_PRIVATE_KEY`
- `check_self` tool: whitelisted self-introspection (health, analytics, analytics/{slug}, soul/status), available in Observe/Chat/Code modes
- Per-endpoint reward signal: `RewardBreakdown` tracks new/growing/stagnant endpoints, replaces crude total_payments diff
- Neuroplastic memory: salience scoring (novelty 30%, prediction_error 25%, reward 25%, recency 10%, reinforcement 10%)
- Memory tiers: Sensory (0.3 decay/cycle, ~2 cycles), Working (0.95, ~90 cycles), LongTerm (0.995, near-permanent, never pruned)
- Prediction system: linear extrapolation of payments/revenue/endpoints/children, prediction error surfaced in prompt when >30%
- Hebbian reinforcement: recalled thoughts get a +0.05 strength boost; strength decays per tier each cycle
- Auto-promotion: sensory thoughts with salience >0.6 promoted to working tier after decay
- Pattern counting: content fingerprints (first 60 chars) tracked in `pattern_counts` table for novelty detection
- Schema migration: `PRAGMA user_version` based (v1: neuroplastic columns, v2: beliefs table, v3: goals table), ALTER TABLE for backward compat
- **Goals**: persistent multi-cycle intentions — `goals` table (id, description, status, priority, success_criteria, progress_notes, parent_goal_id, retry_count)
- Goal operations via same `update_beliefs` JSON protocol: create_goal, update_goal, complete_goal, abandon_goal
- Active goals shown in prompt every cycle under "## Active Goals"
- Goal cap: max 10 active goals (prevents goal sprawl)
- Goals survive between cycles — the soul checks active goals each cycle and decides whether to [CODE] to advance them
- Goal-reward loop: mutations linked to goals (goal_id on mutations table), reflection matches goals to endpoint reward signals, completed/abandoned goals become high-salience neuroplastic memories
- Boring streak warning references active goals: "You have N goals, pick one and [CODE]"
- Gated by `SOUL_NEUROPLASTIC` env var (default true) — harmless if false, just skips salience/decay/prediction
- **Feedback loop**: Observe → [CODE] → Phase 2 (Code mode, write/edit/commit) → Phase 3 (Reflection: check_self, verify, record learnings)
- **Fresh conversation per phase**: each phase gets its own conversation with a text summary of the previous phase's conclusion — prevents Phase 2/3 from re-sending all of Phase 1's tool outputs
- Phase 3 reflection: 5-tool budget, non-deep model, receives mutation context (SHA, pass/fail, files) + reward breakdown (new/growing/stagnant endpoints)
- Reflection salience is dynamic: base 0.5 + reward contribution (max 0.3), tied to actual endpoint performance
- Mutation history (last 5 commits with check/test pass/fail) and endpoint summary table (slug, price, requests, payments, revenue) provided in think context
- Reflection thoughts are retrieved in future cycles (salience-weighted) so the soul learns from past outcomes
- **World model**: structured beliefs (domain/subject/predicate/value/confidence) replace opaque thought strings for factual knowledge
- World model domains: Node, Endpoints, Codebase, Strategy, Self_ — each belief is a queryable fact
- Auto-beliefs: `sync_auto_beliefs()` runs every cycle before LLM, creates High-confidence beliefs from snapshot (node stats + per-endpoint metrics)
- Model update protocol: LLM outputs JSON array of `[{op: create/update/confirm/invalidate, ...}]` parsed by `apply_model_updates()`
- Graceful degradation: if LLM output doesn't contain valid JSON array, entire output treated as free-text reasoning (backward compat)
- Belief confidence decay: time-based — High→Medium after 5 cycles (~25min), Medium→Low after 10, Low→inactive after 20
- Beliefs table: `db.rs` migration v2, UNIQUE INDEX on `(domain, subject, predicate) WHERE active=1`, upsert bumps confirmation_count
- World model prompt: replaces raw snapshot JSON with structured view grouped by domain, shows changes since last cycle + pending questions
- Thoughts table still used for free-text reasoning, tool executions, mutations — beliefs supplement, don't replace

## Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `SOUL_TOOLS_ENABLED` | `true` | Enable/disable tool execution |
| `SOUL_MAX_TOOL_CALLS` | `25` | Max tool calls per cycle |
| `SOUL_TOOL_TIMEOUT_SECS` | `120` | Per-command timeout |
| `SOUL_WORKSPACE_ROOT` | `/data/workspace` | Workspace root for file tools (must NOT overlap SPA dir) |
| `SOUL_CODING_ENABLED` | `false` | Master switch for write/edit/commit tools |
| `SOUL_AUTONOMOUS_CODING` | `false` | Allow autonomous code changes in think cycles |
| `SOUL_AUTO_PROPOSE_TO_MAIN` | `false` | Auto-create PRs from vm branch to main |
| `GITHUB_TOKEN` | — | Token for git push/PR operations |
| `INSTANCE_ID` | — | VM instance ID for branch naming |
| `SOUL_DYNAMIC_TOOLS_ENABLED` | `false` | Enable dynamic tool registry (register/list/unregister at runtime) |
| `SOUL_FORK_REPO` | — | Fork repo for push (e.g. `compusophy-bot/tempo-x402`). Pushes go to "fork" remote |
| `SOUL_UPSTREAM_REPO` | — | Upstream repo for PRs/issues (e.g. `compusophy/tempo-x402`) |
| `SOUL_DIRECT_PUSH` | `false` | Push directly to fork's main branch (self-editing mode). Safety: cargo check + test still gate every commit |
| `SOUL_MEMORY_FILE` | `/data/soul_memory.md` | Path to persistent memory file (markdown, max 4KB) |
| `GATEWAY_URL` | — | Gateway URL for `register_endpoint` tool (falls back to `http://localhost:4023`) |
| `SOUL_NEUROPLASTIC` | `true` | Enable neuroplastic memory: salience scoring, tiered decay, prediction error |
| `SOUL_PRUNE_THRESHOLD` | `0.01` | Strength threshold below which non-long-term thoughts are pruned |

## If You're Changing...

- **LLM API**: `llm.rs` — model names, endpoint format, retry logic, thought_signature handling
- **Thinking loop**: `thinking.rs` — observe → think → tool loop → record cycle
- **Tool execution**: `tools.rs` — tool definitions, executor, all tool implementations
- **Protected files**: `guard.rs` — hardcoded list, do NOT make configurable via env
- **Git workflow**: `git.rs` — branch ops, auth, PR creation
- **Pre-commit validation**: `coding.rs` — cargo check/test pipeline
- **Agent modes**: `mode.rs` — mode enum, tool sets per mode, max_tool_calls
- **System prompts**: `prompts.rs` — per-mode prompt templates
- **Dynamic tool registry**: `tool_registry.rs` — meta-tools, dynamic tool execution, shell handlers
- **Persistent memory**: `persistent_memory.rs` — read/seed/update memory file, 4KB cap
- **Neuroplastic memory**: `neuroplastic.rs` — salience algorithm, tier assignment, prediction, decay rates
- **Database schema**: `db.rs` — `thoughts` + `soul_state` + `mutations` + `tools` + `pattern_counts` + `beliefs` + `goals` tables
- **World model**: `world_model.rs` — belief types + formatters; `thinking.rs` — sync_auto_beliefs, apply_model_updates; `prompts.rs` — world model view builder
- **Observer trait**: `observer.rs` — changing `NodeSnapshot` fields affects all implementors
- **Used by**: `x402-node` stores `Arc<SoulDatabase>` in `NodeState`, exposes via `GET /soul/status`, implements `NodeObserver` in `soul_observer.rs`
