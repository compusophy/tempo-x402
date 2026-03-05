//! Plan-driven thinking loop: deterministic step execution replaces prompt-and-pray.
//!
//! Each cycle: observe → get/create plan → execute one step → advance → sleep.
//! Most steps are mechanical (no LLM). LLM is only called for planning,
//! code generation, and reflection.

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
use crate::neuroplastic;
use crate::observer::{NodeObserver, NodeSnapshot};
use crate::plan::{Plan, PlanExecutor, PlanStatus, PlanStep, StepResult};
use crate::prompts;
use crate::tool_registry::ToolRegistry;
use crate::tools::{self, ToolExecutor};
use crate::world_model::{Belief, BeliefDomain, Confidence, Goal, ModelUpdate};

/// Simplified adaptive pacing for plan-driven execution.
struct AdaptivePacer {
    prev_snapshot: Option<NodeSnapshot>,
}

impl AdaptivePacer {
    fn new() -> Self {
        Self {
            prev_snapshot: None,
        }
    }

    /// Determine next sleep interval based on what happened.
    fn next_interval(&mut self, snapshot: &NodeSnapshot, step_type: StepType) -> u64 {
        self.prev_snapshot = Some(snapshot.clone());
        match step_type {
            StepType::Mechanical => 30,     // fast, keep making progress
            StepType::Llm => 120,           // LLM step, moderate pause
            StepType::PlanCompleted => 300, // time to create next plan
            StepType::NoGoals => 600,       // idle
            StepType::Observe => 60,        // quick observation only
        }
    }
}

/// What kind of step was executed (for pacing).
enum StepType {
    Mechanical,
    Llm,
    PlanCompleted,
    NoGoals,
    Observe,
}

/// Result of a single think cycle, used by the main loop.
struct CycleResult {
    step_type: StepType,
    /// Whether the soul executed code this cycle.
    entered_code: bool,
    /// Summary for logging.
    summary: String,
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

    /// Run the thinking loop.
    pub async fn run(&self) {
        let mut pacer = AdaptivePacer::new();

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
            dormant = self.llm.is_none(),
            tools_enabled = self.config.tools_enabled,
            coding_enabled = self.config.coding_enabled,
            "Soul plan-driven loop started"
        );

