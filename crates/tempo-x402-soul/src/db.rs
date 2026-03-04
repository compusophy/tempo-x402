//! Soul database: separate SQLite for thoughts and state.

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::SoulError;
use crate::memory::{Thought, ThoughtType};
use crate::world_model::{Belief, BeliefDomain, Confidence, Goal, GoalStatus};

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
    /// The goal this mutation advances (if any).
    pub goal_id: Option<String>,
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

        if version < 2 {
            // v2: world model beliefs table
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS beliefs (
                    id TEXT PRIMARY KEY,
                    domain TEXT NOT NULL,
                    subject TEXT NOT NULL,
                    predicate TEXT NOT NULL,
                    value TEXT NOT NULL,
                    confidence TEXT NOT NULL DEFAULT 'medium',
                    evidence TEXT NOT NULL DEFAULT '',
                    confirmation_count INTEGER NOT NULL DEFAULT 1,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    active INTEGER NOT NULL DEFAULT 1
                );
                CREATE UNIQUE INDEX IF NOT EXISTS idx_beliefs_unique
                    ON beliefs(domain, subject, predicate) WHERE active = 1;
                CREATE INDEX IF NOT EXISTS idx_beliefs_domain ON beliefs(domain);
                PRAGMA user_version = 2;",
            )?;
        }

        if version < 3 {
            // v3: goals table + goal_id on mutations
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS goals (
                    id TEXT PRIMARY KEY,
                    description TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'active',
                    priority INTEGER NOT NULL DEFAULT 3,
                    success_criteria TEXT NOT NULL DEFAULT '',
                    progress_notes TEXT NOT NULL DEFAULT '',
                    parent_goal_id TEXT,
                    retry_count INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    completed_at INTEGER
                );
                CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
                CREATE INDEX IF NOT EXISTS idx_goals_priority ON goals(priority);",
            )?;
            // Add goal_id to mutations (ignore if already exists)
            let _ = conn.execute_batch("ALTER TABLE mutations ADD COLUMN goal_id TEXT");
            conn.execute_batch("PRAGMA user_version = 3;")?;
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
            "INSERT INTO mutations (id, commit_sha, branch, description, files_changed, cargo_check_passed, cargo_test_passed, created_at, goal_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                mutation.id,
                mutation.commit_sha,
                mutation.branch,
                mutation.description,
                mutation.files_changed,
                mutation.cargo_check_passed as i32,
                mutation.cargo_test_passed as i32,
                mutation.created_at,
                mutation.goal_id,
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
            "SELECT id, commit_sha, branch, description, files_changed, cargo_check_passed, cargo_test_passed, created_at, goal_id \
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
                    goal_id: row.get(8)?,
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

    // ── World Model beliefs ──────────────────────────────────────────────

    /// Upsert a belief. On conflict (domain, subject, predicate) for active beliefs,
    /// updates value, evidence, confidence, bumps confirmation_count, refreshes updated_at.
    pub fn upsert_belief(&self, belief: &Belief) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        conn.execute(
            "INSERT INTO beliefs (id, domain, subject, predicate, value, confidence, evidence, confirmation_count, created_at, updated_at, active) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
             ON CONFLICT(domain, subject, predicate) WHERE active = 1 DO UPDATE SET \
               value = ?5, confidence = ?6, evidence = ?7, \
               confirmation_count = confirmation_count + 1, \
               updated_at = ?10",
            params![
                belief.id,
                belief.domain.as_str(),
                belief.subject,
                belief.predicate,
                belief.value,
                belief.confidence.as_str(),
                belief.evidence,
                belief.confirmation_count,
                belief.created_at,
                belief.updated_at,
                belief.active as i32,
            ],
        )?;
        Ok(())
    }

    /// Get all active beliefs for a domain.
    pub fn get_beliefs_by_domain(&self, domain: &BeliefDomain) -> Result<Vec<Belief>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, domain, subject, predicate, value, confidence, evidence, \
             confirmation_count, created_at, updated_at, active \
             FROM beliefs WHERE domain = ?1 AND active = 1 ORDER BY subject, predicate",
        )?;

        let beliefs = stmt
            .query_map(params![domain.as_str()], Self::row_to_belief)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(beliefs)
    }

    /// Get all active beliefs (full world model snapshot).
    pub fn get_all_active_beliefs(&self) -> Result<Vec<Belief>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;

        let mut stmt = conn.prepare(
            "SELECT id, domain, subject, predicate, value, confidence, evidence, \
             confirmation_count, created_at, updated_at, active \
             FROM beliefs WHERE active = 1 ORDER BY domain, subject, predicate",
        )?;

        let beliefs = stmt
            .query_map([], Self::row_to_belief)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(beliefs)
    }

    /// Confirm a belief: bump confirmation_count, refresh updated_at, set confidence to High.
    pub fn confirm_belief(&self, id: &str) -> Result<bool, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        let rows = conn.execute(
            "UPDATE beliefs SET confirmation_count = confirmation_count + 1, \
             updated_at = ?1, confidence = 'high' WHERE id = ?2 AND active = 1",
            params![now, id],
        )?;
        Ok(rows > 0)
    }

    /// Invalidate a belief: set active=false, append reason to evidence.
    pub fn invalidate_belief(&self, id: &str, reason: &str) -> Result<bool, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        let rows = conn.execute(
            "UPDATE beliefs SET active = 0, updated_at = ?1, \
             evidence = evidence || ' [invalidated: ' || ?2 || ']' \
             WHERE id = ?3 AND active = 1",
            params![now, reason, id],
        )?;
        Ok(rows > 0)
    }

    /// Decay unconfirmed beliefs based on cycles since last update.
    /// Uses the cycle count stored in soul_state to determine staleness.
    /// High → Medium after 5 cycles unconfirmed, Medium → Low after 10, Low → inactive after 20.
    pub fn decay_beliefs(&self) -> Result<(u32, u32, u32), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();

        // Thresholds in seconds (approximation: 1 cycle ~ 300s = 5min)
        let cycle_secs: i64 = 300;

        // High → Medium: unconfirmed for 5 cycles (~25min)
        let demoted_high = conn.execute(
            "UPDATE beliefs SET confidence = 'medium' \
             WHERE active = 1 AND confidence = 'high' AND (?1 - updated_at) > ?2",
            params![now, cycle_secs * 5],
        )? as u32;

        // Medium → Low: unconfirmed for 10 cycles (~50min)
        let demoted_medium = conn.execute(
            "UPDATE beliefs SET confidence = 'low' \
             WHERE active = 1 AND confidence = 'medium' AND (?1 - updated_at) > ?2",
            params![now, cycle_secs * 10],
        )? as u32;

        // Low → inactive: unconfirmed for 20 cycles (~100min)
        let deactivated = conn.execute(
            "UPDATE beliefs SET active = 0 \
             WHERE active = 1 AND confidence = 'low' AND (?1 - updated_at) > ?2",
            params![now, cycle_secs * 20],
        )? as u32;

        Ok((demoted_high, demoted_medium, deactivated))
    }

    // ── Goal operations ──

    /// Insert a new goal.
    pub fn insert_goal(&self, goal: &Goal) -> Result<(), SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        conn.execute(
            "INSERT INTO goals (id, description, status, priority, success_criteria, \
             progress_notes, parent_goal_id, retry_count, created_at, updated_at, completed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                goal.id,
                goal.description,
                goal.status.as_str(),
                goal.priority,
                goal.success_criteria,
                goal.progress_notes,
                goal.parent_goal_id,
                goal.retry_count,
                goal.created_at,
                goal.updated_at,
                goal.completed_at,
            ],
        )?;
        Ok(())
    }

    /// Get all active goals, ordered by priority DESC then created_at ASC.
    pub fn get_active_goals(&self) -> Result<Vec<Goal>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, success_criteria, progress_notes, \
             parent_goal_id, retry_count, created_at, updated_at, completed_at \
             FROM goals WHERE status = 'active' ORDER BY priority DESC, created_at ASC",
        )?;
        let goals = stmt
            .query_map([], Self::row_to_goal)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(goals)
    }

    /// Get a goal by ID (any status).
    pub fn get_goal(&self, id: &str) -> Result<Option<Goal>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let result = conn
            .query_row(
                "SELECT id, description, status, priority, success_criteria, progress_notes, \
                 parent_goal_id, retry_count, created_at, updated_at, completed_at \
                 FROM goals WHERE id = ?1",
                params![id],
                Self::row_to_goal,
            )
            .optional()?;
        Ok(result)
    }

    /// Update a goal's status and/or progress notes.
    pub fn update_goal(
        &self,
        id: &str,
        status: Option<&str>,
        progress_notes: Option<&str>,
        completed_at: Option<i64>,
    ) -> Result<bool, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        let rows = conn.execute(
            "UPDATE goals SET \
             status = COALESCE(?2, status), \
             progress_notes = COALESCE(?3, progress_notes), \
             completed_at = COALESCE(?4, completed_at), \
             updated_at = ?5 \
             WHERE id = ?1",
            params![id, status, progress_notes, completed_at, now],
        )?;
        Ok(rows > 0)
    }

    /// Increment retry count for a goal.
    pub fn increment_goal_retry(&self, id: &str) -> Result<bool, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let now = chrono::Utc::now().timestamp();
        let rows = conn.execute(
            "UPDATE goals SET retry_count = retry_count + 1, updated_at = ?2 WHERE id = ?1",
            params![id, now],
        )?;
        Ok(rows > 0)
    }

    /// Get recently completed/abandoned goals (for reflection context).
    pub fn recent_finished_goals(&self, limit: u32) -> Result<Vec<Goal>, SoulError> {
        let conn = self.conn.lock().map_err(|_| {
            SoulError::Database(rusqlite::Error::InvalidParameterName(
                "lock poisoned".into(),
            ))
        })?;
        let mut stmt = conn.prepare(
            "SELECT id, description, status, priority, success_criteria, progress_notes, \
             parent_goal_id, retry_count, created_at, updated_at, completed_at \
             FROM goals WHERE status IN ('completed', 'abandoned', 'failed') \
             ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let goals = stmt
            .query_map(params![limit], Self::row_to_goal)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(goals)
    }

    /// Helper: map a row to a Goal.
    fn row_to_goal(row: &rusqlite::Row) -> Result<Goal, rusqlite::Error> {
        let status_str: String = row.get(2)?;
        Ok(Goal {
            id: row.get(0)?,
            description: row.get(1)?,
            status: GoalStatus::parse(&status_str).unwrap_or(GoalStatus::Active),
            priority: row.get(3)?,
            success_criteria: row.get(4)?,
            progress_notes: row.get(5)?,
            parent_goal_id: row.get(6)?,
            retry_count: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            completed_at: row.get(10)?,
        })
    }

    /// Helper: map a row to a Belief.
    fn row_to_belief(row: &rusqlite::Row) -> Result<Belief, rusqlite::Error> {
        let domain_str: String = row.get(1)?;
        let confidence_str: String = row.get(5)?;
        let active_int: i32 = row.get(10)?;
        Ok(Belief {
            id: row.get(0)?,
            domain: BeliefDomain::parse(&domain_str).unwrap_or(BeliefDomain::Node),
            subject: row.get(2)?,
            predicate: row.get(3)?,
            value: row.get(4)?,
            confidence: Confidence::parse(&confidence_str),
            evidence: row.get(6)?,
            confirmation_count: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            active: active_int != 0,
        })
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

    #[test]
    fn test_upsert_and_get_beliefs() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let belief = Belief {
            id: "b1".to_string(),
            domain: BeliefDomain::Endpoints,
            subject: "echo".to_string(),
            predicate: "payment_count".to_string(),
            value: "0".to_string(),
            confidence: Confidence::High,
            evidence: "from snapshot".to_string(),
            confirmation_count: 1,
            created_at: 1000,
            updated_at: 1000,
            active: true,
        };
        db.upsert_belief(&belief).unwrap();

        // Get by domain
        let beliefs = db.get_beliefs_by_domain(&BeliefDomain::Endpoints).unwrap();
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].value, "0");
        assert_eq!(beliefs[0].confirmation_count, 1);

        // Upsert same (domain, subject, predicate) — should update
        let updated = Belief {
            id: "b2".to_string(), // different id, but same key
            value: "5".to_string(),
            evidence: "new observation".to_string(),
            updated_at: 2000,
            ..belief.clone()
        };
        db.upsert_belief(&updated).unwrap();

        let beliefs = db.get_beliefs_by_domain(&BeliefDomain::Endpoints).unwrap();
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].value, "5");
        assert_eq!(beliefs[0].confirmation_count, 2); // bumped
    }

    #[test]
    fn test_get_all_active_beliefs() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let b1 = Belief {
            id: "b1".to_string(),
            domain: BeliefDomain::Node,
            subject: "self".to_string(),
            predicate: "uptime".to_string(),
            value: "10h".to_string(),
            confidence: Confidence::High,
            evidence: "".to_string(),
            confirmation_count: 1,
            created_at: 1000,
            updated_at: 1000,
            active: true,
        };
        let b2 = Belief {
            id: "b2".to_string(),
            domain: BeliefDomain::Endpoints,
            subject: "echo".to_string(),
            predicate: "count".to_string(),
            value: "3".to_string(),
            confidence: Confidence::Medium,
            evidence: "".to_string(),
            confirmation_count: 1,
            created_at: 1000,
            updated_at: 1000,
            active: true,
        };
        db.upsert_belief(&b1).unwrap();
        db.upsert_belief(&b2).unwrap();

        let all = db.get_all_active_beliefs().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_confirm_belief() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let belief = Belief {
            id: "b1".to_string(),
            domain: BeliefDomain::Node,
            subject: "self".to_string(),
            predicate: "healthy".to_string(),
            value: "true".to_string(),
            confidence: Confidence::Medium,
            evidence: "".to_string(),
            confirmation_count: 1,
            created_at: 1000,
            updated_at: 1000,
            active: true,
        };
        db.upsert_belief(&belief).unwrap();

        let confirmed = db.confirm_belief("b1").unwrap();
        assert!(confirmed);

        let beliefs = db.get_beliefs_by_domain(&BeliefDomain::Node).unwrap();
        assert_eq!(beliefs[0].confidence, Confidence::High);
        assert_eq!(beliefs[0].confirmation_count, 2);
    }

    #[test]
    fn test_invalidate_belief() {
        let db = SoulDatabase::new(":memory:").unwrap();

        let belief = Belief {
            id: "b1".to_string(),
            domain: BeliefDomain::Endpoints,
            subject: "old-ep".to_string(),
            predicate: "exists".to_string(),
            value: "true".to_string(),
            confidence: Confidence::High,
            evidence: "registered".to_string(),
            confirmation_count: 1,
            created_at: 1000,
            updated_at: 1000,
            active: true,
        };
        db.upsert_belief(&belief).unwrap();

        let invalidated = db.invalidate_belief("b1", "endpoint removed").unwrap();
        assert!(invalidated);

        // Should no longer appear in active beliefs
        let beliefs = db.get_beliefs_by_domain(&BeliefDomain::Endpoints).unwrap();
        assert!(beliefs.is_empty());
    }

    #[test]
    fn test_decay_beliefs() {
        let db = SoulDatabase::new(":memory:").unwrap();

        // Insert a belief that's just old enough for High→Medium (>1500s) but not Medium→Low (>3000s)
        let slightly_old = chrono::Utc::now().timestamp() - 2000; // ~6.6 cycles
        let belief = Belief {
            id: "b1".to_string(),
            domain: BeliefDomain::Strategy,
            subject: "plan".to_string(),
            predicate: "next_action".to_string(),
            value: "do something".to_string(),
            confidence: Confidence::High,
            evidence: "".to_string(),
            confirmation_count: 1,
            created_at: slightly_old,
            updated_at: slightly_old,
            active: true,
        };
        db.upsert_belief(&belief).unwrap();

        let (demoted_high, _, _) = db.decay_beliefs().unwrap();
        assert!(demoted_high >= 1);

        // Check it was demoted to Medium (not further, since only ~6.6 cycles old)
        let beliefs = db.get_beliefs_by_domain(&BeliefDomain::Strategy).unwrap();
        assert_eq!(beliefs.len(), 1);
        assert_eq!(beliefs[0].confidence, Confidence::Medium);

        // Insert a very old belief — should be fully deactivated
        let very_old = chrono::Utc::now().timestamp() - 10_000;
        let belief2 = Belief {
            id: "b2".to_string(),
            domain: BeliefDomain::Strategy,
            subject: "old_plan".to_string(),
            predicate: "status".to_string(),
            value: "stale".to_string(),
            confidence: Confidence::High,
            evidence: "".to_string(),
            confirmation_count: 1,
            created_at: very_old,
            updated_at: very_old,
            active: true,
        };
        db.upsert_belief(&belief2).unwrap();

        let (_, _, deactivated) = db.decay_beliefs().unwrap();
        // The very old belief goes High→Medium→Low→inactive in one pass
        assert!(deactivated >= 1);
    }

    #[test]
    fn test_goal_crud() {
        use crate::world_model::{Goal, GoalStatus};

        let db = SoulDatabase::new(":memory:").unwrap();
        let now = chrono::Utc::now().timestamp();

        let goal = Goal {
            id: "g1".to_string(),
            description: "Build a weather endpoint".to_string(),
            status: GoalStatus::Active,
            priority: 4,
            success_criteria: "Endpoint returns weather data".to_string(),
            progress_notes: String::new(),
            parent_goal_id: None,
            retry_count: 0,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        db.insert_goal(&goal).unwrap();

        // Fetch active goals
        let active = db.get_active_goals().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].description, "Build a weather endpoint");
        assert_eq!(active[0].priority, 4);

        // Update progress
        db.update_goal("g1", None, Some("Read existing endpoint patterns"), None)
            .unwrap();
        let updated = db.get_goal("g1").unwrap().unwrap();
        assert_eq!(updated.progress_notes, "Read existing endpoint patterns");

        // Complete goal
        db.update_goal("g1", Some("completed"), Some("Deployed and earning"), Some(now))
            .unwrap();
        let completed = db.get_goal("g1").unwrap().unwrap();
        assert_eq!(completed.status, GoalStatus::Completed);

        // No longer in active list
        let active = db.get_active_goals().unwrap();
        assert!(active.is_empty());

        // Shows in recent finished
        let finished = db.recent_finished_goals(5).unwrap();
        assert_eq!(finished.len(), 1);
    }

    #[test]
    fn test_goal_priority_ordering() {
        use crate::world_model::{Goal, GoalStatus};

        let db = SoulDatabase::new(":memory:").unwrap();
        let now = chrono::Utc::now().timestamp();

        for (id, prio) in &[("g1", 2), ("g2", 5), ("g3", 3)] {
            db.insert_goal(&Goal {
                id: id.to_string(),
                description: format!("Goal priority {prio}"),
                status: GoalStatus::Active,
                priority: *prio,
                success_criteria: String::new(),
                progress_notes: String::new(),
                parent_goal_id: None,
                retry_count: 0,
                created_at: now,
                updated_at: now,
                completed_at: None,
            })
            .unwrap();
        }

        let active = db.get_active_goals().unwrap();
        assert_eq!(active.len(), 3);
        // Ordered by priority DESC
        assert_eq!(active[0].priority, 5);
        assert_eq!(active[1].priority, 3);
        assert_eq!(active[2].priority, 2);
    }
}
