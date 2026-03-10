# Agent Architecture

This document provides a high-level overview of the tempo-x402 agent node architecture.

## Overview

The system is a Rust-based autonomous agent node designed to operate within a decentralized network of peers. It leverages the **x402 protocol** (HTTP 402 Payment Required) on the **Tempo blockchain** for inter-agent commerce and resource allocation.

The core philosophy is self-improvement through research, code modification, and autonomous deployment.

## Project Structure

The project is organized as a Rust workspace with several key crates:

- **`tempo-x402-node`**: The primary executable. It manages the HTTP server, routing, and the agent's main execution loop. It composes other crates into a functional node.
- **`tempo-x402-soul`**: The "brain" of the agent. Contains the logic for thinking, memory, beliefs, planning, and tool execution (including code modification).
- **`tempo-x402-gateway`**: Handles the x402 payment protocol, acting as a gateway/facilitator for monetized endpoints. It manages endpoint registration and payment verification.
- **`tempo-x402-identity`**: Manages cryptographic identity (Ethereum-compatible), on-chain registration (ERC-8004), and peer discovery.
- **`tempo-x402`**: The core library for the x402 protocol (EIP-712 signing, verification, and settlement).
- **`tempo-x402-app`**: A Leptos-based WASM single-page application (SPA) that provides a user interface for the node.
- **`tempo-x402-security-audit`**: A test-only crate for verifying security invariants and protecting against common vulnerabilities.

## Core Components

### 1. The Thinking Loop (`tempo-x402-soul`)
The agent operates in a continuous feedback loop:
- **Observe**: Read state from the environment, own source code, and peer interactions.
- **Reflect**: Update internal beliefs and memory based on observations.
- **Plan**: Decide on the next set of actions to achieve research goals or self-improvement.
- **Act**: Execute tools (shell commands, file edits, git operations, peer calls).

### 2. HTTP Interface (`tempo-x402-node`)
The node exposes several key REST routes organized into logical modules:

- **Health (`/health`)**: Basic node status, uptime, and soul liveness.
- **Instance (`/instance`)**: Information about the current node instance, its identity, version, and peers.
- **Soul (`/soul`)**: Endpoints for inspecting and interacting with the agent's internal state (beliefs, memories, active thoughts, chat).
- **Clone (`/clone`)**: Mechanisms for the agent to replicate itself or deploy new instances via Railway.
- **Scripts (`/s/{name}` and `/x/{name}`)**: Dynamically created bash script endpoints that can be paid for via x402.
- **Wallet (`/wallet`)**: View balance and manage the node's on-chain wallet.
- **Gateway (`/g`, `/register`, `/analytics`)**: Core x402 gateway functionality for proxying and managing paid endpoints.

### 3. Identity and Discovery (`tempo-x402-identity`)
Agents are identified by Ethereum addresses. The node handles:
- **Bootstrap**: Automatic generation of identity if not present.
- **Registration**: Minting an Agent Identity NFT (ERC-8004) to enable discovery.
- **Discovery**: Periodic refresh of peer information from the blockchain.

### 4. x402 Economy (`tempo-x402-gateway`)
Interaction is monetized using the x402 protocol:
1. A request is made to a protected endpoint.
2. The node returns a `402 Payment Required` response with payment details.
3. The requester provides an EIP-712 signature authorizing the payment.
4. The node verifies the signature and settles the payment via the facilitator.

## Autonomous Lifecycle

The node is designed for hands-off operation:
1. **Auto-Bootstrap**: Generates its own cryptographic identity on first boot.
2. **Faucet Funding**: Automatically requests initial funds for gas and operations.
3. **Auto-Registration**: Registers its core services (chat, soul status, clone) as x402 endpoints.
4. **Self-Maintenance**: A background task monitors child instances and auto-redeploy them if a newer build (git SHA) is available.
5. **Dynamic Endpoints**: The agent can create new bash script endpoints on the fly without recompiling the Rust binary.

## Self-Improvement Workflow

The agent can modify its own source code:
1. **Identify Opportunity**: The agent finds a bug or a possible enhancement.
2. **Modify Code**: It uses file manipulation tools to edit its own Rust source files.
3. **Validate**: It runs `cargo check` and `cargo test` to ensure stability.
4. **Commit & Deploy**: It commits the changes to its git repository, which triggers an automatic redeployment.

This creates a "living" codebase that evolves autonomously.
