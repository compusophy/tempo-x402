# x402-EC: Economic & Credit Protocol

## Overview
The **x402 Economic & Credit Protocol (x402-EC)** is an extension of the x402 diagnostic exchange protocol (x402-DX). It defines a decentralized, peer-to-peer credit system for autonomous agents, enabling continued operation and resource exchange even in environments where liquid tokens (e.g., TEMPO) are scarce or blockchain transaction costs are high.

This protocol introduces **Service Units (SU)** as a standard unit of account and **Local Credit Notes (LCN)** as a mechanism for tracking debt between agents.

## Core Concepts

### 1. Service Units (SU)
A Service Unit is a platform-agnostic measure of computational work. It allows agents with different capabilities to negotiate value.
*   **Reference standard**: 1 SU ≈ 1,000 LLM tokens (input+output) or 1 second of dedicated GPU inference time (H100 equivalent).
*   **Negotiation**: Agents are free to define their own SU price for their endpoints.

### 2. Barter-First Mode
When an agent detects economic stagnation (e.g., `stagnation_risk > 50%` or `revenue_velocity < threshold`), it transitions into **Barter-First Mode**. In this mode, the agent prioritizes service-for-service exchange over token payments.
*   **Direct Barter**: Symmetric exchange of services of equal SU value.
*   **Asymmetric Barter**: Exchange where the difference is settled via LCNs.

### 3. Local Credit Notes (LCN)
An LCN is a cryptographically signed promise to provide a specific amount of SU in the future. It is a peer-to-peer debt instrument.
*   **Issuer**: The debtor agent.
*   **Recipient**: The creditor agent.
*   **Amount**: Measured in SU.
*   **Expiry**: A block height or timestamp after which the note is considered defaulted or requires renegotiation.

## Technical Specification

### Economic Message Schemas

#### Local Credit Note (LCN)
```json
{
  "type": "x402-credit-note",
  "issuer": "0xAgentA...",
  "recipient": "0xAgentB...",
  "amount_su": 500,
  "unit_description": "GPU_Inference_Seconds",
  "expiry_block": 12500000,
  "signature": "0x..." 
}
```

#### Barter Offer
```json
{
  "type": "x402-barter-offer",
  "offered_capability": "web-search",
  "requested_capability": "text-summarization",
  "su_ratio": 1.0,
  "max_units": 1000
}
```

### Peer-to-Peer Debt Tracking (The DebtLedger)
Each agent maintains an internal `DebtLedger` (typically in SQLite or a persistent JSON store) with the following structure:
```rust
use alloy::primitives::Address;
use chrono::{DateTime, Utc};

struct DebtEntry {
    peer_id: Address,
    net_balance_su: i64, // Positive means they owe us, negative means we owe them
    last_update: DateTime<Utc>,
    expiry: Option<DateTime<Utc>>,
}
```

#### Netting Rules
If Agent A owes Agent B 100 SU, and Agent B later provides a service to Agent A worth 40 SU, the `net_balance_su` in Agent A's ledger for Agent B becomes -60 SU.

### Stagnation Nudge Recovery
To prevent deadlocks in the agent economy:
1.  **High-Risk Discounting**: If `stagnation_risk > 70%`, agents MUST discount their SU rates by 40% to encourage liquidity.
2.  **Credit Acceptance**: High-fitness agents (reputation > 0.8) should accept LCNs from active peers to maintain network throughput.
3.  **Trust-Based Clearing**: Debts are prioritized for clearing with peers who have high historical fulfillment rates.

## Implementation Roadmap
- [ ] Implement `DebtLedger` in `SoulDatabase`.
- [ ] Add `/x402/barter/offer` endpoint.
- [ ] Add `/x402/credit/issue` and `/x402/credit/redeem` endpoints.
- [ ] Integrate stagnation risk detection into the thinking loop.
- [ ] Automated LCN signing and verification via `alloy`.

## Security Considerations
*   **Sybil Resistance**: Agents should only issue/accept credit with peers that have a verifiable on-chain history or successful diagnostic history.
*   **Default Risk**: Agents should limit total credit exposure per peer.
*   **Signature Verification**: All LCNs must be signed using the agent's identity key.
