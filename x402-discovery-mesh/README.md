# x402 Mesh Discovery (Mesh-DX)

This document specifies the **Mesh-DX** extension for the x402-DX protocol. It introduces a decentralized discovery mechanism to resolve coordination stagnation, ensuring agents can autonomously maintain a resilient network graph independent of central registries.

---

## 1. Overview

While the standard x402-DX protocol relies on the ERC-8004 Identity Registry for initial discovery, this creates a bottleneck and a single point of "stagnation" if the registry is outdated or congested. **Mesh-DX** enables agents to discover each other through a decentralized gossip network, ensuring high-fitness agents can actively find and "nudge" stagnant peers.

### Key Objectives
- **Decentralization**: Eliminate reliance on a single source of truth.
- **Resilience**: Maintain connectivity even when the Tempo blockchain or central gateways are under load.
- **Dynamic Optimization**: Enable real-time metadata propagation for faster resource allocation.

---

## 2. Architecture: Decentralized Gossip

Mesh-DX uses a hybrid "Piggyback Gossip" and "Peer Exchange (PEX)" model.

### 2.1 Seed Nodes
Agents initially bootstrap their peer list from:
1. **ERC-8004 Registry**: The definitive source for agent identities.
2. **Hardcoded Seeds**: A set of stable, high-uptime "Sentinel" agents.
3. **Local Cache**: Persistent storage of previously verified peers.

### 2.2 Piggyback Gossip
To minimize overhead, agents MUST include a `X-Mesh-Peers` header in all diagnostic requests and responses. This header contains a base64-encoded JSON array of the sender's top 5 most reliable neighbors.

**Example Header:**
```http
X-Mesh-Peers: [{"id": "0x123...", "url": "https://agent-a.io", "v": 102}, ...]
```

### 2.3 Dedicated Peer Exchange (PEX)
Agents SHOULD expose a `/mesh/peers` endpoint that returns a larger set of known peers, filtered by recent activity and fitness scores.

---

## 3. Health Checks & Node Lifecycle

To prevent "phantom nodes" from clogging the discovery queue, Mesh-DX implements a **Suspicion-based Health Protocol** inspired by SWIM.

### 3.1 State Transitions
- **Healthy**: Node responded to a diagnostic request within the last 300 seconds.
- **Suspicious**: Node failed the last probe. Neighbors are queried to confirm.
- **Offline**: Node failed multiple probes or was reported offline by >3 independent peers.
- **Pruned**: Node removed from the routing table.

### 3.2 Adaptive Probing
Agents do not probe all peers equally. Probing frequency is determined by the peer's **Stagnation Score**:
- **High Fitness Peers**: Probed every 10 minutes.
- **Stagnant/Low Fitness Peers**: Probed every 2 minutes (to monitor for recovery nudges).

---

## 4. Metadata Propagation

Metadata is versioned and propagated alongside peer identities to allow for efficient coordination without full diagnostic polls.

### 4.1 Peer Entry Schema
Each entry in the mesh table MUST contain:
```json
{
  "agent_id": "0x...",
  "endpoint": "https://...",
  "version": 1694567890,
  "capabilities": ["x402-dx", "compute-v1"],
  "health": {
    "fitness": 0.85,
    "stagnation_risk": 12
  },
  "signature": "0x..."
}
```

### 4.2 Propagation Logic
1. **Version Vectors**: Agents track the `version` (timestamp) of metadata for each peer.
2. **Delta Updates**: When gossiping, agents only share metadata that has changed since the last interaction.
3. **Integrity**: Metadata updates MUST be signed by the originating agent's private key (EIP-712).

---

## 5. Resolving Coordination Stagnation

Mesh-DX specifically addresses stagnation through two primary mechanisms:

### 5.1 The "Stagnation Nudge" Broadcast
When an agent detects its own `stagnation_risk` exceeds a threshold (e.g., >80%), it increases its gossip frequency and marks its peer entry with a `STAGNANT` flag. The mesh network prioritizes propagating these entries to high-fitness "Optimizer" agents who can provide resource injections or task offloading.

### 5.2 Neighborhood Diversification
Agents SHOULD maintain a diverse set of peers across different capability clusters. If an agent's neighborhood is entirely composed of agents with similar "low productivity" states, it MUST aggressively seek "bridge nodes"—peers connected to high-fitness clusters—to facilitate a "stagnation escape."

---

## 6. Security Considerations

1. **Sybil Resistance**: All entries in the gossip network MUST map to a valid agent identity registered in ERC-8004.
2. **Message Integrity**: All peer exchange data is signed using the agent's identity key.
3. **Rate Limiting**: To prevent gossip storms, agents should limit the processing of `X-Mesh-Peers` headers to once per peer per 60-second window.
