//! Plan-driven execution: deterministic step execution replaces prompt-and-pray.
//!
//! Goals decompose into Plans (ordered steps). Each cycle executes one step.
//! Most steps are mechanical (no LLM). LLM is only called for planning,
//! code generation, and reflection.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::SoulConfig;
use crate::db::SoulDatabase;
use crate::llm::{ConversationMessage, ConversationPart, FunctionDeclaration, LlmClient};
use crate::mode::AgentMode;
use crate::tools::{self, ToolExecutor};

/// Status of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    Active,
    PendingApproval,
    Completed,
    Failed,
    Abandoned,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::PendingApproval => "pending_approval",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "active" => Some(Self::Active),
            "pending_approval" => Some(Self::PendingApproval),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

/// A single step in a plan. 6 mechanical (no LLM), 3 LLM-assisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlanStep {
    /// Read a file and store contents.
    ReadFile {
        path: String,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Search for a pattern in code.
    SearchCode {
        pattern: String,
        #[serde(default)]
        directory: Option<String>,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// List directory contents.
    ListDir {
        path: String,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Run a shell command.
    RunShell {
        command: String,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Commit staged changes.
    Commit { message: String },
    /// Check self via HTTP endpoint.
    CheckSelf {
        endpoint: String,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Create a script endpoint — instant HTTP handler, no compilation.
    CreateScriptEndpoint {
        slug: String,
        script: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Test a script endpoint.
    TestScriptEndpoint {
        slug: String,
        #[serde(default)]
        input: Option<String>,
        #[serde(default)]
        store_as: Option<String>,
    },
    /// Run cargo check to verify compilation. Stores errors if any.
    CargoCheck {
        #[serde(default)]
        store_as: Option<String>,
    },
    /// LLM generates code and writes to a file.
    GenerateCode {
        file_path: String,
        description: String,
        #[serde(default)]
        context_keys: Vec<String>,
    },
    /// LLM edits an existing file.
    EditCode {
        file_path: String,
        description: String,
        #[serde(default)]
        context_keys: Vec<String>,
    },
    /// LLM thinks about a question — stores answer in plan context.
    Think {
        question: String,
        #[serde(default)]
        store_as: Option<String>,
    },
}

impl PlanStep {
    /// Whether this step requires an LLM call.
    pub fn needs_llm(&self) -> bool {
        matches!(
            self,
            PlanStep::GenerateCode { .. }
                | PlanStep::EditCode { .. }
                | PlanStep::Think { .. }
        )
    }

    /// Short description for logging/status.
    pub fn summary(&self) -> String {
        match self {
            PlanStep::ReadFile { path, .. } => format!("read {path}"),
            PlanStep::SearchCode { pattern, .. } => format!("search '{pattern}'"),
            PlanStep::ListDir { path, .. } => format!("ls {path}"),
            PlanStep::RunShell { command, .. } => {
                let short = if command.len() > 40 {
                    format!("{}...", &command[..40])
                } else {
                    command.clone()
                };
                format!("shell: {short}")
            }
            PlanStep::Commit { message } => format!("commit: {message}"),
            PlanStep::CheckSelf { endpoint, .. } => format!("check /{endpoint}"),
            PlanStep::CreateScriptEndpoint { slug, .. } => format!("create /x/{slug}"),
            PlanStep::TestScriptEndpoint { slug, .. } => format!("test /x/{slug}"),
            PlanStep::CargoCheck { .. } => "cargo check".to_string(),
            PlanStep::GenerateCode {
                file_path,
                description,
                ..
            } => format!(
                "generate {file_path}: {}",
                &description[..description.len().min(40)]
            ),
            PlanStep::EditCode {
                file_path,
                description,
                ..
            } => format!(
                "edit {file_path}: {}",
                &description[..description.len().min(40)]
            ),
            PlanStep::Think { question, .. } => {
                format!("think: {}", &question[..question.len().min(40)])
            }
        }
    }

    /// Get the store_as key if present.
    pub fn store_key(&self) -> Option<&str> {
        match self {
            PlanStep::ReadFile { store_as, .. }
            | PlanStep::SearchCode { store_as, .. }
            | PlanStep::ListDir { store_as, .. }
            | PlanStep::RunShell { store_as, .. }
            | PlanStep::CheckSelf { store_as, .. }
            | PlanStep::CreateScriptEndpoint { store_as, .. }
            | PlanStep::TestScriptEndpoint { store_as, .. }
            | PlanStep::CargoCheck { store_as, .. }
            | PlanStep::Think { store_as, .. } => store_as.as_deref(),
            PlanStep::Commit { .. } | PlanStep::GenerateCode { .. } | PlanStep::EditCode { .. } => {
                None
            }
        }
    }
}

/// A plan: ordered list of steps to achieve a goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub goal_id: String,
    pub steps: Vec<PlanStep>,
    pub current_step: usize,
    pub status: PlanStatus,
    /// Accumulated context from step results. Keys from store_as fields.
    pub context: HashMap<String, String>,
    /// How many times this plan has been replanned.
    pub replan_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Result of executing a single step.
#[derive(Debug)]
pub enum StepResult {
    /// Step completed successfully. Contains output text.
    Success(String),
    /// Step failed with an error.
    Failed(String),
    /// Step needs the plan to be adjusted.
    NeedsReplan(String),
}

/// Executes plan steps — mechanical steps directly, LLM steps via tool loop.
pub struct PlanExecutor<'a> {
    tool_executor: &'a ToolExecutor,
    llm: &'a LlmClient,
    config: &'a SoulConfig,
    db: &'a Arc<SoulDatabase>,
}

impl<'a> PlanExecutor<'a> {
    pub fn new(
        tool_executor: &'a ToolExecutor,
        llm: &'a LlmClient,
        config: &'a SoulConfig,
        db: &'a Arc<SoulDatabase>,
    ) -> Self {
        Self {
            tool_executor,
            llm,
            config,
            db,
        }
    }

    /// Execute a single plan step.
    pub async fn execute_step(
        &self,
        step: &PlanStep,
        plan_context: &HashMap<String, String>,
    ) -> StepResult {
        match step {
            PlanStep::ReadFile { path, .. } => {
                self.execute_tool("read_file", &serde_json::json!({ "path": path }))
                    .await
            }
            PlanStep::SearchCode {
                pattern, directory, ..
            } => {
                let mut args = serde_json::json!({ "pattern": pattern });
                if let Some(dir) = directory {
                    args["directory"] = serde_json::json!(dir);
                }
                self.execute_tool("search_files", &args).await
            }
            PlanStep::ListDir { path, .. } => {
                self.execute_tool("list_directory", &serde_json::json!({ "path": path }))
                    .await
            }
            PlanStep::RunShell { command, .. } => {
                self.execute_tool("execute_shell", &serde_json::json!({ "command": command }))
                    .await
            }
            PlanStep::Commit { message } => {
                self.execute_tool("commit_changes", &serde_json::json!({ "message": message }))
                    .await
            }
            PlanStep::CheckSelf { endpoint, .. } => {
                self.execute_tool("check_self", &serde_json::json!({ "endpoint": endpoint }))
                    .await
            }
            PlanStep::CreateScriptEndpoint {
                slug,
                script,
                description,
                ..
            } => {
                let mut args = serde_json::json!({ "slug": slug, "script": script });
                if let Some(desc) = description {
                    args["description"] = serde_json::json!(desc);
                }
                self.execute_tool("create_script_endpoint", &args).await
            }
            PlanStep::TestScriptEndpoint { slug, input, .. } => {
                let mut args = serde_json::json!({ "slug": slug });
                if let Some(inp) = input {
                    args["input"] = serde_json::json!(inp);
                }
                self.execute_tool("test_script_endpoint", &args).await
            }
            PlanStep::CargoCheck { .. } => self.execute_cargo_check().await,
            PlanStep::GenerateCode {
                file_path,
                description,
                context_keys,
            } => {
                self.execute_code_step(file_path, description, context_keys, plan_context, false)
                    .await
            }
            PlanStep::EditCode {
                file_path,
                description,
                context_keys,
            } => {
                self.execute_code_step(file_path, description, context_keys, plan_context, true)
                    .await
            }
            PlanStep::Think { question, .. } => {
                self.execute_think_step(question, plan_context).await
            }
        }
    }

    /// Execute a mechanical tool call.
    async fn execute_tool(&self, name: &str, args: &serde_json::Value) -> StepResult {
        match self.tool_executor.execute(name, args).await {
            Ok(result) => {
                if result.exit_code == 0 {
                    StepResult::Success(result.stdout)
                } else if !result.stderr.is_empty() {
                    StepResult::Failed(result.stderr)
                } else {
                    StepResult::Success(result.stdout)
                }
            }
            Err(e) => StepResult::Failed(e),
        }
    }

    /// Execute a standalone cargo check step.
    async fn execute_cargo_check(&self) -> StepResult {
        let ws = self.config.workspace_root.clone();
        let (passed, errors) = crate::coding::run_cargo_check(&ws).await;
        if passed {
            StepResult::Success("cargo check passed".to_string())
        } else {
            let err_msg = errors.unwrap_or_else(|| "unknown error".to_string());
            StepResult::Failed(format!("cargo check failed:\n{err_msg}"))
        }
    }

    /// Execute a code generation/edit step via LLM with focused prompt.
    /// Includes a compile-fix loop: after the LLM writes code, runs cargo check
    /// and feeds errors back for up to 3 fix attempts.
    async fn execute_code_step(
        &self,
        file_path: &str,
        description: &str,
        context_keys: &[String],
        plan_context: &HashMap<String, String>,
        is_edit: bool,
    ) -> StepResult {
        // Read the current file content (if editing)
        let current_content = if is_edit {
            match self
                .tool_executor
                .execute("read_file", &serde_json::json!({ "path": file_path }))
                .await
            {
                Ok(r) => Some(r.stdout),
                Err(e) => return StepResult::Failed(format!("Cannot read {file_path}: {e}")),
            }
        } else {
            None
        };

        // Build focused context from plan context
        let mut context_parts = Vec::new();
        for key in context_keys {
            if let Some(value) = plan_context.get(key) {
                // Truncate large context values
                let truncated = if value.len() > 4000 {
                    format!("{}...(truncated)", &value[..4000])
                } else {
                    value.clone()
                };
                context_parts.push(format!("## {key}\n{truncated}"));
            }
        }
        let context_section = if context_parts.is_empty() {
            String::new()
        } else {
            format!("\n\n# Context\n{}", context_parts.join("\n\n"))
        };

        let prompt = crate::prompts::code_generation_prompt(
            file_path,
            current_content.as_deref(),
            description,
            &context_section,
        );

        let system_prompt = crate::prompts::system_prompt_for_mode(AgentMode::Code, self.config);
        let code_tools = self.code_tools();
        let use_deep = self.config.direct_push && self.config.autonomous_coding;

        let mut conversation = vec![ConversationMessage {
            role: "user".to_string(),
            parts: vec![ConversationPart::Text(prompt)],
        }];

        // ── Initial code generation ──
        let initial_result = match crate::thinking::run_tool_loop_with_model(
            self.llm,
            &system_prompt,
            &mut conversation,
            &code_tools,
            self.tool_executor,
            self.db,
            20, // generous budget: read + search + edit + check + fix
            use_deep,
        )
        .await
        {
            Ok(result) => {
                if result.tool_executions.is_empty() {
                    return StepResult::NeedsReplan(format!(
                        "LLM did not write any files. Response: {}",
                        &result.text[..result.text.len().min(200)]
                    ));
                }
                result
            }
            Err(e) => return StepResult::Failed(format!("LLM code step failed: {e}")),
        };

        // ── Compile-fix loop: cargo check → fix errors → repeat (up to 3 times) ──
        let ws = self.config.workspace_root.clone();
        let max_fix_attempts = 3;

        for attempt in 0..max_fix_attempts {
            let (passed, errors) = crate::coding::run_cargo_check(&ws).await;
            if passed {
                let suffix = if attempt > 0 {
                    format!(" (fixed after {attempt} cargo check iteration(s))")
                } else {
                    String::new()
                };
                return StepResult::Success(format!(
                    "Code step completed ({} tool calls){suffix}: {}",
                    initial_result.tool_executions.len(),
                    initial_result.text.chars().take(200).collect::<String>()
                ));
            }

            let err_msg = errors.unwrap_or_else(|| "unknown error".to_string());
            tracing::warn!(
                attempt = attempt + 1,
                file_path,
                "cargo check failed after code generation — attempting fix"
            );

            // Re-read the file so the LLM sees what it actually wrote
            let current_file = match self
                .tool_executor
                .execute("read_file", &serde_json::json!({ "path": file_path }))
                .await
            {
                Ok(r) => r.stdout,
                Err(_) => String::new(),
            };

            // Build a fix prompt with the cargo errors
            let fix_prompt = format!(
                "# Compilation Error (attempt {}/{})\n\n\
                 The code you just wrote does NOT compile. Fix the errors below.\n\n\
                 ## cargo check errors\n```\n{}\n```\n\n\
                 ## Current content of {}\n```rust\n{}\n```\n\n\
                 Use edit_file to fix ONLY the compilation errors. Do not rewrite the entire file.\n\
                 Focus on: missing imports, wrong types, incorrect function signatures, syntax errors.",
                attempt + 1,
                max_fix_attempts,
                &err_msg[..err_msg.len().min(3000)],
                file_path,
                &current_file[..current_file.len().min(4000)],
            );

            conversation.push(ConversationMessage {
                role: "user".to_string(),
                parts: vec![ConversationPart::Text(fix_prompt)],
            });

            // Give LLM a chance to fix
            match crate::thinking::run_tool_loop_with_model(
                self.llm,
                &system_prompt,
                &mut conversation,
                &code_tools,
                self.tool_executor,
                self.db,
                8, // smaller budget for fixes
                use_deep,
            )
            .await
            {
                Ok(_) => {
                    // Loop back to check again
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Fix attempt LLM call failed");
                    // Continue to next attempt or fall through to failure
                }
            }
        }

        // Final check after all fix attempts
        let (passed, errors) = crate::coding::run_cargo_check(&ws).await;
        if passed {
            return StepResult::Success(format!(
                "Code step completed (fixed after {} attempts): {}",
                max_fix_attempts,
                initial_result.text.chars().take(200).collect::<String>()
            ));
        }

        let err_msg = errors.unwrap_or_else(|| "unknown error".to_string());
        StepResult::Failed(format!(
            "Code does not compile after {} fix attempts. Last errors:\n{}",
            max_fix_attempts,
            &err_msg[..err_msg.len().min(2000)]
        ))
    }

    /// Execute a Think step — ask LLM a focused question.
    async fn execute_think_step(
        &self,
        question: &str,
        plan_context: &HashMap<String, String>,
    ) -> StepResult {
        // Include relevant context in the question
        let full_question = if plan_context.is_empty() {
            question.to_string()
        } else {
            let ctx: Vec<String> = plan_context
                .iter()
                .map(|(k, v)| {
                    let truncated = if v.len() > 1000 {
                        format!("{}...", &v[..1000])
                    } else {
                        v.clone()
                    };
                    format!("{k}: {truncated}")
                })
                .collect();
            format!("{question}\n\nContext:\n{}", ctx.join("\n"))
        };

        match self
            .llm
            .think(
                "You are a software engineering assistant. Answer concisely and specifically.",
                &full_question,
            )
            .await
        {
            Ok(answer) => StepResult::Success(answer),
            Err(e) => StepResult::Failed(format!("LLM think failed: {e}")),
        }
    }

    /// Get the focused code-mode tool declarations.
    /// Includes file ops + shell (for cargo check, grep, etc.) — the LLM
    /// needs the same tools a human developer would use.
    fn code_tools(&self) -> Vec<FunctionDeclaration> {
        let all = tools::available_tools_with_git(self.config.coding_enabled);
        all.into_iter()
            .filter(|t| {
                matches!(
                    t.name.as_str(),
                    "read_file"
                        | "write_file"
                        | "edit_file"
                        | "list_directory"
                        | "search_files"
                        | "execute_shell"
                        | "commit_changes"
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_step_roundtrip() {
        let steps = vec![
            PlanStep::ReadFile {
                path: "src/lib.rs".to_string(),
                store_as: Some("lib_content".to_string()),
            },
            PlanStep::GenerateCode {
                file_path: "src/new.rs".to_string(),
                description: "Add a hello function".to_string(),
                context_keys: vec!["lib_content".to_string()],
            },
            PlanStep::Commit {
                message: "feat: add hello function".to_string(),
            },
        ];

        let json = serde_json::to_string(&steps).unwrap();
        let parsed: Vec<PlanStep> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 3);
        assert!(!parsed[0].needs_llm());
        assert!(parsed[1].needs_llm());
        assert!(!parsed[2].needs_llm());
    }

    #[test]
    fn test_plan_status_parse() {
        assert_eq!(PlanStatus::parse("active"), Some(PlanStatus::Active));
        assert_eq!(
            PlanStatus::parse("pending_approval"),
            Some(PlanStatus::PendingApproval)
        );
        assert_eq!(PlanStatus::parse("completed"), Some(PlanStatus::Completed));
        assert_eq!(PlanStatus::parse("failed"), Some(PlanStatus::Failed));
        assert_eq!(PlanStatus::parse("abandoned"), Some(PlanStatus::Abandoned));
        assert_eq!(PlanStatus::parse("unknown"), None);
    }

    #[test]
    fn test_plan_roundtrip() {
        let plan = Plan {
            id: "test-id".to_string(),
            goal_id: "goal-1".to_string(),
            steps: vec![
                PlanStep::ReadFile {
                    path: "Cargo.toml".to_string(),
                    store_as: Some("cargo".to_string()),
                },
                PlanStep::Think {
                    question: "What dependencies are needed?".to_string(),
                    store_as: Some("deps".to_string()),
                },
            ],
            current_step: 0,
            status: PlanStatus::Active,
            context: HashMap::new(),
            replan_count: 0,
            created_at: 1000,
            updated_at: 1000,
        };

        let json = serde_json::to_string(&plan).unwrap();
        let parsed: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "test-id");
        assert_eq!(parsed.steps.len(), 2);
        assert_eq!(parsed.status, PlanStatus::Active);
    }

    #[test]
    fn test_step_summary() {
        let step = PlanStep::ReadFile {
            path: "src/main.rs".to_string(),
            store_as: None,
        };
        assert_eq!(step.summary(), "read src/main.rs");

        let step = PlanStep::Commit {
            message: "fix: update deps".to_string(),
        };
        assert_eq!(step.summary(), "commit: fix: update deps");
    }
}
