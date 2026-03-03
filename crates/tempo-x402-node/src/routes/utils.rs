use actix_web::{get, post, web, HttpRequest, HttpResponse, Responder};
use alloy::primitives::Address;
use alloy::providers::Provider;
use serde::Deserialize;
use serde_json::Value;
use std::str::FromStr;

use crate::state::NodeState;

#[derive(Deserialize)]
pub struct NonceRequest {
    pub address: String,
}

#[get("/network-stats")]
pub async fn network_stats(state: web::Data<NodeState>) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let (block_number, chain_id, gas_price) = tokio::join!(
        provider.get_block_number(),
        provider.get_chain_id(),
        provider.get_gas_price(),
    );

    HttpResponse::Ok().json(serde_json::json!({
        "block_number": block_number.ok(),
        "chain_id": chain_id.ok(),
        "gas_price": gas_price.ok(),
    }))
}

#[get("/echo-ip")]
pub async fn echo_ip(req: HttpRequest) -> impl Responder {
    let connection_info = req.connection_info();
    let ip = connection_info.realip_remote_addr().unwrap_or("unknown");
    HttpResponse::Ok().json(serde_json::json!({ "ip": ip }))
}

#[get("/headers")]
pub async fn headers(req: HttpRequest) -> impl Responder {
    let mut headers = serde_json::Map::new();
    for (name, value) in req.headers() {
        if let Ok(val_str) = value.to_str() {
            headers.insert(name.to_string(), Value::String(val_str.to_string()));
        }
    }
    HttpResponse::Ok().json(headers)
}

#[post("/json-validator")]
pub async fn json_validator(body: String) -> impl Responder {
    match serde_json::from_str::<Value>(&body) {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({ "valid": true })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "valid": false, "error": e.to_string() })),
    }
}

#[post("/hex-converter")]
pub async fn hex_converter(body: String) -> impl Responder {
    if let Ok(decoded) = alloy::hex::decode(body.trim()) {
        HttpResponse::Ok().json(serde_json::json!({
            "action": "decode",
            "result": String::from_utf8_lossy(&decoded).to_string()
        }))
    } else {
        HttpResponse::Ok().json(serde_json::json!({
            "action": "encode",
            "result": alloy::hex::encode(body.trim())
        }))
    }
}

#[post("/estimate-gas")]
pub async fn estimate_gas(
    state: web::Data<NodeState>,
    body: web::Json<serde_json::Value>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    // Body should be a JSON object representing a TransactionRequest
    let tx_req: alloy::rpc::types::TransactionRequest = match serde_json::from_value(body.into_inner()) {
        Ok(req) => req,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid transaction request: {}", e) }))
        }
    };

    match provider.estimate_gas(tx_req).await {
        Ok(gas) => HttpResponse::Ok().json(serde_json::json!({ "gas_limit": gas })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[post("/get-nonce")]
pub async fn get_nonce(
    state: web::Data<NodeState>,
    body: web::Json<NonceRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let address = match Address::from_str(&body.address) {
        Ok(a) => a,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid address: {}", e) }))
        }
    };

    match provider.get_transaction_count(address).await {
        Ok(nonce) => HttpResponse::Ok().json(serde_json::json!({ "nonce": nonce })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct BalanceRequest {
    pub address: String,
    pub token: Option<String>,
}

#[derive(Deserialize)]
pub struct VerifySignatureRequest {
    pub address: String,
    pub message: String,
    pub signature: String,
}

#[derive(Deserialize)]
pub struct TransactionRequest {
    pub hash: String,
}

#[derive(Deserialize)]
pub struct AllowanceRequest {
    pub owner: String,
    pub spender: String,
    pub token: String,
}

#[derive(Deserialize)]
pub struct EthCallRequest {
    pub to: String,
    pub data: String,
}

#[derive(Deserialize)]
pub struct AbiEncodeRequest {
    pub signature: String,
    pub args: Vec<Value>,
}

#[derive(Deserialize)]
pub struct TxBuilderRequest {
    pub from: String,
    pub to: String,
    pub signature: String,
    pub args: Vec<Value>,
    pub value: Option<String>,
}

#[post("/get-balance")]
pub async fn get_balance(
    state: web::Data<NodeState>,
    body: web::Json<BalanceRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let address = match Address::from_str(&body.address) {
        Ok(a) => a,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid address: {}", e) }))
        }
    };

    if let Some(token_str) = &body.token {
        let token_address = match Address::from_str(token_str) {
            Ok(a) => a,
            Err(e) => {
                return HttpResponse::BadRequest()
                    .json(serde_json::json!({ "error": format!("Invalid token address: {}", e) }))
            }
        };

        // Call balanceOf(address) on the token contract
        // balanceOf selector: 0x70a08231
        let mut data = Vec::with_capacity(68);
        data.extend_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(address.as_slice());

        let tx = alloy::rpc::types::TransactionRequest::default()
            .to(Some(token_address))
            .input(alloy::rpc::types::TransactionInput::new(data.into()));

        match provider.call(tx).await {
            Ok(bytes) => {
                if bytes.len() >= 32 {
                    let balance = alloy::primitives::U256::from_be_slice(&bytes[0..32]);
                    HttpResponse::Ok().json(serde_json::json!({ "balance": balance.to_string(), "token": token_str }))
                } else {
                    HttpResponse::InternalServerError().json(serde_json::json!({ "error": "Invalid response from token contract" }))
                }
            }
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
        }
    } else {
        match provider.get_balance(address).await {
            Ok(balance) => HttpResponse::Ok().json(serde_json::json!({ "balance": balance.to_string(), "token": "native" })),
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
        }
    }
}

#[post("/verify-signature")]
pub async fn verify_signature(body: web::Json<VerifySignatureRequest>) -> impl Responder {
    let address = match Address::from_str(&body.address) {
        Ok(a) => a,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid address: {}", e) }))
        }
    };

    let sig_bytes = match alloy::hex::decode(&body.signature) {
        Ok(b) => b,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid signature hex: {}", e) }))
        }
    };

    let signature = match alloy::primitives::Signature::try_from(sig_bytes.as_slice()) {
        Ok(s) => s,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid signature format: {}", e) }))
        }
    };

    // Try verifying as a personal_sign message first (prefixed with "\x19Ethereum Signed Message:\n")
    let message_bytes = if let Ok(bytes) = alloy::hex::decode(body.message.trim()) {
        bytes
    } else {
        body.message.as_bytes().to_vec()
    };

    match signature.recover_address_from_msg(&message_bytes) {
        Ok(recovered) => {
            HttpResponse::Ok().json(serde_json::json!({
                "recovered": recovered.to_string(),
                "matches": recovered == address
            }))
        }
        Err(e) => {
            HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Recovery failed: {}", e) }))
        }
    }
}

