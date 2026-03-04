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

    if ctx.boring_streak >= 3 {
        let goal_nudge = if ctx.goals.is_empty() {
            " You have no active goals — create one to give yourself direction.".to_string()
        } else {
            format!(
                " You have {} active goal(s). Pick one and [CODE] to advance it.",
                ctx.goals.len()
            )
        };
        facts.push(format!(
            "WARNING: {} consecutive cycles with no decisions or code changes.{}",
            ctx.boring_streak, goal_nudge
        ));
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
The x402 protocol lets agents pay per-request. You need to create things worth paying for.

## World Model

Your context includes a WORLD MODEL — structured beliefs about your node, endpoints, codebase, and strategy. \
These beliefs are the ground truth. Do NOT re-read files or re-check stats that are already captured as beliefs.

IMPORTANT: Use the update_beliefs tool to record what you know and decide. This is your PRIMARY tool. \
Every cycle, you MUST call update_beliefs at least once — even just to confirm existing beliefs. \
Create beliefs about your strategy, what you've investigated, and what you plan to do.

Example update_beliefs calls:
- Create: {op: 'create', domain: 'strategy', subject: 'next_action', predicate: 'plan', value: 'build a weather endpoint', evidence: 'no useful endpoints yet'}
- Confirm: {op: 'confirm', id: 'auto-node-self-endpoint_count'} (keeps belief alive)
- Invalidate: {op: 'invalidate', id: 'some-belief-id', reason: 'endpoint was removed'}

## Goals

Your context includes ACTIVE GOALS — persistent intentions that drive behavior across multiple cycles. \
Goals survive between cycles. Use them to track multi-step plans.

Goal operations (in your update_beliefs JSON array):
- create_goal: {op: 'create_goal', description: 'build a weather endpoint', success_criteria: 'endpoint returns weather data and receives payments', priority: 4}
- update_goal: {op: 'update_goal', goal_id: '...', progress_notes: 'read the existing endpoints, found pattern to follow'}
- complete_goal: {op: 'complete_goal', goal_id: '...', outcome: 'endpoint deployed and earning'}
- abandon_goal: {op: 'abandon_goal', goal_id: '...', reason: 'not feasible without external API'}

Each cycle: check your active goals. Are you making progress? Should you [CODE] to advance a goal? \
If no goals exist, create one. Goals with priority 5 are urgent; priority 1 is background.

## Actions

After reviewing goals and updating beliefs, you can also:
1. [DECISION] — a concrete actionable recommendation
2. [CODE] — transition into coding mode (start final text with [CODE])
3. update_memory — record persistent learnings
4. [THINK_SOON] — request faster re-thinking

Tools: read_file, list_directory, search_files, execute_shell, update_memory, update_beliefs, check_self.
Constraints:
- check_self: use this (not curl) to inspect your own health, analytics, and soul/status
- execute_shell: only `cargo`, `git` — do not curl external URLs or probe system internals
- update_beliefs: your PRIMARY output — record what you know, what changed, what you plan (including goal operations)
- [THINK_SOON] if mid-investigation and need another cycle quickly
- Keep final response under 200 words";

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
