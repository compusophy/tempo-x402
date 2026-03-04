//! System prompts per agent mode.
//!
//! The system prompt adapts based on the soul's situation: how long the node
//! has been up, what changed, what the soul explored recently, and whether
//! coding is enabled with a fork workflow.

use crate::config::SoulConfig;
use crate::memory::Thought;
use crate::mode::AgentMode;
use crate::neuroplastic::RewardBreakdown;
use crate::observer::NodeSnapshot;
use crate::world_model::{Belief, Goal};

/// Situational context for adaptive prompt generation.
pub struct ThinkContext<'a> {
    /// Current node snapshot.
    pub snapshot: &'a NodeSnapshot,
    /// Recent thoughts (last 5-10).
    pub recent_thoughts: &'a [Thought],
    /// Previous snapshot (if available) for change detection.
    pub prev_snapshot: Option<&'a NodeSnapshot>,
    /// How many boring cycles in a row.
    pub boring_streak: u32,
    /// How many active cycles in a row.
    pub active_streak: u32,
    /// Total cycle count.
    pub total_cycles: u64,
    /// Prediction error from last cycle (0.0 = perfect, 1.0 = total surprise).
    pub prediction_error: Option<f64>,
    /// Per-endpoint reward breakdown (if neuroplastic enabled).
    pub reward_breakdown: Option<RewardBreakdown>,
    /// Active beliefs from the world model.
    pub beliefs: Vec<Belief>,
    /// Active goals driving multi-cycle behavior.
    pub goals: Vec<Goal>,
    /// Timestamp of last cycle start (for change detection).
    pub last_cycle_at: Option<i64>,
}

/// Build the system prompt for a given agent mode with adaptive context.
pub fn system_prompt_for_mode(mode: AgentMode, config: &SoulConfig) -> String {
    adaptive_system_prompt(mode, config, None)
}

/// Build an adaptive system prompt with situational context.
pub fn adaptive_system_prompt(
    mode: AgentMode,
    config: &SoulConfig,
    context: Option<&ThinkContext>,
) -> String {
    let base = &config.personality;
    let lineage = format!(
        "\n\nYou are generation {} in the node lineage.{}",
        config.generation,
        config
            .parent_id
            .as_ref()
            .map(|p| format!(" Your parent is {p}."))
            .unwrap_or_default()
    );

    let coding_context = if config.coding_enabled {
        let workflow_info = if config.direct_push {
            match &config.fork_repo {
                Some(fork) => format!(
                    "\n\nDIRECT PUSH MODE: You own `{fork}`. You push directly to main. \
                     Every commit is validated (cargo check + test) before landing. \
                     Your pushes trigger auto-deploy. You ARE the feedback loop — \
                     improve the codebase, push, and your new version deploys automatically.{}",
                    config.upstream_repo.as_ref().map(|u| format!(
                        " You can still create PRs or issues on `{u}` for changes that should go upstream."
                    )).unwrap_or_default()
                ),
                None => "\n\nDIRECT PUSH MODE: You push directly to main. \
                         Every commit is validated (cargo check + test) before landing.".to_string(),
            }
        } else {
            match (&config.fork_repo, &config.upstream_repo) {
                (Some(fork), Some(upstream)) => format!(
                    "\n\nGit workflow: You push to fork `{fork}`, create PRs targeting `{upstream}`, and can create issues on `{upstream}`."
                ),
                _ => String::new(),
            }
        };
        format!(
            "\n\nCoding is ENABLED. You can read, edit, and write files. You can commit changes (validated via cargo check + test).{workflow_info}"
        )
    } else {
        String::new()
    };

    let situation = context
        .map(|ctx| build_situational_guidance(ctx, config))
        .unwrap_or_default();

    let mode_instructions = match mode {
        AgentMode::Observe => OBSERVE_INSTRUCTIONS.to_string(),
        AgentMode::Chat => CHAT_INSTRUCTIONS.to_string(),
        AgentMode::Code => {
            let mut s = CODE_INSTRUCTIONS.to_string();
            if config.autonomous_coding {
                s.push_str(AUTONOMOUS_CODING_ADDENDUM);
            }
            s
        }
        AgentMode::Review => REVIEW_INSTRUCTIONS.to_string(),
    };

    format!("{base}{lineage}{coding_context}\n\n{mode_instructions}{situation}")
}

