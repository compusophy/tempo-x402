//! Soul endpoints — status, interactive chat with sessions, plan approval.

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
    /// Active plan info.
    #[serde(skip_serializing_if = "Option::is_none")]
    active_plan: Option<PlanInfo>,
    /// Pending approval plan info.
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_plan: Option<PlanInfo>,
    recent_thoughts: Vec<ThoughtEntry>,
    /// Active beliefs from the world model.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    beliefs: Vec<BeliefEntry>,
    /// Active goals driving multi-cycle behavior.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    goals: Vec<GoalEntry>,
}

#[derive(Serialize)]
struct PlanInfo {
    id: String,
    goal_id: String,
    current_step: usize,
    total_steps: usize,
    status: String,
    replan_count: u32,
    current_step_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    goal_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<Vec<String>>,
}

#[derive(Serialize)]
struct GoalEntry {
    id: String,
    description: String,
    status: String,
    priority: u32,
    success_criteria: String,
    progress_notes: String,
    retry_count: u32,
    created_at: i64,
    updated_at: i64,
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
    last_cycle_entered_code: bool,
    total_code_entries: u64,
    cycles_since_last_commit: u64,
    failed_plans_count: u64,
    goals_active: u64,
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
                    "last_cycle_entered_code": false,
                    "total_code_entries": 0,
                    "cycles_since_last_commit": 0,
                    "failed_plans_count": 0,
                    "goals_active": 0
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

    // Show what the soul *thinks*, not what it *greps* — filter out ToolExecution
    let recent_thoughts: Vec<ThoughtEntry> = soul_db
        .recent_thoughts_by_type(
            &[
                x402_soul::ThoughtType::Decision,
                x402_soul::ThoughtType::Reasoning,
                x402_soul::ThoughtType::Observation,
                x402_soul::ThoughtType::Reflection,
                x402_soul::ThoughtType::MemoryConsolidation,
                x402_soul::ThoughtType::Prediction,
            ],
            15,
        )
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
    let cycles_since_last_commit: u64 = soul_db
        .get_state("cycles_since_last_commit")
        .ok()
        .flatten()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let failed_plans_count: u64 = soul_db.count_plans_by_status("failed").unwrap_or(0);
    let goals_active: u64 = soul_db
        .get_active_goals()
        .map(|g| g.len() as u64)
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

    // Fetch active goals
    let goals: Vec<GoalEntry> = soul_db
        .get_active_goals()
        .unwrap_or_default()
        .into_iter()
        .map(|g| GoalEntry {
            id: g.id,
            description: g.description,
            status: g.status.as_str().to_string(),
            priority: g.priority,
            success_criteria: g.success_criteria,
            progress_notes: g.progress_notes,
            retry_count: g.retry_count,
            created_at: g.created_at,
            updated_at: g.updated_at,
        })
        .collect();

    // Fetch active plan
    let active_plan = soul_db.get_active_plan().ok().flatten().map(|p| {
        let current_step_type = p.steps.get(p.current_step).map(|s| s.summary());
        PlanInfo {
            id: p.id,
            goal_id: p.goal_id,
            current_step: p.current_step,
            total_steps: p.steps.len(),
            status: p.status.as_str().to_string(),
            replan_count: p.replan_count,
            current_step_type,
            goal_description: None,
            steps: None,
        }
    });

    // Fetch pending approval plan
    let pending_plan = soul_db.get_pending_approval_plan().ok().flatten().map(|p| {
        let goal_desc = soul_db
            .get_goal(&p.goal_id)
            .ok()
            .flatten()
            .map(|g| g.description);
        let step_summaries: Vec<String> = p.steps.iter().map(|s| s.summary()).collect();
        PlanInfo {
            id: p.id,
            goal_id: p.goal_id,
            current_step: p.current_step,
            total_steps: p.steps.len(),
            status: p.status.as_str().to_string(),
            replan_count: p.replan_count,
            current_step_type: None,
            goal_description: goal_desc,
            steps: Some(step_summaries),
        }
    });

    HttpResponse::Ok().json(SoulStatus {
        active: true,
        dormant: state.soul_dormant,
        total_cycles,
        last_think_at,
        mode,
        tools_enabled,
        coding_enabled,
        cycle_health: CycleHealth {
            last_cycle_entered_code,
            total_code_entries,
            cycles_since_last_commit,
            failed_plans_count,
            goals_active,
        },
        active_plan,
        pending_plan,
        recent_thoughts,
        beliefs,
        goals,
    })
}

// ── Chat endpoints ──

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default)]
    session_id: Option<String>,
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

    match x402_soul::handle_chat(
        message,
        body.session_id.as_deref(),
        config,
        soul_db,
        observer,
    )
    .await
    {
        Ok(reply) => HttpResponse::Ok().json(serde_json::json!({
            "reply": reply.reply,
            "tool_executions": reply.tool_executions,
            "thought_ids": reply.thought_ids,
            "session_id": reply.session_id,
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Soul chat failed");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("chat failed: {e}")
            }))
        }
    }
}

