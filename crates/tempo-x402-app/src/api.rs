//! API utilities for making paid requests and gateway interactions

#![allow(dead_code)]

use crate::{PaymentRequirements, SettleResponse, WalletMode, WalletState};
use base64::Engine;
use gloo_net::http::Request;
use serde::{Deserialize, Serialize};

/// Gateway URL — empty string means same-origin (SPA served by gateway).
/// Override at compile time via GATEWAY_URL env var for dev/testing.
const GATEWAY_URL: &str = {
    match option_env!("GATEWAY_URL") {
        Some(url) => url,
        None => "",
    }
};

/// Get the gateway base URL for API calls.
pub fn gateway_base_url() -> &'static str {
    GATEWAY_URL
}

/// 402 Payment Required response body
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PaymentRequiredBody {
    #[serde(rename = "x402Version")]
    pub x402_version: u32,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Payment payload to send in PAYMENT-SIGNATURE header
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u32,
    pub payload: PaymentData,
}

/// Payment data for EIP-712 signing
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentData {
    pub from: String,
    pub to: String,
    pub value: String,
    pub token: String,
    pub valid_after: u64,
    pub valid_before: u64,
    pub nonce: String,
    pub signature: String,
}

/// Make a paid request through the gateway.
///
/// 1. GET the endpoint -> if 402, parse payment requirements
/// 2. Sign payment (MetaMask for browser wallet, WalletSigner for demo/embedded)
/// 3. Retry with PAYMENT-SIGNATURE header
/// 4. Return body + settlement info
pub async fn make_paid_request(
    wallet: &WalletState,
) -> Result<(String, Option<SettleResponse>), String> {
    // Use the demo endpoint (paid via gateway proxy)
    let url = format!("{}/g/demo", GATEWAY_URL);

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    // If not 402, return directly (free endpoint)
    if resp.status() != 402 {
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read body: {}", e))?;
        return Ok((body, None));
    }

    // Parse 402 payment requirements
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read 402 body: {}", e))?;
    let payment_body: PaymentRequiredBody =
        serde_json::from_str(&body_text).map_err(|e| format!("Failed to parse 402: {}", e))?;

    if payment_body.accepts.is_empty() {
        return Err("No payment schemes accepted".to_string());
    }

    let requirements = &payment_body.accepts[0];

    // Sign the payment based on wallet mode
    let payment_header = sign_for_wallet(wallet, requirements).await?;

    // Retry with payment signature
    let paid_resp = Request::get(&url)
        .header("PAYMENT-SIGNATURE", &payment_header)
        .send()
        .await
        .map_err(|e| format!("Paid request failed: {}", e))?;

    let settle = paid_resp
        .headers()
        .get("payment-response")
        .as_ref()
        .and_then(|s| {
            // Handle HMAC-signed format: "base64payload.hmac_hex"
            let payload_part = s.split('.').next().unwrap_or(s);
            base64::engine::general_purpose::STANDARD
                .decode(payload_part)
                .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<SettleResponse>(&bytes).ok());

    let result_body = paid_resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    Ok((result_body, settle))
}

/// Make a paid request to a specific gateway endpoint slug.
pub async fn make_paid_endpoint_request(
    wallet: &WalletState,
    slug: &str,
    path: &str,
) -> Result<(String, Option<SettleResponse>), String> {
    let url = if path.is_empty() {
        format!("{}/g/{}", GATEWAY_URL, slug)
    } else {
        format!("{}/g/{}/{}", GATEWAY_URL, slug, path)
    };

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if resp.status() != 402 {
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read body: {}", e))?;
        let settle = None;
        return Ok((body, settle));
    }

    // Parse 402
    let body_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read 402 body: {}", e))?;
    let payment_body: PaymentRequiredBody =
        serde_json::from_str(&body_text).map_err(|e| format!("Failed to parse 402: {}", e))?;

    if payment_body.accepts.is_empty() {
        return Err("No payment schemes accepted".to_string());
    }

    let requirements = &payment_body.accepts[0];
    let payment_header = sign_for_wallet(wallet, requirements).await?;

    let paid_resp = Request::get(&url)
        .header("PAYMENT-SIGNATURE", &payment_header)
        .send()
        .await
        .map_err(|e| format!("Paid request failed: {}", e))?;

    let settle = paid_resp
        .headers()
        .get("payment-response")
        .as_ref()
        .and_then(|s| {
            let payload_part = s.split('.').next().unwrap_or(s);
            base64::engine::general_purpose::STANDARD
                .decode(payload_part)
                .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<SettleResponse>(&bytes).ok());

    let result_body = paid_resp
        .text()
        .await
        .map_err(|e| format!("Failed to read response: {}", e))?;

    Ok((result_body, settle))
}

/// Sign a payment based on the current wallet mode.
async fn sign_for_wallet(
    wallet: &WalletState,
    requirements: &PaymentRequirements,
) -> Result<String, String> {
    match wallet.mode {
        WalletMode::MetaMask => sign_with_metamask(wallet, requirements).await,
        WalletMode::DemoKey | WalletMode::Embedded => sign_with_local_key(wallet, requirements),
        WalletMode::Disconnected => Err("Wallet not connected".to_string()),
    }
}

/// Sign using MetaMask via browser ethereum provider.
async fn sign_with_metamask(
    wallet: &WalletState,
    requirements: &PaymentRequirements,
) -> Result<String, String> {
    let address = wallet.address.as_deref().ok_or("No address")?;

    let now_secs = (js_sys::Date::now() / 1000.0) as u64;
    let valid_after = now_secs.saturating_sub(60);
    let valid_before = now_secs.saturating_add(requirements.max_timeout_seconds);
    let nonce = random_nonce();

    let domain = serde_json::json!({
        "name": "x402-tempo",
        "version": "1",
        "chainId": 42431,
        "verifyingContract": requirements.asset
    });

    let types = serde_json::json!({
        "EIP712Domain": [
            {"name": "name", "type": "string"},
            {"name": "version", "type": "string"},
            {"name": "chainId", "type": "uint256"},
            {"name": "verifyingContract", "type": "address"}
        ],
        "PaymentAuthorization": [
            {"name": "from", "type": "address"},
            {"name": "to", "type": "address"},
            {"name": "value", "type": "uint256"},
            {"name": "token", "type": "address"},
            {"name": "validAfter", "type": "uint256"},
            {"name": "validBefore", "type": "uint256"},
            {"name": "nonce", "type": "bytes32"}
        ]
    });

    let message = serde_json::json!({
        "from": address,
        "to": requirements.pay_to,
        "value": requirements.amount,
        "token": requirements.asset,
        "validAfter": valid_after.to_string(),
        "validBefore": valid_before.to_string(),
        "nonce": nonce
    });

    let signature = crate::wallet::sign_typed_data(address, &domain, &types, &message).await?;

    // Build PaymentPayload and base64-encode it
    let payload = PaymentPayload {
        x402_version: 1,
        payload: PaymentData {
            from: address.to_string(),
            to: requirements.pay_to.clone(),
            value: requirements.amount.clone(),
            token: requirements.asset.clone(),
            valid_after,
            valid_before,
            nonce: nonce.clone(),
            signature,
        },
    };

    let json = serde_json::to_string(&payload).map_err(|e| format!("serialize failed: {}", e))?;
    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        json,
    ))
}

