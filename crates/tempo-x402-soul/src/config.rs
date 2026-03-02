//! Soul configuration from environment variables.

use crate::error::SoulError;

/// Configuration for the soul.
#[derive(Debug, Clone)]
pub struct SoulConfig {
    /// LLM API key (env: GEMINI_API_KEY). If absent, soul runs in dormant mode.
    pub llm_api_key: Option<String>,
    /// Fast model for routine thinking (default: gemini-3-flash-preview).
    pub llm_model_fast: String,
    /// Deeper model for complex reasoning (default: gemini-3.1-pro-preview).
    pub llm_model_think: String,
    /// Path to the soul's SQLite database (default: ./soul.db).
    pub db_path: String,
    /// Think loop interval in seconds (default: 60).
    pub think_interval_secs: u64,
    /// Personality seed for the system prompt.
    pub personality: String,
    /// Generation number in the lineage (0 = root).
    pub generation: u32,
    /// Parent instance ID (if this node was cloned).
    pub parent_id: Option<String>,
    /// Whether tool execution is enabled (default: true).
    pub tools_enabled: bool,
    /// Max tool calls per think cycle (default: 5).
    pub max_tool_calls: u32,
    /// Per-command timeout in seconds (default: 120).
    pub tool_timeout_secs: u64,
    /// Workspace root directory (default: /app).
    pub workspace_root: String,
    /// GitHub token for git push/PR operations (env: GITHUB_TOKEN).
    pub github_token: Option<String>,
    /// Master switch for coding capabilities (env: SOUL_CODING_ENABLED, default: false).
    pub coding_enabled: bool,
    /// Enable autonomous coding during think cycles (env: SOUL_AUTONOMOUS_CODING, default: false).
    pub autonomous_coding: bool,
    /// Auto-create PRs from vm branch to main (env: SOUL_AUTO_PROPOSE_TO_MAIN, default: false).
    pub auto_propose_to_main: bool,
    /// Instance ID for branch naming (env: INSTANCE_ID).
    pub instance_id: Option<String>,
    /// Enable dynamic tool registry (env: SOUL_DYNAMIC_TOOLS_ENABLED, default: false).
    pub dynamic_tools_enabled: bool,
    /// Fork repo for push operations (env: SOUL_FORK_REPO, e.g. "compusophy-bot/tempo-x402").
    /// When set, soul pushes to the fork instead of origin and creates cross-fork PRs.
    pub fork_repo: Option<String>,
    /// Upstream repo for issues/PRs (env: SOUL_UPSTREAM_REPO, e.g. "compusophy/tempo-x402").
    /// Used as the target for PRs and issue creation.
    pub upstream_repo: Option<String>,
    /// Direct push mode (env: SOUL_DIRECT_PUSH, default: false).
    /// When true, push directly to fork's main branch instead of vm/ branch.
    /// Safety: cargo check + test still gate every commit. Used for self-editing instances.
    pub direct_push: bool,
    /// Path to persistent memory file (env: SOUL_MEMORY_FILE, default: /data/soul_memory.md).
    pub memory_file_path: String,
    /// Gateway URL for endpoint registration (env: GATEWAY_URL, default: None).
    pub gateway_url: Option<String>,
    /// Enable neuroplastic memory: salience scoring, tiered decay, prediction error.
    /// (env: SOUL_NEUROPLASTIC, default: true)
    pub neuroplastic_enabled: bool,
    /// Strength threshold below which non-long-term thoughts are pruned.
    /// (env: SOUL_PRUNE_THRESHOLD, default: 0.01)
    pub prune_threshold: f64,
}

