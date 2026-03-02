//! World model: structured, queryable beliefs that persist across cycles.
//!
//! Instead of opaque thought strings the LLM must re-parse, beliefs are
//! structured facts the system can reason about. The LLM's job shifts from
//! "write a diary entry" to "update the model": create, update, confirm,
//! or invalidate beliefs.

use serde::{Deserialize, Serialize};

/// Domain categories for beliefs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeliefDomain {
    Node,
    Endpoints,
    Codebase,
    Strategy,
    #[serde(rename = "self")]
    Self_,
}

impl BeliefDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Endpoints => "endpoints",
            Self::Codebase => "codebase",
            Self::Strategy => "strategy",
            Self::Self_ => "self",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "node" => Some(Self::Node),
            "endpoints" => Some(Self::Endpoints),
            "codebase" => Some(Self::Codebase),
            "strategy" => Some(Self::Strategy),
            "self" => Some(Self::Self_),
            _ => None,
        }
    }

    /// All domain variants for iteration.
    pub fn all() -> &'static [BeliefDomain] {
        &[
            BeliefDomain::Node,
            BeliefDomain::Endpoints,
            BeliefDomain::Codebase,
            BeliefDomain::Strategy,
            BeliefDomain::Self_,
        ]
    }
}

/// Confidence level for a belief.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "high" => Self::High,
            "low" => Self::Low,
            _ => Self::Medium,
        }
    }
}

/// A structured belief in the world model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Belief {
    pub id: String,
    pub domain: BeliefDomain,
    /// What the belief is about (e.g. "echo", "main.rs", "self").
    pub subject: String,
    /// What aspect (e.g. "payment_count", "health_status", "plan").
    pub predicate: String,
    /// Current value (e.g. "0", "healthy", "stagnant").
    pub value: String,
    pub confidence: Confidence,
    /// Why we believe this.
    pub evidence: String,
    pub confirmation_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub active: bool,
}

/// Operations the LLM can perform on the world model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ModelUpdate {
    Create {
        domain: String,
        subject: String,
        predicate: String,
        value: String,
        #[serde(default)]
        evidence: String,
    },
    Update {
        id: String,
        value: String,
        #[serde(default)]
        evidence: String,
    },
    Confirm {
        id: String,
    },
    Invalidate {
        id: String,
        reason: String,
    },
}

/// Format the world model as a structured view for the LLM prompt.
pub fn format_world_model(beliefs: &[Belief]) -> String {
    if beliefs.is_empty() {
        return "No beliefs yet — observe and create beliefs about what you see.".to_string();
    }

    let mut sections = Vec::new();

    for domain in BeliefDomain::all() {
        let domain_beliefs: Vec<&Belief> = beliefs.iter().filter(|b| b.domain == *domain).collect();

        if domain_beliefs.is_empty() {
            continue;
        }

        let mut lines = vec![format!("### {}", domain.as_str())];
        for b in &domain_beliefs {
            let conf_marker = match b.confidence {
                Confidence::High => "",
                Confidence::Medium => " ?",
                Confidence::Low => " ??",
            };
            lines.push(format!(
                "- {}.{} = {}{} ({}x confirmed)",
                b.subject, b.predicate, b.value, conf_marker, b.confirmation_count
            ));
        }
        sections.push(lines.join("\n"));
    }

    sections.join("\n\n")
}

/// Format only beliefs that changed since a given timestamp.
pub fn format_changes_since(beliefs: &[Belief], since: i64) -> String {
    let changed: Vec<&Belief> = beliefs
        .iter()
        .filter(|b| b.updated_at > since || b.created_at > since)
        .collect();

    if changed.is_empty() {
        return "No belief changes since last cycle.".to_string();
    }

    changed
        .iter()
        .map(|b| {
            let action = if b.created_at > since { "NEW" } else { "UPD" };
            format!(
                "[{}] {}.{}.{} = {}",
                action,
                b.domain.as_str(),
                b.subject,
                b.predicate,
                b.value
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Find low-confidence beliefs or domains with gaps.
pub fn format_pending_questions(beliefs: &[Belief]) -> String {
    let low_conf: Vec<&Belief> = beliefs
        .iter()
        .filter(|b| b.confidence == Confidence::Low)
        .collect();

    if low_conf.is_empty() {
        return String::new();
    }

    let mut lines = Vec::new();
    for b in &low_conf {
        lines.push(format!(
            "- {}.{}.{} = {} (low confidence: {})",
            b.domain.as_str(),
            b.subject,
            b.predicate,
            b.value,
            b.evidence
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_belief_domain_roundtrip() {
        for domain in BeliefDomain::all() {
            let s = domain.as_str();
            let parsed = BeliefDomain::parse(s).unwrap();
            assert_eq!(*domain, parsed);
        }
    }

    #[test]
    fn test_confidence_roundtrip() {
        for conf in &[Confidence::High, Confidence::Medium, Confidence::Low] {
            let s = conf.as_str();
            let parsed = Confidence::parse(s);
            assert_eq!(*conf, parsed);
        }
    }

    #[test]
    fn test_model_update_deserialize() {
        let json = r#"[
            {"op": "create", "domain": "endpoints", "subject": "echo", "predicate": "payment_count", "value": "0", "evidence": "from snapshot"},
            {"op": "confirm", "id": "belief-123"},
            {"op": "update", "id": "belief-456", "value": "5", "evidence": "observed change"},
            {"op": "invalidate", "id": "belief-789", "reason": "endpoint removed"}
        ]"#;

        let updates: Vec<ModelUpdate> = serde_json::from_str(json).unwrap();
        assert_eq!(updates.len(), 4);
    }

    #[test]
    fn test_format_world_model() {
        let beliefs = vec![
            Belief {
                id: "b1".into(),
                domain: BeliefDomain::Node,
                subject: "self".into(),
                predicate: "endpoint_count".into(),
                value: "3".into(),
                confidence: Confidence::High,
                evidence: "snapshot".into(),
                confirmation_count: 5,
                created_at: 1000,
                updated_at: 2000,
                active: true,
            },
            Belief {
                id: "b2".into(),
                domain: BeliefDomain::Endpoints,
                subject: "echo".into(),
                predicate: "payment_count".into(),
                value: "0".into(),
                confidence: Confidence::Medium,
                evidence: "snapshot".into(),
                confirmation_count: 1,
                created_at: 1000,
                updated_at: 1000,
                active: true,
            },
        ];

        let view = format_world_model(&beliefs);
        assert!(view.contains("### node"));
        assert!(view.contains("self.endpoint_count = 3"));
        assert!(view.contains("echo.payment_count = 0 ?"));
    }

    #[test]
    fn test_format_changes_since() {
        let beliefs = vec![
            Belief {
                id: "b1".into(),
                domain: BeliefDomain::Node,
                subject: "self".into(),
                predicate: "uptime".into(),
                value: "10h".into(),
                confidence: Confidence::High,
                evidence: "".into(),
                confirmation_count: 1,
                created_at: 500,
                updated_at: 1500,
                active: true,
            },
            Belief {
                id: "b2".into(),
                domain: BeliefDomain::Endpoints,
                subject: "echo".into(),
                predicate: "count".into(),
                value: "0".into(),
                confidence: Confidence::High,
                evidence: "".into(),
                confirmation_count: 1,
                created_at: 500,
                updated_at: 500,
                active: true,
            },
        ];

        let changes = format_changes_since(&beliefs, 1000);
        assert!(changes.contains("[UPD]"));
        assert!(!changes.contains("echo")); // not changed since 1000
    }
}
