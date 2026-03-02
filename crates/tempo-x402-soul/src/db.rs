//! Soul database: separate SQLite for thoughts and state.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::SoulError;
use crate::memory::{Thought, ThoughtType};

/// A dynamically registered tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTool {
    pub name: String,
    pub description: String,
    /// JSON schema for parameters.
    pub parameters: String,
    /// "shell_command" or "shell_script"
    pub handler_type: String,
    /// The command or script path.
    pub handler_config: String,
    pub enabled: bool,
    /// JSON array of mode tags, e.g. ["code","chat"]
    pub mode_tags: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A recorded code mutation attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mutation {
    pub id: String,
    pub commit_sha: Option<String>,
    pub branch: String,
    pub description: String,
    /// JSON array of file paths.
    pub files_changed: String,
    pub cargo_check_passed: bool,
    pub cargo_test_passed: bool,
    pub created_at: i64,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS thoughts (
    id TEXT PRIMARY KEY,
    thought_type TEXT NOT NULL,
    content TEXT NOT NULL,
    context TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_thoughts_type ON thoughts(thought_type);
CREATE INDEX IF NOT EXISTS idx_thoughts_created ON thoughts(created_at);

CREATE TABLE IF NOT EXISTS soul_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS tools (
    name TEXT PRIMARY KEY,
    description TEXT NOT NULL,
    parameters TEXT NOT NULL,
    handler_type TEXT NOT NULL,
    handler_config TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    mode_tags TEXT NOT NULL DEFAULT '["code","chat"]',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS mutations (
    id TEXT PRIMARY KEY,
    commit_sha TEXT,
    branch TEXT NOT NULL,
    description TEXT NOT NULL,
    files_changed TEXT NOT NULL,
    cargo_check_passed INTEGER NOT NULL DEFAULT 0,
    cargo_test_passed INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mutations_created ON mutations(created_at);
"#;

/// The soul's dedicated SQLite database.
pub struct SoulDatabase {
    conn: Mutex<Connection>,
}

impl SoulDatabase {
    /// Open (or create) the soul database at the given path.
    pub fn new(path: &str) -> Result<Self, SoulError> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(SCHEMA)?;
        Self::run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Run incremental schema migrations using PRAGMA user_version.
    fn run_migrations(conn: &Connection) -> Result<(), SoulError> {
        let version: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            // v1: neuroplastic memory columns + pattern_counts table
            // Each ALTER TABLE must be a separate statement (SQLite limitation).
            // Use execute_batch with individual error handling — columns may already exist
            // if a previous migration was partially applied.
            let alters = [
                "ALTER TABLE thoughts ADD COLUMN salience REAL",
                "ALTER TABLE thoughts ADD COLUMN salience_factors TEXT",
                "ALTER TABLE thoughts ADD COLUMN memory_tier TEXT",
                "ALTER TABLE thoughts ADD COLUMN strength REAL",
                "ALTER TABLE thoughts ADD COLUMN prediction_error REAL",
            ];
            for alter in &alters {
                // Ignore "duplicate column" errors for idempotency
                let _ = conn.execute_batch(alter);
            }

            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS pattern_counts (
                    fingerprint TEXT PRIMARY KEY,
                    count INTEGER NOT NULL DEFAULT 1,
                    last_seen_at INTEGER NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_thoughts_salience ON thoughts(salience);
                CREATE INDEX IF NOT EXISTS idx_thoughts_tier_strength ON thoughts(memory_tier, strength);",
            )?;

            conn.execute_batch("PRAGMA user_version = 1;")?;
        }

        Ok(())
    }

    /// Store a thought.
    pub fn insert_thought(&self, thought: &Thought) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        conn.execute(
            "INSERT INTO thoughts (id, thought_type, content, context, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                thought.id,
                thought.thought_type.as_str(),
                thought.content,
                thought.context,
                thought.created_at,
            ],
        )?;
        Ok(())
    }

    /// Get the most recent N thoughts, newest first.
    pub fn recent_thoughts(&self, limit: u32) -> Result<Vec<Thought>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, thought_type, content, context, created_at, salience, memory_tier, strength \
             FROM thoughts ORDER BY created_at DESC LIMIT ?1",
        )?;

        let thoughts = stmt
            .query_map(params![limit], |row| {
                let type_str: String = row.get(1)?;
                Ok(Thought {
                    id: row.get(0)?,
                    thought_type: ThoughtType::parse(&type_str).unwrap_or(ThoughtType::Observation),
                    content: row.get(2)?,
                    context: row.get(3)?,
                    created_at: row.get(4)?,
                    salience: row.get(5)?,
                    memory_tier: row.get(6)?,
                    strength: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(thoughts)
    }

    /// Get the most recent N thoughts of specific types, newest first.
    pub fn recent_thoughts_by_type(
        &self,
        types: &[ThoughtType],
        limit: u32,
    ) -> Result<Vec<Thought>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        if types.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: Vec<String> = types
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let query = format!(
            "SELECT id, thought_type, content, context, created_at, salience, memory_tier, strength FROM thoughts \
             WHERE thought_type IN ({}) ORDER BY created_at DESC LIMIT ?{}",
            placeholders.join(", "),
            types.len() + 1
        );

        let mut stmt = conn.prepare(&query)?;

        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = types
            .iter()
            .map(|t| Box::new(t.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params_vec.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let thoughts = stmt
            .query_map(params_refs.as_slice(), |row| {
                let type_str: String = row.get(1)?;
                Ok(Thought {
                    id: row.get(0)?,
                    thought_type: ThoughtType::parse(&type_str).unwrap_or(ThoughtType::Observation),
                    content: row.get(2)?,
                    context: row.get(3)?,
                    created_at: row.get(4)?,
                    salience: row.get(5)?,
                    memory_tier: row.get(6)?,
                    strength: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(thoughts)
    }

    /// Get a soul state value by key.
    pub fn get_state(&self, key: &str) -> Result<Option<String>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let value = conn
            .query_row(
                "SELECT value FROM soul_state WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;

        Ok(value)
    }

    /// Record a mutation (code change attempt).
    pub fn insert_mutation(&self, mutation: &Mutation) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        conn.execute(
            "INSERT INTO mutations (id, commit_sha, branch, description, files_changed, cargo_check_passed, cargo_test_passed, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                mutation.id,
                mutation.commit_sha,
                mutation.branch,
                mutation.description,
                mutation.files_changed,
                mutation.cargo_check_passed as i32,
                mutation.cargo_test_passed as i32,
                mutation.created_at,
            ],
        )?;
        Ok(())
    }

    /// Get recent mutations, newest first.
    pub fn recent_mutations(&self, limit: u32) -> Result<Vec<Mutation>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, commit_sha, branch, description, files_changed, cargo_check_passed, cargo_test_passed, created_at \
             FROM mutations ORDER BY created_at DESC LIMIT ?1",
        )?;

        let mutations = stmt
            .query_map(params![limit], |row| {
                let check: i32 = row.get(5)?;
                let test: i32 = row.get(6)?;
                Ok(Mutation {
                    id: row.get(0)?,
                    commit_sha: row.get(1)?,
                    branch: row.get(2)?,
                    description: row.get(3)?,
                    files_changed: row.get(4)?,
                    cargo_check_passed: check != 0,
                    cargo_test_passed: test != 0,
                    created_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(mutations)
    }

    /// Set a soul state value (upsert).
    pub fn set_state(&self, key: &str, value: &str) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            "INSERT INTO soul_state (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = ?2, updated_at = ?3",
            params![key, value, now],
        )?;
        Ok(())
    }

    // ── Neuroplastic memory ───────────────────────────────────────────────

    /// Insert a thought with salience metadata.
    pub fn insert_thought_with_salience(
        &self,
        thought: &Thought,
        salience: f64,
        salience_factors_json: &str,
        tier: &str,
        strength: f64,
        prediction_error: Option<f64>,
    ) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        conn.execute(
            "INSERT INTO thoughts (id, thought_type, content, context, created_at, salience, salience_factors, memory_tier, strength, prediction_error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                thought.id,
                thought.thought_type.as_str(),
                thought.content,
                thought.context,
                thought.created_at,
                salience,
                salience_factors_json,
                tier,
                strength,
                prediction_error,
            ],
        )?;
        Ok(())
    }

    /// Get the most salient thoughts of given types, ordered by effective salience (salience * strength).
    pub fn salient_thoughts_by_type(
        &self,
        types: &[ThoughtType],
        limit: u32,
    ) -> Result<Vec<Thought>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        if types.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: Vec<String> = types
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let query = format!(
            "SELECT id, thought_type, content, context, created_at, salience, memory_tier, strength FROM thoughts \
             WHERE thought_type IN ({}) \
             ORDER BY COALESCE(salience,0) * COALESCE(strength,1) DESC, created_at DESC \
             LIMIT ?{}",
            placeholders.join(", "),
            types.len() + 1
        );

        let mut stmt = conn.prepare(&query)?;

        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = types
            .iter()
            .map(|t| Box::new(t.as_str().to_string()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        params_vec.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let thoughts = stmt
            .query_map(params_refs.as_slice(), |row| {
                let type_str: String = row.get(1)?;
                Ok(Thought {
                    id: row.get(0)?,
                    thought_type: ThoughtType::parse(&type_str).unwrap_or(ThoughtType::Observation),
                    content: row.get(2)?,
                    context: row.get(3)?,
                    created_at: row.get(4)?,
                    salience: row.get(5)?,
                    memory_tier: row.get(6)?,
                    strength: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(thoughts)
    }

    /// Reinforce recalled thoughts (Hebbian learning): boost strength for accessed thoughts.
    pub fn reinforce_thoughts(&self, ids: &[String], boost: f64) -> Result<(), SoulError> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        for id in ids {
            conn.execute(
                "UPDATE thoughts SET strength = MIN(COALESCE(strength, 1.0) + ?1, 1.0) WHERE id = ?2",
                params![boost, id],
            )?;
        }
        Ok(())
    }

    /// Run a decay cycle: reduce strength per tier, prune thoughts below threshold.
    /// Long-term thoughts are never pruned.
    pub fn run_decay_cycle(&self, prune_threshold: f64) -> Result<(u32, u32), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        // Decay each tier
        let sensory_decayed = conn.execute(
            "UPDATE thoughts SET strength = strength * 0.3 WHERE memory_tier = 'sensory' AND strength IS NOT NULL",
            [],
        )? as u32;
        conn.execute(
            "UPDATE thoughts SET strength = strength * 0.95 WHERE memory_tier = 'working' AND strength IS NOT NULL",
            [],
        )?;
        conn.execute(
            "UPDATE thoughts SET strength = strength * 0.995 WHERE memory_tier = 'long_term' AND strength IS NOT NULL",
            [],
        )?;

        // Prune below threshold (except long_term)
        let pruned = conn.execute(
            "DELETE FROM thoughts WHERE strength IS NOT NULL AND strength < ?1 AND (memory_tier != 'long_term' OR memory_tier IS NULL)",
            params![prune_threshold],
        )? as u32;

        Ok((sensory_decayed, pruned))
    }

    /// Auto-promote high-salience sensory thoughts to working tier.
    pub fn promote_salient_sensory(&self, salience_threshold: f64) -> Result<u32, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let promoted = conn.execute(
            "UPDATE thoughts SET memory_tier = 'working' WHERE memory_tier = 'sensory' AND salience IS NOT NULL AND salience > ?1",
            params![salience_threshold],
        )? as u32;

        Ok(promoted)
    }

    /// Increment a pattern's count. Returns the new count.
    pub fn increment_pattern(&self, fingerprint: &str) -> Result<u64, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT INTO pattern_counts (fingerprint, count, last_seen_at) VALUES (?1, 1, ?2) \
             ON CONFLICT(fingerprint) DO UPDATE SET count = count + 1, last_seen_at = ?2",
            params![fingerprint, now],
        )?;

        let count: u64 = conn.query_row(
            "SELECT count FROM pattern_counts WHERE fingerprint = ?1",
            params![fingerprint],
            |row| row.get(0),
        )?;

        Ok(count)
    }

    /// Get pattern counts for multiple fingerprints.
    pub fn get_pattern_counts(
        &self,
        fingerprints: &[String],
    ) -> Result<HashMap<String, u64>, SoulError> {
        if fingerprints.is_empty() {
            return Ok(HashMap::new());
        }
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let placeholders: Vec<String> = fingerprints
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let query = format!(
            "SELECT fingerprint, count FROM pattern_counts WHERE fingerprint IN ({})",
            placeholders.join(", ")
        );

        let mut stmt = conn.prepare(&query)?;
        let params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = fingerprints
            .iter()
            .map(|f| Box::new(f.clone()) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut result = HashMap::new();
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let fp: String = row.get(0)?;
            let count: u64 = row.get(1)?;
            Ok((fp, count))
        })?;
        for row in rows {
            let (fp, count) = row?;
            result.insert(fp, count);
        }

        Ok(result)
    }

    /// Store a prediction in soul_state as JSON.
    pub fn store_prediction(&self, prediction_json: &str) -> Result<(), SoulError> {
        self.set_state("last_prediction", prediction_json)
    }

    /// Get the last stored prediction JSON.
    pub fn get_last_prediction(&self) -> Result<Option<String>, SoulError> {
        self.get_state("last_prediction")
    }

    // ── Dynamic tools CRUD ──────────────────────────────────────────────

    /// Insert or update a dynamic tool.
    pub fn insert_tool(&self, tool: &DynamicTool) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        conn.execute(
            "INSERT INTO tools (name, description, parameters, handler_type, handler_config, enabled, mode_tags, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(name) DO UPDATE SET description=?2, parameters=?3, handler_type=?4, handler_config=?5, enabled=?6, mode_tags=?7, updated_at=?9",
            params![
                tool.name,
                tool.description,
                tool.parameters,
                tool.handler_type,
                tool.handler_config,
                tool.enabled as i32,
                tool.mode_tags,
                tool.created_at,
                tool.updated_at,
            ],
        )?;
        Ok(())
    }

    /// Get a dynamic tool by name.
    pub fn get_tool(&self, name: &str) -> Result<Option<DynamicTool>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let tool = conn
            .query_row(
                "SELECT name, description, parameters, handler_type, handler_config, enabled, mode_tags, created_at, updated_at \
                 FROM tools WHERE name = ?1",
                params![name],
                |row| {
                    let enabled: i32 = row.get(5)?;
                    Ok(DynamicTool {
                        name: row.get(0)?,
                        description: row.get(1)?,
                        parameters: row.get(2)?,
                        handler_type: row.get(3)?,
                        handler_config: row.get(4)?,
                        enabled: enabled != 0,
                        mode_tags: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                    })
                },
            )
            .optional()?;

        Ok(tool)
    }

    /// List all dynamic tools (enabled only by default).
    pub fn list_tools(&self, enabled_only: bool) -> Result<Vec<DynamicTool>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let query = if enabled_only {
            "SELECT name, description, parameters, handler_type, handler_config, enabled, mode_tags, created_at, updated_at \
             FROM tools WHERE enabled = 1 ORDER BY name"
        } else {
            "SELECT name, description, parameters, handler_type, handler_config, enabled, mode_tags, created_at, updated_at \
             FROM tools ORDER BY name"
        };

        let mut stmt = conn.prepare(query)?;
        let tools = stmt
            .query_map([], |row| {
                let enabled: i32 = row.get(5)?;
                Ok(DynamicTool {
                    name: row.get(0)?,
                    description: row.get(1)?,
                    parameters: row.get(2)?,
                    handler_type: row.get(3)?,
                    handler_config: row.get(4)?,
                    enabled: enabled != 0,
                    mode_tags: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(tools)
    }

    /// Delete a dynamic tool by name. Returns true if a row was deleted.
    pub fn delete_tool(&self, name: &str) -> Result<bool, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let rows = conn.execute("DELETE FROM tools WHERE name = ?1", params![name])?;
        Ok(rows > 0)
    }

    /// Count enabled dynamic tools.
    pub fn count_tools(&self) -> Result<u32, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let count: u32 =
            conn.query_row("SELECT COUNT(*) FROM tools WHERE enabled = 1", [], |row| {
                row.get(0)
            })?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_retrieve_thoughts() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let thought = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Observation,
            content: "Node has 3 endpoints".to_string(),
            context: Some(r#"{"endpoints": 3}"#.to_string()),
            created_at: 1000,
            salience: None,
            memory_tier: None,
            strength: None,
        };
        db.insert_thought(&thought).unwrap();

        let thought2 = Thought {
            id: "t2".to_string(),
            thought_type: ThoughtType::Reasoning,
            content: "Node is healthy".to_string(),
            context: None,
            created_at: 2000,
            salience: None,
            memory_tier: None,
            strength: None,
        };
        db.insert_thought(&thought2).unwrap();

        let recent = db.recent_thoughts(5).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].id, "t2"); // newest first
        assert_eq!(recent[1].id, "t1");
    }

    #[test]
    fn test_state_upsert() {
        let db = SoulDatabase::new(":memory:").unwrap();

        assert!(db.get_state("cycles").unwrap().is_none());

        db.set_state("cycles", "1").unwrap();
        assert_eq!(db.get_state("cycles").unwrap().unwrap(), "1");

        db.set_state("cycles", "2").unwrap();
        assert_eq!(db.get_state("cycles").unwrap().unwrap(), "2");
    }

    #[test]
    fn test_insert_thought_with_salience() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let thought = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Observation,
            content: "Test observation".to_string(),
            context: None,
            created_at: 1000,
            salience: Some(0.8),
            memory_tier: Some("sensory".to_string()),
            strength: Some(1.0),
        };
        db.insert_thought_with_salience(
            &thought,
            0.8,
            r#"{"novelty":1.0}"#,
            "sensory",
            1.0,
            Some(0.5),
        )
        .unwrap();

        let recent = db.recent_thoughts(1).unwrap();
        assert_eq!(recent.len(), 1);
        assert!((recent[0].salience.unwrap() - 0.8).abs() < f64::EPSILON);
        assert_eq!(recent[0].memory_tier.as_deref(), Some("sensory"));
        assert!((recent[0].strength.unwrap() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_salient_thoughts_by_type() {
        let db = SoulDatabase::new(":memory:").unwrap();

        // Insert low-salience thought
        let t1 = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Reasoning,
            content: "Low salience".to_string(),
            context: None,
            created_at: 1000,
            salience: Some(0.2),
            memory_tier: Some("working".to_string()),
            strength: Some(0.5),
        };
        db.insert_thought_with_salience(&t1, 0.2, "{}", "working", 0.5, None)
            .unwrap();

        // Insert high-salience thought
        let t2 = Thought {
            id: "t2".to_string(),
            thought_type: ThoughtType::Reasoning,
            content: "High salience".to_string(),
            context: None,
            created_at: 2000,
            salience: Some(0.9),
            memory_tier: Some("working".to_string()),
            strength: Some(1.0),
        };
        db.insert_thought_with_salience(&t2, 0.9, "{}", "working", 1.0, None)
            .unwrap();

        let results = db
            .salient_thoughts_by_type(&[ThoughtType::Reasoning], 2)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "t2"); // highest effective salience first
    }

    #[test]
    fn test_pattern_counts() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let count = db.increment_pattern("hello world").unwrap();
        assert_eq!(count, 1);

        let count = db.increment_pattern("hello world").unwrap();
        assert_eq!(count, 2);

        let counts = db
            .get_pattern_counts(&["hello world".to_string(), "unknown".to_string()])
            .unwrap();
        assert_eq!(counts.get("hello world"), Some(&2));
        assert!(counts.get("unknown").is_none());
    }

    #[test]
    fn test_decay_and_prune() {
        let db = SoulDatabase::new(":memory:").unwrap();

        // Sensory thought with low strength — should be pruned after decay
        let t1 = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Observation,
            content: "Will decay fast".to_string(),
            context: None,
            created_at: 1000,
            salience: Some(0.1),
            memory_tier: Some("sensory".to_string()),
            strength: Some(0.02),
        };
        db.insert_thought_with_salience(&t1, 0.1, "{}", "sensory", 0.02, None)
            .unwrap();

        // Long-term thought — should never be pruned
        let t2 = Thought {
            id: "t2".to_string(),
            thought_type: ThoughtType::MemoryConsolidation,
            content: "Important consolidation".to_string(),
            context: None,
            created_at: 2000,
            salience: Some(0.9),
            memory_tier: Some("long_term".to_string()),
            strength: Some(0.005),
        };
        db.insert_thought_with_salience(&t2, 0.9, "{}", "long_term", 0.005, None)
            .unwrap();

        let (_decayed, pruned) = db.run_decay_cycle(0.01).unwrap();
        assert!(pruned >= 1); // sensory thought should be pruned

        let remaining = db.recent_thoughts(10).unwrap();
        assert_eq!(remaining.len(), 1); // only long_term remains
        assert_eq!(remaining[0].id, "t2");
    }

    #[test]
    fn test_promote_salient_sensory() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let t1 = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Observation,
            content: "High salience sensory".to_string(),
            context: None,
            created_at: 1000,
            salience: Some(0.8),
            memory_tier: Some("sensory".to_string()),
            strength: Some(1.0),
        };
        db.insert_thought_with_salience(&t1, 0.8, "{}", "sensory", 1.0, None)
            .unwrap();

        let promoted = db.promote_salient_sensory(0.6).unwrap();
        assert_eq!(promoted, 1);

        let thoughts = db.recent_thoughts(1).unwrap();
        assert_eq!(thoughts[0].memory_tier.as_deref(), Some("working"));
    }

    #[test]
    fn test_reinforce_thoughts() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let t1 = Thought {
            id: "t1".to_string(),
            thought_type: ThoughtType::Reasoning,
            content: "Reinforceable".to_string(),
            context: None,
            created_at: 1000,
            salience: Some(0.5),
            memory_tier: Some("working".to_string()),
            strength: Some(0.7),
        };
        db.insert_thought_with_salience(&t1, 0.5, "{}", "working", 0.7, None)
            .unwrap();

        db.reinforce_thoughts(&["t1".to_string()], 0.1).unwrap();

        let thoughts = db.recent_thoughts(1).unwrap();
        assert!((thoughts[0].strength.unwrap() - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_prediction_storage() {
        let db = SoulDatabase::new(":memory:").unwrap();

        assert!(db.get_last_prediction().unwrap().is_none());

        db.store_prediction(r#"{"expected_payments":10}"#).unwrap();
        let pred = db.get_last_prediction().unwrap().unwrap();
        assert!(pred.contains("expected_payments"));
    }
}
