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
            .service(get_balance),
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
}
