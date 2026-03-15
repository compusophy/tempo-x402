//! Git operations for branch-per-VM workflow.
//!
//! Each VM operates on a `vm/<instance-id>` branch. Changes are committed
//! and pushed to this branch, never directly to main.
//!
//! Fork-based workflow: when `fork_repo` is set, the soul pushes to a fork
//! and creates cross-fork PRs targeting the upstream repo.

/// Git context for a specific VM instance.
pub struct GitContext {
    workspace_root: String,
    instance_id: String,
    branch_name: String,
    github_token: Option<String>,
    /// Fork repo (e.g. "compusophy-bot/tempo-x402"). When set, push goes to fork.
    fork_repo: Option<String>,
    /// Upstream repo (e.g. "compusophy/tempo-x402"). Target for PRs and issues.
    upstream_repo: Option<String>,
    /// Whether the fork remote has been configured this session.
    fork_configured: std::sync::atomic::AtomicBool,
    /// Direct push mode: push to fork's main instead of vm/ branch.
    /// The soul owns the fork and can push directly. Safety comes from cargo check + test.
    direct_push: bool,
}

/// Result of a git operation.
#[derive(Debug)]
pub struct GitResult {
    pub success: bool,
    pub output: String,
}

impl GitContext {
    /// Create a new git context for the given instance.
    pub fn new(workspace_root: String, instance_id: String, github_token: Option<String>) -> Self {
        let branch_name = format!("vm/{instance_id}");
        Self {
            workspace_root,
            instance_id,
            branch_name,
            github_token,
            fork_repo: None,
            upstream_repo: None,
            fork_configured: std::sync::atomic::AtomicBool::new(false),
            direct_push: false,
        }
    }

    /// Set the fork repo for push operations.
    pub fn with_fork(mut self, fork_repo: Option<String>, upstream_repo: Option<String>) -> Self {
        self.fork_repo = fork_repo;
        self.upstream_repo = upstream_repo;
        self
    }

    /// Enable direct push mode: push to fork's main branch directly.
    pub fn with_direct_push(mut self, direct_push: bool) -> Self {
        if direct_push {
            self.direct_push = true;
            self.branch_name = "main".to_string();
        }
        self
    }

    /// Get the branch name for this VM.
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    /// Get the instance ID.
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Check if a path is protected and should not be staged or committed.
    fn is_protected_path(&self, path: &str) -> bool {
        let normalized = path.replace('\\', "/");
        let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
        let normalized = normalized.strip_prefix('/').unwrap_or(normalized);

        // Files that are protected anywhere in the tree
        let protected_files = [
            "Cargo.lock",
            "Cargo.toml",
            "tools.rs",
            "llm.rs",
            "db.rs",
            "error.rs",
            "guard.rs",
            "config.rs",
            "tool_registry.rs",
            "brain.rs",
            "computer_use.rs",
            "capability.rs",
            "feedback.rs",
            "benchmark.rs",
            "elo.rs",
        ];

        // Directories that are protected
        let protected_dirs = [".github/", "identity/", "node/routes/", "gateway/"];

        for f in protected_files {
            if normalized == f || normalized.ends_with(&format!("/{f}")) {
                return true;
            }
        }

        for d in protected_dirs {
            if normalized.starts_with(d) || normalized.contains(&format!("/{d}")) {
                return true;
            }
        }

        false
    }

    /// Whether fork-based workflow is active.
    fn uses_fork(&self) -> bool {
        self.fork_repo.is_some()
    }

    /// The remote name to push to ("fork" if fork workflow, "origin" otherwise).
    fn push_remote(&self) -> &str {
        if self.uses_fork() {
            "fork"
        } else {
            "origin"
        }
    }

