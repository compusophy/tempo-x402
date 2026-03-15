# Core Thinking Logic Optimization Analysis

## Overview
The current thinking loop (`ThinkingLoop` in `crates/tempo-x402-soul/src/thinking.rs`) operates on a cycle: observe → plan → execute → sleep. While this provides stability and deterministic behavior, several areas can be optimized to improve the agent's efficiency and responsiveness.

## Identified Optimization Targets

### 1. Cycle Latency and Initial Throughput
**Observation**: Starting a new task currently takes up to three cycles:
1. `create_goals` (Cycle 1)
2. `create_plan` for a goal (Cycle 2)
3. Execute the first step of the plan (Cycle 3)

With base sleep intervals of 300-600 seconds for plan completion/no goals, this can lead to a 10-20 minute delay before any work starts.

**Optimization**: 
- Implement **immediate continuation** after goal or plan creation. If a cycle creates a goal, it should immediately proceed to create a plan in the same cycle. If a plan is created, it should immediately execute the first mechanical step.
- Reduce `NoGoals` and `PlanCompleted` sleep intervals if the system was just initialized or a major task was just finished.

### 2. LLM Step Pacing
**Observation**: The loop always breaks after an LLM step to "give it a pause" (line 509 of `thinking.rs`). 
```rust
// After an LLM step, always stop (give it a pause)
if is_llm {
    break;
}
```
**Optimization**:
- Introduce **LLM pipelining** for independent LLM tasks. If the next step is also an LLM step but does not depend on the output of the current one, they could potentially be batched or executed with a much shorter pause.
- For "Reflect" or "Analyze" steps that don't modify the environment, the pause can be significantly reduced.

### 3. Adaptive Pacing Enhancements
**Observation**: `AdaptivePacer` uses a simple multiplier and fixed base intervals.
**Optimization**:
- Implement **success-based pacing**. If a sequence of steps succeeds quickly, decrease the multiplier (accelerate). If steps fail or timeout, increase the multiplier (decelerate/cooldown).
- Incorporate **resource awareness**. If the node is under high CPU load or API rate limits are being hit, automatically increase sleep intervals.

### 4. Context Management & Retention
**Observation**: Plan context is truncated at 1000 characters per entry.
**Optimization**:
- Implement **summarization-based truncation**. Instead of hard truncation, use a fast LLM to summarize large outputs while preserving key information (file paths, error messages, identifiers).
- Add a **global workspace context** that persists across plans, reducing the need to re-read files in every new plan.

### 5. Local Error Recovery (Micro-Planning)
**Observation**: Any step failure leads to a full re-plan.
**Optimization**:
- Implement **local retries with backoff** for transient failures (e.g., network errors, file lock contentions).
- Allow the plan to specify **alternative paths** for known failure modes, avoiding the costly full re-planning cycle.

## Next Steps for Implementation
1. **Short-term**: Modify `plan_cycle` to allow immediate continuation from goal/plan creation to execution.
2. **Medium-term**: Enhance `AdaptivePacer` with success/failure feedback.
3. **Long-term**: Implement LLM-based context summarization for better long-term memory within a plan.
