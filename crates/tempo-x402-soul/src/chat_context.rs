use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
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

        if context.economic.balance > U256::from(1000) {
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