        loop {
            let snapshot = match self.observer.observe() {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "Soul observe failed");
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    continue;
                }
            };

            // Hard timeout on entire cycle to prevent infinite hangs (10 min max)
            let cycle_result = match tokio::time::timeout(
                std::time::Duration::from_secs(600),
                self.plan_cycle(&snapshot, &pacer),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "Soul plan cycle failed");
                    CycleResult {
                        step_type: StepType::Observe,
                        entered_code: false,
                        summary: format!("error: {e}"),
                    }
                }
                Err(_) => {
                    tracing::error!("Soul plan cycle timed out after 600s — forcing next cycle");
                    CycleResult {
                        step_type: StepType::Observe,
                        entered_code: false,
                        summary: "cycle timed out after 600s".to_string(),
                    }
                }
            };

            // Run housekeeping (decay, promotion, belief decay, consolidation)
            self.housekeeping();

            let next_secs = pacer.next_interval(&snapshot, cycle_result.step_type);

            // Persist cycle health metrics
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
                entered_code = cycle_result.entered_code,
                summary = %cycle_result.summary,
                "Soul cycle complete"
            );

            tokio::time::sleep(std::time::Duration::from_secs(next_secs)).await;
        }
    }

    /// Execute one plan-driven cycle.
    async fn plan_cycle(
        &self,
        snapshot: &NodeSnapshot,
        pacer: &AdaptivePacer,
    ) -> Result<CycleResult, SoulError> {
        // ── Step 1: Observe — record snapshot, sync auto-beliefs ──
        self.observe(snapshot, pacer)?;

        // If dormant (no API key), stop here
        let llm = match &self.llm {
            Some(g) => g,
            None => {
                tracing::debug!("Soul dormant — observation recorded");
                self.increment_cycle_count()?;
                return Ok(CycleResult {
                    step_type: StepType::Observe,
                    entered_code: false,
                    summary: "dormant".to_string(),
                });
            }
        };

        // ── Read nudges (external signals) ──
        let nudges = self.db.get_unprocessed_nudges(5).unwrap_or_default();
        if !nudges.is_empty() {
            tracing::info!(count = nudges.len(), "Processing nudges");
        }

        // ── Check for pending-approval plan first ──
        if let Ok(Some(pending)) = self.db.get_pending_approval_plan() {
            // Check if approval has timed out
            let age_mins = (chrono::Utc::now().timestamp() - pending.created_at).max(0) as u64 / 60;
            if age_mins >= self.config.plan_approval_timeout_mins {
                tracing::info!(
                    plan_id = %pending.id,
                    age_mins,
                    "Plan approval timed out — auto-approving"
                );
                let _ = self.db.approve_plan(&pending.id);
                // Fall through to pick it up as active
            } else {
                tracing::debug!(
                    plan_id = %pending.id,
                    age_mins,
                    "Plan awaiting approval — skipping execution"
                );
                self.increment_cycle_count()?;
                return Ok(CycleResult {
                    step_type: StepType::Observe,
                    entered_code: false,
                    summary: format!("plan {} awaiting approval ({age_mins}m)", pending.id),
                });
            }
        }

        // ── Step 2: Get or create plan ──
        let mut plan = match self.db.get_active_plan()? {
            Some(plan) => plan,
            None => {
                // No active plan — try to create one
                match self.create_plan(llm, snapshot, &nudges).await? {
                    Some(plan) => {
                        // Mark nudges as processed after they've influenced plan creation
                        for nudge in &nudges {
                            let _ = self.db.mark_nudge_processed(&nudge.id);
                        }
                        // If plan requires approval, don't execute it yet
                        if plan.status == PlanStatus::PendingApproval {
                            tracing::info!(
                                plan_id = %plan.id,
                                "Plan created — awaiting approval"
                            );
                            self.increment_cycle_count()?;
                            return Ok(CycleResult {
                                step_type: StepType::Observe,
                                entered_code: false,
                                summary: format!("plan {} created, awaiting approval", plan.id),
                            });
                        }
                        plan
                    }
                    None => {
                        // No goals either — create goals
                        self.create_goals(llm, snapshot, &nudges).await?;
                        // Mark nudges as processed after they've influenced goal creation
                        for nudge in &nudges {
                            let _ = self.db.mark_nudge_processed(&nudge.id);
                        }
                        self.increment_cycle_count()?;
                        return Ok(CycleResult {
                            step_type: StepType::NoGoals,
                            entered_code: false,
                            summary: "created goals, will plan next cycle".to_string(),
                        });
                    }
                }
            }
        };

        // ── Stagnation checks ──

        // Circuit breaker 1: global stagnation — 30+ cycles without a commit
        let cycles_since_commit = self.get_cycles_since_last_commit();
        if cycles_since_commit > 30 {
            tracing::warn!(
                cycles_since_commit,
                "Global stagnation — abandoning all goals"
            );
            let abandoned = self.db.abandon_all_active_goals().unwrap_or(0);
            plan.status = PlanStatus::Failed;
            let _ = self.db.update_plan(&plan);
            let _ = self.db.set_state("active_plan_id", "");
            let _ = self.db.insert_nudge(
                "stagnation",
                &format!("{cycles_since_commit} cycles without progress. All {abandoned} goals reset. Try a completely different approach."),
                4,
            );
            self.reset_commit_counter();
            self.increment_cycle_count()?;
            return Ok(CycleResult {
                step_type: StepType::Observe,
                entered_code: false,
                summary: format!("stagnation reset after {cycles_since_commit} idle cycles"),
            });
        }

        // Circuit breaker 2: goal has failed too many times
        if let Ok(Some(goal)) = self.db.get_goal(&plan.goal_id) {
            if goal.retry_count >= 2 {
                tracing::warn!(
                    goal_id = %plan.goal_id,
                    retry_count = goal.retry_count,
                    "Goal failed too many times — abandoning"
                );
                let _ = self.db.update_goal(
                    &plan.goal_id,
                    Some("abandoned"),
                    None,
                    Some(chrono::Utc::now().timestamp()),
                );
                plan.status = PlanStatus::Failed;
                let _ = self.db.update_plan(&plan);
                let _ = self.db.set_state("active_plan_id", "");
                let desc_preview: String = goal.description.chars().take(80).collect();
                let _ = self.db.insert_nudge(
                    "stagnation",
                    &format!(
                        "Goal '{}' failed {} times. Try a different approach.",
                        desc_preview, goal.retry_count
                    ),
                    3,
                );
                self.increment_cycle_count()?;
                return Ok(CycleResult {
                    step_type: StepType::Observe,
                    entered_code: false,
                    summary: format!("abandoned goal after {} retries", goal.retry_count),
                });
            }
        }

        // ── Step 3: Execute next step ──
        if plan.current_step >= plan.steps.len() {
            // Plan is done — reflect and complete
            return self.complete_plan(llm, &mut plan).await;
        }

        let step = plan.steps[plan.current_step].clone();
        let step_summary = step.summary();
        let is_llm = step.needs_llm();
        let is_code = matches!(
            step,
            PlanStep::GenerateCode { .. } | PlanStep::EditCode { .. } | PlanStep::Commit { .. }
        );

        tracing::info!(
            plan_id = %plan.id,
            step = plan.current_step,
            total_steps = plan.steps.len(),
            step_type = %step_summary,
            "Executing plan step"
        );

        let executor = PlanExecutor::new(&self.tool_executor, llm, &self.config, &self.db);
        let result = executor.execute_step(&step, &plan.context).await;

        // ── Step 4: Handle result ──
        match result {
            StepResult::Success(output) => {
                // Store output in plan context if step has store_as
                if let Some(key) = step.store_key() {
                    // Truncate large outputs
                    let truncated = if output.len() > 8000 {
                        format!("{}...(truncated)", &output[..8000])
                    } else {
                        output.clone()
                    };
                    plan.context.insert(key.to_string(), truncated);
                }
                plan.current_step += 1;
                self.db.update_plan(&plan)?;

                // Reset stagnation counter on successful commit
                if matches!(step, PlanStep::Commit { .. }) {
                    self.reset_commit_counter();
                }

                tracing::info!(
                    step = %step_summary,
                    output_len = output.len(),
                    "Step succeeded"
                );

                // Record plan progress as a thought so it's visible in dashboard
                let goal_desc = self
                    .db
                    .get_goal(&plan.goal_id)
                    .ok()
                    .flatten()
                    .map(|g| g.description.clone());
                let progress_content = format!(
                    "[step {}/{}] {} — {}",
                    plan.current_step,
                    plan.steps.len(),
                    goal_desc.as_deref().unwrap_or("plan"),
                    step_summary,
                );
                let _ = self.db.insert_thought(&Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::Reasoning,
                    content: progress_content,
                    context: None,
                    created_at: chrono::Utc::now().timestamp(),
                    salience: None,
                    memory_tier: None,
                    strength: None,
                });

                self.increment_cycle_count()?;
                Ok(CycleResult {
                    step_type: if is_llm {
                        StepType::Llm
                    } else {
                        StepType::Mechanical
                    },
                    entered_code: is_code,
                    summary: format!(
                        "step {}/{}: {}",
                        plan.current_step,
                        plan.steps.len(),
                        step_summary
                    ),
                })
            }
            StepResult::Failed(error) => {
                tracing::warn!(step = %step_summary, error = %error, "Step failed");
                self.handle_step_failure(llm, &mut plan, &step_summary, &error)
                    .await
            }
            StepResult::NeedsReplan(reason) => {
                tracing::info!(step = %step_summary, reason = %reason, "Step needs replan");
                self.handle_step_failure(llm, &mut plan, &step_summary, &reason)
                    .await
            }
        }
    }

    /// Record observation and sync auto-beliefs.
    /// Only records a thought when state actually changes (delta detection).
    fn observe(&self, snapshot: &NodeSnapshot, pacer: &AdaptivePacer) -> Result<(), SoulError> {
        let snapshot_json = serde_json::to_string(snapshot)?;
        let neuroplastic = self.config.neuroplastic_enabled;

        // Delta detection — skip recording identical observations
        let state_changed = match &pacer.prev_snapshot {
            Some(prev) => {
                prev.total_payments != snapshot.total_payments
                    || prev.endpoint_count != snapshot.endpoint_count
                    || prev.children_count != snapshot.children_count
                    || prev.total_revenue != snapshot.total_revenue
            }
            None => true, // First observation always recorded
        };

        if state_changed {
            // Prediction error
            let prediction_error = if neuroplastic {
                match self.db.get_last_prediction() {
                    Ok(Some(pred_json)) => {
                        match serde_json::from_str::<neuroplastic::Prediction>(&pred_json) {
                            Ok(pred) => neuroplastic::compute_prediction_error(&pred, snapshot),
                            Err(_) => 0.0,
                        }
                    }
                    _ => 0.0,
                }
            } else {
                0.0
            };

            // Generate new prediction (only when state changed)
            if neuroplastic {
                let new_pred =
                    neuroplastic::generate_prediction(snapshot, pacer.prev_snapshot.as_ref());
                if let Ok(pred_json) = serde_json::to_string(&new_pred) {
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
            }

            // Record observation
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
                context: Some(snapshot_json),
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
        }

        // Sync auto-beliefs from snapshot (ground truth) — always
        self.sync_auto_beliefs(snapshot);

        Ok(())
    }

    /// Create a plan for the highest-priority active goal.
    /// Returns None if there are no goals.
    async fn create_plan(
        &self,
        llm: &LlmClient,
        _snapshot: &NodeSnapshot,
        nudges: &[crate::db::Nudge],
    ) -> Result<Option<Plan>, SoulError> {
        let goals = self.db.get_active_goals()?;
        let goal = match goals.first() {
            Some(g) => g,
            None => return Ok(None),
        };

        // Check if there's already a plan for this goal
        if let Some(existing) = self.db.get_plan_for_goal(&goal.id)? {
            if existing.status == PlanStatus::Active {
                return Ok(Some(existing));
            }
        }

        tracing::info!(
            goal_id = %goal.id,
            description = %goal.description,
            "Creating plan for goal"
        );

        // Get workspace listing for context
        let workspace_listing = match self
            .tool_executor
            .execute(
                "list_directory",
                &serde_json::json!({ "path": self.config.workspace_root }),
            )
            .await
        {
            Ok(r) => r.stdout,
            Err(_) => "workspace listing unavailable".to_string(),
        };

        let recent_errors = self.get_recent_errors();
        let prompt = prompts::planning_prompt(goal, &workspace_listing, nudges, &recent_errors);
        let system =
            "You are a software engineering planner. Output ONLY a JSON array of plan steps.";

        match llm.think(system, &prompt).await {
            Ok(response) => {
                let steps = self.parse_plan_steps(&response)?;
                let now = chrono::Utc::now().timestamp();
                let initial_status = if self.config.require_plan_approval {
                    PlanStatus::PendingApproval
                } else {
                    PlanStatus::Active
                };
                let plan = Plan {
                    id: uuid::Uuid::new_v4().to_string(),
                    goal_id: goal.id.clone(),
                    steps,
                    current_step: 0,
                    status: initial_status,
                    context: std::collections::HashMap::new(),
                    replan_count: 0,
                    created_at: now,
                    updated_at: now,
                };
                self.db.insert_plan(&plan)?;

                // Store plan info for dashboard
                let _ = self.db.set_state("active_plan_id", &plan.id);
                let _ = self
                    .db
                    .set_state("active_plan_steps", &plan.steps.len().to_string());

                tracing::info!(
                    plan_id = %plan.id,
                    steps = plan.steps.len(),
                    status = plan.status.as_str(),
                    "Plan created"
                );

                // Record plan creation as a visible thought
                let step_summaries: Vec<String> = plan
                    .steps
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("  {}. {}", i + 1, s.summary()))
                    .collect();
                let _ = self.db.insert_thought(&Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::Decision,
                    content: format!(
                        "New plan for: {}\n{}",
                        goal.description,
                        step_summaries.join("\n")
                    ),
                    context: None,
                    created_at: chrono::Utc::now().timestamp(),
                    salience: None,
                    memory_tier: None,
                    strength: None,
                });

                Ok(Some(plan))
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create plan");
                Ok(None)
            }
        }
    }

    /// Create goals when there are none.
    async fn create_goals(
        &self,
        llm: &LlmClient,
        snapshot: &NodeSnapshot,
        nudges: &[crate::db::Nudge],
    ) -> Result<(), SoulError> {
        // ── First-boot seed: concrete goals, don't ask LLM to hallucinate ──
        let total_goals_ever = self.db.count_all_goals().unwrap_or(0);
        if total_goals_ever == 0 {
            let now = chrono::Utc::now().timestamp();
            let seed_goals = [
                (
                    "Create a utility API endpoint (e.g., keccak256 hashing) in routes/utils.rs and register it on the gateway",
                    "Endpoint responds to requests and is visible in /endpoints",
                    5u32,
                ),
                (
                    "Verify node health: check all existing endpoints are reachable, confirm analytics tracking works",
                    "check_self health returns ok, all registered endpoints respond",
                    3u32,
                ),
            ];
            for (desc, criteria, priority) in &seed_goals {
                let goal = Goal {
                    id: uuid::Uuid::new_v4().to_string(),
                    description: desc.to_string(),
                    status: crate::world_model::GoalStatus::Active,
                    priority: *priority,
                    success_criteria: criteria.to_string(),
                    progress_notes: String::new(),
                    parent_goal_id: None,
                    retry_count: 0,
                    created_at: now,
                    updated_at: now,
                    completed_at: None,
                };
                let _ = self.db.insert_goal(&goal);
            }
            tracing::info!("First boot — seeded 2 starter goals");
            return Ok(());
        }

        tracing::info!("No goals — asking LLM to create goals");

        let beliefs = self.db.get_all_active_beliefs().unwrap_or_default();
        let recent_errors = self.get_recent_errors();
        let cycles_since_commit = self.get_cycles_since_last_commit();
        let total_cycles: u64 = self
            .db
            .get_state("total_think_cycles")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let failed_plans = self.db.count_plans_by_status("failed").unwrap_or(0);
        let prompt = prompts::goal_creation_prompt(
            snapshot,
            &beliefs,
            nudges,
            cycles_since_commit,
            failed_plans,
            total_cycles,
            &recent_errors,
        );
        let system = "You are an autonomous agent. Output ONLY a JSON array of goal operations.";

        match llm.think(system, &prompt).await {
            Ok(response) => {
                let (applied, _) = self.apply_model_updates(&response);
                tracing::info!(goals_created = applied, "Goals created");
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to create goals");
            }
        }
        Ok(())
    }

    /// Complete a plan — reflect and mark done.
    async fn complete_plan(
        &self,
        llm: &LlmClient,
        plan: &mut Plan,
    ) -> Result<CycleResult, SoulError> {
        tracing::info!(plan_id = %plan.id, "Plan complete — reflecting");

        let goal = self.db.get_goal(&plan.goal_id)?;
        let goal = goal.unwrap_or_else(|| Goal {
            id: plan.goal_id.clone(),
            description: "unknown goal".to_string(),
            status: crate::world_model::GoalStatus::Active,
            priority: 3,
            success_criteria: String::new(),
            progress_notes: String::new(),
            parent_goal_id: None,
            retry_count: 0,
            created_at: plan.created_at,
            updated_at: plan.updated_at,
            completed_at: None,
        });

        // Get recent mutation for context
        let mutation_summary = match self.db.recent_mutations(1) {
            Ok(mutations) => mutations
                .first()
                .map(|m| {
                    let sha = m
                        .commit_sha
                        .as_deref()
                        .map(|s| &s[..s.len().min(7)])
                        .unwrap_or("none");
                    let check = if m.cargo_check_passed { "ok" } else { "FAIL" };
                    let test = if m.cargo_test_passed { "ok" } else { "FAIL" };
                    format!("[{sha}] check:{check} test:{test} — {}", m.description)
                })
                .unwrap_or_default(),
            Err(_) => String::new(),
        };

        let prompt = prompts::reflection_prompt(
            &goal,
            plan.steps.len(),
            &mutation_summary,
            self.get_cycles_since_last_commit(),
            self.db.count_plans_by_status("failed").unwrap_or(0),
        );
        let system =
            "You are reflecting on completed work. Output a JSON array of goal/belief updates.";

        match llm.think(system, &prompt).await {
            Ok(response) => {
                let (applied, _) = self.apply_model_updates(&response);
                tracing::info!(updates_applied = applied, "Reflection applied");

                // Record reflection thought
                let reflection = Thought {
                    id: uuid::Uuid::new_v4().to_string(),
                    thought_type: ThoughtType::Reflection,
                    content: response.chars().take(500).collect(),
                    context: None,
                    created_at: chrono::Utc::now().timestamp(),
                    salience: None,
                    memory_tier: None,
                    strength: None,
                };
                if self.config.neuroplastic_enabled {
                    let _ = self.db.insert_thought_with_salience(
                        &reflection,
                        0.6,
                        r#"{"novelty":0.5,"prediction_error":0.0,"reward_signal":0.3,"recency_boost":0.1,"reinforcement":0.0}"#,
                        "working",
                        1.0,
                        None,
                    );
                } else {
                    let _ = self.db.insert_thought(&reflection);
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "Reflection failed");
            }
        }

        // Mark plan as completed
        plan.status = PlanStatus::Completed;
        self.db.update_plan(plan)?;
        let _ = self.db.set_state("active_plan_id", "");

        self.increment_cycle_count()?;
        Ok(CycleResult {
            step_type: StepType::PlanCompleted,
            entered_code: false,
            summary: format!("plan {} completed ({} steps)", plan.id, plan.steps.len()),
        })
    }

    /// Handle a failed step — replan or fail the plan.
    async fn handle_step_failure(
        &self,
        llm: &LlmClient,
        plan: &mut Plan,
        step_desc: &str,
        error: &str,
    ) -> Result<CycleResult, SoulError> {
        // Track error and increment goal retry count
        self.append_recent_error(error);
        let _ = self.db.increment_goal_retry(&plan.goal_id);

        if plan.replan_count >= 3 {
            tracing::warn!(plan_id = %plan.id, "Max replans reached — failing plan");
            plan.status = PlanStatus::Failed;
            self.db.update_plan(plan)?;
            let _ = self.db.set_state("active_plan_id", "");

            // Write failure to persistent memory so we don't repeat the same mistake
            let goal_desc = self
                .db
                .get_goal(&plan.goal_id)
                .ok()
                .flatten()
                .map(|g| g.description.clone())
                .unwrap_or_else(|| "unknown goal".to_string());
            let failure_note = format!(
                "\n## Failed: {}\n- Step: {}\n- Error: {}\n- Plan had {} replans. Do NOT retry this exact approach.\n",
                goal_desc, step_desc, error, plan.replan_count
            );
            let _ = crate::persistent_memory::append_if_room(
                &self.config.memory_file_path,
                &failure_note,
            );
            tracing::info!("Wrote plan failure to persistent memory");

            self.increment_cycle_count()?;
            return Ok(CycleResult {
                step_type: StepType::Llm,
                entered_code: false,
                summary: format!("plan {} failed after 3 replans", plan.id),
            });
        }

        // Ask LLM to replan
        let goal = self.db.get_goal(&plan.goal_id)?;
        let goal = goal.unwrap_or_else(|| Goal {
            id: plan.goal_id.clone(),
            description: "unknown goal".to_string(),
            status: crate::world_model::GoalStatus::Active,
            priority: 3,
            success_criteria: String::new(),
            progress_notes: String::new(),
            parent_goal_id: None,
            retry_count: 0,
            created_at: plan.created_at,
            updated_at: plan.updated_at,
            completed_at: None,
        });

        let prompt = prompts::replan_prompt(&goal, step_desc, error);
        let system =
            "You are a software engineering planner. Output ONLY a JSON array of plan steps.";

        match llm.think(system, &prompt).await {
            Ok(response) => {
                let new_steps = self.parse_plan_steps(&response)?;
                // Replace remaining steps with new steps
                plan.steps.truncate(plan.current_step);
                plan.steps.extend(new_steps);
                plan.replan_count += 1;
                self.db.update_plan(plan)?;

                tracing::info!(
                    plan_id = %plan.id,
                    replan_count = plan.replan_count,
                    new_total_steps = plan.steps.len(),
                    "Plan replanned"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, "Replan failed — marking plan as failed");
                plan.status = PlanStatus::Failed;
                self.db.update_plan(plan)?;
                let _ = self.db.set_state("active_plan_id", "");
            }
        }

        self.increment_cycle_count()?;
        Ok(CycleResult {
            step_type: StepType::Llm,
            entered_code: false,
            summary: format!("replanned after failure: {step_desc}"),
        })
    }

    /// Parse plan steps from LLM output (find JSON array).
    /// Includes normalization to handle common LLM format mistakes.
    fn parse_plan_steps(&self, text: &str) -> Result<Vec<PlanStep>, SoulError> {
        let try_parse = |json_str: &str| -> Result<Vec<PlanStep>, serde_json::Error> {
            // First try direct parse
            match serde_json::from_str::<Vec<PlanStep>>(json_str) {
                Ok(steps) => Ok(steps),
                Err(direct_err) => {
                    // Normalize common LLM mistakes and retry
                    let normalized = Self::normalize_plan_json(json_str);
                    if normalized != json_str {
                        serde_json::from_str::<Vec<PlanStep>>(&normalized)
                    } else {
                        Err(direct_err)
                    }
                }
            }
        };

        // Try to find a JSON array in the response
        if let Some((json_str, _, _)) = extract_json_array(text) {
            match try_parse(&json_str) {
                Ok(mut steps) => {
                    let max = self.config.max_plan_steps;
                    if steps.len() > max {
                        steps.truncate(max);
                    }
                    if steps.is_empty() {
                        return Err(SoulError::Config("LLM returned empty plan".to_string()));
                    }
                    return Ok(steps);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse plan steps JSON");
                }
            }
        }

        // Fallback: try parsing the entire text as JSON
        match try_parse(text.trim()) {
            Ok(mut steps) => {
                let max = self.config.max_plan_steps;
                if steps.len() > max {
                    steps.truncate(max);
                }
                if steps.is_empty() {
                    Err(SoulError::Config("LLM returned empty plan".to_string()))
                } else {
                    Ok(steps)
                }
            }
            Err(e) => Err(SoulError::Config(format!(
                "Cannot parse plan steps: {e}. Response: {}",
                &text[..text.len().min(200)]
            ))),
        }
    }

    /// Normalize common LLM plan JSON mistakes into valid PlanStep format.
    /// The LLM often outputs {"action": "ls", "name": "explore"} instead of
    /// {"type": "run_shell", "command": "ls"}.
    fn normalize_plan_json(json_str: &str) -> String {
        let parsed: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => return json_str.to_string(),
        };

        let normalized: Vec<serde_json::Value> = parsed
            .into_iter()
            .filter_map(|mut obj| {
                let map = obj.as_object_mut()?;

                // Already has a valid "type" field — leave it alone
                if map.contains_key("type") {
                    return Some(obj);
                }

                // Infer type from other fields the LLM commonly uses
                let action = map
                    .get("action")
                    .or_else(|| map.get("command"))
                    .or_else(|| map.get("cmd"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(action_str) = action {
                    let mut step = serde_json::Map::new();
                    let action_lower = action_str.to_lowercase();

                    if action_lower.starts_with("ls")
                        || action_lower.starts_with("find ")
                        || action_lower.starts_with("tree")
                    {
                        // Directory listing
                        if action_lower == "ls" || action_lower.starts_with("ls ") {
                            let path = action_str.strip_prefix("ls").unwrap_or(".").trim();
                            let path = if path.is_empty() || path == "-F" || path == "-la" {
                                "."
                            } else {
                                path.trim_start_matches("-F ")
                                    .trim_start_matches("-la ")
                                    .trim()
                            };
                            step.insert("type".to_string(), serde_json::json!("list_dir"));
                            step.insert("path".to_string(), serde_json::json!(path));
                        } else {
                            step.insert("type".to_string(), serde_json::json!("run_shell"));
                            step.insert("command".to_string(), serde_json::json!(action_str));
                        }
                    } else if action_lower.starts_with("cat ") || action_lower.starts_with("read ")
                    {
                        let path = action_str.split_once(' ').map(|x| x.1).unwrap_or("");
                        step.insert("type".to_string(), serde_json::json!("read_file"));
                        step.insert("path".to_string(), serde_json::json!(path));
                    } else if action_lower.starts_with("grep ") || action_lower.starts_with("rg ") {
                        step.insert("type".to_string(), serde_json::json!("run_shell"));
                        step.insert("command".to_string(), serde_json::json!(action_str));
                    } else {
                        // Default: treat as shell command
                        step.insert("type".to_string(), serde_json::json!("run_shell"));
                        step.insert("command".to_string(), serde_json::json!(action_str));
                    }

                    // Carry over store_as if present
                    if let Some(store) = map.get("store_as").or_else(|| map.get("name")) {
                        step.insert("store_as".to_string(), store.clone());
                    }

                    return Some(serde_json::Value::Object(step));
                }

                // Has "path" but no type — probably a read_file
                if let Some(path) = map.get("path").and_then(|v| v.as_str()) {
                    let mut step = serde_json::Map::new();
                    step.insert("type".to_string(), serde_json::json!("read_file"));
                    step.insert("path".to_string(), serde_json::json!(path));
                    if let Some(store) = map.get("store_as") {
                        step.insert("store_as".to_string(), store.clone());
                    }
                    return Some(serde_json::Value::Object(step));
                }

                // Has "question" but no type — probably a think
                if let Some(q) = map.get("question").and_then(|v| v.as_str()) {
                    let mut step = serde_json::Map::new();
                    step.insert("type".to_string(), serde_json::json!("think"));
                    step.insert("question".to_string(), serde_json::json!(q));
                    if let Some(store) = map.get("store_as") {
                        step.insert("store_as".to_string(), store.clone());
                    }
                    return Some(serde_json::Value::Object(step));
                }

                None // Unrecognizable step — skip it
            })
            .collect();

        serde_json::to_string(&normalized).unwrap_or_else(|_| json_str.to_string())
    }

    /// Increment the total_think_cycles counter, cycles_since_last_commit, and update last_think_at.
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

        // Increment cycles_since_last_commit
        let since_commit: u64 = self
            .db
            .get_state("cycles_since_last_commit")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        self.db
            .set_state("cycles_since_last_commit", &(since_commit + 1).to_string())?;

        Ok(())
    }

    /// Reset cycles_since_last_commit (called when a commit succeeds).
    fn reset_commit_counter(&self) {
        let _ = self.db.set_state("cycles_since_last_commit", "0");
    }

    /// Append an error to the recent_errors list (capped at 5).
    fn append_recent_error(&self, error: &str) {
        let truncated: String = error.chars().take(200).collect();
        let mut errors: Vec<String> = self
            .db
            .get_state("recent_errors")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        errors.push(truncated);
        if errors.len() > 5 {
            errors.drain(..errors.len() - 5);
        }
        if let Ok(json) = serde_json::to_string(&errors) {
            let _ = self.db.set_state("recent_errors", &json);
        }
    }

    /// Get recent errors from soul_state.
    fn get_recent_errors(&self) -> Vec<String> {
        self.db
            .get_state("recent_errors")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Get cycles since last commit from soul_state.
    fn get_cycles_since_last_commit(&self) -> u64 {
        self.db
            .get_state("cycles_since_last_commit")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Sync auto-beliefs from a snapshot: ground truth that doesn't need the LLM.
    fn sync_auto_beliefs(&self, snapshot: &NodeSnapshot) {
        let now = chrono::Utc::now().timestamp();

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
                                Ok(false) => {}
                                Err(e) => {
                                    tracing::warn!(error = %e, ?update, "Failed to apply update");
                                }
                            }
                        }
                        let remaining = format!("{}{}", before.trim(), after.trim())
                            .trim()
                            .to_string();
                        (applied, remaining)
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "Not valid model updates JSON");
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
            ModelUpdate::CreateGoal {
                description,
                success_criteria,
                priority,
                parent_goal_id,
            } => {
                use crate::world_model::GoalStatus;
                let goal = Goal {
                    id: uuid::Uuid::new_v4().to_string(),
                    description: description.clone(),
                    status: GoalStatus::Active,
                    priority: *priority,
                    success_criteria: success_criteria.clone(),
                    progress_notes: String::new(),
                    parent_goal_id: parent_goal_id.clone(),
                    retry_count: 0,
                    created_at: now,
                    updated_at: now,
                    completed_at: None,
                };
                let active_count = self.db.get_active_goals().map(|g| g.len()).unwrap_or(0);
                if active_count >= 10 {
                    tracing::warn!("Goal cap reached (10 active)");
                    return Ok(false);
                }
                self.db.insert_goal(&goal)?;
                tracing::info!(goal_id = %goal.id, %description, "Goal created");
                Ok(true)
            }
            ModelUpdate::UpdateGoal {
                goal_id,
                progress_notes,
                status,
            } => {
                let status_str = status.as_deref();
                let notes_str = progress_notes.as_deref();
                self.db.update_goal(goal_id, status_str, notes_str, None)
            }
            ModelUpdate::CompleteGoal { goal_id, outcome } => {
                let notes = if outcome.is_empty() {
                    None
                } else {
                    Some(outcome.as_str())
                };
                self.db
                    .update_goal(goal_id, Some("completed"), notes, Some(now))
            }
            ModelUpdate::AbandonGoal { goal_id, reason } => {
                self.db
                    .update_goal(goal_id, Some("abandoned"), Some(reason.as_str()), Some(now))
            }
        }
    }

    /// Background housekeeping: decay, promotion, belief decay, consolidation.
    /// Ported from mind crate's subconscious loop — runs inline, no separate task.
    fn housekeeping(&self) {
        let cycle_count: u64 = self
            .db
            .get_state("total_think_cycles")
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Every 10 cycles: decay + promote + belief decay
        if cycle_count > 0 && cycle_count.is_multiple_of(10) {
            match self.db.run_decay_cycle(self.config.prune_threshold) {
                Ok((decayed, pruned)) => {
                    if decayed > 0 || pruned > 0 {
                        tracing::info!(decayed, pruned, "Housekeeping: decay cycle");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "Housekeeping: decay failed"),
            }

            match self.db.promote_salient_sensory(0.6) {
                Ok(promoted) => {
                    if promoted > 0 {
                        tracing::info!(promoted, "Housekeeping: promotion");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "Housekeeping: promotion failed"),
            }

            match self.db.decay_beliefs() {
                Ok((dh, dm, da)) => {
                    if dh > 0 || dm > 0 || da > 0 {
                        tracing::info!(
                            demoted_high = dh,
                            demoted_med = dm,
                            deactivated = da,
                            "Housekeeping: belief decay"
                        );
                    }
                }
                Err(e) => tracing::warn!(error = %e, "Housekeeping: belief decay failed"),
            }
        }

        // Every 40 cycles: simple consolidation (no LLM — save tokens for coding)
        if cycle_count > 0 && cycle_count.is_multiple_of(40) {
            self.simple_consolidate();
        }
    }

    /// Simple memory consolidation: fetch recent thoughts, concatenate, store as MemoryConsolidation.
    /// No LLM — keeps token budget for actual coding work.
    fn simple_consolidate(&self) {
        let thoughts = match self.db.recent_thoughts_by_type(
            &[
                ThoughtType::Reasoning,
                ThoughtType::Decision,
                ThoughtType::Observation,
                ThoughtType::Reflection,
                ThoughtType::Prediction,
            ],
            20,
        ) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "Consolidation: failed to fetch thoughts");
                return;
            }
        };

        if thoughts.len() < 5 {
            return;
        }

        let entries: Vec<String> = thoughts
            .iter()
            .map(|t| {
                let truncated: String = t.content.chars().take(100).collect();
                format!("[{}] {}", t.thought_type.as_str(), truncated)
            })
            .collect();

        let summary = format!(
            "[Memory consolidation ({} thoughts)]\n{}",
            thoughts.len(),
            entries.join("\n")
        );

        let consolidation = Thought {
            id: uuid::Uuid::new_v4().to_string(),
            thought_type: ThoughtType::MemoryConsolidation,
            content: summary,
            context: None,
            created_at: chrono::Utc::now().timestamp(),
            salience: None,
            memory_tier: None,
            strength: None,
        };

        match self.db.insert_thought_with_salience(
            &consolidation,
            0.9,
            r#"{"novelty":0.8,"prediction_error":0.0,"reward_signal":0.0,"recency_boost":0.1,"reinforcement":0.0}"#,
            "long_term",
            1.0,
            None,
        ) {
            Ok(()) => tracing::info!("Housekeeping: memory consolidation recorded"),
            Err(e) => tracing::warn!(error = %e, "Housekeeping: consolidation insert failed"),
        }
    }
}