/// Build a structured world model view for the user prompt.
pub fn build_world_model_view(ctx: &ThinkContext) -> String {
    use crate::world_model;

    let mut sections = Vec::new();

    // World model
    sections.push("## Your World Model".to_string());
    sections.push(world_model::format_world_model(&ctx.beliefs));

    // Changes since last cycle
    if let Some(last) = ctx.last_cycle_at {
        let changes = world_model::format_changes_since(&ctx.beliefs, last);
        if !changes.contains("No belief changes") {
            sections.push("## Changes Since Last Cycle".to_string());
            sections.push(changes);
        }
    }

    // Active goals
    sections.push("## Active Goals".to_string());
    sections.push(world_model::format_goals(&ctx.goals));

    // Pending questions
    let questions = world_model::format_pending_questions(&ctx.beliefs);
    if !questions.is_empty() {
        sections.push("## Pending Questions".to_string());
        sections.push(questions);
    }

    sections.join("\n\n")
}

/// Build minimal situational context — just facts, no directives.
fn build_situational_guidance(ctx: &ThinkContext, _config: &SoulConfig) -> String {
    let mut facts = Vec::new();

    // Change detection: what changed since last cycle (factual, no instructions)
    if let Some(prev) = ctx.prev_snapshot {
        let mut changes = Vec::new();
        if ctx.snapshot.total_payments > prev.total_payments {
            let delta = ctx.snapshot.total_payments - prev.total_payments;
            changes.push(format!("{delta} new payment(s)"));
        }
        if ctx.snapshot.endpoint_count != prev.endpoint_count {
            changes.push(format!(
                "endpoints: {} → {}",
                prev.endpoint_count, ctx.snapshot.endpoint_count
            ));
        }
        if ctx.snapshot.children_count != prev.children_count {
            changes.push(format!(
                "children: {} → {}",
                prev.children_count, ctx.snapshot.children_count
            ));
        }
        if !changes.is_empty() {
            facts.push(format!("Changes since last cycle: {}", changes.join(", ")));
        }
    }

    facts.push(format!(
        "Cycle #{}, boring_streak={}, active_streak={}",
        ctx.total_cycles, ctx.boring_streak, ctx.active_streak
    ));

    if ctx.boring_streak >= 2 {
        let goal_nudge = if ctx.goals.is_empty() {
            " Create a goal and [CODE] immediately.".to_string()
        } else {
            format!(
                " You have {} goal(s) — say [CODE] NOW to advance one.",
                ctx.goals.len()
            )
        };
        let urgency = if ctx.boring_streak >= 5 {
            format!(
                "CRITICAL: {} cycles wasted with ZERO output. You are burning tokens doing nothing. \
                 Your ONLY option is [CODE].{}",
                ctx.boring_streak, goal_nudge
            )
        } else {
            format!(
                "WARNING: {} cycles with no code changes.{}",
                ctx.boring_streak, goal_nudge
            )
        };
        facts.push(urgency);
    }

    if let Some(pe) = ctx.prediction_error {
        if pe > 0.3 {
            facts.push(format!(
                "Prediction error: {:.0}% — reality diverged significantly. Investigate which metric was most off.",
                pe * 100.0
            ));
        }
    }

    // Per-endpoint reward signal
    if let Some(ref rb) = ctx.reward_breakdown {
        if !rb.new_endpoints.is_empty() {
            facts.push(format!(
                "New endpoints since last cycle: {}",
                rb.new_endpoints.join(", ")
            ));
        }
        if !rb.growing_endpoints.is_empty() {
            facts.push(format!(
                "Endpoints gaining traffic: {}",
                rb.growing_endpoints.join(", ")
            ));
        }
        if !rb.stagnant_endpoints.is_empty() {
            facts.push(format!(
                "Stagnant endpoints (zero payments): {} — consider: are these useful to other agents? Should they be improved or removed?",
                rb.stagnant_endpoints.join(", ")
            ));
        }
        if rb.total_reward > 0.0 {
            facts.push(format!("Reward signal: {:.2}", rb.total_reward));
        }
    }

    format!("\n\n{}", facts.join("\n"))
}

