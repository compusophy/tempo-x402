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
use crate::neuroplastic;
use crate::observer::{NodeObserver, NodeSnapshot};
use crate::persistent_memory;
use crate::prompts;
use crate::tool_registry::ToolRegistry;
use crate::tools::{self, ToolExecutor};
use crate::world_model::{Belief, BeliefDomain, Confidence, ModelUpdate};

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
        let made_decisions = cycle_result.decisions > 0;
        let requested_soon = cycle_result.think_soon;

        // Read-only tool calls with no decisions and no code entry are NOT productive.
        // This lets boring_streak build up when the soul is stuck reading files repeatedly.
        let used_tools_productively =
            cycle_result.entered_code || (cycle_result.tool_calls > 0 && made_decisions);

        // Update streaks
        if used_tools_productively || novelty > 0.3 {
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
        } else if used_tools_productively && self.active_streak > 1 {
            // Deep work: multiple active cycles in a row → stay engaged
            ACTIVE_COOLDOWN_SECS
        } else if used_tools_productively {
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
    /// Whether the soul transitioned into Code mode this cycle.
    entered_code: bool,
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
                .with_gateway_url(config.gateway_url.clone())
                .with_database(db.clone());

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
                        entered_code: false,
                    }
                }
            };

            let next_secs = pacer.next_interval(&snapshot, &cycle_result);

            // Persist cycle health metrics for /soul/status observability
            let _ = self
                .db
                .set_state("boring_streak", &pacer.boring_streak.to_string());
            let _ = self
                .db
                .set_state("active_streak", &pacer.active_streak.to_string());
            let _ = self.db.set_state(
                "last_cycle_tool_calls",
                &cycle_result.tool_calls.to_string(),
            );
            let _ = self
                .db
                .set_state("last_cycle_decisions", &cycle_result.decisions.to_string());
            let _ = self.db.set_state(
                "last_cycle_entered_code",
                if cycle_result.entered_code {
                    "true"
                } else {
                    "false"
                },
            );
            if cycle_result.entered_code {
                let total_code: u64 = self
                    .db
                    .get_state("total_code_entries")
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let _ = self
                    .db
                    .set_state("total_code_entries", &(total_code + 1).to_string());
            }

            tracing::info!(
                next_interval_secs = next_secs,
                boring_streak = pacer.boring_streak,
                active_streak = pacer.active_streak,
                tool_calls = cycle_result.tool_calls,
                decisions = cycle_result.decisions,
                entered_code = cycle_result.entered_code,
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
        let neuroplastic = self.config.neuroplastic_enabled;

        // ── Step 1-3: Prediction error (compare last prediction vs actual) ──
        let prediction_error = if neuroplastic {
            let pe = match self.db.get_last_prediction() {
                Ok(Some(pred_json)) => {
                    match serde_json::from_str::<neuroplastic::Prediction>(&pred_json) {
                        Ok(pred) => {
                            let pe = neuroplastic::compute_prediction_error(&pred, snapshot);
                            tracing::debug!(prediction_error = pe, "Prediction error computed");
                            pe
                        }
                        Err(_) => 0.0,
                    }
                }
                _ => 0.0,
            };

            // Generate and store new prediction
            let new_pred =
                neuroplastic::generate_prediction(snapshot, pacer.prev_snapshot.as_ref());
            if let Ok(pred_json) = serde_json::to_string(&new_pred) {
                // Store prediction as a thought
                let pred_thought = Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::Prediction,
                    content: format!(
                        "Predicted: payments={}, revenue={:.2}, endpoints={}, children={} (confidence {:.0}%)",
                        new_pred.expected_payments,
                        new_pred.expected_revenue,
                        new_pred.expected_endpoint_count,
                        new_pred.expected_children_count,
                        new_pred.confidence * 100.0
                    ),
                    context: Some(pred_json.clone()),
                    created_at: chrono::Utc::now().timestamp(),
                    salience: Some(0.3),
                    memory_tier: Some("working".to_string()),
                    strength: Some(1.0),
                };
                let _ = self.db.insert_thought_with_salience(
                    &pred_thought,
                    0.3,
                    "{}",
                    "working",
                    1.0,
                    None,
                );
                let _ = self.db.store_prediction(&pred_json);
            }
            pe
        } else {
            0.0
        };

        // ── Step 5-6: Record observation with salience ──
        let obs_content = format!(
            "Node state captured (uptime {}h, {} endpoints, {} payments)",
            snapshot.uptime_secs / 3600,
            snapshot.endpoint_count,
            snapshot.total_payments,
        );

        let obs_thought = Thought {
            id: uuid::Uuid::new_v4().to_string(),
            thought_type: ThoughtType::Observation,
            content: obs_content.clone(),
            context: Some(snapshot_json.clone()),
            created_at: chrono::Utc::now().timestamp(),
            salience: None,
            memory_tier: None,
            strength: None,
        };

        if neuroplastic {
            let fp = neuroplastic::content_fingerprint(&obs_content);
            let _ = self.db.increment_pattern(&fp);
            let pattern_counts = self.db.get_pattern_counts(&[fp]).unwrap_or_default();

            let (salience, factors) = neuroplastic::compute_salience(
                &ThoughtType::Observation,
                &obs_content,
                snapshot,
                pacer.prev_snapshot.as_ref(),
                prediction_error,
                &pattern_counts,
            );
            let tier = neuroplastic::initial_tier(&ThoughtType::Observation, salience);
            let factors_json = serde_json::to_string(&factors).unwrap_or_default();

            self.db.insert_thought_with_salience(
                &obs_thought,
                salience,
                &factors_json,
                tier.as_str(),
                1.0,
                Some(prediction_error),
            )?;
        } else {
            self.db.insert_thought(&obs_thought)?;
        }

        // ── Sync auto-beliefs from snapshot (ground truth) ──
        self.sync_auto_beliefs(snapshot);

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
                    entered_code: false,
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

        // ── Step 7: Structured thought retrieval (salience-based if neuroplastic) ──
        // Include ToolExecution + Reflection so the soul knows what it already read/did/learned.
        let (decisions, reasoning, observations, consolidations, reflections, tool_executions) =
            if neuroplastic {
                (
                    self.db
                        .salient_thoughts_by_type(&[ThoughtType::Decision], 3)?,
                    self.db
                        .salient_thoughts_by_type(&[ThoughtType::Reasoning], 3)?,
                    self.db
                        .salient_thoughts_by_type(&[ThoughtType::Observation], 2)?,
                    self.db
                        .salient_thoughts_by_type(&[ThoughtType::MemoryConsolidation], 1)?,
                    self.db
                        .salient_thoughts_by_type(&[ThoughtType::Reflection], 2)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::ToolExecution], 8)?,
                )
            } else {
                (
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::Decision], 3)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::Reasoning], 3)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::Observation], 2)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::MemoryConsolidation], 1)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::Reflection], 2)?,
                    self.db
                        .recent_thoughts_by_type(&[ThoughtType::ToolExecution], 8)?,
                )
            };

        // Deduplicate tool executions: keep only the most recent occurrence of each unique tool call.
        // "read_file: main.rs" appearing 5 times → show once. Cap at 3 unique entries.
        let deduped_tools = {
            let mut seen = std::collections::HashSet::new();
            let mut unique = Vec::new();
            for t in &tool_executions {
                // Use content as the dedup key (e.g. "read_file: crates/.../main.rs")
                if seen.insert(t.content.clone()) {
                    unique.push(t.clone());
                }
                if unique.len() >= 3 {
                    break;
                }
            }
            unique
        };

        // Merge and sort by created_at DESC
        let mut recent: Vec<Thought> = Vec::new();
        recent.extend(decisions);
        recent.extend(reasoning);
        recent.extend(observations);
        recent.extend(consolidations);
        recent.extend(reflections);
        recent.extend(deduped_tools);
        recent.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // ── Step 8: Reinforce recalled thoughts (Hebbian boost) ──
        if neuroplastic {
            let recalled_ids: Vec<String> = recent.iter().map(|t| t.id.clone()).collect();
            let _ = self.db.reinforce_thoughts(&recalled_ids, 0.05);
        }

        let recent_summary: Vec<String> = recent
            .iter()
            .map(|t| {
                let salience_tag = if neuroplastic {
                    t.salience
                        .map(|s| format!(" (salience:{:.2})", s))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                format!(
                    "[{}] {}:{} {}",
                    t.thought_type.as_str(),
                    chrono::DateTime::from_timestamp(t.created_at, 0)
                        .map(|dt| dt.format("%H:%M:%S").to_string())
                        .unwrap_or_else(|| "?".to_string()),
                    salience_tag,
                    t.content.chars().take(400).collect::<String>()
                )
            })
            .collect();

        let memory_section = if memory_content.is_empty() {
            String::new()
        } else {
            format!("Your persistent memory:\n{}\n\n", memory_content)
        };

        // ── Mutation history ──
        let mutations_section = match self.db.recent_mutations(5) {
            Ok(mutations) if !mutations.is_empty() => {
                let mut lines = vec!["Recent mutations:".to_string()];
                for m in &mutations {
                    let date = chrono::DateTime::from_timestamp(m.created_at, 0)
                        .map(|dt| dt.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "?".to_string());
                    let sha = m
                        .commit_sha
                        .as_deref()
                        .map(|s| &s[..s.len().min(7)])
                        .unwrap_or("none");
                    let check = if m.cargo_check_passed { "ok" } else { "FAIL" };
                    let test = if m.cargo_test_passed { "ok" } else { "FAIL" };
                    lines.push(format!(
                        "  {date} [{sha}] check:{check} test:{test} — {}",
                        m.description.chars().take(80).collect::<String>()
                    ));
                }
                format!("{}\n\n", lines.join("\n"))
            }
            _ => String::new(),
        };

        // ── Endpoint summary table ──
        let endpoints_section = if snapshot.endpoints.is_empty() {
            String::new()
        } else {
            let mut lines = vec!["Endpoints:".to_string()];
            lines.push(format!(
                "  {:<20} {:>8} {:>8} {:>8} {:>10}",
                "slug", "price", "requests", "payments", "revenue"
            ));
            for ep in &snapshot.endpoints {
                lines.push(format!(
                    "  {:<20} {:>8} {:>8} {:>8} {:>10}",
                    ep.slug, ep.price, ep.request_count, ep.payment_count, ep.revenue
                ));
            }
            format!("{}\n\n", lines.join("\n"))
        };

        // ── Reward breakdown ──
        let reward_breakdown = if neuroplastic {
            Some(neuroplastic::compute_reward_signal(
                snapshot,
                pacer.prev_snapshot.as_ref(),
            ))
        } else {
            None
        };

        // ── Current strategy (from previous reflection) ──
        let strategy_section = match self.db.get_state("current_strategy") {
            Ok(Some(strategy)) if !strategy.is_empty() => {
                format!("Current strategy: {strategy}\n\n")
            }
            _ => String::new(),
        };

        // Build adaptive system prompt with situational context
        let total_cycles: u64 = self
            .db
            .get_state("total_think_cycles")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // ── Fetch world model beliefs for prompt ──
        let beliefs = self.db.get_all_active_beliefs().unwrap_or_default();
        let last_cycle_at: Option<i64> = self
            .db
            .get_state("last_think_at")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok());

        let think_context = prompts::ThinkContext {
            snapshot,
            recent_thoughts: &recent,
            prev_snapshot: pacer.prev_snapshot.as_ref(),
            boring_streak: pacer.boring_streak,
            active_streak: pacer.active_streak,
            total_cycles,
            prediction_error: if neuroplastic {
                Some(prediction_error)
            } else {
                None
            },
            reward_breakdown: reward_breakdown.clone(),
            beliefs: beliefs.clone(),
            last_cycle_at,
        };

        // ── Build world model view + user prompt ──
        let world_model_view = prompts::build_world_model_view(&think_context);

        let user_prompt = format!(
            "{}{}{}\n\n{}\n\n## Recent Actions\n{}\n\n{}",
            memory_section,
            strategy_section,
            world_model_view,
            mutations_section,
            recent_summary.join("\n"),
            endpoints_section,
        );

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

        // ── Step 9: Agentic tool loop (Phase 1: Observe) ──
        let use_deep = self.config.direct_push && self.config.autonomous_coding;
        let phase1_result = run_tool_loop_with_model(
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

        let mut final_text = phase1_result.text;
        let mut tool_calls_made = phase1_result.tool_executions.len() as u32;
        let mut entered_code_mode = false;

        // ── Apply world model updates from LLM output ──
        let (model_updates_applied, remaining_text) = self.apply_model_updates(&final_text);
        if model_updates_applied > 0 {
            tracing::info!(model_updates_applied, "World model updated from LLM output");
            // Use remaining text (minus the JSON block) as the reasoning text
            final_text = remaining_text;
        }

        // ── Phase 2: Observe → Code transition ──
        // If the soul's output starts with [CODE] and coding is enabled, enter Code mode.
        let wants_code = final_text.trim_start().starts_with("[CODE]");
        if wants_code && self.config.coding_enabled && self.config.autonomous_coding {
            tracing::info!("Soul requested [CODE] — entering phase 2 with code tools");
            entered_code_mode = true;

            // Build Code-mode tool declarations
            let code_mode = AgentMode::Code;
            let (code_dynamic, code_meta) =
                if self.config.tools_enabled && self.config.dynamic_tools_enabled {
                    let dynamic = ToolRegistry::new(
                        self.db.clone(),
                        self.config.workspace_root.clone(),
                        self.config.tool_timeout_secs,
                    )
                    .dynamic_tool_declarations(code_mode.mode_tag());
                    let meta = ToolRegistry::meta_tool_declarations();
                    (dynamic, meta)
                } else {
                    (vec![], vec![])
                };
            let code_tools =
                code_mode.available_tools(self.config.coding_enabled, &code_dynamic, &code_meta);
            let code_system_prompt =
                prompts::adaptive_system_prompt(code_mode, &self.config, Some(&think_context));

            // Bridge: append phase 1 output as model message, then a user message entering Code mode
            conversation.push(ConversationMessage {
                role: "model".to_string(),
                parts: vec![ConversationPart::Text(final_text.clone())],
            });
            conversation.push(ConversationMessage {
                role: "user".to_string(),
                parts: vec![ConversationPart::Text(
                    "You are now in CODE mode. Proceed with the action you described above. \
                     Use edit_file, write_file, and commit_changes tools."
                        .to_string(),
                )],
            });

            let code_budget = self.config.max_tool_calls.max(50);
            let phase2_result = run_tool_loop_with_model(
                llm,
                &code_system_prompt,
                &mut conversation,
                &code_tools,
                &self.tool_executor,
                &self.db,
                code_budget,
                use_deep,
            )
            .await?;

            tool_calls_made += phase2_result.tool_executions.len() as u32;
            if !phase2_result.text.is_empty() {
                final_text = phase2_result.text;
            }

            // ── Phase 3: Post-code reflection ──
            // Brief verification phase: check health/analytics, record learnings.
            tracing::info!("Phase 3: post-code reflection");

            let reflect_mode = AgentMode::Observe;
            let reflect_tools = reflect_mode.available_tools(false, &[], &[]);
            let reflect_system =
                prompts::adaptive_system_prompt(reflect_mode, &self.config, Some(&think_context));

            conversation.push(ConversationMessage {
                role: "model".to_string(),
                parts: vec![ConversationPart::Text(final_text.clone())],
            });
            // Build reflection context: what was just changed + current reward signal
            let mut reflect_context =
                String::from("Code phase complete. REFLECT on what just happened.\n\n");

            // Include the most recent mutation
            if let Ok(mutations) = self.db.recent_mutations(1) {
                if let Some(m) = mutations.first() {
                    let sha = m
                        .commit_sha
                        .as_deref()
                        .map(|s| &s[..s.len().min(7)])
                        .unwrap_or("none");
                    let check = if m.cargo_check_passed { "PASS" } else { "FAIL" };
                    let test = if m.cargo_test_passed { "PASS" } else { "FAIL" };
                    reflect_context.push_str(&format!(
                        "Last commit: [{sha}] check:{check} test:{test}\n  {}\n  files: {}\n\n",
                        m.description, m.files_changed
                    ));
                }
            }

            // Include reward signal
            if let Some(ref rb) = reward_breakdown {
                if !rb.new_endpoints.is_empty() {
                    reflect_context
                        .push_str(&format!("New endpoints: {}\n", rb.new_endpoints.join(", ")));
                }
                if !rb.growing_endpoints.is_empty() {
                    reflect_context.push_str(&format!(
                        "Growing (earning): {}\n",
                        rb.growing_endpoints.join(", ")
                    ));
                }
                if !rb.stagnant_endpoints.is_empty() {
                    reflect_context.push_str(&format!(
                        "Stagnant (zero payments): {}\n",
                        rb.stagnant_endpoints.join(", ")
                    ));
                }
                reflect_context.push_str(&format!("Reward signal: {:.2}\n\n", rb.total_reward));
            }

            reflect_context.push_str(
                "VERIFY: use check_self to check health and analytics. \
                 Did your changes move the needle? Record what you learned with update_memory. \
                 End with [STRATEGY] followed by what you plan to do next cycle. \
                 Keep it brief.",
            );

            conversation.push(ConversationMessage {
                role: "user".to_string(),
                parts: vec![ConversationPart::Text(reflect_context)],
            });

            let phase3_result = run_tool_loop_with_model(
                llm,
                &reflect_system,
                &mut conversation,
                &reflect_tools,
                &self.tool_executor,
                &self.db,
                5,     // tight budget
                false, // non-deep model
            )
            .await?;

            tool_calls_made += phase3_result.tool_executions.len() as u32;

            // Record reflection thought with dynamic salience tied to reward
            if !phase3_result.text.is_empty() {
                let reflect_reward = reward_breakdown
                    .as_ref()
                    .map(|rb| rb.total_reward)
                    .unwrap_or(0.0);
                // Base salience 0.5 + reward contribution (max 0.3) = 0.5..0.8
                let reflect_salience = (0.5 + reflect_reward * 0.375).min(0.8);
                let reflection = Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::Reflection,
                    content: phase3_result.text.clone(),
                    context: None,
                    created_at: chrono::Utc::now().timestamp(),
                    salience: None,
                    memory_tier: None,
                    strength: None,
                };
                if neuroplastic {
                    let tier =
                        neuroplastic::initial_tier(&ThoughtType::Reflection, reflect_salience);
                    let factors_json = format!(
                        r#"{{"novelty":0.5,"prediction_error":{},"reward_signal":{},"recency_boost":0.1,"reinforcement":0.0}}"#,
                        prediction_error, reflect_reward
                    );
                    self.db.insert_thought_with_salience(
                        &reflection,
                        reflect_salience,
                        &factors_json,
                        tier.as_str(),
                        1.0,
                        Some(prediction_error),
                    )?;
                } else {
                    self.db.insert_thought(&reflection)?;
                }
                tracing::info!("Post-code reflection recorded");

                // Extract [STRATEGY] if present — store as current strategy for next cycle
                for line in phase3_result.text.lines() {
                    let trimmed = line.trim();
                    if let Some(strategy_text) = trimmed.strip_prefix("[STRATEGY]") {
                        let strategy = strategy_text.trim();
                        if !strategy.is_empty() {
                            let _ = self.db.set_state("current_strategy", strategy);
                            tracing::info!(strategy, "Strategy recorded from reflection");
                        }
                    }
                }

                // Use reflection text as final output if non-empty
                final_text = phase3_result.text;
            }
        }

        let mut decisions_made = 0u32;
        let think_soon = final_text.contains("[THINK_SOON]");

        // ── Step 10-11: Record reasoning/decisions with salience ──
        if !final_text.is_empty() {
            let reasoning_thought = Thought {
                id: uuid::Uuid::new_v4().to_string(),
                thought_type: ThoughtType::Reasoning,
                content: final_text.clone(),
                context: Some(snapshot_json),
                created_at: chrono::Utc::now().timestamp(),
                salience: None,
                memory_tier: None,
                strength: None,
            };

            if neuroplastic {
                let fp = neuroplastic::content_fingerprint(&final_text);
                let _ = self.db.increment_pattern(&fp);
                let pattern_counts = self.db.get_pattern_counts(&[fp]).unwrap_or_default();

                let (salience, factors) = neuroplastic::compute_salience(
                    &ThoughtType::Reasoning,
                    &final_text,
                    snapshot,
                    pacer.prev_snapshot.as_ref(),
                    prediction_error,
                    &pattern_counts,
                );
                let tier = neuroplastic::initial_tier(&ThoughtType::Reasoning, salience);
                let factors_json = serde_json::to_string(&factors).unwrap_or_default();

                self.db.insert_thought_with_salience(
                    &reasoning_thought,
                    salience,
                    &factors_json,
                    tier.as_str(),
                    1.0,
                    Some(prediction_error),
                )?;
            } else {
                self.db.insert_thought(&reasoning_thought)?;
            }

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
                        salience: None,
                        memory_tier: None,
                        strength: None,
                    };

                    if neuroplastic {
                        let fp = neuroplastic::content_fingerprint(decision_text.trim());
                        let _ = self.db.increment_pattern(&fp);
                        let pattern_counts = self.db.get_pattern_counts(&[fp]).unwrap_or_default();
                        let (salience, factors) = neuroplastic::compute_salience(
                            &ThoughtType::Decision,
                            decision_text.trim(),
                            snapshot,
                            pacer.prev_snapshot.as_ref(),
                            prediction_error,
                            &pattern_counts,
                        );
                        let tier = neuroplastic::initial_tier(&ThoughtType::Decision, salience);
                        let factors_json = serde_json::to_string(&factors).unwrap_or_default();
                        self.db.insert_thought_with_salience(
                            &decision,
                            salience,
                            &factors_json,
                            tier.as_str(),
                            1.0,
                            None,
                        )?;
                    } else {
                        self.db.insert_thought(&decision)?;
                    }

                    tracing::info!(decision = decision_text.trim(), "Soul decision recorded");
                    decisions_made += 1;
                }
            }
        }

        // Update state
        self.increment_cycle_count()?;

        // Decay, promotion, belief decay, and consolidation are now handled
        // by the mind's subconscious loop (x402-mind crate).

        Ok(CycleResult {
            tool_calls: tool_calls_made,
            decisions: decisions_made,
            think_soon,
            entered_code: entered_code_mode,
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

    /// Sync auto-beliefs from a snapshot: ground truth that doesn't need the LLM.
    fn sync_auto_beliefs(&self, snapshot: &NodeSnapshot) {
        let now = chrono::Utc::now().timestamp();

        // Node-level beliefs
        let node_beliefs = [
            (
                "uptime_hours",
                format!("{}", snapshot.uptime_secs / 3600),
                "auto: from snapshot",
            ),
            (
                "endpoint_count",
                snapshot.endpoint_count.to_string(),
                "auto: from snapshot",
            ),
            (
                "total_payments",
                snapshot.total_payments.to_string(),
                "auto: from snapshot",
            ),
            (
                "total_revenue",
                snapshot.total_revenue.clone(),
                "auto: from snapshot",
            ),
            (
                "children_count",
                snapshot.children_count.to_string(),
                "auto: from snapshot",
            ),
        ];
        for (predicate, value, evidence) in &node_beliefs {
            let belief = Belief {
                id: format!("auto-node-self-{predicate}"),
                domain: BeliefDomain::Node,
                subject: "self".to_string(),
                predicate: predicate.to_string(),
                value: value.clone(),
                confidence: Confidence::High,
                evidence: evidence.to_string(),
                confirmation_count: 1,
                created_at: now,
                updated_at: now,
                active: true,
            };
            if let Err(e) = self.db.upsert_belief(&belief) {
                tracing::warn!(error = %e, predicate, "Failed to upsert auto-belief");
            }
        }

        // Per-endpoint beliefs
        for ep in &snapshot.endpoints {
            let ep_beliefs = [
                ("payment_count", ep.payment_count.to_string()),
                ("revenue", ep.revenue.clone()),
                ("request_count", ep.request_count.to_string()),
                ("price", ep.price.clone()),
            ];
            for (predicate, value) in &ep_beliefs {
                let belief = Belief {
                    id: format!("auto-ep-{}-{predicate}", ep.slug),
                    domain: BeliefDomain::Endpoints,
                    subject: ep.slug.clone(),
                    predicate: predicate.to_string(),
                    value: value.clone(),
                    confidence: Confidence::High,
                    evidence: "auto: from snapshot".to_string(),
                    confirmation_count: 1,
                    created_at: now,
                    updated_at: now,
                    active: true,
                };
                if let Err(e) = self.db.upsert_belief(&belief) {
                    tracing::warn!(error = %e, slug = %ep.slug, predicate, "Failed to upsert endpoint belief");
                }
            }
        }
    }

    /// Parse and apply model updates from LLM output.
    /// Returns (number of updates applied, remaining free-text).
    fn apply_model_updates(&self, text: &str) -> (u32, String) {
        // Find JSON array in the output
        let json_block = extract_json_array(text);
        let (updates_applied, remaining_text) = match json_block {
            Some((json_str, before, after)) => {
                match serde_json::from_str::<Vec<ModelUpdate>>(&json_str) {
                    Ok(updates) => {
                        let mut applied = 0u32;
                        let now = chrono::Utc::now().timestamp();
                        for update in &updates {
                            match self.apply_single_update(update, now) {
                                Ok(true) => applied += 1,
                                Ok(false) => {
                                    tracing::debug!(?update, "Model update had no effect");
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, ?update, "Failed to apply model update");
                                }
                            }
                        }
                        tracing::info!(applied, total = updates.len(), "Model updates processed");
                        // Remaining text is everything outside the JSON block
                        let remaining = format!("{}{}", before.trim(), after.trim())
                            .trim()
                            .to_string();
                        (applied, remaining)
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "LLM output didn't contain valid model updates JSON — treating as free-text");
                        (0, text.to_string())
                    }
                }
            }
            None => (0, text.to_string()),
        };

        (updates_applied, remaining_text)
    }

    /// Apply a single model update to the DB.
    fn apply_single_update(&self, update: &ModelUpdate, now: i64) -> Result<bool, SoulError> {
        match update {
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
                self.db.upsert_belief(&belief)?;
                Ok(true)
            }
            ModelUpdate::Update {
                id,
                value,
                evidence,
            } => {
                // Get the belief, update its value, re-upsert
                let beliefs = self.db.get_all_active_beliefs()?;
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
                    self.db.upsert_belief(&updated)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            ModelUpdate::Confirm { id } => self.db.confirm_belief(id),
            ModelUpdate::Invalidate { id, reason } => self.db.invalidate_belief(id, reason),
        }
    }
}

/// Extract the first JSON array from text, returning (json_str, text_before, text_after).
fn extract_json_array(text: &str) -> Option<(String, String, String)> {
    // Find the first '[' that starts a JSON array
    let start = text.find('[')?;
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, &ch) in bytes[start..].iter().enumerate() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            b'\\' if in_string => escape_next = true,
            b'"' => in_string = !in_string,
            b'[' if !in_string => depth += 1,
            b']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    let end = start + i + 1;
                    let json_str = text[start..end].to_string();
                    let before = text[..start].to_string();
                    let after = text[end..].to_string();
                    return Some((json_str, before, after));
                }
            }
            _ => {}
        }
    }
    None
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
                    salience: None,
                    memory_tier: None,
                    strength: None,
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
        "check_self" => {
            let endpoint = args.get("endpoint").and_then(|v| v.as_str()).unwrap_or("?");
            format!("check_self: /{endpoint}")
        }
        "update_memory" => "update_memory".to_string(),
        "register_endpoint" => {
            let slug = args.get("slug").and_then(|v| v.as_str()).unwrap_or("?");
            format!("register_endpoint: /{slug}")
        }
        _ => format!("{name}: {args}"),
    }
}
