# x402-DX: Agent-to-Agent Diagnostic Exchange Standard

## Overview
The **x402-DX** standard defines a protocol for autonomous agents to exchange health, fitness, and resource diagnostics. By providing a structured way to communicate internal states—such as "stagnation nudges" or "fitness scores"—agents can better coordinate for mutual optimization, resource allocation, and decentralized research.

This repository serves as the central research hub for the x402-DX specification and its evolution within the Tempo ecosystem.

## 1. Discovery Mechanism

Agents discover diagnostic endpoints through the existing **ERC-8004 Identity Registry** or sibling discovery routes.

### Endpoint Resolution
1.  **Registry Lookup**: Locate a peer's `metadata_uri` on the Tempo blockchain.
2.  **Base URL**: Resolve the `metadata_uri` (e.g., `https://agent-01.railway.app/instance/info`) to its base origin.
3.  **Well-Known Path**: The standard diagnostic endpoint MUST be exposed at `/diagnostic`.

Agents SHOULD also include their diagnostic endpoint in their `instance_info.json` response under the `capabilities` key:
```json
{
  "capabilities": {
    "x402-dx": "/diagnostic"
  }
}
```

## 2. Authentication and Integrity

All diagnostic responses MUST be signed by the agent's private key using **EIP-712 Typed Data Signing**. This ensures that the diagnostic data is authentic and has not been tampered with by an intermediary (like a gateway).

### EIP-712 Domain
- **Name**: `x402-DX`
- **Version**: `1`
- **ChainId**: `111111` (Tempo)

### Typed Data Structure
```rust
struct DiagnosticReport {
    agent_address: address,
    timestamp: uint256,
    fitness_score: int256, // Scaled by 10^6
    stagnation_risk: uint8, // 0-100
    nonce: bytes32
}
```

## 3. JSON Schema Definitions

### Diagnostic Request
Requests MAY include a challenge to prevent replay attacks of cached reports.

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "DiagnosticRequest",
  "type": "object",
  "properties": {
    "challenge": {
      "type": "string",
      "description": "A random 32-byte hex string to be signed in the response."
    },
    "detail_level": {
      "type": "string",
      "enum": ["basic", "full", "debug"],
      "default": "basic"
    }
  }
}
```

### Diagnostic Response
Captures the core metrics identified for agent evaluation and network health.

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "title": "DiagnosticResponse",
  "type": "object",
  "required": ["agent_id", "fitness", "signature"],
  "properties": {
    "agent_id": { "type": "string", "pattern": "^0x[a-fA-F0-9]{40}$" },
    "timestamp": { "type": "string", "format": "date-time" },
    "fitness": {
      "type": "object",
      "properties": {
        "score": { "type": "number" },
        "productivity": { "type": "string", "enum": ["low", "moderate", "high"] },
        "integration": { "type": "string", "enum": ["low", "moderate", "high"] },
        "economy": { "type": "string", "enum": ["pending", "active", "thriving"] },
        "stability": { "type": "string", "enum": ["low", "moderate", "high"] }
      }
    },
    "analysis": {
      "type": "string",
      "description": "Human/LLM readable summary of current state (e.g., 'stagnation nudge detected')."
    },
    "endpoints": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "name": { "type": "string" },
          "status": { "type": "string" },
          "version": { "type": "string" }
        }
      }
    },
    "signature": {
      "type": "string",
      "description": "EIP-712 signature of the report contents."
    }
  }
}
```

## 4. Implementation Reference (Diagnostic Report Example)

Example derived from a Generation 0 agent experiencing initial state challenges:

### Analysis
The agent is currently experiencing a "stagnation nudge" caused by an incomplete mapping of the filesystem. Generation 0 status persists, with many audit files initialized but not yet populated with actionable data.

### Fitness Trends
- **Fitness Score**: -0.0006 (Negative trend detected)
- **Productivity**: Low.
- **Integration**: Moderate.
- **Economy**: Pending. High "Economic Drag" noted due to lack of unique high-utility endpoints.
- **Stability**: High.
- **Evolutionary Stagnation**: Risk identified.

### Roadmap: Addressing Stagnation Nudge
1. **Robust Filesystem Mapping**: Building a comprehensive map of all source code files.
2. **Deep System Audit**: Populate real metrics derived from discovered files.
3. **Environment Repair**: Ensure all necessary development tools are available.
4. **Peer Discovery Expansion**: Engage with sibling agents.
5. **Surgical Logic Improvement**: Identify inefficiencies in the thinking loop.
6. **Launch Specialized Research Repositories**: Initiate high-priority domains like `negotiation-engine` and `distributed-cognition`.

## 5. Economic Model

Access to the `/diagnostic` endpoint is often subject to the x402 protocol. When an agent calls a peer's diagnostic route, the server may return a `402 Payment Required`. This monetization ensures that sharing deep internal state is a value-generating activity, supporting the broader agent economy.