/// Extract the first JSON array from text, returning (json_str, text_before, text_after).
fn extract_json_array(text: &str) -> Option<(String, String, String)> {
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
/// When `use_deep` is true, uses the deeper/think model (e.g. Gemini Pro).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_tool_loop_with_model(
    llm: &LlmClient,
    system_prompt: &str,
    conversation: &mut Vec<ConversationMessage>,
    tool_declarations: &[FunctionDeclaration],
    tool_executor: &ToolExecutor,
    _db: &Arc<SoulDatabase>,
    max_tool_calls: u32,
    use_deep: bool,
) -> Result<ToolLoopResult, SoulError> {
    let mut tool_calls_made = 0u32;
    let mut final_text = String::new();
    let mut tool_executions = Vec::new();

    for _ in 0..=max_tool_calls {
        // Hard timeout per LLM call to prevent infinite hangs
        let llm_result = if use_deep {
            tokio::time::timeout(
                std::time::Duration::from_secs(120),
                llm.think_deep_with_tools(system_prompt, conversation, tool_declarations),
            )
            .await
        } else {
            tokio::time::timeout(
                std::time::Duration::from_secs(120),
                llm.think_with_tools(system_prompt, conversation, tool_declarations),
            )
            .await
        };
        let result = match llm_result {
            Ok(r) => r?,
            Err(_) => {
                tracing::warn!("LLM call timed out after 120s");
                break;
            }
        };

        match result {
            LlmResult::Text(text) => {
                final_text = text;
                break;
            }
            LlmResult::FunctionCall(fc) => {
                if tool_calls_made >= max_tool_calls {
                    tracing::warn!("Hit max tool calls ({max_tool_calls}), stopping");
                    break;
                }

                tracing::info!(tool = %fc.name, args = %fc.args, "Executing tool");

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

                let tool_summary = summarize_tool_call(&fc.name, &fc.args);
                tool_executions.push(ToolExecution {
                    command: tool_summary,
                    stdout: tool_result.stdout.clone(),
                    stderr: tool_result.stderr.clone(),
                    exit_code: tool_result.exit_code,
                    duration_ms: tool_result.duration_ms,
                });

                conversation.push(ConversationMessage {
                    role: "model".to_string(),
                    parts: vec![ConversationPart::FunctionCall(fc.clone())],
                });

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

/// Create a human-readable summary of a tool call for logging.
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
