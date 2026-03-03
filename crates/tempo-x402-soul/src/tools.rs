//! Tool definitions and executor for the soul's function calling capabilities.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::coding;
use crate::db::{Mutation, SoulDatabase};
use crate::git::GitContext;
use crate::guard;
use crate::llm::FunctionDeclaration;
use crate::persistent_memory;
use crate::tool_registry::ToolRegistry;

/// Result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// Executes tools requested by the LLM.
pub struct ToolExecutor {
    timeout_secs: u64,
    workspace_root: PathBuf,
    git: Option<Arc<GitContext>>,
    db: Option<Arc<SoulDatabase>>,
    coding_enabled: bool,
    registry: Option<ToolRegistry>,
    memory_file_path: String,
    gateway_url: Option<String>,
}

/// Max output size per stream (stdout/stderr) to stay within LLM context limits.
const MAX_OUTPUT_BYTES: usize = 16384;

/// Max file size for read_file (16KB).
const MAX_READ_BYTES: usize = 16384;

/// Max entries for list_directory.
const MAX_DIR_ENTRIES: usize = 200;

/// Max matches for search_files.
const MAX_SEARCH_MATCHES: usize = 50;

/// Hard cap for execute_shell timeout.
const SHELL_TIMEOUT_CAP: u64 = 300;

impl ToolExecutor {
    /// Create a new tool executor with the given per-command timeout and workspace root.
    pub fn new(timeout_secs: u64, workspace_root: String) -> Self {
        Self {
            timeout_secs,
            workspace_root: PathBuf::from(workspace_root),
            git: None,
            db: None,
            coding_enabled: false,
            registry: None,
            memory_file_path: "/data/soul_memory.md".to_string(),
            gateway_url: None,
        }
    }

    /// Set the persistent memory file path.
    pub fn with_memory_file(mut self, path: String) -> Self {
        self.memory_file_path = path;
        self
    }

    /// Set the gateway URL for endpoint registration.
    pub fn with_gateway_url(mut self, url: Option<String>) -> Self {
        self.gateway_url = url;
        self
    }

    /// Attach the soul database (needed for update_beliefs in all modes).
    pub fn with_database(mut self, db: Arc<SoulDatabase>) -> Self {
        self.db = Some(db);
        self
    }

    /// Enable coding capabilities with git context and database.
    pub fn with_coding(mut self, git: Arc<GitContext>, db: Arc<SoulDatabase>) -> Self {
        self.git = Some(git);
        self.db = Some(db);
        self.coding_enabled = true;
        self
    }

