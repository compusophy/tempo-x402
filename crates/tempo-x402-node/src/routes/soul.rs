//! Soul endpoints — status and interactive chat.
// This is a test comment

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::state::NodeState;

#[derive(Serialize)]
struct SoulStatus {
    active: bool,
    dormant: bool,
    total_cycles: u64,
    last_think_at: Option<i64>,
    mode: String,
    tools_enabled: bool,
    coding_enabled: bool,
    /// Cycle health metrics for observability.
    cycle_health: CycleHealth,
    recent_thoughts: Vec<ThoughtEntry>,
    /// Active beliefs from the world model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    beliefs: Vec<BeliefEntry>,
}

#[derive(Serialize)]
struct BeliefEntry {
    id: String,
    domain: String,
    subject: String,
    predicate: String,
    value: String,
    confidence: String,
    confirmation_count: u32,
}

#[derive(Serialize)]
struct CycleHealth {
    boring_streak: u32,
    active_streak: u32,
    last_cycle_tool_calls: u32,
    last_cycle_decisions: u32,
    last_cycle_entered_code: bool,
    total_code_entries: u64,
}

#[derive(Serialize)]
struct ThoughtEntry {
    #[serde(rename = "type")]
    thought_type: String,
    content: String,
    created_at: i64,
}

async fn soul_status(state: web::Data<NodeState>) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            let (tools_enabled, coding_enabled) = match &state.soul_config {
                Some(c) => (c.tools_enabled, c.coding_enabled),
                None => (false, false),
            };
            return HttpResponse::Ok().json(serde_json::json!({
                "active": false,
                "dormant": state.soul_dormant,
                "total_cycles": 0,
                "last_think_at": null,
                "mode": "observe",
                "tools_enabled": tools_enabled,
                "coding_enabled": coding_enabled,
                "cycle_health": {
                    "boring_streak": 0,
                    "active_streak": 0,
                    "last_cycle_tool_calls": 0,
                    "last_cycle_decisions": 0,
                    "last_cycle_entered_code": false,
                    "total_code_entries": 0
                },
                "recent_thoughts": []
            }));
        }
    };

    let total_cycles: u64 = soul_db
        .get_state("total_think_cycles")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let last_think_at: Option<i64> = soul_db
        .get_state("last_think_at")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok());

    let recent_thoughts: Vec<ThoughtEntry> = soul_db
        .recent_thoughts(15)
        .unwrap_or_default()
        .into_iter()
        .map(|t| ThoughtEntry {
            thought_type: t.thought_type.as_str().to_string(),
            content: t.content,
            created_at: t.created_at,
        })
        .collect();

    let (tools_enabled, coding_enabled) = match &state.soul_config {
        Some(c) => (c.tools_enabled, c.coding_enabled),
        None => (false, false),
    };

    // Read cycle health metrics from soul_state
    let boring_streak: u32 = soul_db
        .get_state("boring_streak")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let active_streak: u32 = soul_db
        .get_state("active_streak")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let last_cycle_tool_calls: u32 = soul_db
        .get_state("last_cycle_tool_calls")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let last_cycle_decisions: u32 = soul_db
        .get_state("last_cycle_decisions")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let last_cycle_entered_code: bool = soul_db
        .get_state("last_cycle_entered_code")
        .ok()
        .flatten()
        .map(|s| s == "true")
        .unwrap_or(false);
    let total_code_entries: u64 = soul_db
        .get_state("total_code_entries")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Determine displayed mode based on last cycle
    let mode = if last_cycle_entered_code {
        "code".to_string()
    } else {
        "observe".to_string()
    };

    // Fetch world model beliefs
    let beliefs: Vec<BeliefEntry> = soul_db
        .get_all_active_beliefs()
        .unwrap_or_default()
        .into_iter()
        .map(|b| BeliefEntry {
            id: b.id,
            domain: b.domain.as_str().to_string(),
            subject: b.subject,
            predicate: b.predicate,
            value: b.value,
            confidence: b.confidence.as_str().to_string(),
            confirmation_count: b.confirmation_count,
        })
        .collect();

    HttpResponse::Ok().json(SoulStatus {
        active: true,
        dormant: state.soul_dormant,
        total_cycles,
        last_think_at,
        mode,
        tools_enabled,
        coding_enabled,
        cycle_health: CycleHealth {
            boring_streak,
            active_streak,
            last_cycle_tool_calls,
            last_cycle_decisions,
            last_cycle_entered_code,
            total_code_entries,
        },
        recent_thoughts,
        beliefs,
    })
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
}