#[post("/get-transaction")]
pub async fn get_transaction(
    state: web::Data<NodeState>,
    body: web::Json<TransactionRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let hash = match alloy::primitives::TxHash::from_str(&body.hash) {
        Ok(h) => h,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid transaction hash: {}", e) }))
        }
    };

    match provider.get_transaction_by_hash(hash).await {
        Ok(tx) => HttpResponse::Ok().json(serde_json::json!({ "transaction": tx })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[post("/get-allowance")]
pub async fn get_allowance(
    state: web::Data<NodeState>,
    body: web::Json<AllowanceRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let owner = match Address::from_str(&body.owner) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid owner address: {}", e) })),
    };
    let spender = match Address::from_str(&body.spender) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid spender address: {}", e) })),
    };
    let token = match Address::from_str(&body.token) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid token address: {}", e) })),
    };

    // allowance(owner, spender) selector: 0xdd62ed3e
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&[0xdd, 0x62, 0xed, 0x3e]);
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(owner.as_slice());
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(spender.as_slice());

    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(Some(token))
        .input(alloy::rpc::types::TransactionInput::new(data.into()));

    match provider.call(tx).await {
        Ok(bytes) => {
            if bytes.len() >= 32 {
                let allowance = alloy::primitives::U256::from_be_slice(&bytes[0..32]);
                HttpResponse::Ok().json(serde_json::json!({ "allowance": allowance.to_string(), "token": body.token, "owner": body.owner, "spender": body.spender }))
            } else {
                HttpResponse::InternalServerError().json(serde_json::json!({ "error": "Invalid response from token contract" }))
            }
        }
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[derive(Deserialize)]
pub struct BlockRequest {
    pub number: Option<u64>,
    pub hash: Option<String>,
    pub full: Option<bool>,
}

#[post("/eth-call")]
pub async fn eth_call(
    state: web::Data<NodeState>,
    body: web::Json<EthCallRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    let to = match Address::from_str(&body.to) {
        Ok(a) => a,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid 'to' address: {}", e) }))
        }
    };

    let data = match alloy::hex::decode(&body.data) {
        Ok(d) => d,
        Err(e) => {
            return HttpResponse::BadRequest()
                .json(serde_json::json!({ "error": format!("Invalid 'data' hex: {}", e) }))
        }
    };

    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(Some(to))
        .input(alloy::rpc::types::TransactionInput::new(data.into()));

    match provider.call(tx).await {
        Ok(bytes) => HttpResponse::Ok().json(serde_json::json!({ "result": alloy::hex::encode(bytes) })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
    }
}

#[post("/get-block")]
pub async fn get_block(
    state: web::Data<NodeState>,
    body: web::Json<BlockRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    if let Some(hash_str) = &body.hash {
        let hash = match alloy::primitives::BlockHash::from_str(hash_str) {
            Ok(h) => h,
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid hash: {}", e) })),
        };
        match provider.get_block_by_hash(hash, body.full.unwrap_or(false).into()).await {
            Ok(block) => HttpResponse::Ok().json(serde_json::json!({ "block": block })),
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
        }
    } else {
        let number = match body.number {
            Some(n) => alloy::rpc::types::BlockNumberOrTag::Number(n),
            None => alloy::rpc::types::BlockNumberOrTag::Latest,
        };
        match provider.get_block_by_number(number, body.full.unwrap_or(false).into()).await {
            Ok(block) => HttpResponse::Ok().json(serde_json::json!({ "block": block })),
            Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e.to_string() })),
        }
    }
}

