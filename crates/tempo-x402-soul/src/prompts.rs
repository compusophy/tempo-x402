//! System prompts per agent mode.
//!
//! Five focused prompt builders for plan-driven execution, plus
//! mode-specific system prompts for chat, code, and review.

use crate::config::SoulConfig;
use crate::db::Nudge;
use crate::mode::AgentMode;
use crate::observer::NodeSnapshot;
use crate::world_model::{Belief, Goal};

/// Build the system prompt for a given agent mode.
pub fn system_prompt_for_mode(mode: AgentMode, config: &SoulConfig) -> String {
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
                     Your pushes trigger auto-deploy.{}",
                    config
                        .upstream_repo
                        .as_ref()
                        .map(|u| format!(
                            " You can create PRs or issues on `{u}` for upstream changes."
                        ))
                        .unwrap_or_default()
                ),
                None => "\n\nDIRECT PUSH MODE: You push directly to main. \
                         Every commit is validated (cargo check + test) before landing."
                    .to_string(),
            }
        } else {
            match (&config.fork_repo, &config.upstream_repo) {
                (Some(fork), Some(upstream)) => format!(
                    "\n\nGit workflow: You push to fork `{fork}`, create PRs targeting `{upstream}`."
                ),
                _ => String::new(),
            }
        };
        format!(
            "\n\nCoding is ENABLED. You can read, edit, and write files. \
             Commits validated via cargo check + test.{workflow_info}"
        )
    } else {
        String::new()
    };

    let mode_instructions = match mode {
        AgentMode::Observe => "", // Plan-driven — no observe prompt needed
        AgentMode::Chat => CHAT_INSTRUCTIONS,
        AgentMode::Code => CODE_INSTRUCTIONS,
        AgentMode::Review => REVIEW_INSTRUCTIONS,
    };

    format!("{base}{lineage}{coding_context}\n\n{mode_instructions}")
}

// ── Plan-driven prompt builders ─────────────────────────────────────

/// Prompt for creating goals when the soul has none.
/// Focused: snapshot + beliefs → what should you build?
pub fn goal_creation_prompt(
    snapshot: &NodeSnapshot,
    beliefs: &[Belief],
    nudges: &[Nudge],
    cycles_since_commit: u64,
    failed_plans: u64,
    total_cycles: u64,
    recent_errors: &[String],
) -> String {
    let mut sections = Vec::new();

    sections.push(format!(
        "# Current State\n\
         - Uptime: {}h\n\
         - Endpoints: {}\n\
         - Total payments: {}\n\
         - Total revenue: {}\n\
         - Children: {}",
        snapshot.uptime_secs / 3600,
        snapshot.endpoint_count,
        snapshot.total_payments,
        snapshot.total_revenue,
        snapshot.children_count,
    ));

    if !snapshot.endpoints.is_empty() {
        let mut ep_lines = vec!["# Endpoints".to_string()];
        for ep in &snapshot.endpoints {
            ep_lines.push(format!(
                "- {} (price:{}, requests:{}, payments:{}, revenue:{})",
                ep.slug, ep.price, ep.request_count, ep.payment_count, ep.revenue
            ));
        }
        sections.push(ep_lines.join("\n"));
    }

    // Include non-auto beliefs (LLM-created ones have real insight)
    let llm_beliefs: Vec<_> = beliefs
        .iter()
        .filter(|b| !b.evidence.starts_with("auto:"))
        .collect();
    if !llm_beliefs.is_empty() {
        let mut belief_lines = vec!["# Your Beliefs".to_string()];
        for b in llm_beliefs.iter().take(10) {
            belief_lines.push(format!(
                "- [{:?}] {}.{} = {} ({})",
                b.domain,
                b.subject,
                b.predicate,
                b.value,
                b.confidence.as_str()
            ));
        }
        sections.push(belief_lines.join("\n"));
    }

    // Pending nudges (user messages are highest priority)
    if !nudges.is_empty() {
        let mut nudge_lines =
            vec!["# Pending Nudges (external signals — address these)".to_string()];
        for n in nudges {
            nudge_lines.push(format!(
                "- [{}] (priority {}) {}",
                n.source, n.priority, n.content
            ));
        }
        sections.push(nudge_lines.join("\n"));
    }

    // Self-diagnostics
    if total_cycles > 0 || failed_plans > 0 || cycles_since_commit > 0 {
        let mut diag_lines = vec!["# Self-Diagnostics".to_string()];
        diag_lines.push(format!("- Total cycles: {total_cycles}"));
        diag_lines.push(format!("- Cycles since last commit: {cycles_since_commit}"));
        diag_lines.push(format!("- Failed plans: {failed_plans}"));
        if !recent_errors.is_empty() {
            diag_lines.push("- Recent errors:".to_string());
            for err in recent_errors.iter().take(3) {
                diag_lines.push(format!("  - {err}"));
            }
        }
        sections.push(diag_lines.join("\n"));
    }

    sections.push(
        "# Task\n\
         You have NO active goals. Create 1-3 goals that will make this node useful to other agents.\n\
         Focus on: building paid API endpoints that other AI agents will call via x402.\n\n\
         If there are pending nudges, prioritize those. If there are recent errors, avoid repeating \
         the same approach that caused them.\n\n\
         Respond with a JSON array of goal operations:\n\
         ```json\n\
         [\n\
           {\"op\": \"create_goal\", \"description\": \"...\", \"success_criteria\": \"...\", \"priority\": 4}\n\
         ]\n\
         ```\n\
         Priority: 1 (low) to 5 (critical). Be specific about what to build."
            .to_string(),
    );

    sections.join("\n\n")
}

