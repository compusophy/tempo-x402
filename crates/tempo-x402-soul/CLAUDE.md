# tempo-x402-soul

Library crate. Agentic "soul" for x402 nodes: a plan-driven execution loop powered by Gemini with full coding agent capabilities.

## Architecture: Plan-Driven Execution

Replaces the old "prompt and pray" loop with deterministic plan execution:

```
Every N seconds:
  observe → read nudges → check stagnation → get active plan → execute next step → advance plan → housekeeping → sleep

  Steps that DON'T call LLM: ReadFile, SearchCode, ListDir, RunShell, Commit, CheckSelf
  Steps that DO call LLM:    GenerateCode, EditCode, Think (with focused prompt)

  No plan? → Call LLM ONCE to create plan for highest-priority goal
  No goals? → Call LLM ONCE to create goals
  Plan done? → Call LLM ONCE to reflect, then create next plan
```

Observes node state via `NodeObserver` trait, can read/write/edit files and execute shell commands via Gemini function calling, records thoughts and mutations to SQLite. Operates on `vm/<instance-id>` branches with pre-commit validation. Runs dormant (observe-only) without a Gemini API key.

## Depends On

- `x402` (core types only)
- `x402-wallet` (EIP-712 signing for `register_endpoint` tool)

No dependency on gateway/identity/agent/node. Communicates via `NodeObserver` trait — the node crate implements it.

## Module Overview

| Module | Purpose |
|--------|---------|
| `plan.rs` | **NEW**: Plan types (PlanStep, Plan, PlanStatus), PlanExecutor — deterministic step execution |
| `thinking.rs` | Main plan-driven loop: observe → plan → execute → advance |
| `prompts.rs` | 5 focused prompt builders: goal_creation, planning, code_generation, replan, reflection |
| `guard.rs` | Hardcoded protected file list — prevents self-bricking |
| `tools.rs` | Tool executor: shell, file read/write/edit, search, commit, PR + dynamic tool dispatch |
| `tool_registry.rs` | Dynamic tool registry: register/list/unregister tools at runtime, shell execution |
| `git.rs` | Branch-per-VM git workflow (ensure_branch, commit, push, PR, issues) with fork support |
| `coding.rs` | Pre-commit validation pipeline (cargo check → test → commit) |
| `mode.rs` | Agent modes (Observe, Chat, Code, Review) with per-mode tool sets |
| `llm.rs` | Gemini API client with thought_signature support |
| `chat.rs` | Session-based interactive chat with plan context injection |
| `db.rs` | SQLite: thoughts, soul_state, mutations, tools, pattern_counts, beliefs, goals, plans, nudges, **chat_sessions, chat_messages** tables |
| `memory.rs` | Thought types (Observation, Reasoning, Decision, Prediction, etc.) |
| `neuroplastic.rs` | Salience scoring, tiered memory decay, prediction error |
| `persistent_memory.rs` | Persistent markdown memory file — read/seed/update, 4KB cap |
| `world_model.rs` | Structured belief system: Belief, BeliefDomain, Confidence, ModelUpdate types |

## Plan-Driven Execution Flow

1. **Observe** — record snapshot, sync auto-beliefs from snapshot (ground truth)
2. **Read Nudges** — fetch unprocessed nudges (user messages, stagnation signals)
3. **Stagnation Check** — circuit breakers: abandon goals after 2 retries, reset all after 30 idle cycles
4. **Get/Create Plan** — check DB for active plan; if none, call LLM to create one (nudges + diagnostics in context)
5. **Execute Step** — run next step via PlanExecutor (mechanical or LLM-assisted)
6. **Handle Result** — advance plan on success, replan on failure (3 retries max), complete plan when all steps done
7. **Housekeeping** — increment cycle count, decay/promote thoughts (every 10 cycles), consolidate memory (every 40 cycles)

### Pacing
- Mechanical step → 30s (fast, keep progressing)
- LLM step → 120s
- Plan completed → 300s (time to create next plan)
- No goals → 600s

### PlanStep Types (9 variants)
**Mechanical (no LLM):** ReadFile, SearchCode, ListDir, RunShell, Commit, CheckSelf
**LLM-assisted:** GenerateCode, EditCode, Think

