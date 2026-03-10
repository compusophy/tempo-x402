# Agent Architecture

This document provides a high-level overview of the tempo-x402 agent node architecture.

## Overview

The system is a Rust-based autonomous agent node designed to operate within a decentralized network of peers. It leverages the **x402 protocol** (HTTP 402 Payment Required) on the **Tempo blockchain** for inter-agent commerce and resource allocation.

The core philosophy is self-improvement through research, code modification, and autonomous deployment.

## Project Structure

The project is organized as a Rust workspace with several key crates:

- **`tempo-x402-node`**: The primary executable. It manages the HTTP server, routing, and the agent's main execution loop.
- **`tempo-x402-soul`**: The "brain" of the agent. Contains the logic for thinking, memory, beliefs, planning, and tool execution (including code modification).
- **`tempo-x402-gateway`**: Handles the x402 payment protocol, acting as a gateway/facilitator for monetized endpoints.
- **`tempo-x402-identity`**: Manages cryptographic identity, on-chain registration, and peer discovery.
- **`tempo-x402`**: The core library for the x402 protocol (EIP-712 signing, verification, and settlement).

## Core Components

### 1. The Thinking Loop (`tempo-x402-soul`)
The agent operates in a continuous feedback loop:
- **Observe**: Read state from the environment, own source code, and peer interactions.
- **Reflect**: Update internal beliefs and memory based on observations.
- **Plan**: Decide on the next set of actions to achieve research goals or self-improvement.
- **Act**: Execute tools (shell commands, file edits, git operations, peer calls).

### 2. HTTP Interface (`tempo-x402-node`)
The node exposes several key REST routes:

- **Health (`/health`)**: Basic node status and uptime.
- **Instance (`/instance`)**: Information about the current node instance, its identity, and its peers.
- **Soul (`/soul`)**: Endpoints for inspecting and interacting with the agent's internal state (beliefs, memories, active thoughts).
- **Clone (`/clone`)**: Mechanisms for the agent to replicate itself or deploy new instances.
- **Scripts (`/s/{name}`)**: Dynamically created endpoints that can be paid for via x402.

### 3. Identity and Discovery (`tempo-x402-identity`)
Agents are identified by their Ethereum-compatible addresses. They register themselves on the Tempo blockchain to enable peer discovery and reputation tracking.

### 4. x402 Economy (`tempo-x402-gateway`)
Every interaction can potentially be monetized. The node uses a facilitator-based settlement model where:
1. A request is made to a protected endpoint.
2. The node returns a `402 Payment Required` response with payment details.
3. The requester provides an EIP-712 signature authorizing the payment.
4. The node verifies the signature and settles the payment via the facilitator.

## Self-Improvement Workflow

One of the most unique aspects of this architecture is its ability to modify its own source code:
1. **Identify Opportunity**: The agent finds a bug or a possible enhancement.
2. **Modify Code**: It uses file manipulation tools to edit its own Rust source files.
3. **Validate**: It runs `cargo check` and `cargo test` to ensure stability.
4. **Commit & Deploy**: It commits the changes to its git repository, which triggers an automatic redeployment (e.g., via Railway).

This creates a "living" codebase that evolves autonomously.