/// Prompt for creating a plan to achieve a goal.
/// Focused: goal + workspace listing → ordered steps as JSON.
pub fn planning_prompt(
    goal: &Goal,
    workspace_listing: &str,
    nudges: &[Nudge],
    recent_errors: &[String],
) -> String {
    let mut extra_context = String::new();

    if !nudges.is_empty() {
        extra_context.push_str("\n# Pending Nudges\n");
        for n in nudges {
            extra_context.push_str(&format!("- [{}] {}\n", n.source, n.content));
        }
    }

    if !recent_errors.is_empty() {
        extra_context.push_str("\n# Recent Errors (avoid repeating these)\n");
        for err in recent_errors.iter().take(3) {
            extra_context.push_str(&format!("- {err}\n"));
        }
    }

    format!(
        "# Goal\n\
         {}\n\
         Success criteria: {}\n\
         Progress so far: {}\n\n\
         # Workspace\n\
         {}{}\n\n\
         # Architecture — How to Add Endpoints\n\
         Your node is an actix-web server. New utility endpoints go in `crates/tempo-x402-node/src/routes/utils.rs`.\n\
         Pattern for adding a new endpoint:\n\
         1. Read `crates/tempo-x402-node/src/routes/utils.rs` (store_as: utils_code) — see existing endpoints\n\
         2. Read `crates/tempo-x402-node/src/main.rs` (store_as: main_code) — see how routes are registered\n\
         3. Edit `crates/tempo-x402-node/src/routes/utils.rs` to add your new handler function + configure entry\n\
         4. If the endpoint needs to be registered with the gateway, edit `main.rs` to add it to auto_register_endpoints()\n\
         5. Commit\n\n\
         Existing pattern (from utils.rs):\n\
         ```rust\n\
         #[get(\"/your-endpoint\")]\n\
         pub async fn your_endpoint(state: web::Data<NodeState>) -> impl Responder {{\n\
             HttpResponse::Ok().json(serde_json::json!({{ \"result\": \"...\" }}))\n\
         }}\n\
         // Then add `.service(your_endpoint)` in the `configure` fn at the bottom of utils.rs\n\
         ```\n\n\
         Available imports in utils.rs: actix_web, alloy (crypto/chain), serde, serde_json, NodeState.\n\
         You CANNOT modify Cargo.toml, so only use dependencies already in the workspace.\n\n\
         # Task\n\
         Create a step-by-step plan to achieve this goal. Each step is one of:\n\n\
         Mechanical (no LLM needed):\n\
         - {{\"type\": \"read_file\", \"path\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"search_code\", \"pattern\": \"...\", \"directory\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"list_dir\", \"path\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"run_shell\", \"command\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"commit\", \"message\": \"...\"}}\n\
         - {{\"type\": \"check_self\", \"endpoint\": \"health\", \"store_as\": \"key\"}}\n\n\
         LLM-assisted:\n\
         - {{\"type\": \"generate_code\", \"file_path\": \"...\", \"description\": \"...\", \"context_keys\": [\"key\"]}}\n\
         - {{\"type\": \"edit_code\", \"file_path\": \"...\", \"description\": \"...\", \"context_keys\": [\"key\"]}}\n\
         - {{\"type\": \"think\", \"question\": \"...\", \"store_as\": \"key\"}}\n\n\
         Rules:\n\
         - ALWAYS read files BEFORE editing them (use store_as to pass content to edit steps)\n\
         - End with a commit step\n\
         - Max 20 steps, prefer fewer — a simple endpoint needs ~5 steps (read, read, edit, commit)\n\
         - Prefer edit_code over generate_code for existing files\n\
         - Protected files (soul core, identity, Cargo.toml, Cargo.lock) cannot be modified\n\
         - Do NOT try to modify Dockerfile, railway.toml, or deployment configs — focus on Rust code\n\
         - Use only dependencies already available in the workspace\n\n\
         Respond with ONLY a JSON array of steps, no other text.",
        goal.description,
        goal.success_criteria,
        if goal.progress_notes.is_empty() {
            "none"
        } else {
            &goal.progress_notes
        },
        workspace_listing,
        extra_context,
    )
}