// ── Session endpoints ──

async fn chat_sessions(state: web::Data<NodeState>) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::Ok().json(serde_json::json!([]));
        }
    };

    match soul_db.list_sessions(20) {
        Ok(sessions) => HttpResponse::Ok().json(sessions),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to list sessions");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to list sessions: {e}")
            }))
        }
    }
}

async fn session_messages(state: web::Data<NodeState>, path: web::Path<String>) -> HttpResponse {
    let session_id = path.into_inner();
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul is not active"
            }));
        }
    };

    match soul_db.get_session_messages(&session_id, 50) {
        Ok(messages) => HttpResponse::Ok().json(messages),
        Err(e) => {
            tracing::warn!(error = %e, session_id = %session_id, "Failed to get session messages");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to get messages: {e}")
            }))
        }
    }
}

// ── Plan approval endpoints ──

#[derive(Deserialize)]
struct PlanApproveRequest {
    plan_id: String,
}

#[derive(Deserialize)]
struct PlanRejectRequest {
    plan_id: String,
    #[serde(default)]
    reason: Option<String>,
}

async fn plan_approve(
    state: web::Data<NodeState>,
    body: web::Json<PlanApproveRequest>,
) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul is not active"
            }));
        }
    };

    match soul_db.approve_plan(&body.plan_id) {
        Ok(true) => HttpResponse::Ok().json(serde_json::json!({
            "status": "approved",
            "plan_id": body.plan_id,
        })),
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no pending plan with that ID"
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to approve plan");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to approve plan: {e}")
            }))
        }
    }
}

async fn plan_reject(
    state: web::Data<NodeState>,
    body: web::Json<PlanRejectRequest>,
) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul is not active"
            }));
        }
    };

    match soul_db.reject_plan(&body.plan_id) {
        Ok(true) => {
            // Insert a nudge with the rejection reason
            if let Some(reason) = &body.reason {
                let _ = soul_db.insert_nudge("user", &format!("Plan rejected: {}", reason), 5);
            }
            HttpResponse::Ok().json(serde_json::json!({
                "status": "rejected",
                "plan_id": body.plan_id,
            }))
        }
        Ok(false) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "no pending plan with that ID"
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to reject plan");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to reject plan: {e}")
            }))
        }
    }
}

async fn plan_pending(state: web::Data<NodeState>) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::Ok().json(serde_json::json!(null));
        }
    };

    match soul_db.get_pending_approval_plan() {
        Ok(Some(plan)) => {
            let goal_desc = soul_db
                .get_goal(&plan.goal_id)
                .ok()
                .flatten()
                .map(|g| g.description);
            let step_summaries: Vec<String> = plan.steps.iter().map(|s| s.summary()).collect();
            HttpResponse::Ok().json(serde_json::json!({
                "id": plan.id,
                "goal_id": plan.goal_id,
                "goal_description": goal_desc,
                "steps": step_summaries,
                "total_steps": plan.steps.len(),
                "created_at": plan.created_at,
            }))
        }
        Ok(None) => HttpResponse::Ok().json(serde_json::json!(null)),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to get pending plan");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to get pending plan: {e}")
            }))
        }
    }
}

// ── Nudge endpoints ──

#[derive(Deserialize)]
struct NudgeRequest {
    message: String,
    priority: Option<u32>,
}

async fn soul_nudge(state: web::Data<NodeState>, body: web::Json<NudgeRequest>) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::ServiceUnavailable().json(serde_json::json!({
                "error": "soul is not active"
            }));
        }
    };

    let message = body.message.trim();
    if message.is_empty() || message.len() > 2048 {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "message must be 1-2048 characters"
        }));
    }

    // User nudges get highest priority (5) by default
    let priority = body.priority.unwrap_or(5).min(5);

    match soul_db.insert_nudge("user", message, priority) {
        Ok(id) => HttpResponse::Ok().json(serde_json::json!({
            "id": id,
            "status": "queued"
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to insert nudge");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to queue nudge: {e}")
            }))
        }
    }
}

async fn soul_nudges(state: web::Data<NodeState>) -> HttpResponse {
    let soul_db = match &state.soul_db {
        Some(db) => db,
        None => {
            return HttpResponse::Ok().json(serde_json::json!([]));
        }
    };

    match soul_db.get_unprocessed_nudges(20) {
        Ok(nudges) => HttpResponse::Ok().json(nudges),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to fetch nudges");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("failed to fetch nudges: {e}")
            }))
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/soul/status", web::get().to(soul_status))
        .route("/soul/chat", web::post().to(soul_chat))
        .route("/soul/chat/sessions", web::get().to(chat_sessions))
        .route("/soul/chat/sessions/{id}", web::get().to(session_messages))
        .route("/soul/plan/approve", web::post().to(plan_approve))
        .route("/soul/plan/reject", web::post().to(plan_reject))
        .route("/soul/plan/pending", web::get().to(plan_pending))
        .route("/soul/nudge", web::post().to(soul_nudge))
        .route("/soul/nudges", web::get().to(soul_nudges));
}
