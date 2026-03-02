//! Children table extension for the gateway database.

use rusqlite::params;
use rusqlite::OptionalExtension;
use x402_gateway::db::Database;
use x402_gateway::error::GatewayError;

/// Child instance record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChildInstance {
    pub id: i64,
    pub instance_id: String,
    pub address: String,
    pub url: Option<String>,
    pub railway_service_id: Option<String>,
    pub funded_amount: Option<String>,
    pub funding_tx: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

const CHILDREN_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS children (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        instance_id TEXT UNIQUE NOT NULL,
        address TEXT NOT NULL DEFAULT '',
        url TEXT,
        railway_service_id TEXT,
        funded_amount TEXT,
        funding_tx TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_children_instance_id ON children(instance_id);
"#;

/// Initialize the children table on an existing gateway database.
pub fn init_children_schema(db: &Database) -> Result<(), GatewayError> {
    db.execute_schema(CHILDREN_SCHEMA)
}

/// Atomically check the children count is under the limit, then insert.
/// Returns `Ok(true)` if under limit (row inserted), `Ok(false)` if limit reached.
/// This prevents the TOCTOU race where two concurrent requests both pass the check.
#[allow(dead_code)]
pub fn create_child_if_under_limit(
    db: &Database,
    max_children: u32,
    instance_id: &str,
    url: Option<&str>,
    railway_service_id: Option<&str>,
) -> Result<bool, GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        // Use BEGIN IMMEDIATE to acquire a write lock atomically
        conn.execute_batch("BEGIN IMMEDIATE")?;

        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM children", [], |row| row.get(0))
            .map_err(|e| GatewayError::Internal(format!("count query failed: {e}")))?;

        if count >= max_children {
            conn.execute_batch("ROLLBACK")?;
            return Ok(false);
        }

        conn.execute(
            "INSERT INTO children (instance_id, url, railway_service_id, status, created_at, updated_at) \
             VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
            params![instance_id, url, railway_service_id, now, now],
        )?;

        conn.execute_batch("COMMIT")?;
        Ok(true)
    })
}

/// Reserve a child slot atomically before starting Railway deployment.
/// Inserts a row with status `'queued'` if under the limit.
/// Returns `Ok(true)` if reserved, `Ok(false)` if limit reached.
pub fn reserve_child_slot(
    db: &Database,
    max_children: u32,
    instance_id: &str,
) -> Result<bool, GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;

        let count: u32 = conn
            .query_row(
                "SELECT COUNT(*) FROM children WHERE status != 'failed'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| GatewayError::Internal(format!("count query failed: {e}")))?;

        if count >= max_children {
            conn.execute_batch("ROLLBACK")?;
            return Ok(false);
        }

        conn.execute(
            "INSERT INTO children (instance_id, status, created_at, updated_at) \
             VALUES (?1, 'queued', ?2, ?3)",
            params![instance_id, now, now],
        )?;

        conn.execute_batch("COMMIT")?;
        Ok(true)
    })
}

/// Update a child after Railway deployment succeeds.
/// Fills in the URL, service ID, and transitions status to `'deploying'`.
pub fn update_child_deployment(
    db: &Database,
    instance_id: &str,
    url: &str,
    railway_service_id: &str,
    status: &str,
) -> Result<(), GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE children SET url = ?1, railway_service_id = ?2, status = ?3, updated_at = ?4 \
             WHERE instance_id = ?5",
            params![url, railway_service_id, status, now, instance_id],
        )?;
        Ok(())
    })
}

/// Mark a child as failed after a deploy error or cleanup.
pub fn mark_child_failed(db: &Database, instance_id: &str) -> Result<(), GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE children SET status = 'failed', updated_at = ?1 WHERE instance_id = ?2",
            params![now, instance_id],
        )?;
        Ok(())
    })
}

