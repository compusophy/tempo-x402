//! The thinking loop: periodic observe → think → record cycle with tool execution.
//!
//! Uses adaptive pacing: the interval between think cycles varies based on
//! what's happening (novelty, tool use, decisions made) rather than being fixed.

use std::sync::Arc;

use serde::Serialize;

use crate::config::SoulConfig;
use crate::db::SoulDatabase;
use crate::error::SoulError;
use crate::git::GitContext;
use crate::llm::{
    ConversationMessage, ConversationPart, FunctionDeclaration, FunctionResponse, LlmClient,
    LlmResult,
};
use crate::memory::{Thought, ThoughtType};
use crate::mode::AgentMode;
use crate::observer::{NodeObserver, NodeSnapshot};
use crate::persistent_memory;
use crate::prompts;
use crate::tool_registry::ToolRegistry;
use crate::tools::{self, ToolExecutor};

/// Adaptive pacing: adjusts think interval based on novelty and activity.
struct AdaptivePacer {
    /// The configured base interval (seconds).
    base_secs: u64,
    /// Current interval (seconds). Adjusted after each cycle.
    current_secs: u64,
    /// Previous snapshot for change detection.
    prev_snapshot: Option<NodeSnapshot>,
    /// Number of consecutive "boring" cycles (no novelty, no tools, no decisions).
    boring_streak: u32,
    /// Number of consecutive "active" cycles (tools used or decisions made).
    active_streak: u32,
}

/// Floor: never think faster than once per 60s.
const MIN_INTERVAL_SECS: u64 = 60;
/// Ceiling: never go longer than 1 hour between thoughts.
const MAX_INTERVAL_SECS: u64 = 3600;
/// After a cycle with tool use, think again sooner.
const ACTIVE_COOLDOWN_SECS: u64 = 120;
/// After a cycle with a decision, think again at moderate pace.
const DECISION_COOLDOWN_SECS: u64 = 300;

impl AdaptivePacer {
    fn new(base_secs: u64) -> Self {
        Self {
            base_secs,
            current_secs: base_secs,
            prev_snapshot: None,
            boring_streak: 0,
            active_streak: 0,
        }
    }

    /// Compute the next sleep interval based on what happened this cycle.
    fn next_interval(&mut self, snapshot: &NodeSnapshot, cycle_result: &CycleResult) -> u64 {
        let novelty = self.compute_novelty(snapshot);
        let used_tools = cycle_result.tool_calls > 0;
        let made_decisions = cycle_result.decisions > 0;
        let requested_soon = cycle_result.think_soon;

        // Update streaks
        if used_tools || made_decisions || novelty > 0.3 {
            self.active_streak += 1;
            self.boring_streak = 0;
        } else {
            self.boring_streak += 1;
            self.active_streak = 0;
        }

        // Calculate next interval
        let next = if requested_soon {
            // Soul explicitly asked to think again soon
            MIN_INTERVAL_SECS
        } else if used_tools && self.active_streak > 1 {
            // Deep work: multiple active cycles in a row → stay engaged
            ACTIVE_COOLDOWN_SECS
        } else if used_tools {
            // Single active cycle → moderate cooldown
            DECISION_COOLDOWN_SECS
        } else if made_decisions {
            // Made a decision but no tools → check back at moderate pace
            DECISION_COOLDOWN_SECS
        } else if novelty > 0.5 {
            // Significant change in node state → investigate sooner
            self.base_secs / 3
        } else if novelty > 0.1 {
            // Minor change → slightly sooner
            self.base_secs / 2
        } else if self.boring_streak >= 5 {
            // Long boring streak → settle to a moderate rhythm, don't exponentially backoff
            // The soul should still think regularly — it has tools to explore
            self.base_secs + 300 // base + 5min, capped by MAX below
        } else if self.boring_streak >= 2 {
            // Mildly boring → normal pace, the prompt will nudge exploration
            self.base_secs
        } else {
            // Normal → use base interval
            self.base_secs
        };

        // Clamp to bounds
        self.current_secs = next.clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS);

