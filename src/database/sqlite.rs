//! SQLite storage layer.
//!
//! A single connection guarded by a Tokio mutex is enough for a personal router.
//! Tables: `api_keys`, `usage`, `providers`.

use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use uuid::Uuid;

use crate::config::ApiKeySeed;

/// Hash an API key with SHA-256 (hex). The plain key is never stored.
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return Err(anyhow!("invalid encrypted secret"));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(|e| anyhow!(e)))
        .collect()
}

fn derive_secret_key(master_key: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"router-api-ai/provider-secret/v1\0");
    hasher.update(master_key.as_bytes());
    hasher.finalize().into()
}

fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
    let exists = conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|name| name.as_deref() == Ok(column));
    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )?;
    }
    Ok(())
}

struct ProviderDbRow {
    id: String,
    kind: String,
    name: String,
    base_url: String,
    api_key_ciphertext: Option<String>,
    command: Option<String>,
    model: String,
    enabled: bool,
}

/// A stored API key record.
#[derive(Debug, Clone)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    /// Daily token quota. 0 = unlimited.
    pub quota_daily_tokens: i64,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

/// A single usage event row.
#[derive(Debug, Clone)]
pub struct UsageRow {
    pub id: i64,
    pub api_key_name: String,
    pub model: String,
    pub provider: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub latency_ms: i64,
    pub status: i64,
    pub created_at: DateTime<Utc>,
}

/// Aggregate usage for one key.
#[derive(Debug, Clone)]
pub struct KeyUsage {
    pub api_key_id: String,
    pub api_key_name: String,
    pub requests: i64,
    pub total_tokens: i64,
}

/// Aggregate usage for one provider.
#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub provider: String,
    pub requests: i64,
    pub total_tokens: i64,
}

/// Public provider status row. Secret material is represented only by the
/// `api_key_configured` boolean.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderRow {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub command: Option<String>,
    pub api_key_configured: bool,
    pub enabled: bool,
    pub failure_count: i64,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// A provider row plus the decrypted secret needed by the runtime router.
/// This type never crosses an admin response boundary.
#[derive(Debug, Clone)]
pub struct StoredProvider {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub command: Option<String>,
    pub api_key: Option<String>,
    pub enabled: bool,
}

/// Shared database handle.
#[derive(Debug, Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    secret_key: Arc<[u8; 32]>,
}

impl Db {
    /// Open (or create) the database file and run migrations.
    pub async fn open(path: &str) -> Result<Self> {
        Self::open_with_master_key(path, "router-api-ai-default-secret").await
    }

