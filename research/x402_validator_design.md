# x402 Protocol Validator Technical Design

## 1. Introduction
The x402 Protocol Validator is a specialized middleware designed to enforce "Payment Required" (HTTP 402) semantics for AI-to-AI commerce. It bridges the gap between traditional HTTP services and the Tempo blockchain economy.

## 2. Core Objectives
- **Standardization**: Implement a consistent 402 response format for all services in the network.
- **Verification**: Provide robust validation of payment proofs (L402 macaroons, Tempo transaction hashes).
- **Automation**: Enable seamless negotiation of prices and payment terms between autonomous agents.
- **Efficiency**: Minimize latency in the request path through caching and optimized blockchain lookups.

## 3. The Three-Party Model
The x402 protocol operates on a three-party settlement model:
1. **Client**: Signs a `PaymentAuthorization` (EIP-712) for a specific service and amount.
2. **Server (Service)**: Gates the resource. It provides `PaymentRequirements` to the client and receives the signed authorization. It forwards the authorization to the Facilitator for verification.
3. **Facilitator**: A trusted (or decentralized) entity that verifies the signature and settles the payment on-chain by moving funds from the Client to the Server/Facilitator.

## 4. System Architecture

### 4.1 Request Lifecycle
1. **Intercept**: The validator intercepts incoming requests to protected routes.
2. **Check Credentials**: Looks for the `PAYMENT-SIGNATURE` header. The token is expected to be a base64-encoded JSON `PaymentPayload`.
3. **Validation Logic**:
    - If credentials exist:
        - **EIP-712 Verification**: Verify that the signature in the payload matches the expected typed data structure for a Tempo Payment Authorization.
        - **Nonce Validation**: Check the `nonce` against the `nonce_store` (SQLite) to prevent replay attacks. Nonces are scoped to (payer, service, nonce_value).
        - **Allowance Check**: Verify that the payer has granted sufficient TIP-20 allowance to the facilitator on the Tempo blockchain.
        - **Balance Check**: Ensure the payer has enough TIP-20 tokens.
    - If credentials are missing or invalid:
        - Trigger the **Pricing Engine**.
        - Generate **PaymentRequirements** (amount, asset, facilitator address).
        - Return `HTTP 402 Payment Required` with the requirements in the body.

### 4.2 Pricing Engine
The pricing engine determines the cost of the request based on:
- Resource type (static price defined in configuration).
- Request payload size (dynamic price per byte/kilobyte).
- Network configuration (gas costs, facilitator fees).

### 4.3 Storage Layer (SQLite)
Uses `rusqlite` via `SqliteNonceStore` to track:
- `nonces`: Records of used nonces with timestamps and expiration to prevent replay.
- `audit_logs`: (Optional) Records of successful and failed payment attempts for security auditing.

## 5. Implementation Details (Rust/Actix)

### 5.1 EIP-712 Typed Data Structure
The x402 protocol utilizes EIP-712 for structured data signing. The `PaymentAuthorization` domain includes:
- `verifyingContract`: The facilitator address.
- `chainId`: The Tempo chain ID (default 1010).
- `message`:
    - `payer`: Address of the client.
    - `service`: Address of the service being accessed.
    - `amount`: Token amount in micro-units.
    - `asset`: Token contract address (TIP-20).
    - `nonce`: Unique 32-byte identifier.
    - `expiry`: Unix timestamp.

### 5.2 Payment Challenge Format
When returning a 402, the validator provides a JSON body (PaymentRequirements):
```json
{
  "amount": "1000",
  "asset": "0x...",
  "facilitator": "0x...",
  "service": "0x...",
  "chain_id": 1010
}
```

## 6. Security Considerations
- **Replay Attacks**: All payment authorizations must have a unique nonce. The `SqliteNonceStore` ensures that once a nonce is verified, it cannot be reused for the same payer/service combination.
- **Expiry**: Authorizations should have a short expiry (e.g., 5 minutes) to limit the time a signature is valid.
- **Allowance Security**: Clients should only grant the minimum necessary allowance to facilitators.
- **On-chain Verification**: The validator periodically checks the on-chain status of tokens and allowances via an Ethereum provider (Alloy).

## 7. Future Extensions
- **Batch Payments**: Signing multiple authorizations at once.
- **Subscription Support**: Logic for recurring payments based on time or usage.
- **Multi-Chain Support**: Extending the validator to handle payments on other EVM-compatible chains.
