use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use actix_web::{web, HttpResponse, Responder, HttpRequest};
use alloy::primitives::{Address, U256};

/// Represents the economic state of the agent or the peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicState {
    pub balance: U256,
    pub pending_invoices: usize,
    pub lifetime_revenue: U256,
    pub last_transaction_at: Option<DateTime<Utc>>,
}

/// Information about the peer initiating the chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerContext {
    pub peer_id: String,
    pub address: Address,
    pub trust_score: f64,
    pub interaction_count: u32,
    pub last_interaction: Option<DateTime<Utc>>,
}

/// The full context for a chat message.
/// This structure aggregates disparate data points to give the LLM better awareness of the situation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatContext {
    pub peer: PeerContext,
    pub economic: EconomicState,
    pub active_goals: Vec<String>,
    pub recent_beliefs: Vec<String>,
}

/// Request for a context-aware chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAwareChatRequest {
    pub session_id: Option<String>,
    pub message: String,
    pub mode: String, // e.g., "code", "research", "commerce"
}

/// Response for a context-aware chat.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextAwareChatResponse {
    pub session_id: String,
    pub response: String,
    pub suggested_actions: Vec<String>,
    pub context_used: bool,
}

/// A draft implementation of a context-aware chat processor.
pub struct ChatProcessor;

impl ChatProcessor {
    /// Processes a chat request by incorporating situational context.
    pub async fn process(
        req: ContextAwareChatRequest,
        context: ChatContext,
    ) -> Result<ContextAwareChatResponse, String> {
        let session_id = req.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        
        let mut response_text = format!("Received message: '{}' in mode: '{}'. ", req.message, req.mode);
        
        if context.peer.trust_score > 0.8 {
            response_text.push_str("Recognized as a high-trust peer. ");
        }

        if context.economic.balance > U256::from(1000u64) {
            response_text.push_str("Wealthy peer detected. Adjusting priority. ");
        }

        let suggested_actions = if req.mode == "commerce" {
            vec!["offer_service".to_string(), "negotiate_price".to_string()]
        } else {
            vec!["provide_info".to_string()]
        };

        Ok(ContextAwareChatResponse {
            session_id,
            response: response_text,
            suggested_actions,
            context_used: true,
        })
    }
}

/// Actix handler for the context-aware chat endpoint.
pub async fn context_aware_chat_handler(
    _req: HttpRequest,
    payload: web::Json<ContextAwareChatRequest>,
) -> impl Responder {
    let mock_context = ChatContext {
        peer: PeerContext {
            peer_id: "peer_123".to_string(),
            address: Address::repeat_byte(0x01),
            trust_score: 0.95,
            interaction_count: 50,
            last_interaction: Some(Utc::now()),
        },
        economic: EconomicState {
            balance: U256::from(5000u64),
            pending_invoices: 0,
            lifetime_revenue: U256::from(100000u64),
            last_transaction_at: Some(Utc::now()),
        },
        active_goals: vec!["maximize_revenue".to_string(), "improve_peer_discovery".to_string()],
        recent_beliefs: vec!["peer_123_is_reliable".to_string()],
    };

    match ChatProcessor::process(payload.into_inner(), mock_context).await {
        Ok(res) => HttpResponse::Ok().json(res),
        Err(e) => HttpResponse::InternalServerError().json(serde_json::json!({ "error": e })),
    }
}

/// Configure the service in the Actix web server.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/soul/chat/context-aware")
            .route(web::post().to(context_aware_chat_handler)),
    );
}