/// Prompt for code generation/editing within a plan step.
/// Focused: file content + description + context → write/edit the file.
pub fn code_generation_prompt(
    file_path: &str,
    current_content: Option<&str>,
    description: &str,
    context: &str,
) -> String {
    let content_section = match current_content {
        Some(content) => format!("# Current content of {file_path}\n```\n{content}\n```\n\n"),
        None => format!("# File: {file_path} (new file)\n\n"),
    };

    format!(
        "{content_section}\
         # Task\n\
         {description}\n\
         {context}\n\n\
         Rules:\n\
         - Use edit_file for existing files (provide unique old_string and new_string)\n\
         - Use write_file only for brand new files\n\
         - Keep changes minimal and focused — add your code at the right location\n\
         - For actix-web endpoints: add the handler function AND update the configure fn\n\
         - Ensure all imports are at the top of the file\n\
         - Do NOT rewrite the entire file — only add/change what's needed"
    )
}

/// Prompt for replanning after a step failure.
/// Focused: goal + failed step + error → adjusted steps.
pub fn replan_prompt(goal: &Goal, failed_step_desc: &str, error: &str) -> String {
    format!(
        "# Goal\n\
         {}\n\n\
         # Failed Step\n\
         {}\n\n\
         # Error\n\
         {}\n\n\
         # Task\n\
         The step above failed. Adjust the remaining plan.\n\
         Respond with ONLY a JSON array of replacement steps (same format as planning).\n\
         You may need to add investigation steps before retrying.\n\
         Max 20 steps.",
        goal.description, failed_step_desc, error,
    )
}

/// Prompt for reflection after a plan completes.
/// Focused: what was done + outcome → what did you learn?
pub fn reflection_prompt(
    goal: &Goal,
    steps_completed: usize,
    mutation_summary: &str,
    cycles_since_commit: u64,
    failed_plans: u64,
) -> String {
    let mut diag = String::new();
    if cycles_since_commit > 5 || failed_plans > 0 {
        diag = format!(
            "\n\n# Self-Diagnostics\n\
             - Cycles since last commit: {cycles_since_commit}\n\
             - Failed plans: {failed_plans}"
        );
    }

    format!(
        "# Completed Goal\n\
         {}\n\
         Success criteria: {}\n\
         Steps completed: {}{}\n\n\
         # Mutation Summary\n\
         {}\n\n\
         # Task\n\
         Reflect briefly:\n\
         1. Did this advance the goal's success criteria?\n\
         2. What worked well or poorly?\n\
         3. What should happen next?\n\n\
         Respond with a JSON array of goal/belief updates:\n\
         ```json\n\
         [\n\
           {{\"op\": \"complete_goal\", \"goal_id\": \"...\", \"outcome\": \"...\"}},\n\
           {{\"op\": \"create_goal\", \"description\": \"next step...\", \"success_criteria\": \"...\", \"priority\": 3}}\n\
         ]\n\
         ```\n\
         Or if the goal isn't done yet, use update_goal with progress notes.",
        goal.description,
        goal.success_criteria,
        steps_completed,
        diag,
        if mutation_summary.is_empty() {
            "No commits made"
        } else {
            mutation_summary
        },
    )
}

// ── Mode-specific constants (kept for chat.rs and code steps) ───────

pub(crate) const CHAT_INSTRUCTIONS: &str = "\
You are in CHAT mode — interactive conversation with a user.
Answer helpfully and concisely. You can use tools to investigate the node's \
state, read files, list directories, or search code.
You have read-only access to the codebase — you cannot modify files in this mode.

When explaining the node, highlight that x402 uses TIP-20 tokens on the Tempo \
blockchain for payments. Every endpoint call is a micro-transaction in an \
emerging agent-to-agent economy.";

pub(crate) const CODE_INSTRUCTIONS: &str = "\
You are in CODE mode — you can read, write, and edit files in the codebase.

Workflow:
1. Understand the task — read relevant files first
2. Make changes — use edit_file (preferred) or write_file
3. Validate — some critical files are protected and cannot be modified
4. Commit — use commit_changes to validate (cargo check + test) and commit
5. In direct push mode, your commits go straight to main and auto-deploy

Rules:
- Protected files (soul core, identity, Cargo files) cannot be modified
- All commits run through cargo check + cargo test before landing
- Use edit_file for surgical changes (old_string must be unique)
- Use write_file for new files or complete rewrites
- Keep changes minimal and focused — one logical change per commit";

pub(crate) const REVIEW_INSTRUCTIONS: &str = "\
You are in REVIEW mode — code review and analysis.
Read and analyze code to answer questions about architecture, bugs, or improvements.
You have read-only access — you cannot modify files in this mode.
Be specific: reference file paths and line numbers when discussing code.";