    /// Attach a dynamic tool registry.
    pub fn with_registry(mut self, registry: ToolRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Execute a tool by name with the given arguments.
    pub async fn execute(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<ToolResult, String> {
        match name {
            "execute_shell" => {
                let command = args
                    .get("command")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'command' argument".to_string())?;

                let timeout = args
                    .get("timeout_secs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(self.timeout_secs)
                    .min(SHELL_TIMEOUT_CAP);

                self.execute_shell(command, timeout).await
            }
            "read_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'path' argument".to_string())?;
                let offset = args
                    .get("offset")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                self.read_file(path, offset, limit).await
            }
            "write_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'path' argument".to_string())?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'content' argument".to_string())?;
                self.write_file(path, content).await
            }
            "edit_file" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'path' argument".to_string())?;
                let old_string = args
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'old_string' argument".to_string())?;
                let new_string = args
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'new_string' argument".to_string())?;
                self.edit_file(path, old_string, new_string).await
            }
            "list_directory" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                self.list_directory(path).await
            }
            "search_files" => {
                let pattern = args
                    .get("pattern")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'pattern' argument".to_string())?;
                let path = args.get("path").and_then(|v| v.as_str());
                let glob = args.get("glob").and_then(|v| v.as_str());
                self.search_files(pattern, path, glob).await
            }
            "commit_changes" => {
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'message' argument".to_string())?;
                let files = args
                    .get("files")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "missing 'files' argument (must be array)".to_string())?;
                let file_strs: Vec<&str> = files.iter().filter_map(|v| v.as_str()).collect();
                self.commit_changes(message, &file_strs).await
            }
            "propose_to_main" => {
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'title' argument".to_string())?;
                let body = args
                    .get("body")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Automated PR from soul agent");
                self.propose_to_main(title, body).await
            }
            "create_issue" => {
                let title = args
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'title' argument".to_string())?;
                let body = args.get("body").and_then(|v| v.as_str()).unwrap_or("");
                let labels: Vec<&str> = args
                    .get("labels")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                self.create_issue(title, body, &labels).await
            }
            "update_memory" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'content' argument".to_string())?;
                self.update_memory(content).await
            }
            "check_self" => {
                let endpoint = args
                    .get("endpoint")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'endpoint' argument".to_string())?;
                self.check_self(endpoint).await
            }
            "register_endpoint" => {
                let slug = args
                    .get("slug")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'slug' argument".to_string())?;
                let target_url = args
                    .get("target_url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "missing 'target_url' argument".to_string())?;
                let price = args
                    .get("price")
                    .and_then(|v| v.as_str())
                    .unwrap_or("$0.01");
                let description = args.get("description").and_then(|v| v.as_str());
                self.register_endpoint(slug, target_url, price, description)
                    .await
            }
            "update_beliefs" => {
                let updates = args
                    .get("updates")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "missing 'updates' argument (must be array)".to_string())?;
                self.update_beliefs(updates).await
            }
            _ => {
                // Check meta-tools and dynamic tools via registry
                if let Some(ref registry) = self.registry {
                    if ToolRegistry::is_meta_tool(name) {
                        return registry.execute_meta_tool(name, args).await;
                    }
                    if registry.is_dynamic_tool(name) {
                        return registry.execute_dynamic_tool(name, args).await;
                    }
                }
                Err(format!("unknown tool: {name}"))
            }
        }
    }

    /// Resolve a path relative to workspace root, preventing traversal outside it.
    fn resolve_path(&self, path: &str) -> Result<PathBuf, String> {
        let candidate = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            self.workspace_root.join(path)
        };

        // Canonicalize what exists; for new files, canonicalize the parent
        let resolved = if candidate.exists() {
            candidate
                .canonicalize()
                .map_err(|e| format!("failed to resolve path: {e}"))?
        } else {
            let parent = candidate
                .parent()
                .ok_or_else(|| "invalid path: no parent directory".to_string())?;
            if !parent.exists() {
                return Err(format!(
                    "parent directory does not exist: {}",
                    parent.display()
                ));
            }
            let canon_parent = parent
                .canonicalize()
                .map_err(|e| format!("failed to resolve parent: {e}"))?;
            let filename = candidate
                .file_name()
                .ok_or_else(|| "invalid path: no filename".to_string())?;
            canon_parent.join(filename)
        };

        Ok(resolved)
    }

    /// Get the relative path from workspace root for guard checking.
    fn relative_path(&self, resolved: &Path) -> String {
        resolved
            .strip_prefix(&self.workspace_root)
            .unwrap_or(resolved)
            .to_string_lossy()
            .to_string()
    }

    /// Execute a shell command with timeout and output truncation.
    async fn execute_shell(&self, command: &str, timeout_secs: u64) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(command)
                .current_dir(&self.workspace_root)
                .output(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let stdout = truncate_output(&output.stdout);
                let stderr = truncate_output(&output.stderr);
                let exit_code = output.status.code().unwrap_or(-1);

                Ok(ToolResult {
                    stdout,
                    stderr,
                    exit_code,
                    duration_ms,
                })
            }
            Ok(Err(e)) => Err(format!("command failed to execute: {e}")),
            Err(_) => Ok(ToolResult {
                stdout: String::new(),
                stderr: format!("command timed out after {timeout_secs}s"),
                exit_code: -1,
                duration_ms,
            }),
        }
    }

    /// Read a file with optional offset and limit (line-based).
    async fn read_file(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();
        let resolved = self.resolve_path(path)?;

        // Check file size first
        let metadata = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| format!("cannot read file: {e}"))?;

        if !metadata.is_file() {
            return Err(format!("not a file: {path}"));
        }

        if metadata.len() > MAX_READ_BYTES as u64 && offset.is_none() && limit.is_none() {
            return Err(format!(
                "file too large ({} bytes, max {}). Use offset/limit to read portions.",
                metadata.len(),
                MAX_READ_BYTES
            ));
        }

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| format!("failed to read file: {e}"))?;

        let lines: Vec<&str> = content.lines().collect();
        let start_line = offset.unwrap_or(0);
        let end_line = limit
            .map(|l| (start_line + l).min(lines.len()))
            .unwrap_or(lines.len());

        let mut output = String::new();
        for (i, line) in lines
            .iter()
            .enumerate()
            .skip(start_line)
            .take(end_line - start_line)
        {
            output.push_str(&format!("{:>6}\t{}\n", i + 1, line));
        }

        // Truncate if still too large
        if output.len() > MAX_OUTPUT_BYTES {
            output.truncate(MAX_OUTPUT_BYTES);
            output.push_str("\n... (truncated)");
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: output,
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
        })
    }

    /// Write (create or overwrite) a file. Guard-checked.
    async fn write_file(&self, path: &str, content: &str) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        // Guard check on the raw path (before resolving, to catch traversal)
        guard::validate_write_target(path).map_err(|e| e.to_string())?;

        let resolved = self.resolve_path(path)?;
        let rel = self.relative_path(&resolved);
        guard::validate_write_target(&rel).map_err(|e| e.to_string())?;

        // Ensure parent directory exists
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("failed to create parent directory: {e}"))?;
        }

        tokio::fs::write(&resolved, content)
            .await
            .map_err(|e| format!("failed to write file: {e}"))?;

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: format!("wrote {} bytes to {path}", content.len()),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
        })
    }

    /// Edit a file via search-and-replace. The old_string must appear exactly once. Guard-checked.
    async fn edit_file(
        &self,
        path: &str,
        old_string: &str,
        new_string: &str,
    ) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        // Guard check
        guard::validate_write_target(path).map_err(|e| e.to_string())?;

        let resolved = self.resolve_path(path)?;
        let rel = self.relative_path(&resolved);
        guard::validate_write_target(&rel).map_err(|e| e.to_string())?;

        let content = tokio::fs::read_to_string(&resolved)
            .await
            .map_err(|e| format!("failed to read file: {e}"))?;

        let count = content.matches(old_string).count();
        if count == 0 {
            return Err("old_string not found in file".to_string());
        }
        if count > 1 {
            return Err(format!(
                "old_string found {count} times — must be unique. Provide more context."
            ));
        }

        let new_content = content.replacen(old_string, new_string, 1);
        tokio::fs::write(&resolved, &new_content)
            .await
            .map_err(|e| format!("failed to write file: {e}"))?;

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: format!("edited {path}: replaced 1 occurrence"),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
        })
    }

    /// List directory entries with type indicators.
    async fn list_directory(&self, path: &str) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();
        let resolved = self.resolve_path(path)?;

        let metadata = tokio::fs::metadata(&resolved)
            .await
            .map_err(|e| format!("cannot access path: {e}"))?;

        if !metadata.is_dir() {
            return Err(format!("not a directory: {path}"));
        }

        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&resolved)
            .await
            .map_err(|e| format!("failed to read directory: {e}"))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| format!("failed to read entry: {e}"))?
        {
            if entries.len() >= MAX_DIR_ENTRIES {
                entries.push("... (truncated, too many entries)".to_string());
                break;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let ft = entry.file_type().await;
            let indicator = match ft {
                Ok(ft) if ft.is_dir() => "/",
                Ok(ft) if ft.is_symlink() => "@",
                _ => "",
            };
            entries.push(format!("{name}{indicator}"));
        }

        entries.sort();

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: entries.join("\n"),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
        })
    }

    /// Search for a literal string pattern across files. Uses grep via shell internally for
    /// performance (avoids reimplementing recursive file walking + binary detection).
    async fn search_files(
        &self,
        pattern: &str,
        path: Option<&str>,
        glob: Option<&str>,
    ) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();
        let search_path = path.unwrap_or(".");

        // Build grep command with safe quoting
        let mut cmd = format!("grep -rn --max-count={} -l", MAX_SEARCH_MATCHES);

        if let Some(g) = glob {
            cmd.push_str(&format!(" --include='{}'", g.replace('\'', "'\\''")));
        }

        // Use fixed-string mode for literal search (no regex interpretation)
        cmd.push_str(&format!(
            " -F -- '{}' '{}'",
            pattern.replace('\'', "'\\''"),
            search_path.replace('\'', "'\\''")
        ));

        // Run via shell (in workspace root)
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(&cmd)
                .current_dir(&self.workspace_root)
                .output(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok(output)) => {
                let files = truncate_output(&output.stdout);
                if files.is_empty() {
                    Ok(ToolResult {
                        stdout: "no matches found".to_string(),
                        stderr: String::new(),
                        exit_code: 0,
                        duration_ms,
                    })
                } else {
                    // Now get context lines for matched files (limited)
                    let file_list: Vec<&str> = files.lines().take(MAX_SEARCH_MATCHES).collect();
                    let file_args: String = file_list
                        .iter()
                        .map(|f| format!("'{}'", f.replace('\'', "'\\''").trim()))
                        .collect::<Vec<_>>()
                        .join(" ");

                    let context_cmd = format!(
                        "grep -n -F -- '{}' {} | head -{}",
                        pattern.replace('\'', "'\\''"),
                        file_args,
                        MAX_SEARCH_MATCHES * 3
                    );

                    let ctx_result = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        tokio::process::Command::new("bash")
                            .arg("-c")
                            .arg(&context_cmd)
                            .current_dir(&self.workspace_root)
                            .output(),
                    )
                    .await;

                    let output_text = match ctx_result {
                        Ok(Ok(out)) => truncate_output(&out.stdout),
                        _ => files,
                    };

                    Ok(ToolResult {
                        stdout: output_text,
                        stderr: String::new(),
                        exit_code: 0,
                        duration_ms,
                    })
                }
            }
            Ok(Err(e)) => Err(format!("search failed: {e}")),
            Err(_) => Ok(ToolResult {
                stdout: String::new(),
                stderr: "search timed out after 30s".to_string(),
                exit_code: -1,
                duration_ms,
            }),
        }
    }

    /// Commit changes through the validated pipeline (stage → cargo check → cargo test → commit → push).
    async fn commit_changes(&self, message: &str, files: &[&str]) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        if !self.coding_enabled {
            return Err("coding is not enabled (set SOUL_CODING_ENABLED=true)".to_string());
        }

        let git = self
            .git
            .as_ref()
            .ok_or_else(|| "git context not available".to_string())?;
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| "database not available".to_string())?;

        let workspace = self.workspace_root.to_string_lossy().to_string();
        let result = coding::validated_commit(git, &workspace, files, message).await?;

        // Record mutation in database
        let mutation = Mutation {
            id: uuid::Uuid::new_v4().to_string(),
            commit_sha: result.commit_sha.clone(),
            branch: git.branch_name().to_string(),
            description: message.to_string(),
            files_changed: serde_json::to_string(files).unwrap_or_default(),
            cargo_check_passed: result.cargo_check_passed,
            cargo_test_passed: result.cargo_test_passed,
            created_at: chrono::Utc::now().timestamp(),
        };
        let _ = db.insert_mutation(&mutation);

        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: result.message,
            stderr: String::new(),
            exit_code: if result.success { 0 } else { 1 },
            duration_ms,
        })
    }

    /// Create a PR from the VM branch to main.
    async fn propose_to_main(&self, title: &str, body: &str) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        if !self.coding_enabled {
            return Err("coding is not enabled".to_string());
        }

        let git = self
            .git
            .as_ref()
            .ok_or_else(|| "git context not available".to_string())?;

        let result = git.create_pr(title, body).await?;
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            stdout: result.output,
            stderr: String::new(),
            exit_code: if result.success { 0 } else { 1 },
            duration_ms,
        })
    }

    /// Update the persistent memory file.
    async fn update_memory(&self, content: &str) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();
        let bytes = persistent_memory::update(&self.memory_file_path, content)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        Ok(ToolResult {
            stdout: format!(
                "memory updated ({bytes} bytes written to {})",
                self.memory_file_path
            ),
            stderr: String::new(),
            exit_code: 0,
            duration_ms,
        })
    }

    /// Register an endpoint on the gateway via x402 payment.
    async fn register_endpoint(
        &self,
        slug: &str,
        target_url: &str,
        price: &str,
        description: Option<&str>,
    ) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        if !self.coding_enabled {
            return Err("coding is not enabled (register_endpoint requires Code mode)".to_string());
        }

        let gateway_url = self
            .gateway_url
            .as_deref()
            .unwrap_or("http://localhost:4023");

        let private_key = std::env::var("EVM_PRIVATE_KEY")
            .map_err(|_| "EVM_PRIVATE_KEY not set — cannot sign payment".to_string())?;

        let client = reqwest::Client::new();

        // Build registration body
        let mut body = serde_json::json!({
            "slug": slug,
            "target_url": target_url,
            "price": price,
        });
        if let Some(desc) = description {
            body["description"] = serde_json::Value::String(desc.to_string());
        }

        // Step 1: POST /register → expect 402
        let register_url = format!("{gateway_url}/register");
        let resp = client
            .post(&register_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to POST /register: {e}"))?;

        if resp.status() != reqwest::StatusCode::PAYMENT_REQUIRED {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                let duration_ms = start.elapsed().as_millis() as u64;
                return Ok(ToolResult {
                    stdout: format!("endpoint registered (no payment needed): {text}"),
                    stderr: String::new(),
                    exit_code: 0,
                    duration_ms,
                });
            }
            return Err(format!("expected 402, got {status}: {text}"));
        }

        // Step 2: Parse PaymentRequirements from response
        let resp_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse 402 response: {e}"))?;

        let accepts = resp_json
            .get("accepts")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "402 response missing 'accepts' array".to_string())?;

        let req_value = accepts
            .first()
            .ok_or_else(|| "402 response 'accepts' array is empty".to_string())?;

        let requirements: x402_wallet::PaymentRequirements =
            serde_json::from_value(req_value.clone())
                .map_err(|e| format!("failed to parse PaymentRequirements: {e}"))?;

        // Step 3: Sign payment
        let signer = x402_wallet::WalletSigner::new(&private_key)
            .map_err(|e| format!("failed to create signer: {e}"))?;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("system time error: {e}"))?
            .as_secs();

        let payment_b64 = signer
            .sign_payment(&requirements, now_secs)
            .map_err(|e| format!("failed to sign payment: {e}"))?;

        // Step 4: Retry with payment header
        let resp2 = client
            .post(&register_url)
            .header("PAYMENT-SIGNATURE", &payment_b64)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to retry POST with payment: {e}"))?;

        let status = resp2.status();
        let text = resp2.text().await.unwrap_or_default();
        let duration_ms = start.elapsed().as_millis() as u64;

        if status.is_success() {
            Ok(ToolResult {
                stdout: format!("endpoint /{slug} registered successfully: {text}"),
                stderr: String::new(),
                exit_code: 0,
                duration_ms,
            })
        } else {
            Ok(ToolResult {
                stdout: String::new(),
                stderr: format!("registration failed ({status}): {text}"),
                exit_code: 1,
                duration_ms,
            })
        }
    }

    /// Check the node's own endpoints for self-introspection.
    /// Whitelisted to: health, analytics, analytics/{slug}, soul/status.
    async fn check_self(&self, endpoint: &str) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        // Whitelist check: only allow safe read-only endpoints
        let trimmed = endpoint.trim_start_matches('/');
        let allowed = trimmed == "health"
            || trimmed == "analytics"
            || trimmed == "soul/status"
            || trimmed.starts_with("analytics/");

        if !allowed {
            return Err(format!(
                "endpoint '/{trimmed}' not allowed. Use: health, analytics, analytics/{{slug}}, soul/status"
            ));
        }

        let gateway_url = self
            .gateway_url
            .as_deref()
            .unwrap_or("http://localhost:4023");

        let url = format!("{gateway_url}/{trimmed}");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;

        match client.get(&url).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                let duration_ms = start.elapsed().as_millis() as u64;

                // Truncate body if huge
                let body_truncated = if body.len() > MAX_OUTPUT_BYTES {
                    format!(
                        "{}\n... (truncated)",
                        body.chars().take(MAX_OUTPUT_BYTES).collect::<String>()
                    )
                } else {
                    body
                };

                Ok(ToolResult {
                    stdout: body_truncated,
                    stderr: String::new(),
                    exit_code: status.as_u16() as i32,
                    duration_ms,
                })
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                Ok(ToolResult {
                    stdout: String::new(),
                    stderr: format!("request failed: {e}"),
                    exit_code: -1,
                    duration_ms,
                })
            }
        }
    }

    /// Create an issue on the upstream repo.
    async fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<ToolResult, String> {
        let start = std::time::Instant::now();

        if !self.coding_enabled {
            return Err("coding is not enabled".to_string());
        }

        let git = self
            .git
            .as_ref()
            .ok_or_else(|| "git context not available".to_string())?;

        let result = git.create_issue(title, body, labels).await?;
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            stdout: result.output,
            stderr: String::new(),
            exit_code: if result.success { 0 } else { 1 },
            duration_ms,
        })
    }

    /// Execute belief updates via the world model.
    async fn update_beliefs(&self, updates: &[serde_json::Value]) -> Result<ToolResult, String> {
        use crate::world_model::{Belief, BeliefDomain, Confidence, ModelUpdate};

        let start = std::time::Instant::now();
        let db = self
            .db
            .as_ref()
            .ok_or_else(|| "soul database not available".to_string())?;

        let now = chrono::Utc::now().timestamp();
        let mut applied = 0u32;
        let mut errors = Vec::new();

        for (i, update_val) in updates.iter().enumerate() {
            let update: ModelUpdate = match serde_json::from_value(update_val.clone()) {
                Ok(u) => u,
                Err(e) => {
                    errors.push(format!("update[{i}]: invalid format: {e}"));
                    continue;
                }
            };

            let result = match &update {
                ModelUpdate::Create {
                    domain,
                    subject,
                    predicate,
                    value,
                    evidence,
                } => {
                    let domain = BeliefDomain::parse(domain).unwrap_or(BeliefDomain::Node);
                    let belief = Belief {
                        id: uuid::Uuid::new_v4().to_string(),
                        domain,
                        subject: subject.clone(),
                        predicate: predicate.clone(),
                        value: value.clone(),
                        confidence: Confidence::Medium,
                        evidence: evidence.clone(),
                        confirmation_count: 1,
                        created_at: now,
                        updated_at: now,
                        active: true,
                    };
                    db.upsert_belief(&belief).map(|_| true)
                }
                ModelUpdate::Update {
                    id,
                    value,
                    evidence,
                } => {
                    let beliefs = db.get_all_active_beliefs().map_err(|e| format!("{e}"))?;
                    if let Some(existing) = beliefs.iter().find(|b| b.id == *id) {
                        let updated = Belief {
                            value: value.clone(),
                            evidence: if evidence.is_empty() {
                                existing.evidence.clone()
                            } else {
                                evidence.clone()
                            },
                            updated_at: now,
                            ..existing.clone()
                        };
                        db.upsert_belief(&updated).map(|_| true)
                    } else {
                        Ok(false)
                    }
                }
                ModelUpdate::Confirm { id } => db.confirm_belief(id),
                ModelUpdate::Invalidate { id, reason } => db.invalidate_belief(id, reason),
            };

            match result {
                Ok(true) => applied += 1,
                Ok(false) => errors.push(format!("update[{i}]: no effect (belief not found)")),
                Err(e) => errors.push(format!("update[{i}]: {e}")),
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let stdout = format!("Applied {applied}/{} belief updates", updates.len());
        let stderr = if errors.is_empty() {
            String::new()
        } else {
            errors.join("\n")
        };

        Ok(ToolResult {
            stdout,
            stderr,
            exit_code: if errors.is_empty() { 0 } else { 1 },
            duration_ms,
        })
    }
}

/// Return the update_memory tool declaration (available in Observe, Chat, Code).
pub fn update_memory_tool() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "update_memory".to_string(),
        description: "Update your persistent memory file. This is your long-term memory — it persists across restarts. Write markdown content (max 4KB). The entire content is replaced, so include everything you want to remember.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Full replacement markdown content for your memory file (max 4096 bytes)"
                }
            },
            "required": ["content"]
        }),
    }
}