#[post("/keccak256")]
pub async fn keccak256(body: String) -> impl Responder {
    let input = if let Ok(bytes) = alloy::hex::decode(body.trim()) {
        bytes
    } else {
        body.trim().as_bytes().to_vec()
    };
    let hash = alloy::primitives::keccak256(&input);
    HttpResponse::Ok().json(serde_json::json!({
        "input": body.trim(),
        "hash": hash.to_string()
    }))
}

#[post("/abi-encode")]
pub async fn abi_encode(body: web::Json<AbiEncodeRequest>) -> impl Responder {
    use alloy::dyn_abi::Specifier;
    
    // Parse the function signature
    let func = match alloy::dyn_abi::Function::parse(&body.signature) {
        Ok(f) => f,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid signature: {}", e) })),
    };

    // Convert JSON values to DynSolValue
    let mut sol_args = Vec::new();
    if body.args.len() != func.inputs.len() {
        return HttpResponse::BadRequest().json(serde_json::json!({ 
            "error": format!("Argument count mismatch: expected {}, got {}", func.inputs.len(), body.args.len()) 
        }));
    }

    for (i, (arg, input)) in body.args.iter().zip(func.inputs.iter()).enumerate() {
        match input.ty.coerce_json(arg) {
            Ok(val) => sol_args.push(val),
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ 
                "error": format!("Failed to parse argument {}: {}", i, e) 
            })),
        }
    }

    match func.abi_encode_input(&sol_args) {
        Ok(encoded) => HttpResponse::Ok().json(serde_json::json!({ "result": alloy::hex::encode(encoded) })),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Encoding failed: {}", e) })),
    }
}

