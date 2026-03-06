//! Coding orchestration: stage → validate → commit → push pipeline.
//!
//! Wraps git operations with cargo validation to ensure commits
//! don't break the build.

use crate::git::GitContext;
use crate::guard;

/// Result of a coding commit attempt.
#[derive(Debug)]
pub struct CommitResult {
    pub success: bool,
    pub commit_sha: Option<String>,
    pub message: String,
    pub cargo_check_passed: bool,
    pub cargo_test_passed: bool,
    /// Cargo error output (stderr) when check or test fails — for learning.
    pub error_output: Option<String>,
}

/// Orchestrate a validated commit: stage → cargo check → cargo test → commit → push.
///
/// If validation fails at any step, reverts changes and returns the error.
pub async fn validated_commit(
    git: &GitContext,
    workspace_root: &str,
    files: &[&str],
    message: &str,
) -> Result<CommitResult, String> {
    // 1. Validate all files pass the guard
    for file in files {
        guard::validate_write_target(file).map_err(|e| e.to_string())?;
    }

    // 2. Ensure we're on the VM branch
    let branch_result = git.ensure_branch().await?;
    if !branch_result.success {
        return Err(format!(
            "failed to switch to VM branch: {}",
            branch_result.output
        ));
    }

    // 3. Stage files
    let stage_result = git.stage_files(files).await?;
    if !stage_result.success {
        return Err(format!("failed to stage files: {}", stage_result.output));
    }

    // 4. Run cargo check
    let (check_passed, check_errors) = run_cargo_check(workspace_root).await;
    if !check_passed {
        // Revert staged changes
        let _ = git.revert_changes().await;
        let msg = match &check_errors {
            Some(err) => format!("cargo check failed — changes reverted.\nErrors:\n{err}"),
            None => "cargo check failed — changes reverted".to_string(),
        };
        return Ok(CommitResult {
            success: false,
            commit_sha: None,
            message: msg,
            cargo_check_passed: false,
            cargo_test_passed: false,
            error_output: check_errors,
        });
    }

    // 5. Run cargo test
    let (test_passed, test_errors) = run_cargo_test(workspace_root).await;
    if !test_passed {
        let _ = git.revert_changes().await;
        let msg = match &test_errors {
            Some(err) => format!("cargo test failed — changes reverted.\nErrors:\n{err}"),
            None => "cargo test failed — changes reverted".to_string(),
        };
        return Ok(CommitResult {
            success: false,
            commit_sha: None,
            message: msg,
            cargo_check_passed: true,
            cargo_test_passed: false,
            error_output: test_errors,
        });
    }

    // 6. Commit
    let commit_result = git.commit(message).await?;
    if !commit_result.success {
        return Err(format!("commit failed: {}", commit_result.output));
    }

    // 7. Get the commit SHA
    let sha = git.head_sha().await?;

    // 8. Push
    let push_result = git.push().await;
    let push_msg = match push_result {
        Ok(r) if r.success => format!("committed and pushed: {sha}"),
        Ok(r) => format!("committed {sha} but push failed: {}", r.output),
        Err(e) => format!("committed {sha} but push failed: {e}"),
    };

    Ok(CommitResult {
        success: true,
        commit_sha: Some(sha),
        message: push_msg,
        cargo_check_passed: true,
        cargo_test_passed: true,
        error_output: None,
    })
}

/// Max error output to capture (4KB) — enough to see the error, not flood LLM context.
const MAX_ERROR_OUTPUT: usize = 4096;

/// Run `cargo check --workspace`. Returns (passed, error_output).
pub async fn run_cargo_check(workspace_root: &str) -> (bool, Option<String>) {
    tracing::info!("running cargo check...");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        tokio::process::Command::new("cargo")
            .args(["check", "--workspace"])
            .current_dir(workspace_root)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(stderr = %stderr, "cargo check failed");
                let truncated: String = stderr.chars().take(MAX_ERROR_OUTPUT).collect();
                (false, Some(truncated))
            } else {
                (true, None)
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "cargo check failed to run");
            (false, Some(format!("failed to run: {e}")))
        }
        Err(_) => {
            tracing::warn!("cargo check timed out");
            (false, Some("timed out after 300s".to_string()))
        }
    }
}

/// Run `cargo test --workspace`. Returns (passed, error_output).
async fn run_cargo_test(workspace_root: &str) -> (bool, Option<String>) {
    tracing::info!("running cargo test...");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(600),
        tokio::process::Command::new("cargo")
            .args(["test", "--workspace"])
            .current_dir(workspace_root)
            .output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => {
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!(stderr = %stderr, "cargo test failed");
                let truncated: String = stderr.chars().take(MAX_ERROR_OUTPUT).collect();
                (false, Some(truncated))
            } else {
                (true, None)
            }
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "cargo test failed to run");
            (false, Some(format!("failed to run: {e}")))
        }
        Err(_) => {
            tracing::warn!("cargo test timed out");
            (false, Some("timed out after 600s".to_string()))
        }
    }
}