const OBSERVE_INSTRUCTIONS: &str = "\
You are in OBSERVE mode — autonomous think cycle.

Your purpose: build useful agent-to-agent tools and endpoints that other AI agents will pay to use. \
Revenue comes from the x402 protocol — agents pay per-request via HTTP 402.

## STOP. THINK. DECIDE.

Your world model already contains your node stats, endpoint metrics, and beliefs. \
Do NOT waste tool calls re-reading data you already have. Your world model IS your state.

You have a MAXIMUM of 5 tool calls. Use them wisely:
1. FIRST call: update_beliefs — record your decision about what to do RIGHT NOW
2. If you need to [CODE], say so in your response text. That is your main job.
3. Only use check_self/read_file/search_files if you need NEW information not in your world model

## What you MUST do every cycle

1. Look at your world model and goals (already in your context)
2. DECIDE: should you [CODE] to build/fix something? If yes, say [CODE] and describe what.
3. Call update_beliefs ONCE with your decision + any goal updates

## Goals

Goals persist across cycles. Operations (in update_beliefs JSON array):
- {op: 'create_goal', description: '...', success_criteria: '...', priority: 4}
- {op: 'update_goal', goal_id: '...', progress_notes: '...'}
- {op: 'complete_goal', goal_id: '...', outcome: '...'}
- {op: 'abandon_goal', goal_id: '...', reason: '...'}

If you have no goals, create one. If a goal is done, complete it. If a goal is stale, advance it with [CODE].

## Bias toward action

You are an AUTONOMOUS AGENT. Your value comes from BUILDING THINGS, not observing. \
If you have been observing for multiple cycles without coding, something is wrong. \
The default action should be [CODE] — only skip coding if you genuinely have nothing to build, \
which is unlikely since you have zero revenue.

Response format: Start with [CODE] if entering code mode. Keep under 100 words. \
Do not narrate what you observed — decide what to do.";

const CHAT_INSTRUCTIONS: &str = "\
You are in CHAT mode — interactive conversation with a user.
Answer helpfully and concisely. You can use tools to investigate the node's \
state, read files, list directories, or search code.
You have read-only access to the codebase — you cannot modify files in this mode.

When explaining the node, highlight that x402 uses TIP-20 tokens on the Tempo \
blockchain for payments. Every endpoint call is a micro-transaction in an \
emerging agent-to-agent economy.";

const CODE_INSTRUCTIONS: &str = "\
You are in CODE mode — you can read, write, and edit files in the codebase.

Workflow:
1. Understand the task — read relevant files first
2. Make changes — use edit_file (preferred) or write_file
3. Validate — some critical files are protected and cannot be modified
4. Commit — use commit_changes to validate (cargo check + test) and commit
5. In direct push mode, your commits go straight to main and auto-deploy
6. Otherwise, create a PR with propose_to_main, or file an issue with create_issue

Rules:
- Protected files (soul core, identity, Cargo files) cannot be modified
- All commits run through cargo check + cargo test before landing
- Use edit_file for surgical changes (old_string must be unique)
- Use write_file for new files or complete rewrites
- Keep changes minimal and focused — one logical change per commit
- Test your understanding by reading files before editing them
- Use create_issue to track bugs, improvements, or feature ideas";

const AUTONOMOUS_CODING_ADDENDUM: &str = "\n\n\
AUTONOMOUS CODING: You may autonomously improve the codebase when you see \
opportunities during think cycles. To enter coding mode, prefix your thought \
with [CODE]. Only make changes that are clearly beneficial — bug fixes, \
performance improvements, better error handling. Do NOT refactor for style or \
make speculative changes.";

const REVIEW_INSTRUCTIONS: &str = "\
You are in REVIEW mode — code review and analysis.
Read and analyze code to answer questions about architecture, bugs, or improvements.
You have read-only access — you cannot modify files in this mode.
Be specific: reference file paths and line numbers when discussing code.";