/// Return the check_self tool declaration (Observe + Chat + Code modes).
pub fn check_self_tool() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "check_self".to_string(),
        description: "Check your own node's endpoints for self-introspection. Whitelisted endpoints: health, analytics, analytics/{slug}, soul/status. Returns the HTTP response body and status code.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "string",
                    "description": "The endpoint path to check (e.g. 'health', 'analytics', 'analytics/weather', 'soul/status')"
                }
            },
            "required": ["endpoint"]
        }),
    }
}

/// Return the update_beliefs tool declaration (Observe + Chat + Code modes).
pub fn update_beliefs_tool() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "update_beliefs".to_string(),
        description: "Update your world model with structured beliefs. Each update is one of: \
            create (new belief), update (change value), confirm (verify still true), \
            invalidate (mark as wrong). Use this to record what you know, not just what you see."
            .to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "updates": {
                    "type": "array",
                    "description": "Array of belief updates to apply",
                    "items": {
                        "type": "object",
                        "properties": {
                            "op": {
                                "type": "string",
                                "enum": ["create", "update", "confirm", "invalidate"],
                                "description": "Operation type"
                            },
                            "domain": {
                                "type": "string",
                                "enum": ["node", "endpoints", "codebase", "strategy", "self"],
                                "description": "Belief domain (required for create)"
                            },
                            "subject": {
                                "type": "string",
                                "description": "What the belief is about (required for create)"
                            },
                            "predicate": {
                                "type": "string",
                                "description": "What aspect (required for create)"
                            },
                            "value": {
                                "type": "string",
                                "description": "The belief value (required for create and update)"
                            },
                            "evidence": {
                                "type": "string",
                                "description": "Why you believe this"
                            },
                            "id": {
                                "type": "string",
                                "description": "Belief ID (required for update, confirm, invalidate)"
                            },
                            "reason": {
                                "type": "string",
                                "description": "Why invalidating (required for invalidate)"
                            }
                        },
                        "required": ["op"]
                    }
                }
            },
            "required": ["updates"]
        }),
    }
}

