use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("poisoned lock")]
    Poisoned,
    #[error("blocking task failed: {0}")]
    Join(String),
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS apiKeys (id TEXT PRIMARY KEY, key TEXT UNIQUE NOT NULL, name TEXT, machineId TEXT, isActive INTEGER DEFAULT 1, createdAt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS combos (id TEXT PRIMARY KEY, name TEXT UNIQUE NOT NULL, kind TEXT, models TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS kv (scope TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (scope, key));
CREATE TABLE IF NOT EXISTS providerConnections (id TEXT PRIMARY KEY, provider TEXT NOT NULL, authType TEXT NOT NULL, name TEXT, email TEXT, priority INTEGER, isActive INTEGER DEFAULT 1, data TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS providerNodes (id TEXT PRIMARY KEY, type TEXT, name TEXT, data TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS proxyPools (id TEXT PRIMARY KEY, isActive INTEGER DEFAULT 1, testStatus TEXT, data TEXT NOT NULL, createdAt TEXT NOT NULL, updatedAt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS requestDetails (id TEXT PRIMARY KEY, timestamp TEXT NOT NULL, provider TEXT, model TEXT, connectionId TEXT, status TEXT, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK (id = 1), data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usageDaily (dateKey TEXT PRIMARY KEY, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS usageHistory (id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp TEXT NOT NULL, provider TEXT, model TEXT, connectionId TEXT, apiKey TEXT, endpoint TEXT, promptTokens INTEGER DEFAULT 0, completionTokens INTEGER DEFAULT 0, cost REAL DEFAULT 0, status TEXT, tokens TEXT, meta TEXT);
";

pub fn migrate(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}

pub fn open_memory() -> Result<Connection, DbError> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn is_valid_api_key(conn: &Connection, key: &str) -> Result<bool, DbError> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM apiKeys WHERE key = ?1 AND isActive = 1",
        params![key],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// A model combo configuration representing a virtual model routing to a sequence of models.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Combo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub models: Vec<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

/// A provider credential/account row, mirroring the original `providerConnections`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConnection {
    pub id: String,
    pub provider: String,
    #[serde(rename = "authType")]
    pub auth_type: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(rename = "isActive", default = "default_true")]
    pub is_active: bool,
    pub data: Value,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
}

fn default_true() -> bool {
    true
}

/// Thread-safe sqlite handle. All operations are short; use \`spawn_blocking\`
/// from async contexts via the \`*_async\` helpers.
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &str) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_memory() -> Result<Self, DbError> {
        Ok(Self {
            conn: Mutex::new(open_memory()?),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DbError> {
        self.conn.lock().map_err(|_| DbError::Poisoned)
    }

    pub fn list_api_keys(&self) -> Result<Vec<String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT key FROM apiKeys WHERE isActive = 1")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn upsert_connection(&self, c: &ProviderConnection) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO providerConnections (id, provider, authType, name, email, priority, isActive, data, createdAt, updatedAt)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, email=excluded.email, priority=excluded.priority,
               isActive=excluded.isActive, data=excluded.data, updatedAt=excluded.updatedAt",
            params![
                c.id,
                c.provider,
                c.auth_type,
                c.name,
                c.email,
                c.priority,
                if c.is_active { 1 } else { 0 },
                serde_json::to_string(&c.data)?,
                c.created_at,
                c.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn list_connections(
        &self,
        provider: Option<&str>,
    ) -> Result<Vec<ProviderConnection>, DbError> {
        let conn = self.lock()?;
        let (sql, filter) = match provider {
            Some(p) => (
                "SELECT id, provider, authType, name, email, priority, isActive, data, createdAt, updatedAt FROM providerConnections WHERE provider = ?1 ORDER BY priority IS NULL, priority, createdAt",
                Some(p.to_string()),
            ),
            None => (
                "SELECT id, provider, authType, name, email, priority, isActive, data, createdAt, updatedAt FROM providerConnections ORDER BY provider, priority IS NULL, priority, createdAt",
                None,
            ),
        };
        let mut stmt = conn.prepare(sql)?;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<ProviderConnection> {
            let data: String = r.get(7)?;
            Ok(ProviderConnection {
                id: r.get(0)?,
                provider: r.get(1)?,
                auth_type: r.get(2)?,
                name: r.get(3)?,
                email: r.get(4)?,
                priority: r.get(5)?,
                is_active: r.get::<_, i64>(6)? != 0,
                data: serde_json::from_str(&data).unwrap_or(Value::Null),
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
            })
        };
        let rows = match filter {
            Some(p) => stmt
                .query_map(params![p], map)?
                .filter_map(Result::ok)
                .collect(),
            None => stmt.query_map([], map)?.filter_map(Result::ok).collect(),
        };
        Ok(rows)
    }

    pub fn get_connection(&self, id: &str) -> Result<Option<ProviderConnection>, DbError> {
        Ok(self
            .list_connections(None)?
            .into_iter()
            .find(|c| c.id == id))
    }

    pub fn delete_connection(&self, id: &str) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute("DELETE FROM providerConnections WHERE id = ?1", params![id])?)
    }

    pub fn update_connection_data(
        &self,
        id: &str,
        data: &Value,
        now: &str,
    ) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE providerConnections SET data = ?1, updatedAt = ?2 WHERE id = ?3",
            params![serde_json::to_string(data)?, now, id],
        )?)
    }

    pub fn set_connection_active(&self, id: &str, active: bool) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "UPDATE providerConnections SET isActive = ?1 WHERE id = ?2",
            params![if active { 1 } else { 0 }, id],
        )?)
    }

    pub fn kv_set(&self, scope: &str, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO kv (scope, key, value) VALUES (?1,?2,?3)
             ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value",
            params![scope, key, value],
        )?;
        Ok(())
    }

    pub fn kv_get(&self, scope: &str, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT value FROM kv WHERE scope = ?1 AND key = ?2")?;
        let mut rows = stmt.query(params![scope, key])?;
        match rows.next()? {
            Some(r) => Ok(Some(r.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn kv_delete(&self, scope: &str, key: &str) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "DELETE FROM kv WHERE scope = ?1 AND key = ?2",
            params![scope, key],
        )?)
    }

    pub fn list_combos(&self) -> Result<Vec<Combo>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, models, createdAt, updatedAt FROM combos ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| {
            let models_json: String = r.get(3)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
            Ok(Combo {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                models,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
            })
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn get_combo(&self, id: &str) -> Result<Option<Combo>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, models, createdAt, updatedAt FROM combos WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(r) = rows.next()? {
            let models_json: String = r.get(3)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
            Ok(Some(Combo {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                models,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_combo_by_name(&self, name: &str) -> Result<Option<Combo>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, models, createdAt, updatedAt FROM combos WHERE name = ?1",
        )?;
        let mut rows = stmt.query(params![name])?;
        if let Some(r) = rows.next()? {
            let models_json: String = r.get(3)?;
            let models: Vec<String> = serde_json::from_str(&models_json).unwrap_or_default();
            Ok(Some(Combo {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                models,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn upsert_combo(&self, c: &Combo) -> Result<(), DbError> {
        let conn = self.lock()?;
        let models_json = serde_json::to_string(&c.models)?;
        conn.execute(
            "INSERT INTO combos (id, name, kind, models, createdAt, updatedAt)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, kind=excluded.kind,
               models=excluded.models, updatedAt=excluded.updatedAt",
            params![
                c.id,
                c.name,
                c.kind,
                models_json,
                c.created_at,
                c.updated_at
            ],
        )?;
        Ok(())
    }

    pub fn delete_combo(&self, id: &str) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute("DELETE FROM combos WHERE id = ?1", params![id])?)
    }

    pub fn get_model_aliases(&self) -> Result<std::collections::HashMap<String, String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT key, value FROM kv WHERE scope = 'modelAliases'")?;
        let rows = stmt.query_map([], |r| {
            let k: String = r.get(0)?;
            let v: String = r.get(1)?;
            // If value is a JSON string (e.g. ""target""), unwrap it
            let target: String = serde_json::from_str(&v).unwrap_or(v);
            Ok((k, target))
        })?;
        let mut map = std::collections::HashMap::new();
        for r in rows.filter_map(Result::ok) {
            map.insert(r.0, r.1);
        }
        Ok(map)
    }

    pub fn set_model_alias(&self, alias: &str, target: &str) -> Result<(), DbError> {
        let conn = self.lock()?;
        let val = serde_json::to_string(target)?;
        conn.execute(
            "INSERT INTO kv (scope, key, value) VALUES ('modelAliases', ?1, ?2)
             ON CONFLICT(scope, key) DO UPDATE SET value = excluded.value",
            params![alias, val],
        )?;
        Ok(())
    }

    pub fn delete_model_alias(&self, alias: &str) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute(
            "DELETE FROM kv WHERE scope = 'modelAliases' AND key = ?1",
            params![alias],
        )?)
    }

    pub fn record_usage(
        &self,
        provider: &str,
        model: &str,
        connection_id: Option<&str>,
        endpoint: &str,
        status: &str,
    ) -> Result<(), DbError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO usageHistory (timestamp, provider, model, connectionId, endpoint, status)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                chrono::Utc::now().to_rfc3339(),
                provider,
                model,
                connection_id,
                endpoint,
                status
            ],
        )?;
        Ok(())
    }

    pub fn usage_totals(&self) -> Result<Value, DbError> {
        let conn = self.lock()?;
        let (requests, prompt, completion, cost): (i64, i64, i64, f64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(promptTokens),0), COALESCE(SUM(completionTokens),0), COALESCE(SUM(cost),0) FROM usageHistory",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        Ok(serde_json::json!({
            "totalRequests": requests,
            "totalPromptTokens": prompt,
            "totalCompletionTokens": completion
            ,"totalCachedTokens": 0,
            "totalCost": cost,
            "byProvider": {},
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_rec(id: &str, provider: &str, priority: i64) -> ProviderConnection {
        let now = chrono::Utc::now().to_rfc3339();
        ProviderConnection {
            id: id.into(),
            provider: provider.into(),
            auth_type: "oauth".into(),
            name: Some("acct".into()),
            email: None,
            priority: Some(priority),
            is_active: true,
            data: serde_json::json!({"access_token": "secret-value", "expires_at": 123}),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    #[test]
    fn schema_bootstraps() {
        let c = open_memory().unwrap();
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(n >= 11);
    }

    #[test]
    fn api_key_lookup() {
        let c = open_memory().unwrap();
        c.execute(
            "INSERT INTO apiKeys (id, key, createdAt) VALUES ('1','sk-test','t')",
            [],
        )
        .unwrap();
        assert!(is_valid_api_key(&c, "sk-test").unwrap());
        assert!(!is_valid_api_key(&c, "nope").unwrap());
    }

    #[test]
    fn connection_roundtrip_and_ordering() {
        let s = Store::open_memory().unwrap();
        s.upsert_connection(&conn_rec("b", "codex", 5)).unwrap();
        s.upsert_connection(&conn_rec("a", "codex", 1)).unwrap();
        s.upsert_connection(&conn_rec("z", "cursor", 1)).unwrap();
        let codex = s.list_connections(Some("codex")).unwrap();
        assert_eq!(codex.len(), 2);
        assert_eq!(codex[0].id, "a", "lower priority first");
        assert_eq!(s.list_connections(None).unwrap().len(), 3);
        assert!(s.get_connection("a").unwrap().is_some());
        assert!(s.get_connection("missing").unwrap().is_none());
    }

    #[test]
    fn connection_update_delete_and_deactivate() {
        let s = Store::open_memory().unwrap();
        s.upsert_connection(&conn_rec("a", "codex", 1)).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let n = s
            .update_connection_data("a", &serde_json::json!({"access_token": "new"}), &now)
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(
            s.get_connection("a").unwrap().unwrap().data["access_token"],
            "new"
        );
        assert_eq!(s.set_connection_active("a", false).unwrap(), 1);
        assert!(!s.get_connection("a").unwrap().unwrap().is_active);
        assert_eq!(s.delete_connection("a").unwrap(), 1);
        assert!(s.get_connection("a").unwrap().is_none());
    }

    #[test]
    fn kv_roundtrip() {
        let s = Store::open_memory().unwrap();
        s.kv_set("oauth_state", "st", "{\"v\":1}").unwrap();
        assert_eq!(
            s.kv_get("oauth_state", "st").unwrap().as_deref(),
            Some("{\"v\":1}")
        );
        s.kv_set("oauth_state", "st", "{\"v\":2}").unwrap();
        assert_eq!(
            s.kv_get("oauth_state", "st").unwrap().as_deref(),
            Some("{\"v\":2}")
        );
        assert_eq!(s.kv_delete("oauth_state", "st").unwrap(), 1);
        assert!(s.kv_get("oauth_state", "st").unwrap().is_none());
    }

    #[test]
    fn combo_crud_lifecycle() {
        let s = Store::open_memory().unwrap();
        let c = Combo {
            id: "combo-1".into(),
            name: "fast-blend".into(),
            kind: Some("llm".into()),
            models: vec!["gpt-4o-mini".into(), "claude-3-5-haiku".into()],
            created_at: "2026-09-13T00:00:00Z".into(),
            updated_at: "2026-09-13T00:00:00Z".into(),
        };
        s.upsert_combo(&c).unwrap();
        let list = s.list_combos().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "fast-blend");
        assert_eq!(list[0].models.len(), 2);

        let by_name = s.get_combo_by_name("fast-blend").unwrap().unwrap();
        assert_eq!(by_name.id, "combo-1");

        let by_id = s.get_combo("combo-1").unwrap().unwrap();
        assert_eq!(by_id.name, "fast-blend");

        assert_eq!(s.delete_combo("combo-1").unwrap(), 1);
        assert!(s.get_combo("combo-1").unwrap().is_none());
    }

    #[test]
    fn model_alias_crud_lifecycle() {
        let s = Store::open_memory().unwrap();
        s.set_model_alias("gpt", "openai/gpt-4o").unwrap();
        s.set_model_alias("claude", "anthropic/claude-sonnet-4-6")
            .unwrap();
        let aliases = s.get_model_aliases().unwrap();
        assert_eq!(
            aliases.get("gpt").map(|s| s.as_str()),
            Some("openai/gpt-4o")
        );
        assert_eq!(
            aliases.get("claude").map(|s| s.as_str()),
            Some("anthropic/claude-sonnet-4-6")
        );

        assert_eq!(s.delete_model_alias("gpt").unwrap(), 1);
        let updated = s.get_model_aliases().unwrap();
        assert_eq!(updated.get("gpt"), None);
        assert_eq!(
            updated.get("claude").map(|s| s.as_str()),
            Some("anthropic/claude-sonnet-4-6")
        );
    }

    #[test]
    fn usage_totals_accumulate() {
        let s = Store::open_memory().unwrap();
        s.record_usage("openai", "gpt-4o", None, "/v1/chat/completions", "ok")
            .unwrap();
        s.record_usage("anthropic", "claude", None, "/v1/messages", "ok")
            .unwrap();
        assert_eq!(s.usage_totals().unwrap()["totalRequests"], 2);
    }
}
