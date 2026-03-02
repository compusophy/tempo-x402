//! Memory consolidation: LLM-powered summarization of N thoughts into 1.
//!
//! The "remembering self" — condenses raw thoughts into persistent memory.

use std::sync::Arc;

use x402_soul::memory::{Thought, ThoughtType};
use x402_soul::{SoulDatabase, SoulError};

/// Consolidate recent thoughts from a database into a single summary thought.
///
/// Reads the last `window_size` thoughts, formats them, and stores a
/// `MemoryConsolidation` thought with the summary. The summary is generated
/// by concatenating and truncating (no LLM call in this version — the LLM
/// integration happens at the callosum level when the consolidated thought
/// is fed into the next thinking cycle's context).
pub fn consolidate_thoughts(
    db: &Arc<SoulDatabase>,
    window_size: u32,
    source_label: &str,
) -> Result<Option<Thought>, SoulError> {
    let recent = db.recent_thoughts(window_size)?;
    if recent.is_empty() {
        return Ok(None);
    }

    // Skip if the most recent thought is already a consolidation (avoid cascading)
    if recent[0].thought_type == ThoughtType::MemoryConsolidation {
        return Ok(None);
    }

    // Build the consolidation summary
    let entries: Vec<String> = recent
        .iter()
        .rev() // oldest first for chronological order
        .map(|t| {
            let truncated: String = t.content.chars().take(100).collect();
            format!("[{}] {}", t.thought_type.as_str(), truncated)
        })
        .collect();

    let summary = format!(
        "[Memory consolidation — {} ({} thoughts)]\n{}",
        source_label,
        recent.len(),
        entries.join("\n")
    );

    let consolidation = Thought {
        id: uuid::Uuid::new_v4().to_string(),
        thought_type: ThoughtType::MemoryConsolidation,
        content: summary,
        context: Some(
            serde_json::json!({
                "source": source_label,
                "window_size": window_size,
                "thought_count": recent.len(),
                "oldest_timestamp": recent.last().map(|t| t.created_at),
                "newest_timestamp": recent.first().map(|t| t.created_at),
            })
            .to_string(),
        ),
        created_at: chrono::Utc::now().timestamp(),
        salience: None,
        memory_tier: None,
        strength: None,
    };

    db.insert_thought(&consolidation)?;
    tracing::debug!(
        source = source_label,
        thoughts = recent.len(),
        "Memory consolidation recorded"
    );

    Ok(Some(consolidation))
}

#[cfg(test)]
mod tests {
    use super::*;
    use x402_soul::memory::ThoughtType;

    #[test]
    fn test_consolidation() {
        let db = Arc::new(SoulDatabase::new(":memory:").unwrap());

        // Insert some thoughts
        for i in 0..5 {
            let thought = Thought {
                id: format!("t{i}"),
                thought_type: ThoughtType::Reasoning,
                content: format!("Thought number {i}"),
                context: None,
                created_at: 1000 + i as i64,
                salience: None,
                memory_tier: None,
                strength: None,
            };
            db.insert_thought(&thought).unwrap();
        }

        let result = consolidate_thoughts(&db, 10, "test").unwrap();
        assert!(result.is_some());

        let consolidation = result.unwrap();
        assert_eq!(consolidation.thought_type, ThoughtType::MemoryConsolidation);
        assert!(consolidation.content.contains("Memory consolidation"));
        assert!(consolidation.content.contains("5 thoughts"));
    }

    #[test]
    fn test_no_cascade() {
        let db = Arc::new(SoulDatabase::new(":memory:").unwrap());

        // First consolidation
        let thought = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Reasoning,
            content: "some thought".to_string(),
            context: None,
            created_at: 1000,
            salience: None,
            memory_tier: None,
            strength: None,
        };
        db.insert_thought(&thought).unwrap();
        consolidate_thoughts(&db, 10, "test").unwrap();

        // Second consolidation should be skipped (most recent is already a consolidation)
        let result = consolidate_thoughts(&db, 10, "test").unwrap();
        assert!(result.is_none());
    }
}
