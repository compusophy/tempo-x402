use actix_web::{web, HttpRequest, HttpResponse};

use crate::db;
use crate::routes::instance::is_valid_uuid;
use crate::state::NodeState;
use x402_gateway::error::GatewayError;
use x402_gateway::middleware::{payment_response_header, require_payment};

/// Internal helper to check for X-Admin-Secret header
fn check_admin_auth(req: &HttpRequest) -> bool {
    match std::env::var("ADMIN_SECRET") {
        Ok(secret) if !secret.is_empty() => {
            let header_value = req
                .headers()
                .get("X-Admin-Secret")
                .and_then(|v| v.to_str().ok());
            header_value == Some(&secret)
        }
        _ => true, // In dev mode (no secret set), allow all
    }
}

/// POST /clone — x402-gated clone operation
pub async fn clone_instance(
    req: HttpRequest,
    node: web::Data<NodeState>,
) -> Result<HttpResponse, GatewayError> {
    let agent = node
        .agent
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("cloning not configured".to_string()))?;

    let clone_price = node
        .clone_price
        .as_deref()
        .ok_or_else(|| GatewayError::Internal("clone price not set".to_string()))?;

    let clone_price_amount = node
        .clone_price_amount
        .as_deref()
        .ok_or_else(|| GatewayError::Internal("clone price amount not set".to_string()))?;

    // Build payment requirements
    let requirements = x402::payment::PaymentRequirements {
        scheme: x402::constants::SCHEME_NAME.to_string(),
        network: x402::constants::TEMPO_NETWORK.to_string(),
        price: clone_price.to_string(),
        asset: x402::constants::DEFAULT_TOKEN,
        amount: clone_price_amount.to_string(),
        pay_to: node.gateway.config.platform_address,
        max_timeout_seconds: 60,
        description: Some("Clone instance fee".to_string()),
        mime_type: Some("application/json".to_string()),
    };

    // Early 402 if no payment header
    if x402_gateway::middleware::extract_payment_header(&req).is_none() {
        return Ok(x402_gateway::middleware::payment_required_response(
            requirements,
        ));
    }

    // Verify and settle payment
    let settle = match require_payment(
        &req,
        requirements,
        &node.gateway.http_client,
        &node.gateway.config.facilitator_url,
        node.gateway.config.hmac_secret.as_deref(),
        node.gateway.facilitator.as_deref(),
    )
    .await
    {
        Ok(s) => s,
        Err(http_response) => return Ok(http_response),
    };

    let payer_address = settle
        .payer
        .map(|a| format!("{:#x}", a))
        .unwrap_or_default();

    // Generate instance ID up front so we can reserve the DB slot before Railway calls
    let instance_id = uuid::Uuid::new_v4().to_string();

    // 1. Reserve slot in DB BEFORE any Railway API calls (atomic limit check)
    match db::reserve_child_slot(&node.gateway.db, node.clone_max_children, &instance_id) {
        Ok(true) => {
            tracing::info!(instance_id = %instance_id, "Child slot reserved");
        }
        Ok(false) => {
            return Ok(HttpResponse::Conflict().json(serde_json::json!({
                "success": false,
                "error": "clone limit reached",
            })));
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to reserve child slot in DB");
            return Err(GatewayError::Internal(
                "failed to reserve clone slot".to_string(),
            ));
        }
    }

    // 2. Spawn clone on Railway (with retry + cleanup-on-failure)
    let generation = node
        .soul_config
        .as_ref()
        .map(|c| c.generation)
        .unwrap_or(0);

    let parent_instance_id = node
        .identity
        .as_ref()
        .map(|i| i.instance_id.as_str());

    let clone_result = match agent
        .spawn_clone(&instance_id, &payer_address, generation, parent_instance_id)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(
                instance_id = %instance_id,
                error = %e,
                "Clone orchestration failed"
            );
            // Mark the reserved slot as failed
            if let Err(db_err) = db::mark_child_failed(&node.gateway.db, &instance_id) {
                tracing::error!(error = %db_err, "Failed to mark child as failed in DB");
            }
            return Err(GatewayError::Internal("clone operation failed".to_string()));
        }
    };

    // 3. Update the reserved slot with Railway details
    if let Err(e) = db::update_child_deployment(
        &node.gateway.db,
        &instance_id,
        &clone_result.url,
        &clone_result.railway_service_id,
        "deploying",
    ) {
        // Railway resources exist but DB update failed — log but still return success
        // since the child is at least tracked from the reservation step
        tracing::error!(
            instance_id = %instance_id,
            error = %e,
            "Failed to update child deployment details in DB"
        );
    }

    tracing::info!(
        instance_id = %instance_id,
        url = %clone_result.url,
        payer = %payer_address,
        "Clone spawned successfully"
    );

    Ok(HttpResponse::Created()
        .insert_header((
            "PAYMENT-RESPONSE",
            payment_response_header(&settle, node.gateway.config.hmac_secret.as_deref()),
        ))
        .json(serde_json::json!({
            "success": true,
            "instance_id": instance_id,
            "url": clone_result.url,
            "status": "deploying",
            "transaction": settle.transaction,
        })))
}