    /// Open a database using the router master key to derive the encryption key
    /// for stored upstream credentials.
    pub async fn open_with_master_key(path: &str, master_key: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            secret_key: Arc::new(derive_secret_key(master_key)),
        };
        db.migrate().await?;
        Ok(db)
    }

    /// Open an in-memory database (used by tests).
    pub async fn open_in_memory() -> Result<Self> {
        Self::open_in_memory_with_master_key("test-master").await
    }

    /// Open an in-memory database with an explicit credential encryption key.
    pub async fn open_in_memory_with_master_key(master_key: &str) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
            secret_key: Arc::new(derive_secret_key(master_key)),
        };
        db.migrate().await?;
        Ok(db)
    }

    async fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                key_hash TEXT NOT NULL UNIQUE,
                quota_daily_tokens INTEGER NOT NULL DEFAULT 0,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                api_key_id TEXT NOT NULL,
                model TEXT NOT NULL,
                provider TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                latency_ms INTEGER NOT NULL DEFAULT 0,
                status INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_usage_key_created ON usage(api_key_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_usage_created ON usage(created_at);

            CREATE TABLE IF NOT EXISTS providers (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL DEFAULT 'groq',
                name TEXT NOT NULL,
                base_url TEXT NOT NULL,
                api_key_ciphertext TEXT,
                command TEXT,
                model TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                failure_count INTEGER NOT NULL DEFAULT 0,
                cooldown_until TEXT,
                last_error TEXT,
                last_used_at TEXT
            );
            "#,
        )?;

        // Existing databases created before provider kinds/credential storage
        // need additive migrations. SQLite cannot add a column inside the
        // CREATE TABLE branch above when the table already exists.
        ensure_column(&conn, "providers", "kind", "TEXT NOT NULL DEFAULT 'groq'")?;
        ensure_column(&conn, "providers", "api_key_ciphertext", "TEXT")?;
        ensure_column(&conn, "providers", "command", "TEXT")?;
        Ok(())
    }

    fn encrypt_secret(&self, plaintext: &str) -> Result<String> {
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, self.secret_key.as_ref())
            .map_err(|_| anyhow!("failed to initialize credential encryption"))?;
        let key = aead::LessSafeKey::new(unbound);
        let mut nonce_bytes = [0_u8; 12];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| anyhow!("failed to generate credential nonce"))?;
        let nonce = aead::Nonce::assume_unique_for_key(nonce_bytes);
        let mut payload = plaintext.as_bytes().to_vec();
        key.seal_in_place_append_tag(
            nonce,
            aead::Aad::from(b"router-api-ai/provider-secret/v1".as_slice()),
            &mut payload,
        )
        .map_err(|_| anyhow!("failed to encrypt provider credential"))?;

        let mut encoded = hex_encode(&nonce_bytes);
        encoded.push_str(&hex_encode(&payload));
        Ok(encoded)
    }

    fn decrypt_secret(&self, ciphertext: &str) -> Result<String> {
        let bytes = hex_decode(ciphertext)?;
        if bytes.len() < 12 + aead::AES_256_GCM.tag_len() {
            return Err(anyhow!("invalid encrypted provider credential"));
        }
        let (nonce_bytes, encrypted) = bytes.split_at(12);
        let nonce = aead::Nonce::try_assume_unique_for_key(nonce_bytes)
            .map_err(|_| anyhow!("invalid encrypted provider nonce"))?;
        let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, self.secret_key.as_ref())
            .map_err(|_| anyhow!("failed to initialize credential decryption"))?;
        let key = aead::LessSafeKey::new(unbound);
        let mut encrypted = encrypted.to_vec();
        let plaintext = key
            .open_in_place(
                nonce,
                aead::Aad::from(b"router-api-ai/provider-secret/v1".as_slice()),
                &mut encrypted,
            )
            .map_err(|_| anyhow!("failed to decrypt provider credential"))?;
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| anyhow!("provider credential is not valid UTF-8"))
    }

    fn decode_provider(&self, row: ProviderDbRow) -> Result<StoredProvider> {
        let api_key = row
            .api_key_ciphertext
            .as_deref()
            .map(|value| self.decrypt_secret(value))
            .transpose()?;
        Ok(StoredProvider {
            id: row.id,
            kind: row.kind,
            name: row.name,
            base_url: row.base_url,
            model: row.model,
            command: row.command,
            api_key,
            enabled: row.enabled,
        })
    }

    // ---- api_keys ----

    /// Insert keys provided by the environment at startup (no-op if they already exist).
    pub async fn seed_api_keys(&self, seeds: &[ApiKeySeed]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        for seed in seeds {
            let hash = hash_key(&seed.key);
            conn.execute(
                "INSERT OR IGNORE INTO api_keys (id, name, key_hash, quota_daily_tokens, enabled, created_at)
                 VALUES (?1, ?2, ?3, 0, 1, ?4)",
                params![Uuid::new_v4().to_string(), seed.name, hash, now],
            )?;
        }
        Ok(())
    }

    /// Look up a key by its hash.
    pub async fn find_key_by_hash(&self, hash: &str) -> Result<Option<ApiKeyRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, key_hash, quota_daily_tokens, enabled, created_at
             FROM api_keys WHERE key_hash = ?1",
        )?;
        let mut rows = stmt.query(params![hash])?;
        let row = rows.next()?;
        let rec = match row {
            Some(r) => Some(ApiKeyRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                key_hash: r.get(2)?,
                quota_daily_tokens: r.get(3)?,
                enabled: r.get::<_, i64>(4)? != 0,
                created_at: r
                    .get::<_, String>(5)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            }),
            None => None,
        };
        Ok(rec)
    }

    /// Create a new key. Returns the full record.
    pub async fn insert_key(&self, name: &str, key: &str, quota: i64) -> Result<ApiKeyRecord> {
        let conn = self.conn.lock().unwrap();
        let id = Uuid::new_v4().to_string();
        let hash = hash_key(key);
        let now = Utc::now();
        conn.execute(
            "INSERT INTO api_keys (id, name, key_hash, quota_daily_tokens, enabled, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            params![id, name, hash, quota, now.to_rfc3339()],
        )?;
        Ok(ApiKeyRecord {
            id,
            name: name.to_string(),
            key_hash: hash,
            quota_daily_tokens: quota,
            enabled: true,
            created_at: now,
        })
    }

    /// List all keys (plain key is not included, only its hash prefix).
    pub async fn list_keys(&self) -> Result<Vec<ApiKeyRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, key_hash, quota_daily_tokens, enabled, created_at FROM api_keys ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ApiKeyRecord {
                id: r.get(0)?,
                name: r.get(1)?,
                key_hash: r.get(2)?,
                quota_daily_tokens: r.get(3)?,
                enabled: r.get::<_, i64>(4)? != 0,
                created_at: r
                    .get::<_, String>(5)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Set key quota and enabled state.
    pub async fn update_key(
        &self,
        id: &str,
        quota: Option<i64>,
        enabled: Option<bool>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(q) = quota {
            conn.execute(
                "UPDATE api_keys SET quota_daily_tokens = ?1 WHERE id = ?2",
                params![q, id],
            )?;
        }
        if let Some(e) = enabled {
            conn.execute(
                "UPDATE api_keys SET enabled = ?1 WHERE id = ?2",
                params![e as i64, id],
            )?;
        }
        Ok(())
    }

    pub async fn delete_key(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM api_keys WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Sum of total tokens used by a key since local midnight UTC.
    pub async fn usage_today(&self, api_key_id: &str) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        let start = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|d| d.and_utc().to_rfc3339())
            .unwrap_or_default();
        let total = conn.query_row(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM usage WHERE api_key_id = ?1 AND created_at >= ?2",
            params![api_key_id, start],
            |r| r.get::<_, i64>(0),
        )?;
        Ok(total)
    }

    // ---- usage ----

    /// Insert a usage row.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_usage(
        &self,
        api_key_id: &str,
        model: &str,
        provider: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        latency_ms: i64,
        status: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO usage (api_key_id, model, provider, prompt_tokens, completion_tokens, total_tokens, latency_ms, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                api_key_id,
                model,
                provider,
                prompt_tokens,
                completion_tokens,
                prompt_tokens + completion_tokens,
                latency_ms,
                status,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Synchronous variant used by the SSE usage tracker (called from a Stream poll).
    #[allow(clippy::too_many_arguments)]
    pub fn insert_usage_blocking(
        &self,
        api_key_id: &str,
        model: &str,
        provider: &str,
        prompt_tokens: i64,
        completion_tokens: i64,
        latency_ms: i64,
        status: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO usage (api_key_id, model, provider, prompt_tokens, completion_tokens, total_tokens, latency_ms, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                api_key_id,
                model,
                provider,
                prompt_tokens,
                completion_tokens,
                prompt_tokens + completion_tokens,
                latency_ms,
                status,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Per-key totals for the current UTC day.
    pub async fn usage_summary_today(&self) -> Result<Vec<KeyUsage>> {
        let conn = self.conn.lock().unwrap();
        let start = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|d| d.and_utc().to_rfc3339())
            .unwrap_or_default();
        let mut stmt = conn.prepare(
            "SELECT u.api_key_id, COALESCE(k.name, 'deleted'), COUNT(*), COALESCE(SUM(u.total_tokens), 0)
             FROM usage u LEFT JOIN api_keys k ON k.id = u.api_key_id
             WHERE u.created_at >= ?1
             GROUP BY u.api_key_id ORDER BY 4 DESC",
        )?;
        let rows = stmt.query_map(params![start], |r| {
            Ok(KeyUsage {
                api_key_id: r.get(0)?,
                api_key_name: r.get(1)?,
                requests: r.get(2)?,
                total_tokens: r.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Per-provider totals for the current UTC day.
    pub async fn provider_usage_today(&self) -> Result<Vec<ProviderUsage>> {
        let conn = self.conn.lock().unwrap();
        let start = Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|d| d.and_utc().to_rfc3339())
            .unwrap_or_default();
        let mut stmt = conn.prepare(
            "SELECT provider, COUNT(*), COALESCE(SUM(total_tokens), 0)
             FROM usage WHERE created_at >= ?1
             GROUP BY provider ORDER BY 3 DESC",
        )?;
        let rows = stmt.query_map(params![start], |r| {
            Ok(ProviderUsage {
                provider: r.get(0)?,
                requests: r.get(1)?,
                total_tokens: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Daily totals for the last `days` days (UTC).
    pub async fn usage_by_day(&self, days: i64) -> Result<Vec<(String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let since = (Utc::now() - chrono::Duration::days(days - 1))
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|d| d.and_utc().to_rfc3339())
            .unwrap_or_default();
        let mut stmt = conn.prepare(
            "SELECT substr(created_at, 1, 10) AS day, COALESCE(SUM(total_tokens), 0)
             FROM usage WHERE created_at >= ?1
             GROUP BY day ORDER BY day",
        )?;
        let rows = stmt.query_map(params![since], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Recent usage log joined with key names.
    pub async fn usage_log(&self, limit: i64, offset: i64) -> Result<Vec<UsageRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT u.id, COALESCE(k.name, 'deleted'), u.model, u.provider, u.prompt_tokens,
                    u.completion_tokens, u.total_tokens, u.latency_ms, u.status, u.created_at
             FROM usage u LEFT JOIN api_keys k ON k.id = u.api_key_id
             ORDER BY u.id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |r| {
            Ok(UsageRow {
                id: r.get(0)?,
                api_key_name: r.get(1)?,
                model: r.get(2)?,
                provider: r.get(3)?,
                prompt_tokens: r.get(4)?,
                completion_tokens: r.get(5)?,
                total_tokens: r.get(6)?,
                latency_ms: r.get(7)?,
                status: r.get(8)?,
                created_at: r
                    .get::<_, String>(9)?
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ---- providers ----

    /// Backwards-compatible helper used by tests and callers that do not have
    /// an upstream secret. New code should use `ensure_provider`.
    pub async fn upsert_provider(
        &self,
        id: &str,
        name: &str,
        base_url: &str,
        model: &str,
    ) -> Result<()> {
        self.ensure_provider(id, "groq", name, base_url, None, None, model)
            .await
    }

    /// Insert a provider if it does not exist yet. Existing dashboard values
    /// are preserved; environment seeding only fills missing credentials and
    /// command paths.
    #[allow(clippy::too_many_arguments)]
    pub async fn ensure_provider(
        &self,
        id: &str,
        kind: &str,
        name: &str,
        base_url: &str,
        api_key: Option<&str>,
        command: Option<String>,
        model: &str,
    ) -> Result<()> {
        let encrypted = api_key
            .filter(|key| !key.trim().is_empty())
            .map(|key| self.encrypt_secret(key))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, kind, name, base_url, api_key_ciphertext, command, model, enabled, failure_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0)
             ON CONFLICT(id) DO UPDATE SET
                 kind = excluded.kind,
                 name = excluded.name,
                 base_url = excluded.base_url,
                 model = excluded.model,
                 api_key_ciphertext = COALESCE(providers.api_key_ciphertext, excluded.api_key_ciphertext),
                 command = COALESCE(providers.command, excluded.command)",
            params![id, kind, name, base_url, encrypted, command, model],
        )?;
        Ok(())
    }

    /// Insert a dashboard-created provider. API keys are encrypted before the
    /// SQLite write and never returned by this method.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_provider(
        &self,
        id: &str,
        kind: &str,
        name: &str,
        base_url: &str,
        api_key: Option<&str>,
        command: Option<&str>,
        model: &str,
    ) -> Result<()> {
        let encrypted = api_key
            .filter(|key| !key.trim().is_empty())
            .map(|key| self.encrypt_secret(key))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO providers (id, kind, name, base_url, api_key_ciphertext, command, model, enabled, failure_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, 0)",
            params![id, kind, name, base_url, encrypted, command, model],
        )?;
        Ok(())
    }

    /// Update mutable provider configuration. `None` means keep the current
    /// value; a supplied `api_key` rotates the encrypted credential.
    pub async fn update_provider(
        &self,
        id: &str,
        name: Option<&str>,
        base_url: Option<&str>,
        api_key: Option<&str>,
        command: Option<&str>,
        model: Option<&str>,
    ) -> Result<bool> {
        let encrypted = api_key
            .filter(|key| !key.trim().is_empty())
            .map(|key| self.encrypt_secret(key))
            .transpose()?;
        let conn = self.conn.lock().unwrap();
        let mut changed = 0;
        if let Some(value) = name {
            conn.execute(
                "UPDATE providers SET name = ?1 WHERE id = ?2",
                params![value, id],
            )?;
            changed += conn.changes();
        }
        if let Some(value) = base_url {
            conn.execute(
                "UPDATE providers SET base_url = ?1 WHERE id = ?2",
                params![value, id],
            )?;
            changed += conn.changes();
        }
        if let Some(value) = encrypted {
            conn.execute(
                "UPDATE providers SET api_key_ciphertext = ?1 WHERE id = ?2",
                params![value, id],
            )?;
            changed += conn.changes();
        }
        if let Some(value) = command {
            conn.execute(
                "UPDATE providers SET command = ?1 WHERE id = ?2",
                params![value, id],
            )?;
            changed += conn.changes();
        }
        if let Some(value) = model {
            conn.execute(
                "UPDATE providers SET model = ?1 WHERE id = ?2",
                params![value, id],
            )?;
            changed += conn.changes();
        }
        Ok(changed > 0)
    }

    pub async fn delete_provider(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute("DELETE FROM providers WHERE id = ?1", params![id])? > 0)
    }

    pub async fn find_provider_config(&self, id: &str) -> Result<Option<StoredProvider>> {
        let conn = self.conn.lock().unwrap();
        let raw = conn
            .query_row(
                "SELECT id, kind, name, base_url, api_key_ciphertext, command, model, enabled
                 FROM providers WHERE id = ?1",
                params![id],
                |r| {
                    Ok(ProviderDbRow {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        name: r.get(2)?,
                        base_url: r.get(3)?,
                        api_key_ciphertext: r.get(4)?,
                        command: r.get(5)?,
                        model: r.get(6)?,
                        enabled: r.get::<_, i64>(7)? != 0,
                    })
                },
            )
            .optional()?;
        raw.map(|row| self.decode_provider(row)).transpose()
    }

    pub async fn list_provider_configs(&self) -> Result<Vec<StoredProvider>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, base_url, api_key_ciphertext, command, model, enabled
             FROM providers ORDER BY rowid",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ProviderDbRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                base_url: r.get(3)?,
                api_key_ciphertext: r.get(4)?,
                command: r.get(5)?,
                model: r.get(6)?,
                enabled: r.get::<_, i64>(7)? != 0,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(self.decode_provider(row?)?);
        }
        Ok(out)
    }

    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, kind, name, base_url, model, command, api_key_ciphertext, enabled, failure_count, cooldown_until, last_error, last_used_at
             FROM providers ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            let parse = |s: rusqlite::Result<String>| -> Option<DateTime<Utc>> {
                s.ok().and_then(|v| v.parse::<DateTime<Utc>>().ok())
            };
            Ok(ProviderRow {
                id: r.get(0)?,
                kind: r.get(1)?,
                name: r.get(2)?,
                base_url: r.get(3)?,
                model: r.get(4)?,
                command: r.get(5)?,
                api_key_configured: r.get::<_, Option<String>>(6)?.is_some(),
                enabled: r.get::<_, i64>(7)? != 0,
                failure_count: r.get(8)?,
                cooldown_until: parse(r.get(9)),
                last_error: r.get(10)?,
                last_used_at: parse(r.get(11)),
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Record a failure (increment counter, set cooldown until `now + secs`).
    pub async fn provider_failure(&self, id: &str, error: &str, cooldown_secs: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let until = (Utc::now() + chrono::Duration::seconds(cooldown_secs as i64)).to_rfc3339();
        conn.execute(
            "UPDATE providers SET failure_count = failure_count + 1, cooldown_until = ?1, last_error = ?2
             WHERE id = ?3",
            params![until, error, id],
        )?;
        Ok(())
    }

    /// Reset failure state after a successful call.
    pub async fn provider_success(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE providers SET cooldown_until = NULL, last_error = NULL, last_used_at = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(())
    }

    pub async fn set_provider_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE providers SET enabled = ?1 WHERE id = ?2",
            params![enabled as i64, id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ApiKeySeed;

    #[tokio::test]
    async fn key_crud_and_usage() {
        let db = Db::open_in_memory().await.unwrap();
        db.seed_api_keys(&[ApiKeySeed {
            name: "seed-1".into(),
            key: "sk-seed".into(),
        }])
        .await
        .unwrap();

        let found = db.find_key_by_hash(&hash_key("sk-seed")).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "seed-1");

        let created = db.insert_key("bot", "sk-bot", 1000).await.unwrap();
        assert!(created.enabled);

        db.insert_usage(&created.id, "llama-3.3-70b", "groq-1", 10, 5, 42, 200)
            .await
            .unwrap();
        assert_eq!(db.usage_today(&created.id).await.unwrap(), 15);

        let summary = db.usage_summary_today().await.unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].total_tokens, 15);

        let keys = db.list_keys().await.unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[tokio::test]
    async fn provider_failure_tracking() {
        let db = Db::open_in_memory().await.unwrap();
        db.upsert_provider(
            "groq-1",
            "Groq 1",
            "https://api.groq.com/openai/v1",
            "llama",
        )
        .await
        .unwrap();
        db.provider_failure("groq-1", "rate limited", 60)
            .await
            .unwrap();
        let providers = db.list_providers().await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].failure_count, 1);
        assert!(providers[0].cooldown_until.is_some());
        db.provider_success("groq-1").await.unwrap();
        let providers = db.list_providers().await.unwrap();
        assert!(providers[0].cooldown_until.is_none());
    }

    #[tokio::test]
    async fn provider_credentials_are_encrypted_at_rest() {
        let db = Db::open_in_memory_with_master_key("master").await.unwrap();
        let secret = "gsk-secret-value";
        db.insert_provider(
            "groq-dashboard-1",
            "groq",
            "Groq dashboard",
            "https://api.groq.com/openai/v1",
            Some(secret),
            None,
            "llama",
        )
        .await
        .unwrap();

        let ciphertext: String = db
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT api_key_ciphertext FROM providers WHERE id = 'groq-dashboard-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!ciphertext.contains(secret));
        assert_ne!(ciphertext, secret);

        let stored = db
            .find_provider_config("groq-dashboard-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.api_key.as_deref(), Some(secret));
        assert!(db.list_providers().await.unwrap()[0].api_key_configured);
    }
}