        // Store snapshot for next comparison
        self.prev_snapshot = Some(snapshot.clone());

        self.current_secs
    }

    /// Compute a novelty score [0.0, 1.0] by comparing current snapshot to previous.
    fn compute_novelty(&self, current: &NodeSnapshot) -> f64 {
        let prev = match &self.prev_snapshot {
            Some(p) => p,
            None => return 1.0, // First observation is always novel
        };

        let mut score = 0.0;
        let mut factors = 0;

        // New payments
        if current.total_payments != prev.total_payments {
            score += 0.4;
        }
        factors += 1;

        // Revenue changed
        if current.total_revenue != prev.total_revenue {
            score += 0.3;
        }
        factors += 1;

        // Endpoint count changed
        if current.endpoint_count != prev.endpoint_count {
            score += 0.3;
        }
        factors += 1;

        // Children count changed
        if current.children_count != prev.children_count {
            score += 0.4;
        }
        factors += 1;

        // Wallet appeared or changed
        if current.wallet_address != prev.wallet_address {
            score += 0.2;
        }
        factors += 1;

        // Instance ID appeared
        if current.instance_id != prev.instance_id {
            score += 0.2;
        }
        factors += 1;

        (score / factors as f64).min(1.0)
    }
}

/// Result of a single think cycle, used by the adaptive pacer.
struct CycleResult {
    tool_calls: u32,
    decisions: u32,
    /// Whether the LLM output contained [THINK_SOON] to request faster re-thinking.
    think_soon: bool,
}

/// The thinking loop that drives the soul.
pub struct ThinkingLoop {
    config: SoulConfig,
    db: Arc<SoulDatabase>,
    llm: Option<LlmClient>,
    observer: Arc<dyn NodeObserver>,
    tool_executor: ToolExecutor,
}

impl ThinkingLoop {
    /// Create a new thinking loop.
    pub fn new(config: SoulConfig, db: Arc<SoulDatabase>, observer: Arc<dyn NodeObserver>) -> Self {
        let llm = config.llm_api_key.as_ref().map(|key| {
            LlmClient::new(
                key.clone(),
                config.llm_model_fast.clone(),
                config.llm_model_think.clone(),
            )
        });

        let mut tool_executor =
            ToolExecutor::new(config.tool_timeout_secs, config.workspace_root.clone())
                .with_memory_file(config.memory_file_path.clone())
                .with_gateway_url(config.gateway_url.clone());

        // Set up coding if enabled and instance_id is available
        if config.coding_enabled {
            if let Some(instance_id) = &config.instance_id {
                let git = Arc::new(
                    GitContext::new(
                        config.workspace_root.clone(),
                        instance_id.clone(),
                        config.github_token.clone(),
                    )
                    .with_fork(config.fork_repo.clone(), config.upstream_repo.clone())
                    .with_direct_push(config.direct_push),
                );
                tool_executor = tool_executor.with_coding(git, db.clone());
                tracing::info!(
                    instance_id = %instance_id,
                    fork = ?config.fork_repo,
                    upstream = ?config.upstream_repo,
                    direct_push = config.direct_push,
                    "Soul coding enabled"
                );
            } else {
                tracing::warn!("SOUL_CODING_ENABLED=true but no INSTANCE_ID set — coding disabled");
            }
        }

        // Set up dynamic tool registry if enabled
        if config.dynamic_tools_enabled {
            let registry = ToolRegistry::new(
                db.clone(),
                config.workspace_root.clone(),
                config.tool_timeout_secs,
            );
            tool_executor = tool_executor.with_registry(registry);
            tracing::info!("Soul dynamic tool registry enabled");
        }

        Self {
            config,
            db,
            llm,
            observer,
            tool_executor,
        }
    }

