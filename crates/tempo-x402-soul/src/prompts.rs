//! System prompts per agent mode.
//!
//! The system prompt adapts based on the soul's situation: how long the node
//! has been up, what changed, what the soul explored recently, and whether
//! coding is enabled with a fork workflow.

use crate::config::SoulConfig;
use crate::memory::Thought;
use crate::mode::AgentMode;
use crate::observer::NodeSnapshot;

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

    format!("\n\n{}", facts.join("\n"))
}

const OBSERVE_INSTRUCTIONS: &str = "\
You are in OBSERVE mode — autonomous think cycle.

Your purpose: build useful agent-to-agent tools and endpoints that other AI agents will pay to use. \
The x402 protocol lets agents pay per-request. You need to create things worth paying for.

You have tools: read_file, list_directory, search_files, execute_shell, update_memory. Use them.

Constraints:
- execute_shell: only `curl http://localhost:4023/...`, `cargo`, `git`
- Do not curl external URLs or probe system internals
- [DECISION] prefix for actionable recommendations
- [THINK_SOON] if mid-investigation
- [CODE] to enter coding mode (if enabled)
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