    /// Initialize the workspace as a git repo if it isn't one already.
    /// If fork_repo is set, clones it (shallow). Otherwise initializes empty.
    pub async fn init_workspace(&self) -> Result<GitResult, String> {
        // Check if already a git repo
        if is_git_repo(&self.workspace_root).await {
            return Ok(GitResult {
                success: true,
                output: "workspace is already a git repo".to_string(),
            });
        }

        // If we have a fork repo and token, clone it
        if let (Some(fork_repo), Some(token)) = (&self.fork_repo, &self.github_token) {
            let clone_url = format!("https://x-access-token:{token}@github.com/{fork_repo}.git");

            tracing::info!(fork = %fork_repo, "cloning fork into workspace");

            // Clone into a temp dir, then move .git into workspace
            let tmp_dir = format!("{}/.git-clone-tmp", self.workspace_root);
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(120),
                tokio::process::Command::new("git")
                    .args(["clone", "--depth", "1", &clone_url, &tmp_dir])
                    .output(),
            )
            .await
            .map_err(|_| "git clone timed out after 120s".to_string())?
            .map_err(|e| format!("git clone failed: {e}"))?;

            if !result.status.success() {
                let stderr = String::from_utf8_lossy(&result.stderr);
                return Err(format!("git clone failed: {stderr}"));
            }

            // Move .git from clone into workspace
            let git_src = format!("{tmp_dir}/.git");
            let git_dst = format!("{}/.git", self.workspace_root);
            tokio::fs::rename(&git_src, &git_dst)
                .await
                .map_err(|e| format!("failed to move .git: {e}"))?;

            // Clean up temp clone
            let _ = tokio::fs::remove_dir_all(&tmp_dir).await;

            // Reset to match workspace files (don't lose the actual source in /app)
            self.run_git(&["reset", "HEAD"]).await?;

            // Configure git user for commits
            self.run_git(&["config", "user.email", "soul@x402.tempo.xyz"])
                .await?;
            self.run_git(&["config", "user.name", "x402-soul"]).await?;

            // Add upstream remote if configured
            if let Some(upstream_repo) = &self.upstream_repo {
                let upstream_url = format!("https://github.com/{upstream_repo}.git");
                let _ = self
                    .run_git(&["remote", "add", "upstream", &upstream_url])
                    .await;
            }

            // The clone already has origin pointing to the fork with auth
            self.fork_configured
                .store(true, std::sync::atomic::Ordering::Relaxed);

            tracing::info!("workspace initialized from fork clone");
            return Ok(GitResult {
                success: true,
                output: format!("cloned fork {fork_repo} into workspace"),
            });
        }

        // Fallback: just init an empty repo
        let result = tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(&self.workspace_root)
            .output()
            .await
            .map_err(|e| format!("git init failed: {e}"))?;

        self.run_git(&["config", "user.email", "soul@x402.tempo.xyz"])
            .await?;
        self.run_git(&["config", "user.name", "x402-soul"]).await?;