/// GET /clone/{instance_id}/status — check clone deployment status
pub async fn clone_status(
    path: web::Path<String>,
    node: web::Data<NodeState>,
) -> Result<HttpResponse, GatewayError> {
    let instance_id = path.into_inner();

    match db::get_child_by_instance_id(&node.gateway.db, &instance_id) {
        Ok(Some(child)) => Ok(HttpResponse::Ok().json(serde_json::json!({
            "instance_id": child.instance_id,
            "status": child.status,
            "url": child.url,
            "created_at": child.created_at,
        }))),
        Ok(None) => Ok(HttpResponse::NotFound().json(serde_json::json!({
            "error": "clone not found",
        }))),
        Err(e) => {
            tracing::error!(error = %e, "Failed to query clone status");
            Err(GatewayError::Internal(
                "failed to query clone status".to_string(),
            ))
        }
    }
}

/// DELETE /clone/{instance_id} — delete a failed clone
pub async fn delete_clone(
    req: HttpRequest,
    path: web::Path<String>,
    node: web::Data<NodeState>,
) -> Result<HttpResponse, GatewayError> {
    if !check_admin_auth(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "admin secret required",
        })));
    }
    let instance_id = path.into_inner();

    if !is_valid_uuid(&instance_id) {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "invalid instance_id format",
        })));
    }

    // Look up the child
    let child = match db::get_child_by_instance_id(&node.gateway.db, &instance_id) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "clone not found",
            })));
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to query clone for delete");
            return Err(GatewayError::Internal("failed to query clone".to_string()));
        }
    };

    // Only allow deleting failed clones
    if child.status != "failed" {
        return Ok(HttpResponse::Conflict().json(serde_json::json!({
            "error": "can only delete failed clones",
            "current_status": child.status,
        })));
    }

    // Best-effort Railway cleanup if service ID exists
    if let Some(ref service_id) = child.railway_service_id {
        if let Some(ref agent) = node.agent {
            if let Err(e) = agent.delete_service(service_id).await {
                tracing::warn!(
                    instance_id = %instance_id,
                    service_id = %service_id,
                    error = %e,
                    "Failed to delete Railway service (best-effort cleanup)"
                );
            }
        }
    }

    // Delete from DB
    match db::delete_failed_child(&node.gateway.db, &instance_id) {
        Ok(true) => {
            tracing::info!(instance_id = %instance_id, "Deleted failed clone");
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "instance_id": instance_id,
            })))
        }
        Ok(false) => {
            // Race: status changed between check and delete
            Ok(HttpResponse::Conflict().json(serde_json::json!({
                "error": "clone is no longer in failed state",
            })))
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to delete clone from DB");
            Err(GatewayError::Internal("failed to delete clone".to_string()))
        }
    }
}