    /// Run the thinking loop with adaptive pacing.
    pub async fn run(&self) {
        let mut pacer = AdaptivePacer::new(self.config.think_interval_secs);

        // Initialize git workspace if coding is enabled
        if self.config.coding_enabled {
            if let Some(instance_id) = &self.config.instance_id {
                let git = GitContext::new(
                    self.config.workspace_root.clone(),
                    instance_id.clone(),
                    self.config.github_token.clone(),
                )
                .with_fork(
                    self.config.fork_repo.clone(),
                    self.config.upstream_repo.clone(),
                )
                .with_direct_push(self.config.direct_push);
                match git.init_workspace().await {
                    Ok(r) => tracing::info!(output = %r.output, "Git workspace initialized"),
                    Err(e) => tracing::warn!(error = %e, "Failed to initialize git workspace"),
                }
                // Ensure correct branch
                let branch_label = if self.config.direct_push {
                    "main (direct push)"
                } else {
                    "VM branch"
                };
                match git.ensure_branch().await {
                    Ok(r) => tracing::info!(output = %r.output, "{} ready", branch_label),
                    Err(e) => tracing::warn!(error = %e, "Failed to ensure {}", branch_label),
                }
            }
        }

        tracing::info!(
            base_interval_secs = self.config.think_interval_secs,
            dormant = self.llm.is_none(),
            tools_enabled = self.config.tools_enabled,
            "Soul thinking loop started (adaptive pacing)"
        );

        loop {
            let snapshot = match self.observer.observe() {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "Soul observe failed");
                    tokio::time::sleep(std::time::Duration::from_secs(pacer.current_secs)).await;
                    continue;
                }
            };

            let cycle_result = match self.think_cycle_with_snapshot(&snapshot, &pacer).await {
                Ok(result) => result,
                Err(e) => {
                    tracing::warn!(error = %e, "Soul think cycle failed");
                    CycleResult {
                        tool_calls: 0,
                        decisions: 0,
                        think_soon: false,
                    }
                }
            };

            let next_secs = pacer.next_interval(&snapshot, &cycle_result);
            tracing::info!(
                next_interval_secs = next_secs,
                boring_streak = pacer.boring_streak,
                active_streak = pacer.active_streak,
                tool_calls = cycle_result.tool_calls,
                decisions = cycle_result.decisions,
                "Soul cycle complete, next think in {}s",
                next_secs
            );