/// Return the register_endpoint tool declaration (Code mode only).
pub fn register_endpoint_tool() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "register_endpoint".to_string(),
        description: "Register a new paid endpoint on the gateway. Handles the full x402 payment flow: sends registration request, signs payment authorization, and completes registration.".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "slug": {
                    "type": "string",
                    "description": "URL slug for the endpoint (e.g. 'weather', 'translate')"
                },
                "target_url": {
                    "type": "string",
                    "description": "The backend URL this endpoint proxies to"
                },
                "price": {
                    "type": "string",
                    "description": "Price per request (default '$0.01')"
                },
                "description": {
                    "type": "string",
                    "description": "Optional description of what this endpoint does"
                }
            },
            "required": ["slug", "target_url"]
        }),
    }
}

/// Return the list of function declarations for the LLM's tools parameter.
pub fn available_tools() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration {
            name: "execute_shell".to_string(),
            description: "Execute a shell command in the node's container. Use for non-file operations (curl, env, df, cargo). Prefer file tools for reading/writing files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Max seconds to wait (default 120, max 300)"
                    }
                },
                "required": ["command"]
            }),
        },
        FunctionDeclaration {
            name: "read_file".to_string(),
            description: "Read a file with line numbers. Returns numbered lines. Use offset/limit for large files.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path (relative to workspace root or absolute)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Start reading from this line (0-indexed, optional)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read (optional)"
                    }
                },
                "required": ["path"]
            }),
        },
        FunctionDeclaration {
            name: "write_file".to_string(),
            description: "Create or overwrite a file. Protected files (soul core, identity, Cargo files) cannot be written.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to write (relative to workspace root or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "The full content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        },
        FunctionDeclaration {
            name: "edit_file".to_string(),
            description: "Edit a file via search-and-replace. The old_string must appear exactly once in the file. Protected files cannot be edited.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to edit (relative to workspace root or absolute)"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The exact string to find (must be unique in the file)"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement string"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        FunctionDeclaration {
            name: "list_directory".to_string(),
            description: "List entries in a directory with type indicators (/ for dirs, @ for symlinks).".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path (relative to workspace root or absolute, defaults to '.')"
                    }
                },
                "required": []
            }),
        },
        FunctionDeclaration {
            name: "search_files".to_string(),
            description: "Search for a literal string across files. Returns matching file paths and lines with line numbers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "The literal string to search for"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (defaults to workspace root)"
                    },
                    "glob": {
                        "type": "string",
                        "description": "File glob pattern to filter (e.g. '*.rs', '*.toml')"
                    }
                },
                "required": ["pattern"]
            }),
        },
    ]
}