/// Sign using a local private key (demo or embedded wallet).
fn sign_with_local_key(
    wallet: &WalletState,
    requirements: &PaymentRequirements,
) -> Result<String, String> {
    let private_key = wallet
        .private_key
        .as_deref()
        .ok_or("No private key available")?;

    let signer = x402_wallet::WalletSigner::new(private_key)?;

    let wallet_req = x402_wallet::PaymentRequirements {
        scheme: requirements.scheme.clone(),
        network: requirements.network.clone(),
        price: requirements.price.clone(),
        asset: requirements.asset.clone(),
        amount: requirements.amount.clone(),
        pay_to: requirements.pay_to.clone(),
        max_timeout_seconds: requirements.max_timeout_seconds,
        description: requirements.description.clone(),
    };

    let now_secs = (js_sys::Date::now() / 1000.0) as u64;
    signer.sign_payment(&wallet_req, now_secs)
}

/// Register an endpoint on the gateway
pub async fn register_endpoint(
    slug: &str,
    target_url: &str,
    price: &str,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "slug": slug,
        "target_url": target_url,
        "price": price
    });

    let resp = Request::post(&format!("{}/register", GATEWAY_URL))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).map_err(|e| format!("Failed to serialize body: {}", e))?)
        .map_err(|e| format!("Failed to build request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if resp.status() == 402 {
        return Err("Payment required - connect wallet to pay registration fee".to_string());
    }

    if !resp.ok() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Registration failed: {}", err));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// List all registered endpoints
pub async fn list_endpoints() -> Result<Vec<serde_json::Value>, String> {
    let resp = Request::get(&format!("{}/endpoints", GATEWAY_URL))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let endpoints = data["endpoints"].as_array().cloned().unwrap_or_default();

    Ok(endpoints)
}

/// Fetch analytics data from the gateway
pub async fn fetch_analytics() -> Result<serde_json::Value, String> {
    let resp = Request::get(&format!("{}/analytics", GATEWAY_URL))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Get details of a specific endpoint
pub async fn get_endpoint(slug: &str) -> Result<serde_json::Value, String> {
    let resp = Request::get(&format!("{}/endpoints/{}", GATEWAY_URL, slug))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if resp.status() == 404 {
        return Err("Endpoint not found".to_string());
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Call a proxied endpoint through the gateway (no payment)
pub async fn call_endpoint(
    slug: &str,
    path: &str,
) -> Result<(String, Option<SettleResponse>), String> {
    let url = if path.is_empty() {
        format!("{}/g/{}", GATEWAY_URL, slug)
    } else {
        format!("{}/g/{}/{}", GATEWAY_URL, slug, path)
    };

    let resp = Request::get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if resp.status() == 402 {
        return Err("Payment required - use make_paid_endpoint_request instead".to_string());
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    let settle = resp
        .headers()
        .get("payment-response")
        .as_ref()
        .and_then(|s| {
            let payload_part = s.split('.').next().unwrap_or(s);
            base64::engine::general_purpose::STANDARD
                .decode(payload_part)
                .ok()
        })
        .and_then(|bytes| serde_json::from_slice::<SettleResponse>(&bytes).ok());

    Ok((body, settle))
}

/// Send a chat message to the soul with optional session tracking.
pub async fn send_soul_chat(
    message: &str,
    session_id: Option<&str>,
) -> Result<serde_json::Value, String> {
    let mut body = serde_json::json!({ "message": message });
    if let Some(sid) = session_id {
        body["session_id"] = serde_json::json!(sid);
    }

    let resp = Request::post(&format!("{}/soul/chat", GATEWAY_URL))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).map_err(|e| format!("Failed to serialize: {}", e))?)
        .map_err(|e| format!("Failed to build request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.ok() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Chat failed (HTTP {}): {}", resp.status(), err));
    }

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// List recent chat sessions.
pub async fn list_chat_sessions() -> Result<serde_json::Value, String> {
    let resp = Request::get(&format!("{}/soul/chat/sessions", GATEWAY_URL))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Get the pending plan awaiting approval.
pub async fn get_pending_plan() -> Result<serde_json::Value, String> {
    let resp = Request::get(&format!("{}/soul/plan/pending", GATEWAY_URL))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Approve a pending plan.
pub async fn approve_plan(plan_id: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({ "plan_id": plan_id });

    let resp = Request::post(&format!("{}/soul/plan/approve", GATEWAY_URL))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).map_err(|e| format!("Failed to serialize: {}", e))?)
        .map_err(|e| format!("Failed to build request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Reject a pending plan with optional reason.
pub async fn reject_plan(plan_id: &str, reason: Option<&str>) -> Result<serde_json::Value, String> {
    let mut body = serde_json::json!({ "plan_id": plan_id });
    if let Some(r) = reason {
        body["reason"] = serde_json::json!(r);
    }

    let resp = Request::post(&format!("{}/soul/plan/reject", GATEWAY_URL))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).map_err(|e| format!("Failed to serialize: {}", e))?)
        .map_err(|e| format!("Failed to build request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Fetch soul status from the node
pub async fn fetch_soul_status() -> Result<serde_json::Value, String> {
    let resp = Request::get(&format!("{}/soul/status", GATEWAY_URL))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))
}

/// Send a nudge to the soul
pub async fn send_nudge(message: &str, priority: u32) -> Result<(), String> {
    let body = serde_json::json!({ "message": message, "priority": priority });

    let resp = Request::post(&format!("{}/soul/nudge", GATEWAY_URL))
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body).map_err(|e| format!("Failed to serialize: {}", e))?)
        .map_err(|e| format!("Failed to build request: {}", e))?
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.ok() {
        let err = resp.text().await.unwrap_or_default();
        return Err(format!("Nudge failed (HTTP {}): {}", resp.status(), err));
    }

    Ok(())
}

/// Generate random nonce (32 bytes as hex string)
fn random_nonce() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("getrandom failed");
    format!("0x{}", hex::encode(&bytes))
}

/// Simple hex encoding
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{:02x}", b)).collect()
    }
}