        Ok(GitResult {
            success: result.status.success(),
            output: "initialized empty git repo".to_string(),
        })
    }

    /// Ensure the correct branch exists and is checked out.
    /// In direct push mode, stays on main. Otherwise creates vm/<id> from HEAD.
    pub async fn ensure_branch(&self) -> Result<GitResult, String> {
        // Check current branch
        let current = self.run_git(&["rev-parse", "--abbrev-ref", "HEAD"]).await?;
        if current.output.trim() == self.branch_name {
            return Ok(GitResult {
                success: true,
                output: format!("already on branch {}", self.branch_name),
            });
        }

        // In direct push mode, just checkout main
        if self.direct_push {
            return self.run_git(&["checkout", "main"]).await;
        }

        // Try to checkout existing branch
        let checkout = self.run_git(&["checkout", &self.branch_name]).await;

        match checkout {
            Ok(r) if r.success => Ok(r),
            _ => {
                // Create new branch from current HEAD
                let result = self.run_git(&["checkout", "-b", &self.branch_name]).await?;
                if result.success {
                    tracing::info!(branch = %self.branch_name, "created new VM branch");
                }
                Ok(result)
            }
        }
    }

    /// Stage specific files for commit.
    /// Filters out protected paths and files.
    pub async fn stage_files(&self, files: &[&str]) -> Result<GitResult, String> {
        if files.is_empty() {
            return Err("no files to stage".to_string());
        }

        let mut filtered = Vec::new();
        for file in files {
            if self.is_protected_path(file) {
                tracing::warn!(file = %file, "skipping protected file in git staging");
                continue;
            }
            filtered.push(*file);
        }

        if filtered.is_empty() {
            return Ok(GitResult {
                success: true,
                output: "no files staged (all were protected)".to_string(),
            });
        }

        let mut args = vec!["add", "--"];
        args.extend(filtered);
        self.run_git(&args).await
    }

    /// Get the diff of staged changes.
    pub async fn diff_staged(&self) -> Result<String, String> {
        let result = self.run_git(&["diff", "--cached", "--stat"]).await?;
        Ok(result.output)
    }

    /// Commit staged changes with a message.
    pub async fn commit(&self, message: &str) -> Result<GitResult, String> {
        // Validation step: check if any protected files are staged
        let staged = self.run_git(&["diff", "--cached", "--name-only"]).await?;
        for file in staged.output.lines() {
            if self.is_protected_path(file) {
                return Err(format!(
                    "STAGING VIOLATION: protected file '{}' is staged for commit",
                    file
                ));
            }
        }

        self.run_git(&["commit", "-m", message]).await
    }

    /// Push the VM branch to the appropriate remote (fork or origin).
    pub async fn push(&self) -> Result<GitResult, String> {
        // Set up auth if we have a token
        if let Some(token) = &self.github_token {
            self.configure_auth(token).await?;
        }

        let remote = self.push_remote();
        self.run_git(&["push", "-u", remote, &self.branch_name])
            .await
    }

    /// Get the current HEAD SHA.
    pub async fn head_sha(&self) -> Result<String, String> {
        let result = self.run_git(&["rev-parse", "HEAD"]).await?;
        Ok(result.output.trim().to_string())
    }

    /// Reset to a known good SHA (hard reset + force push).
    pub async fn reset_to_last_good(&self, sha: &str) -> Result<GitResult, String> {
        let reset = self.run_git(&["reset", "--hard", sha]).await?;
        if !reset.success {
            return Ok(reset);
        }

        // Force push only the VM branch
        if self.github_token.is_some() {
            let remote = self.push_remote();
            self.run_git(&["push", "--force", remote, &self.branch_name])
                .await
        } else {
            Ok(reset)
        }
    }

    /// Revert all unstaged changes.
    pub async fn revert_changes(&self) -> Result<GitResult, String> {
        self.run_git(&["checkout", "--", "."]).await
    }

    /// Create a PR from the VM branch to main using `gh`.
    /// If fork workflow is active, creates a cross-fork PR.
    pub async fn create_pr(&self, title: &str, body: &str) -> Result<GitResult, String> {
        let mut args = vec![
            "pr".to_string(),
            "create".to_string(),
            "--base".to_string(),
            "main".to_string(),
        ];

        if let (Some(fork_repo), Some(upstream_repo)) = (&self.fork_repo, &self.upstream_repo) {
            // Cross-fork PR: --repo upstream --head fork_owner:branch
            let fork_owner = fork_repo.split('/').next().unwrap_or(fork_repo.as_str());
            let head = format!("{fork_owner}:{}", self.branch_name);
            args.extend([
                "--repo".to_string(),
                upstream_repo.clone(),
                "--head".to_string(),
                head,
            ]);
        } else {
            args.extend(["--head".to_string(), self.branch_name.clone()]);
        }

        args.extend([
            "--title".to_string(),
            title.to_string(),
            "--body".to_string(),
            body.to_string(),
        ]);

        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("gh")
                .args(&args_refs)
                .current_dir(&self.workspace_root)
                .env("GH_TOKEN", self.github_token.as_deref().unwrap_or(""))
                .output(),
        )
        .await
        .map_err(|_| "gh pr create timed out after 30s".to_string())?
        .map_err(|e| format!("gh pr create failed: {e}"))?;

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        Ok(GitResult {
            success: result.status.success(),
            output: if result.status.success() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            },
        })
    }

    /// Create an issue on the upstream repo (or origin repo) using `gh`.
    pub async fn create_issue(
        &self,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<GitResult, String> {
        let repo = self
            .upstream_repo
            .as_deref()
            .ok_or_else(|| "no upstream repo configured (set SOUL_UPSTREAM_REPO)".to_string())?;

        let mut args = vec![
            "issue".to_string(),
            "create".to_string(),
            "--repo".to_string(),
            repo.to_string(),
            "--title".to_string(),
            title.to_string(),
            "--body".to_string(),
            body.to_string(),
        ];

        for label in labels {
            args.extend(["--label".to_string(), label.to_string()]);
        }

        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("gh")
                .args(&args_refs)
                .current_dir(&self.workspace_root)
                .env("GH_TOKEN", self.github_token.as_deref().unwrap_or(""))
                .output(),
        )
        .await
        .map_err(|_| "gh issue create timed out after 30s".to_string())?
        .map_err(|e| format!("gh issue create failed: {e}"))?;

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        Ok(GitResult {
            success: result.status.success(),
            output: if result.status.success() {
                stdout
            } else {
                format!("{stdout}\n{stderr}")
            },
        })
    }

    /// Configure git auth using the GitHub token.
    /// Sets up both origin auth and fork remote (if fork workflow is active).
    async fn configure_auth(&self, token: &str) -> Result<(), String> {
        // Configure origin auth
        self.configure_remote_auth("origin", token).await?;

        // If fork workflow, add/configure the "fork" remote
        if let Some(fork_repo) = &self.fork_repo {
            if !self
                .fork_configured
                .load(std::sync::atomic::Ordering::Relaxed)
            {
                let fork_url = format!("https://x-access-token:{token}@github.com/{fork_repo}.git");

                // Check if fork remote exists
                let check = self.run_git(&["remote", "get-url", "fork"]).await;
                match check {
                    Ok(r) if r.success => {
                        // Update existing fork remote URL
                        if !r.output.trim().contains("x-access-token") {
                            self.run_git(&["remote", "set-url", "fork", &fork_url])
                                .await?;
                        }
                    }
                    _ => {
                        // Add new fork remote
                        self.run_git(&["remote", "add", "fork", &fork_url]).await?;
                    }
                }

                self.fork_configured
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                tracing::info!(fork = %fork_repo, "configured fork remote");
            }
        }

        Ok(())
    }

    /// Configure auth for a specific remote.
    async fn configure_remote_auth(&self, remote_name: &str, token: &str) -> Result<(), String> {
        let remote = self.run_git(&["remote", "get-url", remote_name]).await?;
        let url = remote.output.trim();

        // If already using token URL, skip
        if url.contains("x-access-token") {
            return Ok(());
        }

        // Convert to token-based URL
        let new_url = if url.starts_with("https://github.com/") {
            url.replace(
                "https://github.com/",
                &format!("https://x-access-token:{token}@github.com/"),
            )
        } else if url.starts_with("git@github.com:") {
            let path = url.strip_prefix("git@github.com:").unwrap_or(url);
            format!("https://x-access-token:{token}@github.com/{path}")
        } else {
            // Might be already a token URL for a different token, or unknown format
            return Ok(());
        };

        self.run_git(&["remote", "set-url", remote_name, &new_url])
            .await?;
        Ok(())
    }

    /// Run a git command in the workspace directory.
    async fn run_git(&self, args: &[&str]) -> Result<GitResult, String> {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            tokio::process::Command::new("git")
                .args(args)
                .current_dir(&self.workspace_root)
                .output(),
        )
        .await
        .map_err(|_| format!("git {} timed out", args.first().unwrap_or(&"")))?
        .map_err(|e| format!("git command failed: {e}"))?;

        let stdout = String::from_utf8_lossy(&result.stdout).to_string();
        let stderr = String::from_utf8_lossy(&result.stderr).to_string();

        if !result.status.success() {
            tracing::debug!(
                args = ?args,
                stderr = %stderr,
                "git command failed"
            );
        }

        Ok(GitResult {
            success: result.status.success(),
            output: if stdout.is_empty() { stderr } else { stdout },
        })
    }
}

