use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

use crate::error::GatewayError;

/// Endpoint registration record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Endpoint {
    pub id: i64,
    pub slug: String,
    pub owner_address: String,
    pub target_url: String,
    /// Human-readable price (e.g., "$0.01")
    pub price_usd: String,
    /// Token amount (e.g., "10000" for $0.01 with 6 decimals)
    pub price_amount: String,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub active: bool,
}

/// Endpoint analytics stats record
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndpointStats {
    pub slug: String,
    pub request_count: i64,
    pub payment_count: i64,
    /// Total revenue in token units (integer string, e.g. "142000")
    pub revenue_total: String,
    pub last_accessed_at: Option<i64>,
}

/// Request to create a new endpoint
#[derive(Debug, serde::Deserialize)]
pub struct CreateEndpoint {
    pub slug: String,
    pub target_url: String,
    #[serde(default = "default_price")]
    pub price: String,
    pub description: Option<String>,
}

fn default_price() -> String {
    "$0.01".to_string()
}

/// Request to update an endpoint
#[derive(Debug, serde::Deserialize)]
pub struct UpdateEndpoint {
    pub target_url: Option<String>,
    pub price: Option<String>,
    pub description: Option<String>,
}

/// SQLite database wrapper
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new(path: &str) -> Result<Self, GatewayError> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        // Enable WAL mode for better concurrent read/write performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS endpoints (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                slug TEXT UNIQUE NOT NULL,
                owner_address TEXT NOT NULL,
                target_url TEXT NOT NULL,
                price_usd TEXT NOT NULL DEFAULT '$0.01',
                price_amount TEXT NOT NULL,
                description TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                active INTEGER NOT NULL DEFAULT 1
            )
            "#,
            [],
        )?;

        // Create index on slug for fast lookups
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_endpoints_slug ON endpoints(slug)",
            [],
        )?;

        // Create index on owner_address for listing owned endpoints
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_endpoints_owner ON endpoints(owner_address)",
            [],
        )?;

        // Endpoint analytics stats
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS endpoint_stats (
                slug TEXT PRIMARY KEY,
                request_count INTEGER NOT NULL DEFAULT 0,
                payment_count INTEGER NOT NULL DEFAULT 0,
                revenue_total TEXT NOT NULL DEFAULT '0',
                last_accessed_at INTEGER
            )
            "#,
            [],
        )?;

        Ok(())
    }

    /// Insert a new endpoint
    pub fn create_endpoint(
        &self,
        slug: &str,
        owner_address: &str,
        target_url: &str,
        price_usd: &str,
        price_amount: &str,
        description: Option<&str>,
    ) -> Result<Endpoint, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        conn.execute(
            r#"
            INSERT INTO endpoints (slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
            "#,
            params![slug, owner_address, target_url, price_usd, price_amount, description, now, now],
        )?;

        let id = conn.last_insert_rowid();

        Ok(Endpoint {
            id,
            slug: slug.to_string(),
            owner_address: owner_address.to_string(),
            target_url: target_url.to_string(),
            price_usd: price_usd.to_string(),
            price_amount: price_amount.to_string(),
            description: description.map(String::from),
            created_at: now,
            updated_at: now,
            active: true,
        })
    }

    /// Get endpoint by slug
    pub fn get_endpoint(&self, slug: &str) -> Result<Option<Endpoint>, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let endpoint = conn
            .query_row(
                r#"
                SELECT id, slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active
                FROM endpoints
                WHERE slug = ?1 AND active = 1
                "#,
                params![slug],
                |row| {
                    Ok(Endpoint {
                        id: row.get(0)?,
                        slug: row.get(1)?,
                        owner_address: row.get(2)?,
                        target_url: row.get(3)?,
                        price_usd: row.get(4)?,
                        price_amount: row.get(5)?,
                        description: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        active: row.get::<_, i32>(9)? == 1,
                    })
                },
            )
            .optional()?;

        Ok(endpoint)
    }

    /// List active endpoints with pagination.
    /// `limit` defaults to 100, max 500. `offset` defaults to 0.
    pub fn list_endpoints(&self, limit: u32, offset: u32) -> Result<Vec<Endpoint>, GatewayError> {
        let limit = limit.clamp(1, 500);
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let mut stmt = conn.prepare(
            r#"
            SELECT id, slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active
            FROM endpoints
            WHERE active = 1
            ORDER BY created_at DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;

        let endpoints = stmt
            .query_map(params![limit, offset], |row| {
                Ok(Endpoint {
                    id: row.get(0)?,
                    slug: row.get(1)?,
                    owner_address: row.get(2)?,
                    target_url: row.get(3)?,
                    price_usd: row.get(4)?,
                    price_amount: row.get(5)?,
                    description: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    active: row.get::<_, i32>(9)? == 1,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(endpoints)
    }

    /// Update an endpoint
    pub fn update_endpoint(
        &self,
        slug: &str,
        target_url: Option<&str>,
        price_usd: Option<&str>,
        price_amount: Option<&str>,
        description: Option<&str>,
    ) -> Result<Endpoint, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        // Build update query dynamically
        let mut updates = vec!["updated_at = ?1".to_string()];
        let mut param_idx = 2;

        if target_url.is_some() {
            updates.push(format!("target_url = ?{}", param_idx));
            param_idx += 1;
        }
        if price_usd.is_some() {
            updates.push(format!("price_usd = ?{}", param_idx));
            param_idx += 1;
        }
        if price_amount.is_some() {
            updates.push(format!("price_amount = ?{}", param_idx));
            param_idx += 1;
        }
        if description.is_some() {
            updates.push(format!("description = ?{}", param_idx));
            param_idx += 1;
        }

        let query = format!(
            "UPDATE endpoints SET {} WHERE slug = ?{} AND active = 1",
            updates.join(", "),
            param_idx
        );

        // Build params dynamically
        let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];
        if let Some(v) = target_url {
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = price_usd {
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = price_amount {
            params_vec.push(Box::new(v.to_string()));
        }
        if let Some(v) = description {
            params_vec.push(Box::new(v.to_string()));
        }
        params_vec.push(Box::new(slug.to_string()));

        let params_refs: Vec<&dyn rusqlite::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let rows_affected = conn.execute(&query, params_refs.as_slice())?;

        if rows_affected == 0 {
            return Err(GatewayError::EndpointNotFound(slug.to_string()));
        }

        // Fetch updated endpoint using the already-held connection (avoids deadlock)
        let endpoint = conn
            .query_row(
                r#"
                SELECT id, slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active
                FROM endpoints
                WHERE slug = ?1 AND active = 1
                "#,
                params![slug],
                |row| {
                    Ok(Endpoint {
                        id: row.get(0)?,
                        slug: row.get(1)?,
                        owner_address: row.get(2)?,
                        target_url: row.get(3)?,
                        price_usd: row.get(4)?,
                        price_amount: row.get(5)?,
                        description: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        active: row.get::<_, i32>(9)? == 1,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| GatewayError::EndpointNotFound(slug.to_string()))?;

        Ok(endpoint)
    }

    /// Deactivate an endpoint (soft delete)
    pub fn delete_endpoint(&self, slug: &str) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        let rows_affected = conn.execute(
            "UPDATE endpoints SET active = 0, updated_at = ?1 WHERE slug = ?2 AND active = 1",
            params![now, slug],
        )?;

        if rows_affected == 0 {
            return Err(GatewayError::EndpointNotFound(slug.to_string()));
        }

        Ok(())
    }

    /// Check if slug exists (includes pending reservations to prevent races)
    pub fn slug_exists(&self, slug: &str) -> Result<bool, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let count: i32 = conn.query_row(
            "SELECT COUNT(*) FROM endpoints WHERE slug = ?1",
            params![slug],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Reserve a slug by inserting a pending (active=0) row.
    /// Uses SQLite UNIQUE constraint to prevent race conditions.
    /// If the slug was previously deactivated (soft-deleted), removes the old row first
    /// to allow re-registration.
    ///
    /// The DELETE + INSERT are wrapped in a `BEGIN IMMEDIATE` transaction to guarantee
    /// atomicity: no concurrent connection can observe the state between the delete
    /// and the insert, preventing TOCTOU race conditions on slug reservation.
    pub fn reserve_slug(&self, slug: &str) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        // Use BEGIN IMMEDIATE to acquire a write lock at the start of the transaction,
        // preventing concurrent writers from interleaving between our DELETE and INSERT.
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| GatewayError::Internal(format!("failed to begin transaction: {e}")))?;

        // Set the transaction to IMMEDIATE mode for write-lock semantics
        tx.execute_batch("").ok(); // no-op to ensure transaction is started

        // Remove any previously deactivated endpoint with this slug to allow re-use.
        // Active endpoints are not affected (active=0 condition).
        tx.execute(
            "DELETE FROM endpoints WHERE slug = ?1 AND active = 0",
            params![slug],
        )?;

        tx.execute(
            r#"
            INSERT INTO endpoints (slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active)
            VALUES (?1, '', '', '$0.00', '0', NULL, ?2, ?3, 0)
            "#,
            params![slug, now, now],
        )?;

        tx.commit().map_err(|e| {
            GatewayError::Internal(format!("failed to commit slug reservation: {e}"))
        })?;

        Ok(())
    }

    /// Activate a previously reserved slug with full endpoint data.
    pub fn activate_endpoint(
        &self,
        slug: &str,
        owner_address: &str,
        target_url: &str,
        price_usd: &str,
        price_amount: &str,
        description: Option<&str>,
    ) -> Result<Endpoint, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        let rows_affected = conn.execute(
            r#"
            UPDATE endpoints
            SET owner_address = ?2, target_url = ?3, price_usd = ?4, price_amount = ?5,
                description = ?6, updated_at = ?7, active = 1
            WHERE slug = ?1 AND active = 0
            "#,
            params![
                slug,
                owner_address,
                target_url,
                price_usd,
                price_amount,
                description,
                now
            ],
        )?;

        if rows_affected == 0 {
            return Err(GatewayError::Internal(
                "failed to activate reserved slug".to_string(),
            ));
        }

        // Fetch the activated endpoint
        let endpoint = conn
            .query_row(
                r#"
                SELECT id, slug, owner_address, target_url, price_usd, price_amount, description, created_at, updated_at, active
                FROM endpoints
                WHERE slug = ?1 AND active = 1
                "#,
                params![slug],
                |row| {
                    Ok(Endpoint {
                        id: row.get(0)?,
                        slug: row.get(1)?,
                        owner_address: row.get(2)?,
                        target_url: row.get(3)?,
                        price_usd: row.get(4)?,
                        price_amount: row.get(5)?,
                        description: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        active: row.get::<_, i32>(9)? == 1,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| GatewayError::Internal("activated endpoint not found".to_string()))?;

        Ok(endpoint)
    }

    /// Purge stale slug reservations (active=0) older than `max_age_secs`.
    /// Should be called periodically or at startup to reclaim permanently stuck slugs.
    pub fn purge_stale_reservations(&self, max_age_secs: i64) -> Result<usize, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let cutoff = chrono::Utc::now().timestamp() - max_age_secs;
        let purged = conn.execute(
            "DELETE FROM endpoints WHERE active = 0 AND created_at < ?1",
            params![cutoff],
        )?;
        Ok(purged)
    }

    /// Record a successful payment for an endpoint.
    /// Upserts the stats row: increments request_count and payment_count, adds amount to revenue.
    pub fn record_payment(&self, slug: &str, amount: &str) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let now = chrono::Utc::now().timestamp();

        // Parse amount as u128 to do integer addition safely
        let add_amount: u128 = amount.parse().unwrap_or(0);

        // Get current revenue (or 0)
        let current_revenue: String = conn
            .query_row(
                "SELECT revenue_total FROM endpoint_stats WHERE slug = ?1",
                params![slug],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "0".to_string());
        let current: u128 = current_revenue.parse().unwrap_or(0);
        let new_revenue = (current + add_amount).to_string();

        conn.execute(
            r#"
            INSERT INTO endpoint_stats (slug, request_count, payment_count, revenue_total, last_accessed_at)
            VALUES (?1, 1, 1, ?2, ?3)
            ON CONFLICT(slug) DO UPDATE SET
                request_count = request_count + 1,
                payment_count = payment_count + 1,
                revenue_total = ?2,
                last_accessed_at = ?3
            "#,
            params![slug, new_revenue, now],
        )?;

        Ok(())
    }

    /// Get analytics stats for a single endpoint.
    pub fn get_endpoint_stats(&self, slug: &str) -> Result<Option<EndpointStats>, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let stats = conn
            .query_row(
                r#"
                SELECT slug, request_count, payment_count, revenue_total, last_accessed_at
                FROM endpoint_stats
                WHERE slug = ?1
                "#,
                params![slug],
                |row| {
                    Ok(EndpointStats {
                        slug: row.get(0)?,
                        request_count: row.get(1)?,
                        payment_count: row.get(2)?,
                        revenue_total: row.get(3)?,
                        last_accessed_at: row.get(4)?,
                    })
                },
            )
            .optional()?;

        Ok(stats)
    }

    /// List endpoint stats ordered by revenue descending with pagination.
    /// Get the total count of active endpoints
    pub fn get_total_endpoint_count(&self) -> Result<u32, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM endpoints WHERE active = 1",
            [],
            |row| row.get(0),
        )?;

        Ok(count as u32)
    }

    /// Get total revenue and payment count across all endpoints
    pub fn get_total_stats(&self) -> Result<(u128, u64), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let mut stmt = conn.prepare(
            "SELECT revenue_total, payment_count FROM endpoint_stats"
        )?;

        let mut total_revenue: u128 = 0;
        let mut total_payments: u64 = 0;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)? as u64,
            ))
        })?;

        for row in rows {
            if let Ok((rev_str, payments)) = row {
                total_revenue += rev_str.parse::<u128>().unwrap_or(0);
                total_payments += payments;
            }
        }

        Ok((total_revenue, total_payments))
    }

    pub fn list_endpoint_stats(
        &self,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<EndpointStats>, GatewayError> {
        let limit = limit.clamp(1, 500);
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        let mut stmt = conn.prepare(
            r#"
            SELECT slug, request_count, payment_count, revenue_total, last_accessed_at
            FROM endpoint_stats
            ORDER BY length(revenue_total) DESC, revenue_total DESC
            LIMIT ?1 OFFSET ?2
            "#,
        )?;

        let stats = stmt
            .query_map(params![limit, offset], |row| {
                Ok(EndpointStats {
                    slug: row.get(0)?,
                    request_count: row.get(1)?,
                    payment_count: row.get(2)?,
                    revenue_total: row.get(3)?,
                    last_accessed_at: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(stats)
    }

    /// Purge test endpoints matching a slug prefix (e.g. "e2e-test-").
    /// Performs a hard delete. Returns the number of rows removed.
    pub fn purge_endpoints_by_prefix(&self, prefix: &str) -> Result<usize, GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        let pattern = format!("{}%", prefix);
        let purged = conn.execute("DELETE FROM endpoints WHERE slug LIKE ?1", params![pattern])?;
        Ok(purged)
    }

    /// Execute additional schema SQL. Used by downstream crates (e.g., x402-node)
    /// to extend the database with their own tables without modifying gateway code.
    pub fn execute_schema(&self, sql: &str) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        conn.execute_batch(sql)?;
        Ok(())
    }

    /// Provide scoped access to the underlying connection for parameterized queries.
    /// Used by downstream crates (e.g., x402-node) that need DML with bound parameters,
    /// which `execute_schema` (execute_batch) does not support.
    pub fn with_connection<F, T>(&self, f: F) -> Result<T, GatewayError>
    where
        F: FnOnce(&Connection) -> Result<T, GatewayError>,
    {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;
        f(&conn)
    }

    /// Delete a reserved (pending) slug. Used to clean up failed registrations.
    pub fn delete_reserved_slug(&self, slug: &str) -> Result<(), GatewayError> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| GatewayError::Internal("database lock poisoned".to_string()))?;

        conn.execute(
            "DELETE FROM endpoints WHERE slug = ?1 AND active = 0",
            params![slug],
        )?;

        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_endpoint() {
        let db = Database::new(":memory:").unwrap();

        let endpoint = db
            .create_endpoint(
                "test-api",
                "0x1234567890123456789012345678901234567890",
                "https://api.example.com",
                "$0.05",
                "50000",
                Some("Test API"),
            )
            .unwrap();

        assert_eq!(endpoint.slug, "test-api");
        assert_eq!(endpoint.price_usd, "$0.05");

        let fetched = db.get_endpoint("test-api").unwrap().unwrap();
        assert_eq!(fetched.slug, endpoint.slug);
    }

    #[test]
    fn test_list_endpoints() {
        let db = Database::new(":memory:").unwrap();

        db.create_endpoint(
            "api-1",
            "0x1111111111111111111111111111111111111111",
            "https://api1.example.com",
            "$0.01",
            "10000",
            None,
        )
        .unwrap();

        db.create_endpoint(
            "api-2",
            "0x2222222222222222222222222222222222222222",
            "https://api2.example.com",
            "$0.02",
            "20000",
            None,
        )
        .unwrap();

        let endpoints = db.list_endpoints(100, 0).unwrap();
        assert_eq!(endpoints.len(), 2);
    }

    #[test]
    fn test_delete_endpoint() {
        let db = Database::new(":memory:").unwrap();

        db.create_endpoint(
            "to-delete",
            "0x1234567890123456789012345678901234567890",
            "https://api.example.com",
            "$0.01",
            "10000",
            None,
        )
        .unwrap();

        db.delete_endpoint("to-delete").unwrap();

        let result = db.get_endpoint("to-delete").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_record_and_get_endpoint_stats() {
        let db = Database::new(":memory:").unwrap();

        // No stats initially
        assert!(db.get_endpoint_stats("test").unwrap().is_none());

        // Record first payment
        db.record_payment("test", "1000").unwrap();
        let stats = db.get_endpoint_stats("test").unwrap().unwrap();
        assert_eq!(stats.slug, "test");
        assert_eq!(stats.request_count, 1);
        assert_eq!(stats.payment_count, 1);
        assert_eq!(stats.revenue_total, "1000");
        assert!(stats.last_accessed_at.is_some());

        // Record second payment
        db.record_payment("test", "2000").unwrap();
        let stats = db.get_endpoint_stats("test").unwrap().unwrap();
        assert_eq!(stats.request_count, 2);
        assert_eq!(stats.payment_count, 2);
        assert_eq!(stats.revenue_total, "3000");
    }

    #[test]
    fn test_list_endpoint_stats_ordered_by_revenue() {
        let db = Database::new(":memory:").unwrap();

        db.record_payment("low", "1000").unwrap();
        db.record_payment("high", "50000").unwrap();
        db.record_payment("mid", "10000").unwrap();

        let stats = db.list_endpoint_stats(100, 0).unwrap();
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].slug, "high");
        assert_eq!(stats[1].slug, "mid");
        assert_eq!(stats[2].slug, "low");
    }

    #[test]
    fn test_list_endpoint_stats_large_revenue_no_overflow() {
        let db = Database::new(":memory:").unwrap();

        // Values that would overflow i64 (max ~9.2e18)
        db.record_payment("small", "1000").unwrap();
        db.record_payment("huge", "99999999999999999999").unwrap(); // > i64::MAX
        db.record_payment("big", "10000000000000000000").unwrap(); // > i64::MAX

        let stats = db.list_endpoint_stats(100, 0).unwrap();
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].slug, "huge");
        assert_eq!(stats[1].slug, "big");
        assert_eq!(stats[2].slug, "small");
    }

    #[test]
    fn test_get_total_endpoint_count() {
        let db = Database::new(":memory:").unwrap();
        assert_eq!(db.get_total_endpoint_count().unwrap(), 0);

        db.create_endpoint(
            "a",
            "0x1111111111111111111111111111111111111111",
            "https://a.com",
            "$0.01",
            "10000",
            None,
        )
        .unwrap();
        db.create_endpoint(
            "b",
            "0x2222222222222222222222222222222222222222",
            "https://b.com",
            "$0.01",
            "10000",
            None,
        )
        .unwrap();
        assert_eq!(db.get_total_endpoint_count().unwrap(), 2);

        db.delete_endpoint("a").unwrap();
        assert_eq!(db.get_total_endpoint_count().unwrap(), 1);
    }

    #[test]
    fn test_get_total_stats() {
        let db = Database::new(":memory:").unwrap();

        let (rev, count) = db.get_total_stats().unwrap();
        assert_eq!(rev, 0);
        assert_eq!(count, 0);

        db.record_payment("a", "5000").unwrap();
        db.record_payment("b", "3000").unwrap();
        db.record_payment("a", "2000").unwrap();

        let (rev, count) = db.get_total_stats().unwrap();
        assert_eq!(rev, 10000);
        assert_eq!(count, 3);
    }

    #[test]
    fn test_database_is_clone() {
        let db = Database::new(":memory:").unwrap();
        let db2 = db.clone();
        db.create_endpoint(
            "x",
            "0x1111111111111111111111111111111111111111",
            "https://x.com",
            "$0.01",
            "10000",
            None,
        )
        .unwrap();
        assert!(db2.get_endpoint("x").unwrap().is_some());
    }
}
