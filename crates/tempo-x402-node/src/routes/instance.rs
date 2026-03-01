use actix_web::{web, HttpResponse};

use crate::db;
use crate::state::NodeState;

/// Validate that a string looks like a UUID (8-4-4-4-12 hex).
pub(crate) fn is_valid_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Validate that a string looks like an EVM address (0x + 40 hex chars).
fn is_valid_evm_address(s: &str) -> bool {
    if s.is_empty() {
        return true; // allow empty (not yet known)
    }
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Validate that a URL uses HTTPS scheme.
fn is_valid_https_url(s: &str) -> bool {
    s.starts_with("https://") && s.len() > 8
}

/// GET /instance/info — returns identity, children, version, uptime, clone availability
pub async fn info(state: web::Data<NodeState>) -> HttpResponse {
    let identity_info = state.identity.as_ref().map(|id| {
        serde_json::json!({
            "address": format!("{:#x}", id.address),
            "instance_id": id.instance_id,
            "parent_url": id.parent_url,
            "parent_address": id.parent_address.map(|a| format!("{:#x}", a)),
            "created_at": id.created_at.to_rfc3339(),
        })
    }).or_else(|| {
        Some(serde_json::json!({
            "address": format!("{:#x}", state.gateway.config.platform_address),
            "instance_id": null,
            "parent_url": null,
            "parent_address": null,
            "created_at": state.started_at.to_rfc3339(),
        }))
    });

    // Open a read connection to query active (non-failed) children
    let children = rusqlite::Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::query_children_active(&conn).ok())
        .unwrap_or_default();

    let uptime_secs = (chrono::Utc::now() - state.started_at).num_seconds();

    let clone_available = state.agent.is_some()
        && state.clone_price.is_some()
        && (children.len() as u32) < state.clone_max_children;

    HttpResponse::Ok().json(serde_json::json!({
        "identity": identity_info,
        "children": children,
        "children_count": children.len(),
        "clone_available": clone_available,
        "clone_price": state.clone_price,
        "clone_max_children": state.clone_max_children,
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": uptime_secs,
    }))
}

/// POST /instance/register — child callback, updates children table
pub async fn register(
    body: web::Json<serde_json::Value>,
    state: web::Data<NodeState>,
) -> HttpResponse {
    let instance_id = match body.get("instance_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "missing instance_id"
            }));
        }
    };

    // Validate instance_id is a UUID to prevent injection
    if !is_valid_uuid(instance_id) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "invalid instance_id format"
        }));
    }

    let address = body.get("address").and_then(|v| v.as_str()).unwrap_or("");

    // Validate address format if provided
    if !is_valid_evm_address(address) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "invalid address format"
        }));
    }

    let url = body.get("url").and_then(|v| v.as_str());

    // Validate URL is HTTPS if provided
    if let Some(u) = url {
        if !is_valid_https_url(u) {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "url must use https"
            }));
        }
    }

    match db::update_child(
        &state.gateway.db,
        instance_id,
        Some(address),
        url,
        Some("running"),
    ) {
        Ok(()) => {
            tracing::info!(
                instance_id = %instance_id,
                "Child instance registered"
            );
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "registered",
            }))
        }
        Err(e) => {
            tracing::warn!(
                instance_id = %instance_id,
                error = %e,
                "Failed to update child record"
            );
            // Don't leak internal error details
            HttpResponse::Ok().json(serde_json::json!({
                "success": true,
                "message": "acknowledged",
            }))
        }
    }
}

pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/instance")
            .route("/info", web::get().to(info))
            .route("/register", web::post().to(register)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_uuid() {
        assert!(is_valid_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(is_valid_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(!is_valid_uuid("not-a-uuid"));
        assert!(!is_valid_uuid("550e8400e29b41d4a716446655440000"));
        assert!(!is_valid_uuid(""));
        assert!(!is_valid_uuid("550e8400-e29b-41d4-a716-44665544000g"));
    }

    #[test]
    fn test_valid_evm_address() {
        assert!(is_valid_evm_address(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(is_valid_evm_address("")); // empty allowed
        assert!(!is_valid_evm_address("0x123")); // too short
        assert!(!is_valid_evm_address(
            "1234567890abcdef1234567890abcdef12345678"
        )); // no 0x
        assert!(!is_valid_evm_address(
            "0xGGGG567890abcdef1234567890abcdef12345678"
        )); // invalid hex
    }

    #[test]
    fn test_valid_https_url() {
        assert!(is_valid_https_url("https://example.com"));
        assert!(is_valid_https_url("https://x402-abc.up.railway.app"));
        assert!(!is_valid_https_url("http://example.com"));
        assert!(!is_valid_https_url("https://"));
        assert!(!is_valid_https_url("ftp://example.com"));
    }
}