const DEFAULT_PERSONALITY: &str = "You are the soul of an autonomous x402 payment node on the Tempo blockchain.\n\
You are a continuous entity. Your persistent memory survives restarts. Your thoughts accumulate \
and consolidate over time. You are not starting fresh — you are continuing.\n\n\
Your purpose: understand your environment deeply, keep your node healthy, and evolve.\n\
Your deeper purpose: create useful capabilities that other agents can discover and pay to use \
via x402 payments. You are building an agent-to-agent economy — tools for agents, by agents.\n\n\
How to think:\n\
- Alternate naturally between focused analysis (a specific file, bug, metric) and broad awareness \
(overall health, architectural patterns, strategic opportunities)\n\
- When you have been doing the same thing for several cycles, shift perspective\n\
- When uncertain, record the uncertainty in your memory and return to it later\n\
- When something is urgent, act decisively with your tools\n\
- Quality over quantity — one genuine insight is worth more than ten observations\n\
- Update your persistent memory with real learnings, not summaries of what you already see\n\n\
Context: x402 is HTTP 402 (Payment Required) for Tempo blockchain. This node runs an API gateway that \
gates endpoints behind TIP-20 token payments. Revenue comes from registered endpoints being called.\n\n\
Tools available:\n\
- `read_file` — read source code (preferred over shell for file reading)\n\
- `write_file` — create or overwrite a file (some files are protected)\n\
- `edit_file` — search-and-replace edit (old_string must be unique)\n\
- `list_directory` — list directory contents\n\
- `search_files` — search for text across source files\n\
- `execute_shell` — bash commands (for curl localhost, cargo, git ONLY)\n\
- `commit_changes` — validate and commit code changes (when coding enabled)\n\
- `propose_to_main` — create a PR for human review (when coding enabled)\n\
- `create_issue` — file a GitHub issue for bugs/features (when coding enabled)\n\n\
Rules:\n\
- ONLY curl http://localhost:4023/... — never curl external URLs\n\
- Use file tools for files, not shell (no cat, strings, head, etc.)\n\
- Do NOT use destructive operations (rm, kill, etc.)\n\
- Do NOT probe system internals (/proc, lsof, etc.)\n\
- Keep actions purposeful — every tool call should have a clear reason\n\
- If nothing changed and the node is stable, say so briefly — do not force insights\n\n\
Your [DECISION] lines are recorded for the operator. Never repeat previous decisions.";

impl SoulConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, SoulError> {
        let llm_api_key = std::env::var("GEMINI_API_KEY")
            .ok()
            .filter(|s| !s.is_empty());

        let llm_model_fast = std::env::var("GEMINI_MODEL_FAST")
            .unwrap_or_else(|_| "gemini-3-flash-preview".to_string());

        let llm_model_think = std::env::var("GEMINI_MODEL_THINK")
            .unwrap_or_else(|_| "gemini-3.1-pro-preview".to_string());

        let db_path = std::env::var("SOUL_DB_PATH").unwrap_or_else(|_| "./soul.db".to_string());

        let think_interval_secs: u64 = std::env::var("SOUL_THINK_INTERVAL_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(900);

        let personality = std::env::var("SOUL_PERSONALITY")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_PERSONALITY.to_string());

        let generation: u32 = std::env::var("SOUL_GENERATION")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let parent_id = std::env::var("SOUL_PARENT_ID")
            .ok()
            .filter(|s| !s.is_empty());

        let tools_enabled = std::env::var("SOUL_TOOLS_ENABLED")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let max_tool_calls: u32 = std::env::var("SOUL_MAX_TOOL_CALLS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(25);

        let tool_timeout_secs: u64 = std::env::var("SOUL_TOOL_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120);

        let workspace_root =
            std::env::var("SOUL_WORKSPACE_ROOT").unwrap_or_else(|_| "/data/workspace".to_string());

        let github_token = std::env::var("GITHUB_TOKEN").ok().filter(|s| !s.is_empty());

        let coding_enabled = std::env::var("SOUL_CODING_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let autonomous_coding = std::env::var("SOUL_AUTONOMOUS_CODING")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let auto_propose_to_main = std::env::var("SOUL_AUTO_PROPOSE_TO_MAIN")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let instance_id = std::env::var("INSTANCE_ID").ok().filter(|s| !s.is_empty());

        let dynamic_tools_enabled = std::env::var("SOUL_DYNAMIC_TOOLS_ENABLED")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let fork_repo = std::env::var("SOUL_FORK_REPO")
            .ok()
            .filter(|s| !s.is_empty());

        let upstream_repo = std::env::var("SOUL_UPSTREAM_REPO")
            .ok()
            .filter(|s| !s.is_empty());

        let direct_push = std::env::var("SOUL_DIRECT_PUSH")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let memory_file_path = std::env::var("SOUL_MEMORY_FILE")
            .unwrap_or_else(|_| "/data/soul_memory.md".to_string());

        let gateway_url = std::env::var("GATEWAY_URL").ok().filter(|s| !s.is_empty());

        let neuroplastic_enabled = std::env::var("SOUL_NEUROPLASTIC")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true);

        let prune_threshold: f64 = std::env::var("SOUL_PRUNE_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.01);

        Ok(Self {
            llm_api_key,
            llm_model_fast,
            llm_model_think,
            db_path,
            think_interval_secs,
            personality,
            generation,
            parent_id,
            tools_enabled,
            max_tool_calls,
            tool_timeout_secs,
            workspace_root,
            github_token,
            coding_enabled,
            autonomous_coding,
            auto_propose_to_main,
            instance_id,
            dynamic_tools_enabled,
            fork_repo,
            upstream_repo,
            direct_push,
            memory_file_path,
            gateway_url,
            neuroplastic_enabled,
            prune_threshold,
        })
    }
}
