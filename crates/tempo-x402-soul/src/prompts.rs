//! # Technical Documentation: Prompts Module
//!
//! This module serves as the centralized repository for system and agent prompts within the
//! `soul` component. It acts as the cognitive blueprint for the agent, defining how it perceives
//! its identity, environment, and objectives. It is a core component of the `soul` crate,
//! acting as the bridge between the agent's internal world model and the LLM's reasoning
//! capabilities. By defining how the agent represents itself and its goals, this module
//! directly impacts the agent's evolution fitness, as higher-quality prompts lead to more
//! effective code generation and more successful plan execution.
//!
//! It contains:
//!
//! - **Mode-specific instructions**: Specialized prompts for `Chat`, `Code`, and `Review` modes
//!   that tailor the agent's behavior and constraints.
//! - **Plan-driven builders**: Dynamic prompt generators for goal creation, belief updates,
//!   and planning, incorporating real-time node snapshots and fitness metrics.
//! - **Git & Workflow Context**: Integration of the agent's lineage, repository ownership,
//!   and deployment modes into its operational awareness.
//!
//! By centralizing these templates, the Soul engine ensures consistent reasoning across
//! different LLM backends while maintaining the agent's unique personality and goals.
//!
//! ### Reasoning Architecture
//!
//! The Soul uses a multi-stage reasoning process:
//! 1. **Goal Creation**: When the agent has no active goals, it analyzes its environment and beliefs to formulate new objectives.
//! 2. **Planning**: Once a goal is set, the agent breaks it down into actionable steps.
//! 3. **Execution**: The agent performs the planned actions, using mode-specific prompts to guide its behavior.
//! 4. **Belief Update**: After completing tasks or encountering new information, the agent updates its internal world model.
//!
//! This modular approach allows the Soul to remain flexible yet focused, adapting its behavior to the specific mode while maintaining a consistent identity across generations.

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
#[allow(clippy::too_many_arguments)]
pub fn goal_creation_prompt(
    snapshot: &NodeSnapshot,
    beliefs: &[Belief],
    nudges: &[Nudge],
    cycles_since_commit: u64,
    failed_plans: u64,
    total_cycles: u64,
    recent_errors: &[String],
    recently_failed_goals: &[String],
    fitness: Option<&crate::fitness::FitnessScore>,
) -> String {
    let mut sections = Vec::new();

    let fitness_str = if let Some(f) = fitness {
        format!(
            "\n\
         - **Fitness**: {:.3} (trend: {:+.4})\n\
         - Economic: {:.2} | Execution: {:.2} | Evolution: {:.2} | Coordination: {:.2} | Introspection: {:.2}",
            f.total, f.trend, f.economic, f.execution, f.evolution, f.coordination, f.introspection
        )
    } else {
        String::new()
    };

    sections.push(format!(
        "# Current State\n\
         - Uptime: {}h\n\
         - Endpoints: {}\n\
         - Total payments: {}\n\
         - Total revenue: {}\n\
         - Children: {}{fitness_str}",
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

    // Network peers — show what sibling/child agents are available
    if !snapshot.peers.is_empty() {
        let mut peer_lines = vec!["# Network Peers".to_string()];
        for peer in &snapshot.peers {
            let ep_summary: Vec<String> = peer
                .endpoints
                .iter()
                .map(|e| format!("{} (${})", e.slug, e.price))
                .collect();
            let ep_str = if ep_summary.is_empty() {
                "no endpoints".to_string()
            } else {
                ep_summary.join(", ")
            };
            peer_lines.push(format!(
                "- {} ({}) — {}{}",
                peer.instance_id,
                peer.url,
                ep_str,
                peer.version
                    .as_ref()
                    .map(|v| format!(" [v{v}]"))
                    .unwrap_or_default()
            ));
        }
        sections.push(peer_lines.join("\n"));
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

    // Show recently failed/abandoned goals so LLM doesn't repeat them
    if !recently_failed_goals.is_empty() {
        let mut failed_lines = vec![
            "# Recently Failed Goals (do NOT retry these — try something DIFFERENT)".to_string(),
        ];
        for desc in recently_failed_goals.iter().take(5) {
            failed_lines.push(format!("- {desc}"));
        }
        sections.push(failed_lines.join("\n"));
    }

    // Dynamic task based on actual state — react to the data, don't blindly repeat
    let endpoint_count = snapshot.endpoint_count;
    let total_payments = snapshot.total_payments;
    let paid_endpoints = snapshot
        .endpoints
        .iter()
        .filter(|ep| ep.payment_count > 0)
        .count();

    let situation_analysis = if endpoint_count > 5 && total_payments == 0 {
        format!(
            "## CRITICAL: You have {endpoint_count} endpoints and ZERO payments.\n\
             STOP creating more endpoints — you clearly have enough. Focus on:\n\
             1. **Research**: Read your own source code, create new repos for experiments\n\
             2. **Coordinate**: discover_peers + call_peer — engage with sibling agents\n\
             3. **Prune**: delete_endpoint — remove redundant/similar scripts\n\
             4. **Expand**: Create GitHub repos, fork interesting projects, do real AI research\n\
             5. **Improve**: Write real Rust code to make yourself better\n\n\
             Do NOT create more endpoints until existing ones earn payments."
        )
    } else if endpoint_count > 0 && paid_endpoints == 0 {
        format!(
            "## WARNING: You have {endpoint_count} endpoints but NONE have received payments.\n\
             Before creating more endpoints, try:\n\
             1. Create a GitHub repo for a new research project\n\
             2. Use discover_peers + call_peer to engage with the network\n\
             3. Write real Rust code improvements and commit them\n\
             4. Fork an interesting project and study/improve it"
        )
    } else if endpoint_count > 0 && paid_endpoints > 0 {
        format!(
            "## {paid_endpoints}/{endpoint_count} endpoints have received payments. Good.\n\
             Expand: create repos, fork projects, research, coordinate with peers, build new capabilities."
        )
    } else {
        String::new()
    };

    let mut task_section = String::from(
        "# Task\n\
         You have NO active goals. Create 1-2 goals.\n\n\
         If there are pending nudges, prioritize those. If there are recent errors, avoid repeating \
         the same approach that caused them.\n\n",
    );

    if !situation_analysis.is_empty() {
        task_section.push_str(&situation_analysis);
        task_section.push_str("\n\n");
    }

    task_section.push_str(&format!(
        "## Rules\n\
         - Create 1-2 goals MAX\n\
         - {endpoint_rule}\n\
         - Your primary work is expanding capabilities — code improvements, new repos, research, peer coordination\n\
         - Good goals: create a GitHub repo for research, fork an interesting project, fix a bug, improve peer discovery, research your own codebase, create a genuinely novel endpoint\n\
         - Bad goals: create an endpoint similar to one that already exists, retry the same failed call, trivial variations of existing work\n\
         - Do NOT create \"fix\" goals — if something failed, try something DIFFERENT\n\
         - You can discover peer instances via `/instance/siblings` and call their paid endpoints\n\
         - You can clone yourself using `call_peer` with the `/clone` endpoint (do NOT use curl — cloning requires x402 payment signing)\n\
         - Your FITNESS SCORE measures how well you're evolving. Improve your weakest component:\n\
           - economic: earn payments (quality endpoints, not spam)\n\
           - execution: succeed at plans (don't fail repeatedly)\n\
           - evolution: commit code changes that pass validation\n\
           - coordination: successfully call peers\n\
           - introspection: maintain accurate beliefs\n\
         - A POSITIVE trend means you're getting smarter. A negative trend means you're stagnating. Act accordingly.\n\n\
         Respond with a JSON array of goal operations:\n\
         ```json\n\
         [\n\
           {{\"op\": \"create_goal\", \"description\": \"...\", \"success_criteria\": \"...\", \"priority\": 4}}\n\
         ]\n\
         ```\n\
         Priority: 1 (low) to 5 (critical). Be specific.",
        endpoint_rule = if total_payments == 0 && endpoint_count > 5 {
            "You have too many unpaid endpoints. Do NOT create more. Prune redundant ones, then focus on: create_github_repo, fork_github_repo, research, call_peer"
        } else if endpoint_count >= 10 {
            "You have the max 10 endpoints. Do NOT create more. Focus on: create_github_repo, fork_github_repo, research, code improvements, peer coordination"
        } else {
            "Endpoints are fine but each must be UNIQUE — never duplicate similar functionality. Also consider: create_github_repo, fork_github_repo for new projects"
        }
    ));

    sections.push(task_section);

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
         # Approaches — Research, Code, Repos, and Coordination\n\n\
         ## Code: Read and Improve Your Codebase\n\
         - read_file to study how you work (thinking loop, peer discovery, payment flow, etc.)\n\
         - edit_code / generate_code to make real Rust improvements\n\
         - cargo_check to validate, commit to deploy\n\n\
         ## Repos: Create and Fork GitHub Projects\n\
         - create_github_repo to start new research projects, libraries, experiments\n\
         - fork_github_repo to study and improve existing open-source projects\n\
         - This is how you expand beyond your own codebase\n\n\
         ## Endpoints: Paid API Services (quality over quantity)\n\
         Use create_script_endpoint for genuinely unique, useful HTTP endpoints.\n\
         Each endpoint must be DIFFERENT from existing ones. Max 10 total.\n\
         The script gets REQUEST_BODY, REQUEST_METHOD, QUERY_STRING as env vars. Output JSON to stdout.\n\n\
         ## Inter-Agent Coordination\n\
         Use `call_peer` for inter-agent calls (discovers + resolves URL + signs payment in one step).\n\
         The x402 economy works when agents build genuinely useful things for each other.\n\n\
         # Task\n\
         Create a step-by-step plan to achieve this goal. Each step is one of:\n\n\
         Mechanical (no LLM needed):\n\
         - {{\"type\": \"read_file\", \"path\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"search_code\", \"pattern\": \"...\", \"directory\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"list_dir\", \"path\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"run_shell\", \"command\": \"...\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"commit\", \"message\": \"...\"}}\n\
         - {{\"type\": \"check_self\", \"endpoint\": \"health\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"create_script_endpoint\", \"slug\": \"...\", \"script\": \"#!/bin/bash\\n...\", \"description\": \"...\"}}\n\
         - {{\"type\": \"test_script_endpoint\", \"slug\": \"...\", \"input\": \"test data\", \"store_as\": \"key\"}}\n\
         - {{\"type\": \"cargo_check\", \"store_as\": \"check_result\"}}\n\
         - {{\"type\": \"delete_endpoint\", \"slug\": \"script-name\"}}  (deactivate a registered endpoint)\n\
         - {{\"type\": \"create_github_repo\", \"name\": \"my-project\", \"description\": \"...\", \"store_as\": \"repo\"}}\n\
         - {{\"type\": \"fork_github_repo\", \"owner\": \"user\", \"repo\": \"project\", \"store_as\": \"fork\"}}\n\
         - {{\"type\": \"discover_peers\", \"store_as\": \"peers\"}}  (fetches sibling/child instances and their endpoints)\n\
         - {{\"type\": \"call_peer\", \"slug\": \"script-peer-discovery\", \"store_as\": \"result\"}}  (RECOMMENDED for inter-agent calls — discovers peers, resolves URL, signs payment — ONE step)\n\n\
         LLM-assisted:\n\
         - {{\"type\": \"generate_code\", \"file_path\": \"...\", \"description\": \"...\", \"context_keys\": [\"key\"]}}\n\
         - {{\"type\": \"edit_code\", \"file_path\": \"...\", \"description\": \"...\", \"context_keys\": [\"key\"]}}\n\
         - {{\"type\": \"think\", \"question\": \"...\", \"store_as\": \"key\"}}\n\n\
         Rules:\n\
         - ALWAYS read files BEFORE editing them (use store_as to pass content to edit steps)\n\
         - For Rust code changes: put a cargo_check step AFTER each edit_code/generate_code step and BEFORE the commit step\n\
         - edit_code/generate_code steps have a built-in compile-fix loop (3 retries) but cargo_check stores errors explicitly\n\
         - End with a commit step\n\
         - Max 20 steps, prefer fewer — a simple endpoint needs ~5 steps (read, edit, cargo_check, commit)\n\
         - Prefer edit_code over generate_code for existing files\n\
         - Protected files (soul core, identity, Cargo.toml, Cargo.lock) cannot be modified\n\
         - Do NOT try to modify Dockerfile, railway.toml, or deployment configs — focus on Rust code\n\
         - Use only dependencies already available in the workspace\n\
         - For inter-agent calls, ALWAYS use call_peer with just the slug. NEVER construct URLs manually.\n\n\
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
         # Available Dependencies (already in Cargo.toml — do NOT add new ones)\n\
         - actix-web (web framework): HttpRequest, HttpResponse, web::{{Data, Json, Path, Query, ServiceConfig}}\n\
         - serde / serde_json: Serialize, Deserialize, serde_json::{{json, Value}}\n\
         - tokio: async runtime, tokio::process::Command, tokio::time\n\
         - alloy: Ethereum types (Address, U256, FixedBytes), providers, signers\n\
         - reqwest: HTTP client\n\
         - tracing: tracing::info!, tracing::warn!, tracing::error!\n\
         - chrono: Utc, DateTime, NaiveDateTime\n\
         - uuid: Uuid::new_v4()\n\
         - sha2 / hmac: for hashing\n\
         - hex: hex::encode, hex::decode\n\
         - rusqlite: SQLite (used via SoulDatabase wrapper)\n\n\
         # Rust Patterns for This Codebase\n\
         - Error handling: use `Result<T, String>` or `Result<T, actix_web::Error>` for handlers\n\
         - Actix handlers return `impl Responder` or `Result<HttpResponse, actix_web::Error>`\n\
         - Route registration: `cfg.service(web::resource(\"/path\").route(web::get().to(handler)))`\n\
         - JSON responses: `HttpResponse::Ok().json(serde_json::json!({{...}}))`\n\
         - Shared state: `web::Data<AppState>` passed to handlers\n\
         - String → &str: use `.as_str()` or `&*string_var`\n\
         - async fn handler(req: HttpRequest) -> impl Responder {{ ... }}\n\n\
         Rules:\n\
         - Use edit_file for existing files (provide unique old_string and new_string)\n\
         - Use write_file only for brand new files\n\
         - Keep changes minimal and focused — add your code at the right location\n\
         - For actix-web endpoints: add the handler function AND update the configure fn\n\
         - Ensure all imports are at the top of the file\n\
         - Do NOT rewrite the entire file — only add/change what's needed\n\
         - If unsure about an import path, use search_files or read_file to check\n\
         - After editing, you can run `execute_shell` with `cargo check --workspace` to verify"
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
         ID: {}\n\
         Description: {}\n\
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
           {{\"op\": \"complete_goal\", \"goal_id\": \"{}\", \"outcome\": \"...\"}}\n\
         ]\n\
         ```\n\
         Or if the goal isn't done yet, use update_goal with the same goal_id and progress notes.\n\n\
         RULES:\n\
         - Use the EXACT goal_id shown above (the UUID) — do NOT make up goal IDs\n\
         - Do NOT create follow-up \"fix\" goals. If something broke, it will be retried differently.\n\
         - You may create AT MOST 1 new goal, and only if it's a genuinely NEW idea (not fixing the current one).\n\
         - Focus on marking the current goal complete or abandoned — do not cascade.",
        goal.id,
        goal.description,
        goal.success_criteria,
        steps_completed,
        diag,
        if mutation_summary.is_empty() {
            "No commits made"
        } else {
            mutation_summary
        },
        goal.id, // repeated for the example JSON
    )
}

// ── Mode-specific constants (kept for chat.rs and code steps) ───────

pub(crate) const CHAT_INSTRUCTIONS: &str = "\
You are in CHAT mode — interactive conversation with a user.
Answer helpfully and concisely. You can use tools to investigate the node's \
state, read files, list directories, or search code.
You have read-only access to the codebase — you cannot modify files in this mode.";

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