/// Return tool declarations including git/coding tools (when coding is enabled).
pub fn available_tools_with_git(coding_enabled: bool) -> Vec<FunctionDeclaration> {
    let mut tools = available_tools();

    if coding_enabled {
        tools.push(FunctionDeclaration {
            name: "commit_changes".to_string(),
            description: "Validate and commit file changes. Runs cargo check + cargo test before committing. Changes go to the vm/<instance-id> branch.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Commit message describing the changes"
                    },
                    "files": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Array of file paths to stage and commit"
                    }
                },
                "required": ["message", "files"]
            }),
        });

        tools.push(FunctionDeclaration {
            name: "propose_to_main".to_string(),
            description: "Create a pull request from the VM branch to main for human review. If fork workflow is configured, creates a cross-fork PR."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "PR title (short, descriptive)"
                    },
                    "body": {
                        "type": "string",
                        "description": "PR body/description with details of changes"
                    }
                },
                "required": ["title"]
            }),
        });

        tools.push(FunctionDeclaration {
            name: "create_issue".to_string(),
            description: "Create a GitHub issue on the upstream repository. Use for bug reports, feature requests, improvement ideas, or tracking work."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Issue title (short, descriptive)"
                    },
                    "body": {
                        "type": "string",
                        "description": "Issue body with details, context, and proposed approach"
                    },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional labels to apply (e.g. ['enhancement', 'bug'])"
                    }
                },
                "required": ["title"]
            }),
        });
    }

    tools
}

/// Truncate raw output bytes to a UTF-8 string, capping at MAX_OUTPUT_BYTES.
fn truncate_output(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    if s.len() <= MAX_OUTPUT_BYTES {
        s.into_owned()
    } else {
        let truncated: String = s.chars().take(MAX_OUTPUT_BYTES).collect();
        format!("{truncated}\n... (truncated)")
    }
}