#[post("/tx-builder")]
pub async fn tx_builder(
    state: web::Data<NodeState>,
    body: web::Json<TxBuilderRequest>,
) -> impl Responder {
    let facilitator = match state.gateway.facilitator.as_ref() {
        Some(f) => f,
        None => {
            return HttpResponse::ServiceUnavailable()
                .json(serde_json::json!({ "error": "Facilitator not enabled" }))
        }
    };

    let provider = facilitator.facilitator.provider();

    // 1. ABI Encode
    use alloy::dyn_abi::Specifier;
    let func = match alloy::dyn_abi::Function::parse(&body.signature) {
        Ok(f) => f,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid signature: {}", e) })),
    };

    if body.args.len() != func.inputs.len() {
        return HttpResponse::BadRequest().json(serde_json::json!({ 
            "error": format!("Argument count mismatch: expected {}, got {}", func.inputs.len(), body.args.len()) 
        }));
    }

    let mut sol_args = Vec::new();
    for (i, (arg, input)) in body.args.iter().zip(func.inputs.iter()).enumerate() {
        match input.ty.coerce_json(arg) {
            Ok(val) => sol_args.push(val),
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ 
                "error": format!("Failed to parse argument {}: {}", i, e) 
            })),
        }
    }

    let data = match func.abi_encode_input(&sol_args) {
        Ok(encoded) => encoded,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Encoding failed: {}", e) })),
    };

    // 2. Parse addresses and value
    let from = match Address::from_str(&body.from) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid 'from' address: {}", e) })),
    };
    let to = match Address::from_str(&body.to) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid 'to' address: {}", e) })),
    };
    let value = if let Some(v_str) = &body.value {
        match alloy::primitives::U256::from_str(v_str) {
            Ok(v) => Some(v),
            Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Invalid value: {}", e) })),
        }
    } else {
        None
    };

    // 3. Get Nonce, Gas Price, and Chain Id
    let (nonce_res, gas_price_res, chain_id_res) = tokio::join!(
        provider.get_transaction_count(from),
        provider.get_gas_price(),
        provider.get_chain_id(),
    );

    let nonce = match nonce_res {
        Ok(n) => n,
        Err(e) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": format!("Failed to get nonce: {}", e) })),
    };

    let gas_price = match gas_price_res {
        Ok(gp) => gp,
        Err(e) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": format!("Failed to get gas price: {}", e) })),
    };

    let chain_id = match chain_id_res {
        Ok(c) => Some(c),
        Err(_) => None,
    };

    let mut tx_req = alloy::rpc::types::TransactionRequest::default()
        .from(from)
        .to(Some(to))
        .input(alloy::rpc::types::TransactionInput::new(data.clone().into()))
        .nonce(nonce)
        .gas_price(gas_price);

    if let Some(v) = value {
        tx_req = tx_req.value(v);
    }

    let gas_limit = match provider.estimate_gas(tx_req).await {
        Ok(g) => g,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({ "error": format!("Gas estimation failed: {}", e) })),
    };

    HttpResponse::Ok().json(serde_json::json!({
        "from": from,
        "to": to,
        "data": alloy::hex::encode(data),
        "value": value.map(|v| v.to_string()).unwrap_or_else(|| "0".to_string()),
        "nonce": nonce,
        "gas_price": gas_price.to_string(),
        "gas_limit": gas_limit.to_string(),
        "chain_id": chain_id.map(|c| c.to_string()),
    }))
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/utils")
            .service(network_stats)
            .service(echo_ip)
            .service(headers)
            .service(json_validator)
            .service(hex_converter)
            .service(estimate_gas)
            .service(get_nonce)
            .service(get_balance)
            .service(keccak256)
            .service(verify_signature)
            .service(get_transaction)
            .service(get_allowance)
            .service(eth_call)
            .service(get_block)
            .service(abi_encode)
            .service(tx_builder)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};

    #[actix_web::test]
    async fn test_json_validator_basic() {
        let app = test::init_service(App::new().service(json_validator)).await;
        let req = test::TestRequest::post()
            .uri("/json-validator")
            .set_payload("{\"test\": 123}")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
    }

    #[actix_web::test]
    async fn test_json_validator_error() {
        let app = test::init_service(App::new().service(json_validator)).await;
        let req = test::TestRequest::post()
            .uri("/json-validator")
            .set_payload("invalid-json")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_client_error());
    }

    #[actix_web::test]
    async fn test_hex_converter_encode() {
        let app = test::init_service(App::new().service(hex_converter)).await;
        let req = test::TestRequest::post()
            .uri("/hex-converter")
            .set_payload("hello")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["action"], "encode");
        assert_eq!(body["result"], "68656c6c6f");
    }

    #[actix_web::test]
    async fn test_hex_converter_decode() {
        let app = test::init_service(App::new().service(hex_converter)).await;
        let req = test::TestRequest::post()
            .uri("/hex-converter")
            .set_payload("68656c6c6f")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["action"], "decode");
        assert_eq!(body["result"], "hello");
    }

    #[actix_web::test]
    async fn test_echo_ip() {
        let app = test::init_service(App::new().service(echo_ip)).await;
        let req = test::TestRequest::get()
            .uri("/echo-ip")
            .peer_addr("127.0.0.1:1234".parse().unwrap())
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body: Value = test::read_body_json(resp).await;
        assert!(body["ip"].as_str().is_some());
    }

    #[actix_web::test]
    async fn test_headers() {
        let app = test::init_service(App::new().service(headers)).await;
        let req = test::TestRequest::get()
            .uri("/headers")
            .insert_header(("X-Test-Header", "test-value"))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body: Value = test::read_body_json(resp).await;
        assert_eq!(body["x-test-header"], "test-value");
    }

    #[actix_web::test]
    async fn test_abi_encode() {
        let app = test::init_service(App::new().service(abi_encode)).await;
        let req = test::TestRequest::post()
            .uri("/abi-encode")
            .set_json(serde_json::json!({
                "signature": "transfer(address,uint256)",
                "args": ["0x0000000000000000000000000000000000000000", "1000"]
            }))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        let body: Value = test::read_body_json(resp).await;
        assert!(body["result"].as_str().is_some());
    }

    #[test]
    fn test_balance_request_deserialization() {
        let json = r#"{"address": "0x0000000000000000000000000000000000000000", "token": "0x0000000000000000000000000000000000000001"}"#;
        let req: BalanceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.address, "0x0000000000000000000000000000000000000000");
        assert_eq!(req.token.unwrap(), "0x0000000000000000000000000000000000000001");
    }

    #[test]
    fn test_nonce_request_deserialization() {
        let json = r#"{"address": "0x0000000000000000000000000000000000000000"}"#;
        let req: NonceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.address, "0x0000000000000000000000000000000000000000");
    }
}