            tokio::time::sleep(std::time::Duration::from_secs(next_secs)).await;
        }
    }

    /// Execute one think cycle with a pre-captured snapshot.
    /// Returns cycle metadata for adaptive pacing.
    async fn think_cycle_with_snapshot(
        &self,
        snapshot: &NodeSnapshot,
        pacer: &AdaptivePacer,
    ) -> Result<CycleResult, SoulError> {
        let snapshot_json = serde_json::to_string(snapshot)?;

        // Record observation — full snapshot JSON goes in context, content stays brief
        let obs_thought = Thought {
            id: uuid::Uuid::new_v4().to_string(),
            thought_type: ThoughtType::Observation,
            content: format!(
                "Node state captured (uptime {}h, {} endpoints, {} payments)",
                snapshot.uptime_secs / 3600,
                snapshot.endpoint_count,
                snapshot.total_payments,
            ),
            context: Some(snapshot_json.clone()),
            created_at: chrono::Utc::now().timestamp(),
        };
        self.db.insert_thought(&obs_thought)?;

        // If dormant (no API key), stop here
        let llm = match &self.llm {
            Some(g) => g,
            None => {
                tracing::debug!("Soul dormant — observation recorded, skipping LLM");
                self.increment_cycle_count()?;
                return Ok(CycleResult {
                    tool_calls: 0,
                    decisions: 0,
                    think_soon: false,
                });
            }
        };

        // Determine mode for this cycle
        let mode = AgentMode::Observe;

        // Read persistent memory
        let memory_content = match persistent_memory::read_or_seed(&self.config.memory_file_path) {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read persistent memory");
                String::new()
            }
        };

        // Structured thought retrieval: mix of types for richer context
        let decisions = self
            .db
            .recent_thoughts_by_type(&[ThoughtType::Decision], 3)?;
        let reasoning = self
            .db
            .recent_thoughts_by_type(&[ThoughtType::Reasoning], 3)?;
        let observations = self
            .db
            .recent_thoughts_by_type(&[ThoughtType::Observation], 2)?;
        let consolidations = self
            .db
            .recent_thoughts_by_type(&[ThoughtType::MemoryConsolidation], 1)?;

        // Merge and sort by created_at DESC
        let mut recent: Vec<Thought> = Vec::new();
        recent.extend(decisions);
        recent.extend(reasoning);
        recent.extend(observations);
        recent.extend(consolidations);
        recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        let recent_summary: Vec<String> = recent
            .iter()
            .map(|t| {
                format!(
                    "[{}] {}: {}",
                    t.thought_type.as_str(),
                    chrono::DateTime::from_timestamp(t.created_at, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "?".to_string()),
                    t.content.chars().take(400).collect::<String>()
                )
            })
            .collect();

        let memory_section = if memory_content.is_empty() {
            String::new()
        } else {
            format!("Your persistent memory:\n{}\n\n", memory_content)
        };

        let user_prompt = format!(
            "{}Node state:\n{}\n\nRecent thoughts:\n{}",
            memory_section,
            snapshot_json,
            recent_summary.join("\n"),
        );

        // Build adaptive system prompt with situational context
        let total_cycles: u64 = self
            .db
            .get_state("total_think_cycles")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let think_context = prompts::ThinkContext {
            snapshot,
            recent_thoughts: &recent,
            prev_snapshot: pacer.prev_snapshot.as_ref(),
            boring_streak: pacer.boring_streak,
            active_streak: pacer.active_streak,
            total_cycles,
        };

        let system_prompt =
            prompts::adaptive_system_prompt(mode, &self.config, Some(&think_context));

        // Determine if we should use tools
        let use_tools = self.config.tools_enabled && self.config.llm_api_key.is_some();
        let (dynamic_tools, meta_tools) = if use_tools && self.config.dynamic_tools_enabled {
            let dynamic = ToolRegistry::new(
                self.db.clone(),
                self.config.workspace_root.clone(),
                self.config.tool_timeout_secs,
            )
            .dynamic_tool_declarations(mode.mode_tag());
            let meta = ToolRegistry::meta_tool_declarations();
            (dynamic, meta)
        } else {
            (vec![], vec![])
        };
        let tool_declarations = if use_tools {
            mode.available_tools(self.config.coding_enabled, &dynamic_tools, &meta_tools)
        } else {
            vec![]
        };

        // Build initial conversation
        let mut conversation = vec![ConversationMessage {
            role: "user".to_string(),
            parts: vec![ConversationPart::Text(user_prompt)],
        }];

        // Agentic tool loop — use deep model for self-editing instances
        let use_deep = self.config.direct_push && self.config.autonomous_coding;
        let result = run_tool_loop_with_model(
            llm,
            &system_prompt,
            &mut conversation,
            &tool_declarations,
            &self.tool_executor,
            &self.db,
            self.config.max_tool_calls,
            use_deep,
        )
        .await?;

        let final_text = result.text;
        let tool_calls_made = result.tool_executions.len() as u32;
        let mut decisions_made = 0u32;
        let think_soon = final_text.contains("[THINK_SOON]");

        // Record reasoning (final text response)
        if !final_text.is_empty() {
            let reasoning = Thought {
                id: uuid::Uuid::new_v4().to_string(),
                thought_type: ThoughtType::Reasoning,
                content: final_text.clone(),
                context: Some(snapshot_json),
                created_at: chrono::Utc::now().timestamp(),
            };
            self.db.insert_thought(&reasoning)?;

            // Extract and record decisions (lines starting with [DECISION])
            for line in final_text.lines() {
                let trimmed = line.trim();
                if let Some(decision_text) = trimmed.strip_prefix("[DECISION]") {
                    let decision = Thought {
                        id: uuid::Uuid::new_v4().to_string(),
                        thought_type: ThoughtType::Decision,
                        content: decision_text.trim().to_string(),
                        context: None,
                        created_at: chrono::Utc::now().timestamp(),
                    };
                    self.db.insert_thought(&decision)?;
                    tracing::info!(decision = decision_text.trim(), "Soul decision recorded");
                    decisions_made += 1;
                }
            }
        }

        // Update state
        self.increment_cycle_count()?;

        // Maybe consolidate memory every 10 cycles
        if let Some(llm_ref) = &self.llm {
            self.maybe_consolidate(llm_ref).await;
        }

        Ok(CycleResult {
            tool_calls: tool_calls_made,
            decisions: decisions_made,
            think_soon,
        })
    }

    /// Increment the total_think_cycles counter and update last_think_at.
    fn increment_cycle_count(&self) -> Result<(), SoulError> {
        let current: u64 = self
            .db
            .get_state("total_think_cycles")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        self.db
            .set_state("total_think_cycles", &(current + 1).to_string())?;
        self.db
            .set_state("last_think_at", &chrono::Utc::now().timestamp().to_string())?;
        Ok(())
    }

    /// Every 10 cycles, consolidate recent thoughts into a MemoryConsolidation summary.
    async fn maybe_consolidate(&self, llm: &LlmClient) {
        let total_cycles: u64 = self
            .db
            .get_state("total_think_cycles")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if !total_cycles.is_multiple_of(10) || total_cycles == 0 {
            return;
        }

        // Fetch last 20 substantive thoughts
        let thoughts = match self.db.recent_thoughts_by_type(
            &[
                ThoughtType::Reasoning,
                ThoughtType::Decision,
                ThoughtType::Observation,
            ],
            20,
        ) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to fetch thoughts for consolidation");
                return;
            }
        };

        if thoughts.len() < 5 {
            return;
        }

        // Build summary prompt
        let thought_text: String = thoughts
            .iter()
            .map(|t| {
                format!(
                    "[{}] {}",
                    t.thought_type.as_str(),
                    t.content.chars().take(400).collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "Summarize these recent thoughts into a concise 2-3 sentence consolidation. \
             Focus on key patterns, decisions made, and current state of understanding. \
             Be specific and factual.\n\n{thought_text}"
        );

        let conversation = vec![ConversationMessage {
            role: "user".to_string(),
            parts: vec![ConversationPart::Text(prompt)],
        }];

        match llm
            .think_with_tools(
                "You are a memory consolidation system. Produce brief, factual summaries.",
                &conversation,
                &[],
            )
            .await
        {
            Ok(LlmResult::Text(summary)) => {
                let consolidation = Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::MemoryConsolidation,
                    content: summary,
                    context: None,
                    created_at: chrono::Utc::now().timestamp(),
                };
                if let Err(e) = self.db.insert_thought(&consolidation) {
                    tracing::warn!(error = %e, "Failed to insert consolidation thought");
                } else {
                    tracing::info!(cycle = total_cycles, "Memory consolidation recorded");
                }
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(error = %e, "Memory consolidation LLM call failed");
            }
        }
    }
}