/// Look up a single child by instance_id.
pub fn get_child_by_instance_id(
    db: &Database,
    instance_id: &str,
) -> Result<Option<ChildInstance>, GatewayError> {
    db.with_connection(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT id, instance_id, address, url, railway_service_id, \
                 funded_amount, funding_tx, status, created_at, updated_at \
                 FROM children WHERE instance_id = ?1",
            )
            .map_err(|e| GatewayError::Internal(format!("prepare failed: {e}")))?;

        let result = stmt
            .query_row(params![instance_id], |row| {
                Ok(ChildInstance {
                    id: row.get(0)?,
                    instance_id: row.get(1)?,
                    address: row.get(2)?,
                    url: row.get(3)?,
                    railway_service_id: row.get(4)?,
                    funded_amount: row.get(5)?,
                    funding_tx: row.get(6)?,
                    status: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })
            .optional()
            .map_err(|e| GatewayError::Internal(format!("query failed: {e}")))?;

        Ok(result)
    })
}

/// Update a child instance when it registers back, using parameterized queries.
pub fn update_child(
    db: &Database,
    instance_id: &str,
    address: Option<&str>,
    url: Option<&str>,
    status: Option<&str>,
) -> Result<(), GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE children SET \
             address = COALESCE(?1, address), \
             url = COALESCE(?2, url), \
             status = COALESCE(?3, status), \
             updated_at = ?4 \
             WHERE instance_id = ?5",
            params![address, url, status, now, instance_id],
        )?;
        Ok(())
    })
}

// Read operations use direct rusqlite connections opened from NodeState's db_path,
// or with_connection for consistency.

/// Query all children (including failed) from a direct rusqlite connection.
#[allow(dead_code)]
pub fn query_children(conn: &rusqlite::Connection) -> Result<Vec<ChildInstance>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, instance_id, address, url, railway_service_id,
               funded_amount, funding_tx, status, created_at, updated_at
        FROM children
        ORDER BY created_at DESC
        "#,
    )?;

    let children = stmt
        .query_map([], |row| {
            Ok(ChildInstance {
                id: row.get(0)?,
                instance_id: row.get(1)?,
                address: row.get(2)?,
                url: row.get(3)?,
                railway_service_id: row.get(4)?,
                funded_amount: row.get(5)?,
                funding_tx: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(children)
}

/// Query only active (non-failed) children from a direct rusqlite connection.
pub fn query_children_active(
    conn: &rusqlite::Connection,
) -> Result<Vec<ChildInstance>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, instance_id, address, url, railway_service_id,
               funded_amount, funding_tx, status, created_at, updated_at
        FROM children
        WHERE status != 'failed'
        ORDER BY created_at DESC
        "#,
    )?;

    let children = stmt
        .query_map([], |row| {
            Ok(ChildInstance {
                id: row.get(0)?,
                instance_id: row.get(1)?,
                address: row.get(2)?,
                url: row.get(3)?,
                railway_service_id: row.get(4)?,
                funded_amount: row.get(5)?,
                funding_tx: row.get(6)?,
                status: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(children)
}

/// Delete a child record only if its status is 'failed'.
/// Returns `Ok(true)` if a row was deleted, `Ok(false)` if not found or not failed.
pub fn delete_failed_child(db: &Database, instance_id: &str) -> Result<bool, GatewayError> {
    db.with_connection(|conn| {
        let rows = conn.execute(
            "DELETE FROM children WHERE instance_id = ?1 AND status = 'failed'",
            params![instance_id],
        )?;
        Ok(rows > 0)
    })
}

/// Force-delete a child record regardless of status.
/// Returns `Ok(true)` if a row was deleted, `Ok(false)` if not found.
pub fn delete_child(db: &Database, instance_id: &str) -> Result<bool, GatewayError> {
    db.with_connection(|conn| {
        let rows = conn.execute(
            "DELETE FROM children WHERE instance_id = ?1",
            params![instance_id],
        )?;
        Ok(rows > 0)
    })
}

/// Update a child's status.
pub fn update_child_status(
    db: &Database,
    instance_id: &str,
    status: &str,
) -> Result<(), GatewayError> {
    let now = chrono::Utc::now().timestamp();

    db.with_connection(|conn| {
        conn.execute(
            "UPDATE children SET status = ?1, updated_at = ?2 WHERE instance_id = ?3",
            params![status, now, instance_id],
        )?;
        Ok(())
    })
}

/// Count children from a direct rusqlite connection.
#[allow(dead_code)]
pub fn query_children_count(conn: &rusqlite::Connection) -> Result<u32, rusqlite::Error> {
    conn.query_row("SELECT COUNT(*) FROM children", params![], |row| row.get(0))
}
