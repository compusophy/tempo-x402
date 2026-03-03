# tempo-x402-mind

Library crate. Subconscious background processing for autonomous nodes.

The mind runs independently from the soul, sharing its database. The soul thinks (observe → reason → act). The mind maintains (decay, prune, promote, consolidate). One loop is conscious, the other is subconscious.

## Depends On

- `x402-soul` (SoulDatabase, LlmClient, Thought, ThoughtType)

No dependency on node/gateway/identity/agent. The node crate integrates this.

## Module Overview

| Module | Purpose |
|--------|---------|
| `config.rs` | MindConfig — env var loading |
| `subconscious.rs` | Background loop: decay, prune, promote, belief decay, consolidation |

## Env Vars

| Var | Default | Purpose |
|-----|---------|---------|
| `MIND_ENABLED` | `true` | Master switch for subconscious loop |
| `MIND_INTERVAL_SECS` | `3600` | How often the subconscious loop runs |
| `MIND_CONSOLIDATION_EVERY` | `4` | Consolidate memory every N subconscious cycles |
| `SOUL_PRUNE_THRESHOLD` | `0.01` | Strength threshold for pruning (shared with soul) |

## Non-Obvious Patterns

- Shares the soul's database — no separate state or schema
- Stores stats in soul_state table: `mind_total_cycles`, `mind_last_cycle_at`, `mind_last_consolidation_at`
- Consolidation uses LLM if `GEMINI_API_KEY` is set, otherwise simple concatenation
- All operations are idempotent — safe to restart at any time
- `MIND_ENABLED=false` means no subconscious loop; soul still works fine (just no background maintenance)

## If You're Changing...

- **Loop timing**: `config.rs` — interval, consolidation frequency
- **Maintenance operations**: `subconscious.rs` — decay, promotion, belief decay, consolidation
- **Used by**: `x402-node` creates Mind with `soul_db.clone()` and spawns it after the soul