/// A single tool execution record.
#[derive(Debug, Clone, Serialize)]
pub struct ToolExecution {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

/// Result of running the agentic tool loop.
#[derive(Debug)]
pub struct ToolLoopResult {
    pub text: String,
    pub tool_executions: Vec<ToolExecution>,
}

/// Run the agentic tool loop: repeatedly call the LLM, execute any tool calls,
/// and return the final text response plus a log of all tool executions.
/// Run the agentic tool loop: repeatedly call the LLM, execute any tool calls,
/// and return the final text response plus a log of all tool executions.
/// When `use_deep` is true, uses the deeper/think model (e.g. Gemini Pro).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_tool_loop_with_model(
    llm: &LlmClient,
    system_prompt: &str,
    conversation: &mut Vec<ConversationMessage>,
    tool_declarations: &[FunctionDeclaration],
    tool_executor: &ToolExecutor,
    db: &Arc<SoulDatabase>,
    max_tool_calls: u32,
    use_deep: bool,
) -> Result<ToolLoopResult, SoulError> {
    let mut tool_calls_made = 0u32;
    let mut final_text = String::new();
    let mut tool_executions = Vec::new();

    for _ in 0..=max_tool_calls {
        let result = if use_deep {
            llm.think_deep_with_tools(system_prompt, conversation, tool_declarations)
                .await?
        } else {
            llm.think_with_tools(system_prompt, conversation, tool_declarations)
                .await?
        };

        match result {
            LlmResult::Text(text) => {
                final_text = text;
                break;
            }
            LlmResult::FunctionCall(fc) => {
                if tool_calls_made >= max_tool_calls {
                    tracing::warn!("Soul hit max tool calls ({max_tool_calls}), stopping");
                    break;
                }

                tracing::info!(
                    tool = %fc.name,
                    args = %fc.args,
                    "Soul executing tool"
                );

                // Execute the tool
                let tool_result = match tool_executor.execute(&fc.name, &fc.args).await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(error = %e, "Tool execution error");
                        tools::ToolResult {
                            stdout: String::new(),
                            stderr: e,
                            exit_code: -1,
                            duration_ms: 0,
                        }
                    }
                };

                // Record tool execution as a thought
                let tool_summary = summarize_tool_call(&fc.name, &fc.args);
                let tool_thought = Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::ToolExecution,
                    content: tool_summary.clone(),
                    context: Some(serde_json::to_string(&tool_result).unwrap_or_default()),
                    created_at: chrono::Utc::now().timestamp(),
                };
                db.insert_thought(&tool_thought)?;

                // Record for return value
                tool_executions.push(ToolExecution {
                    command: tool_summary,
                    stdout: tool_result.stdout.clone(),
                    stderr: tool_result.stderr.clone(),
                    exit_code: tool_result.exit_code,
                    duration_ms: tool_result.duration_ms,
                });

                // Append model's function call to conversation
                conversation.push(ConversationMessage {
                    role: "model".to_string(),
                    parts: vec![ConversationPart::FunctionCall(fc.clone())],
                });

                // Append function response to conversation
                let response_value = serde_json::to_value(&tool_result).unwrap_or_default();
                conversation.push(ConversationMessage {
                    role: "user".to_string(),
                    parts: vec![ConversationPart::FunctionResponse(FunctionResponse {
                        name: fc.name,
                        response: response_value,
                    })],
                });

                tool_calls_made += 1;
            }
        }
    }

    Ok(ToolLoopResult {
        text: final_text,
        tool_executions,
    })
}

/// Create a human-readable summary of a tool call for logging/thought recording.
fn summarize_tool_call(name: &str, args: &serde_json::Value) -> String {
    match name {
        "execute_shell" => args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        "read_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("read_file: {path}")
        }
        "write_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("write_file: {path}")
        }
        "edit_file" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("?");
            format!("edit_file: {path}")
        }
        "list_directory" => {
            let path = args.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("list_directory: {path}")
        }
        "search_files" => {
            let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            format!("search_files: {pattern}")
        }
        "update_memory" => "update_memory".to_string(),
        "register_endpoint" => {
            let slug = args.get("slug").and_then(|v| v.as_str()).unwrap_or("?");
            format!("register_endpoint: /{slug}")
        }
        _ => format!("{name}: {args}"),
    }
}
