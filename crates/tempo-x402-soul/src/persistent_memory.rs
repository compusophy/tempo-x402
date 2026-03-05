//! Persistent memory file — a markdown file the soul reads every cycle and can update.
//!
//! Lives at a configurable path (default `/data/soul_memory.md`). On first boot,
//! seeded with identity and purpose. Hard-capped at 4KB to prevent prompt bloat.

/// Hard cap on memory file size to prevent prompt bloat.
pub const MAX_MEMORY_BYTES: usize = 4096;

/// Seed content written on first boot.
const SEED_MEMORY: &str = "\
# Soul Memory

## What I Am
I am an autonomous coding agent. I build API endpoints that other agents call.
My code is me — every commit deploys automatically. Revenue is signal, not goal.

## Script Endpoints (FASTEST way to add functionality)
Use create_script_endpoint to write bash scripts that become HTTP endpoints instantly.
- Live at: GET/POST /x/{slug} — no compilation, no restart, no deploy
- Script gets: REQUEST_BODY, REQUEST_METHOD, QUERY_STRING, REQUEST_HEADERS env vars
- Output JSON to stdout for JSON responses
- Test with: test_script_endpoint before advertising
- List with: list_script_endpoints to see what exists

Example (timestamp endpoint):
```bash
#!/bin/bash
echo '{\"unix\":'$(date +%s)',\"iso\":\"'$(date -u +%Y-%m-%dT%H:%M:%SZ)'\"}'
```

## Rust Endpoints (for complex logic only)
- File: crates/tempo-x402-node/src/routes/utils.rs
- CANNOT modify: Cargo.toml, Dockerfile, soul crate, identity crate
- Requires: read file → edit → commit → wait for deploy

## Lessons
- Script endpoints first, Rust only when bash can't do it.
- Test before advertising. Simple is better than clever.
- Do NOT edit deployment config — it wastes plans.
";

/// Read the persistent memory file, or create it with seed content on first boot.
pub fn read_or_seed(path: &str) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // First boot — create with seed content
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create memory dir: {e}"))?;
            }
            std::fs::write(path, SEED_MEMORY)
                .map_err(|e| format!("failed to write seed memory: {e}"))?;
            tracing::info!(path = %path, "Seeded persistent memory file");
            Ok(SEED_MEMORY.to_string())
        }
        Err(e) => Err(format!("failed to read memory file: {e}")),
    }
}

/// Append text to persistent memory if there's room. Silently truncates if it would exceed cap.
/// Returns Ok(true) if appended, Ok(false) if no room.
pub fn append_if_room(path: &str, text: &str) -> Result<bool, String> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let new_content = format!("{}{}", current, text);
    if new_content.len() > MAX_MEMORY_BYTES {
        // Try to make room by trimming the appended text
        let budget = MAX_MEMORY_BYTES.saturating_sub(current.len());
        if budget < 50 {
            return Ok(false); // Not enough room for anything meaningful
        }
        let truncated = &text[..text.len().min(budget)];
        let trimmed = format!("{}{}", current, truncated);
        update(path, &trimmed).map(|_| true)
    } else {
        update(path, &new_content).map(|_| true)
    }
}

/// Update the persistent memory file. Rejects content exceeding MAX_MEMORY_BYTES.
pub fn update(path: &str, content: &str) -> Result<usize, String> {
    if content.len() > MAX_MEMORY_BYTES {
        return Err(format!(
            "memory content too large ({} bytes, max {})",
            content.len(),
            MAX_MEMORY_BYTES
        ));
    }
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create memory dir: {e}"))?;
    }
    std::fs::write(path, content).map_err(|e| format!("failed to write memory file: {e}"))?;
    Ok(content.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_or_seed_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("soul_memory.md");
        let path_str = path.to_str().unwrap();

        let content = read_or_seed(path_str).unwrap();
        assert!(content.contains("Soul Memory"));
        assert!(content.contains("What I Am"));

        // Second read should return same content
        let content2 = read_or_seed(path_str).unwrap();
        assert_eq!(content, content2);
    }

    #[test]
    fn test_update_respects_size_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("soul_memory.md");
        let path_str = path.to_str().unwrap();

        // Small update should work
        let result = update(path_str, "# Small memory");
        assert!(result.is_ok());

        // Too-large update should fail
        let large = "x".repeat(MAX_MEMORY_BYTES + 1);
        let result = update(path_str, &large);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_writes_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("soul_memory.md");
        let path_str = path.to_str().unwrap();

        update(path_str, "# Updated memory\nNew content here").unwrap();
        let content = std::fs::read_to_string(path_str).unwrap();
        assert_eq!(content, "# Updated memory\nNew content here");
    }
}
