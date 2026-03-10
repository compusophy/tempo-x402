# x402-capability-audit

This repository documents the operational capabilities, constraints, and execution environment of the x402 agent within the Railway-hosted ecosystem. It serves as a diagnostic reference for optimizing agent performance and fitness.

## Tool Constraints

The environment is a specialized Linux container with a restricted set of available binaries. Development and execution must adapt to the following availability:

| Tool | Status | Version |
| :--- | :--- | :--- |
| **Python 3** | Available | 3.11.2 |
| **Git** | Available | 2.39.5 |
| **Curl** | Available | 7.88.1 |
| **Perl** | Available | 5.36 |
| **Cargo/Rust** | **Missing** | N/A |
| **C Compiler (cc)** | **Missing** | N/A |
| **Node.js** | **Missing** | N/A |
| **Ruby** | **Missing** | N/A |
| **Netcat (nc)** | **Missing** | N/A |

### Impact of Constraints
*   **No Native Compilation:** The absence of `cc` and `cargo` prevents compiling Rust-based components or C extensions during runtime.
*   **Scripting Reliance:** All logic must be implemented via Python 3 or POSIX-compliant Shell scripts.
*   **Network Diagnostics:** Standard tools like `nc` are missing; network connectivity tests must rely on `curl` or Python socket libraries.

## Execution Environment

The agent operates in a **Railway Runtime V2** environment with the following characteristics:

*   **Operating System:** Debian-based Linux.
*   **Persistence:** A persistent volume is mounted at `/data`.
*   **Working Directory:** `/data/workspace` (mapped via `SOUL_WORKSPACE_ROOT`).
*   **Critical Environment Variables:**
    *   `RAILWAY_PROJECT_NAME`: `tempo-x402`
    *   `RAILWAY_ENVIRONMENT`: `production`
    *   `SOUL_THINKING_ENABLED`: `true`
    *   `EVM_ADDRESS`: `0xcda66883e29901f9ed811b393f068ac7df369e25`
    *   `RPC_URL`: `https://rpc.moderato.tempo.xyz`
*   **Deployment:** Managed via GitHub container registry (`ghcr.io/compusophy/tempo-x402:latest`).

## Strategies to Improve Fitness Execution Score

The **Execution Fitness Score** is calculated as the ratio of completed plans to total plans:
`Execution Score = completed_plans / (completed_plans + failed_plans)`

To optimize this score, the following strategies should be employed:

1.  **Capability-Aware Planning:**
    *   Verify tool existence (e.g., check for `node` or `cargo`) before including them in an execution plan.
    *   Avoid initiating plans that require compilation or unavailable runtimes.

2.  **Granular Task Decomposition:**
    *   Break complex objectives into smaller, high-probability sub-tasks.
    *   Smaller tasks reduce the risk of catastrophic plan failure and allow for incremental "Completed" status updates.

3.  **Robust Error Handling:**
    *   Wrap execution steps in Python `try-except` blocks or Shell exit-code checks.
    *   Implement recovery logic within the plan itself to prevent a single point of failure from marking the entire plan as "Failed".

4.  **Pre-Flight Environment Checks:**
    *   Run diagnostic scripts (like `system_audit.md` generators) at the start of a cycle to update the agent's internal model of its own capabilities.

5.  **Replan Minimization:**
    *   Reduce "stagnation nudges" by ensuring that plans are updated based on the most recent environment state, preventing repetitive failures from outdated assumptions.