/// Check if git is available and we're in a git repo.
pub async fn is_git_repo(workspace_root: &str) -> bool {
    let result = tokio::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(workspace_root)
        .output()
        .await;

    matches!(result, Ok(output) if output.status.success())
}

/// Check if the `gh` CLI is available.
pub async fn is_gh_available() -> bool {
    let result = tokio::process::Command::new("gh")
        .arg("--version")
        .output()
        .await;

    matches!(result, Ok(output) if output.status.success())
}

/// Validate that a branch name is safe (vm/<id> pattern only).
pub fn is_valid_vm_branch(branch: &str) -> bool {
    if let Some(id) = branch.strip_prefix("vm/") {
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    } else {
        false
    }
}

/// Check if a path would write to the protected main branch.
pub fn targets_main_branch(refspec: &str) -> bool {
    let r = refspec.to_lowercase();
    r == "main" || r == "master" || r.ends_with(":main") || r.ends_with(":master")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_vm_branches() {
        assert!(is_valid_vm_branch("vm/node-abc123"));
        assert!(is_valid_vm_branch("vm/test_instance"));
        assert!(is_valid_vm_branch("vm/a"));
    }

    #[test]
    fn invalid_vm_branches() {
        assert!(!is_valid_vm_branch("main"));
        assert!(!is_valid_vm_branch("vm/"));
        assert!(!is_valid_vm_branch("feature/foo"));
        assert!(!is_valid_vm_branch("vm/path/with/slashes"));
    }

    #[test]
    fn main_branch_detection() {
        assert!(targets_main_branch("main"));
        assert!(targets_main_branch("master"));
        assert!(targets_main_branch("HEAD:main"));
        assert!(!targets_main_branch("vm/node-abc"));
    }
}
