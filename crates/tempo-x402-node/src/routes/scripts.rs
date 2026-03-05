//! Script endpoints — dynamic HTTP handlers that execute shell scripts.
//!
//! The soul writes scripts to `/data/endpoints/{slug}.sh` and they become
//! instantly available at `GET/POST /x/{slug}`. No Rust compilation needed.
//!
//! This lets Flash-level models add functionality by writing simple bash scripts
//! instead of needing to write, compile, and redeploy Rust code.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Serialize;
use std::path::PathBuf;

/// Directory where endpoint scripts live (persistent volume).
const SCRIPTS_DIR: &str = "/data/endpoints";

/// Max script execution time.
const SCRIPT_TIMEOUT_SECS: u64 = 30;

/// Max output size from scripts.
const MAX_SCRIPT_OUTPUT: usize = 65536;

#[derive(Serialize)]
struct ScriptEndpoint {
    slug: String,
    description: Option<String>,
    method: String,
}

/// `GET/POST /x/{slug}` — execute the script for this endpoint.
pub async fn handle_script(
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Bytes,
) -> HttpResponse {
    let slug = path.into_inner();

    // Validate slug — alphanumeric + hyphens only, no path traversal
    if !slug
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "invalid endpoint slug"
        }));
    }

    let script_path = PathBuf::from(SCRIPTS_DIR).join(format!("{slug}.sh"));

    if !script_path.exists() {
        return HttpResponse::NotFound().json(serde_json::json!({
            "error": format!("endpoint '{slug}' not found")
        }));
    }

    // Build environment for the script
    let method = req.method().to_string();
    let query = req.query_string().to_string();
    let body_str = String::from_utf8_lossy(&body).to_string();

    // Collect request headers
    let mut headers_json = serde_json::Map::new();
    for (name, value) in req.headers() {
        if let Ok(val_str) = value.to_str() {
            headers_json.insert(name.to_string(), serde_json::Value::String(val_str.to_string()));
        }
    }
    let headers_str = serde_json::to_string(&headers_json).unwrap_or_default();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(SCRIPT_TIMEOUT_SECS),
        tokio::process::Command::new("bash")
            .arg(script_path.to_str().unwrap_or_default())
            .env("REQUEST_METHOD", &method)
            .env("QUERY_STRING", &query)
            .env("REQUEST_BODY", &body_str)
            .env("REQUEST_HEADERS", &headers_str)
            .env("ENDPOINT_SLUG", &slug)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if !output.status.success() {
                tracing::warn!(
                    slug = %slug,
                    stderr = %stderr.chars().take(500).collect::<String>(),
                    "Script endpoint failed"
                );
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": "script execution failed",
                    "details": stderr.chars().take(500).collect::<String>()
                }));
            }

            // Truncate output if needed
            let output_str = if stdout.len() > MAX_SCRIPT_OUTPUT {
                &stdout[..MAX_SCRIPT_OUTPUT]
            } else {
                &stdout
            };

            // Try to parse as JSON, otherwise return as text
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(output_str) {
                HttpResponse::Ok().json(json)
            } else {
                HttpResponse::Ok()
                    .content_type("text/plain")
                    .body(output_str.to_string())
            }
        }
        Ok(Err(e)) => {
            tracing::error!(slug = %slug, error = %e, "Script execution error");
            HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "failed to execute script",
                "details": e.to_string()
            }))
        }
        Err(_) => {
            tracing::warn!(slug = %slug, "Script endpoint timed out");
            HttpResponse::GatewayTimeout().json(serde_json::json!({
                "error": format!("script timed out after {SCRIPT_TIMEOUT_SECS}s")
            }))
        }
    }
}

/// `GET /x` — list all available script endpoints.
pub async fn list_scripts() -> HttpResponse {
    let scripts_dir = PathBuf::from(SCRIPTS_DIR);

    if !scripts_dir.exists() {
        return HttpResponse::Ok().json(serde_json::json!({
            "endpoints": [],
            "count": 0
        }));
    }

    let mut endpoints = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "sh") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    // Try to read first line as description (# comment)
                    let description = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|content| {
                            content.lines().next().and_then(|line| {
                                line.strip_prefix("# ").map(String::from)
                            })
                        });

                    endpoints.push(ScriptEndpoint {
                        slug: stem.to_string(),
                        description,
                        method: "GET/POST".to_string(),
                    });
                }
            }
        }
    }

    let count = endpoints.len();
    HttpResponse::Ok().json(serde_json::json!({
        "endpoints": endpoints,
        "count": count
    }))
}

/// Configure script endpoint routes.
pub fn configure(cfg: &mut web::ServiceConfig) {
    cfg.route("/x", web::get().to(list_scripts))
        .route("/x/{slug}", web::get().to(handle_script))
        .route("/x/{slug}", web::post().to(handle_script));
}