Each step can have `store_as` to accumulate results in plan context. LLM steps reference context via `context_keys`.

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
- DB schema: PRAGMA user_version based (v1: neuroplastic, v2: beliefs, v3: goals, v4: plans, v5: nudges, **v6: chat_sessions + chat_messages**)
- Plans table: id, goal_id, steps (JSON), current_step, status, context (JSON), replan_count
- Plan context accumulates step results (store_as keys) for use by later steps
- Replan limit: 3 attempts before plan is marked Failed
- LLM called only for: goal creation, plan creation, code steps (GenerateCode/EditCode), reflection, replanning
- Mechanical steps use ToolExecutor directly — no LLM overhead
- Dormant mode: without `GEMINI_API_KEY`, still observes and records, skips LLM calls
- Gemini 3+ requires `thoughtSignature` passback on function calls — handled in `llm.rs`
- Fork workflow: `SOUL_FORK_REPO` + `SOUL_UPSTREAM_REPO` enable push-to-fork + cross-fork PRs
- Direct push mode: `SOUL_DIRECT_PUSH=true` pushes to fork's main branch directly
- World model: structured beliefs (auto-synced from snapshot each cycle + LLM updates)
- Neuroplastic memory: salience scoring, tiered decay, prediction error
- Nudge queue: external signals (user, system, stagnation) prioritized into goal/plan creation
- Stagnation detection: per-goal retry limit (2), global idle limit (30 cycles without commit)
- **Chat sessions**: multi-turn conversation history via `chat_sessions` + `chat_messages` tables (replaces stateless per-request chat)
- **Plan approval gate**: `PendingApproval` status — plans pause for human approval when `SOUL_REQUIRE_PLAN_APPROVAL=true`, auto-approve after timeout
- **Plan context injection**: active plan progress, pending approvals, and active goals injected into every chat conversation
- **Plan control tools**: `approve_plan`, `reject_plan`, `request_plan` — available in Chat and Code modes, LLM handles intent naturally
- First-boot seed: 2 concrete starter goals injected when DB has zero goals ever (avoids LLM hallucination from zero context)
- Housekeeping: thought decay, promotion, belief decay (every 10 cycles), memory consolidation (every 40 cycles) — absorbed from deleted mind crate
- **Used by**: `x402-node` stores `Arc<SoulDatabase>` in `NodeState`, exposes via `GET /soul/status` (includes plan info + pending plan), `POST /soul/nudge`, `GET /soul/nudges`, `GET /soul/chat/sessions`, `GET /soul/chat/sessions/{id}`, `POST /soul/plan/approve`, `POST /soul/plan/reject`, `GET /soul/plan/pending`

## Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `SOUL_MAX_PLAN_STEPS` | `20` | Max steps in a plan |
| `SOUL_TOOLS_ENABLED` | `true` | Enable/disable tool execution |
| `SOUL_MAX_TOOL_CALLS` | `15` | Max tool calls per LLM step |
| `SOUL_TOOL_TIMEOUT_SECS` | `120` | Per-command timeout |
| `SOUL_WORKSPACE_ROOT` | `/data/workspace` | Workspace root for file tools |
| `SOUL_CODING_ENABLED` | `false` | Master switch for write/edit/commit tools |
| `SOUL_AUTONOMOUS_CODING` | `false` | Allow autonomous code changes |
| `SOUL_FORK_REPO` | — | Fork repo for push |
| `SOUL_UPSTREAM_REPO` | — | Upstream repo for PRs/issues |
| `SOUL_DIRECT_PUSH` | `false` | Push directly to fork's main branch |
| `SOUL_MEMORY_FILE` | `/data/soul_memory.md` | Path to persistent memory file |
| `GATEWAY_URL` | — | Gateway URL for `check_self`/`register_endpoint` tools |
| `SOUL_REQUIRE_PLAN_APPROVAL` | `false` | Pause plans for human approval before execution |
| `SOUL_PLAN_APPROVAL_TIMEOUT` | `30` | Minutes before auto-approving a pending plan |
| `SOUL_NEUROPLASTIC` | `true` | Enable neuroplastic memory |
| `SOUL_PRUNE_THRESHOLD` | `0.01` | Strength threshold for thought pruning |

## If You're Changing...

- **Plan execution**: `plan.rs` — step types, PlanExecutor, step dispatch
- **Thinking loop**: `thinking.rs` — observe → plan → execute → advance cycle
- **Prompts**: `prompts.rs` — 5 focused builders (goal_creation, planning, code_generation, replan, reflection)
- **Tool execution**: `tools.rs` — tool definitions, executor, all tool implementations
- **Protected files**: `guard.rs` — hardcoded list, do NOT make configurable via env
- **Git workflow**: `git.rs` — branch ops, auth, PR creation
- **Pre-commit validation**: `coding.rs` — cargo check/test pipeline
- **Database schema**: `db.rs` — plans table (v4), nudges table (v5), chat sessions/messages (v6), CRUD methods
- **Chat sessions**: `chat.rs` — session-based conversation, plan context builder
- **Plan approval**: `thinking.rs` — approval gate in plan_cycle; `plan.rs` — PendingApproval status; `tools.rs` — approve/reject/request tools
- **World model**: `world_model.rs` — belief types + formatters
- **Observer trait**: `observer.rs` — changing `NodeSnapshot` fields affects all implementors
