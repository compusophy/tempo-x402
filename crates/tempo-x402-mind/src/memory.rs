//! Cross-hemisphere memory: working memory (ephemeral) and shared thought access.

use std::collections::VecDeque;
use std::sync::Mutex;

use x402_soul::memory::Thought;

/// In-memory ring buffer for a hemisphere's working memory (experiencing self).
/// Not persisted — ephemeral per hemisphere lifetime.
pub struct WorkingMemory {
    buffer: Mutex<VecDeque<Thought>>,
    capacity: usize,
}

impl WorkingMemory {
    /// Create a new working memory with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Push a thought into working memory. Oldest is evicted if at capacity.
    pub fn push(&self, thought: Thought) {
        if let Ok(mut buf) = self.buffer.lock() {
            if buf.len() >= self.capacity {
                buf.pop_front();
            }
            buf.push_back(thought);
        }
    }

    /// Get all thoughts in working memory, oldest first.
    pub fn thoughts(&self) -> Vec<Thought> {
        self.buffer
            .lock()
            .map(|buf| buf.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get the most recent N thoughts, newest first.
    pub fn recent(&self, n: usize) -> Vec<Thought> {
        self.buffer
            .lock()
            .map(|buf| buf.iter().rev().take(n).cloned().collect())
            .unwrap_or_default()
    }

    /// Current number of thoughts in working memory.
    pub fn len(&self) -> usize {
        self.buffer.lock().map(|buf| buf.len()).unwrap_or(0)
    }

    /// Whether working memory is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clear all thoughts from working memory.
    pub fn clear(&self) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use x402_soul::memory::ThoughtType;

    fn make_thought(id: &str, content: &str) -> Thought {
        Thought {
            id: id.to_string(),
            thought_type: ThoughtType::Reasoning,
            content: content.to_string(),
            context: None,
            created_at: 0,
            salience: None,
            memory_tier: None,
            strength: None,
        }
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let wm = WorkingMemory::new(3);
        wm.push(make_thought("1", "first"));
        wm.push(make_thought("2", "second"));
        wm.push(make_thought("3", "third"));
        assert_eq!(wm.len(), 3);

        // Fourth push should evict the first
        wm.push(make_thought("4", "fourth"));
        assert_eq!(wm.len(), 3);

        let thoughts = wm.thoughts();
        assert_eq!(thoughts[0].id, "2");
        assert_eq!(thoughts[2].id, "4");
    }

    #[test]
    fn test_recent_order() {
        let wm = WorkingMemory::new(5);
        wm.push(make_thought("1", "a"));
        wm.push(make_thought("2", "b"));
        wm.push(make_thought("3", "c"));

        let recent = wm.recent(2);
        assert_eq!(recent[0].id, "3"); // newest first
        assert_eq!(recent[1].id, "2");
    }
}