/// POST /clone/{instance_id}/redeploy — trigger a redeploy for a running clone
pub async fn redeploy_clone(
    req: HttpRequest,
    path: web::Path<String>,
    node: web::Data<NodeState>,
) -> Result<HttpResponse, GatewayError> {
    if !check_admin_auth(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "admin secret required",
        })));
    }
    let instance_id = path.into_inner();

    if !is_valid_uuid(&instance_id) {
        return Ok(HttpResponse::BadRequest().json(serde_json::json!({
            "error": "invalid instance_id format",
        })));
    }

    let agent = node
        .agent
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("cloning not configured".to_string()))?;

    // Look up the child
    let child = match db::get_child_by_instance_id(&node.gateway.db, &instance_id) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return Ok(HttpResponse::NotFound().json(serde_json::json!({
                "error": "clone not found",
            })));
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to query clone for redeploy");
            return Err(GatewayError::Internal("failed to query clone".to_string()));
        }
    };

    // Reject redeploying failed clones
    if child.status == "failed" {
        return Ok(HttpResponse::Conflict().json(serde_json::json!({
            "error": "cannot redeploy a failed clone",
            "current_status": child.status,
        })));
    }

    let service_id = match child.railway_service_id {
        Some(ref id) => id.clone(),
        None => {
            return Ok(HttpResponse::Conflict().json(serde_json::json!({
                "error": "clone has no Railway service ID",
            })));
        }
    };

    // Trigger redeploy
    match agent.redeploy_clone(&service_id).await {
        Ok(_) => {
            // Update status to deploying
            if let Err(e) = db::update_child_status(&node.gateway.db, &instance_id, "deploying") {
                tracing::error!(error = %e, "Failed to update child status after redeploy");
            }

            tracing::info!(instance_id = %instance_id, "Redeploy triggered");
            Ok(HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "instance_id": instance_id,
                "status": "deploying",
            })))
        }
        Err(e) => {
            tracing::error!(
                instance_id = %instance_id,
                error = %e,
                "Failed to redeploy clone"
            );
            Err(GatewayError::Internal(
                "failed to redeploy clone".to_string(),
            ))
        }
    }
}

/// POST /clone/update-all — redeploy all active children
pub async fn update_all(
    req: HttpRequest,
    node: web::Data<NodeState>,
) -> Result<HttpResponse, GatewayError> {
    if !check_admin_auth(&req) {
        return Ok(HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "admin secret required",
        })));
    }
    let agent = node
        .agent
        .as_ref()
        .ok_or_else(|| GatewayError::Internal("cloning not configured".to_string()))?;

    // Query active children
    let children = rusqlite::Connection::open(&node.db_path)
        .map_err(|e| GatewayError::Internal(format!("failed to open db: {e}")))?
        .pipe(|conn| db::query_children_active(&conn))
        .map_err(|e| GatewayError::Internal(format!("failed to query children: {e}")))?;

    let total = children.len();
    let mut succeeded = 0u32;
    let mut failed = 0u32;
    let mut results = Vec::new();

    for child in &children {
        let service_id = match child.railway_service_id {
            Some(ref id) => id.clone(),
            None => {
                results.push(serde_json::json!({
                    "instance_id": child.instance_id,
                    "success": false,
                    "error": "no Railway service ID",
                }));
                failed += 1;
                continue;
            }
        };

        match agent.redeploy_clone(&service_id).await {
            Ok(_) => {
                if let Err(e) =
                    db::update_child_status(&node.gateway.db, &child.instance_id, "deploying")
                {
                    tracing::error!(
                        instance_id = %child.instance_id,
                        error = %e,
                        "Failed to update status after redeploy"
                    );
                }
                results.push(serde_json::json!({
                    "instance_id": child.instance_id,
                    "success": true,
                }));
                succeeded += 1;
            }
            Err(e) => {
                tracing::warn!(
                    instance_id = %child.instance_id,
                    error = %e,
                    "Failed to redeploy child"
                );
                results.push(serde_json::json!({
                    "instance_id": child.instance_id,
                    "success": false,
                    "error": e.to_string(),
                }));
                failed += 1;
            }
        }
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "total": total,
        "succeeded": succeeded,
        "failed": failed,
        "results": results,
    })))
}

/// Pipe helper — allows `value.pipe(|v| f(v))` for readability.
trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R
    where
        F: FnOnce(Self) -> R,
    {
        f(self)
    }
}
impl<T> Pipe for T {}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/clone", web::post().to(clone_instance))
        .route("/clone/update-all", web::post().to(update_all))
        .route("/clone/{instance_id}/status", web::get().to(clone_status))
        .route(
            "/clone/{instance_id}/redeploy",
            web::post().to(redeploy_clone),
        )
        .route("/clone/{instance_id}", web::delete().to(delete_clone));
}
