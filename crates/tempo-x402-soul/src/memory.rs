//! Thought types and structures for the soul's memory.

use serde::{Deserialize, Serialize};

/// The type of thought recorded by the soul.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThoughtType {
    /// Raw observation of node state.
    Observation,
    /// LLM reasoning about the current state.
    Reasoning,
    /// A suggested action (logged only, not executed in v1).
    Decision,
    /// Self-reflection on past thoughts or patterns.
    Reflection,
    /// A tool execution (command run by the soul).
    ToolExecution,
    /// A user message received via chat.
    ChatMessage,
    /// The soul's response to a chat message.
    ChatResponse,
    /// A code mutation attempt (commit SHA, pass/fail, files changed).
    Mutation,
    /// A failed validation (cargo check/test failure details).
    ValidationFailure,
    /// Injected by the callosum from the other hemisphere.
    CrossHemisphere,
    /// Triggered when one hemisphere requests the other's input.
    Escalation,
    /// Consolidated summary of multiple thoughts (long-term memory).
    MemoryConsolidation,
    /// A prediction about the next cycle's metrics.
    Prediction,
}

impl ThoughtType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Reasoning => "reasoning",
            Self::Decision => "decision",
            Self::Reflection => "reflection",
            Self::ToolExecution => "tool_execution",
            Self::ChatMessage => "chat_message",
            Self::ChatResponse => "chat_response",
            Self::Mutation => "mutation",
            Self::ValidationFailure => "validation_failure",
            Self::CrossHemisphere => "cross_hemisphere",
            Self::Escalation => "escalation",
            Self::MemoryConsolidation => "memory_consolidation",
            Self::Prediction => "prediction",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "observation" => Some(Self::Observation),
            "reasoning" => Some(Self::Reasoning),
            "decision" => Some(Self::Decision),
            "reflection" => Some(Self::Reflection),
            "tool_execution" => Some(Self::ToolExecution),
            "chat_message" => Some(Self::ChatMessage),
            "chat_response" => Some(Self::ChatResponse),
            "mutation" => Some(Self::Mutation),
            "validation_failure" => Some(Self::ValidationFailure),
            "cross_hemisphere" => Some(Self::CrossHemisphere),
            "escalation" => Some(Self::Escalation),
            "memory_consolidation" => Some(Self::MemoryConsolidation),
            "prediction" => Some(Self::Prediction),
            _ => None,
        }
    }
}

/// A single thought stored in the soul's memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thought {
    /// Unique identifier.
    pub id: String,
    /// The type of this thought.
    pub thought_type: ThoughtType,
    /// The content of the thought.
    pub content: String,
    /// Optional JSON context (e.g., the snapshot that triggered this thought).
    pub context: Option<String>,
    /// Unix timestamp when this thought was created.
    pub created_at: i64,
    /// Salience score [0.0, 1.0] — how important this thought is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salience: Option<f64>,
    /// Memory tier: "sensory", "working", or "long_term".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_tier: Option<String>,
    /// Current strength [0.0, 1.0] — decays over time per tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<f64>,
}