async fn soul_chat(state: web::Data<NodeState>, body: web::Json<ChatRequest>) -> HttpResponse {
    // Validate soul is active
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul is not active"
            }));
        }
    };

    // Validate not dormant
    if state.soul_dormant {
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "soul is dormant (no LLM API key)"
        }));
    }

    // Validate message length
    let message = body.message.trim();
    if message.is_empty() || message.len() > 4096 {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "message must be 1-4096 characters"
        }));
    }

    // Get config and observer
    let config = match &state.soul_config {
        Some(c) => c,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul config not available"
            }));
        }
    };

    let observer = match &state.soul_observer {
        Some(o) => o,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul observer not available"
            }));
        }
    };

    match x402_soul::handle_chat(message, config, soul_db, observer).await {
        Ok(reply) => HttpResponse::Ok().json(serde_json::json!({
            "reply": reply.reply,
            "tool_executions": reply.tool_executions,
            "thought_ids": reply.thought_ids,
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Soul chat failed");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("chat failed: {e}")
            }))
        }
    }
}

/// Mind status: both hemispheres + callosum state.
#[derive(Serialize)]
struct MindStatus {
    enabled: bool,
    left: HemisphereStatus,
    right: HemisphereStatus,
}

#[derive(Serialize)]
struct HemisphereStatus {
    active: bool,
    total_cycles: u64,
    last_think_at: Option<i64>,
    recent_thoughts: Vec<ThoughtEntry>,
}

async fn mind_status(state: web::Data<NodeState>) -> HttpResponse {
    if !state.mind_enabled {
        return HttpResponse::Ok().json(serde_json::json!({
            "enabled": false,
            "left": { "active": false, "total_cycles": 0, "last_think_at": null, "recent_thoughts": [] },
            "right": { "active": false, "total_cycles": 0, "last_think_at": null, "recent_thoughts": [] }
        }));
    }

    let left = build_hemisphere_status(state.soul_db.as_ref());
    let right = build_hemisphere_status(state.mind_right_db.as_ref());

    HttpResponse::Ok().json(MindStatus {
        enabled: true,
        left,
        right,
    })
}

fn build_hemisphere_status(
    db: Option<&std::sync::Arc<x402_soul::SoulDatabase>>,
) -> HemisphereStatus {
    let db = match db {
        Some(db) => db,
        None => {
            return HemisphereStatus {
                active: false,
                total_cycles: 0,
                last_think_at: None,
                recent_thoughts: vec![],
            }
        }
    };

    let total_cycles: u64 = db
        .get_state("total_think_cycles")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let last_think_at: Option<i64> = db
        .get_state("last_think_at")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok());

    let recent_thoughts: Vec<ThoughtEntry> = db
        .recent_thoughts(15)
        .unwrap_or_default()
        .into_iter()
        .map(|t| ThoughtEntry {
            thought_type: t.thought_type.as_str().to_string(),
            content: t.content,
            created_at: t.created_at,
        })
        .collect();

    HemisphereStatus {
        active: true,
        total_cycles,
        last_think_at,
        recent_thoughts,
    }
}

/// Mind chat: routes to the appropriate hemisphere based on intent.
async fn mind_chat(state: web::Data<NodeState>, body: web::Json<ChatRequest>) -> HttpResponse {
    if !state.mind_enabled {
        return HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "error": "mind is not enabled (set MIND_ENABLED=true)"
        }));
    }

    // Route to left hemisphere (the default for chat — it has code tools)
    soul_chat(state, body).await
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/soul/status", web::get().to(soul_status))
        .route("/soul/chat", web::post().to(soul_chat))
        .route("/soul/test", web::get().to(|| async { HttpResponse::Ok().body("test") }))
        .route("/mind/status", web::get().to(mind_status))
        .route("/mind/chat", web::post().to(mind_chat));
}
