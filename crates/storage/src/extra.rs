use super::{DbError, Store};

type ApiKeyRow = (String, String, Option<String>, Option<String>, i64, String);
use rusqlite::params;
use serde_json::Value;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

impl Store {
    pub fn settings_get(&self) -> Result<Value, DbError> {
        let conn = self.lock()?;
        let v: Option<String> = conn
            .query_row("SELECT data FROM settings WHERE id = 1", [], |r| r.get(0))
            .ok();
        Ok(v.and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null))
    }

    pub fn settings_merge(&self, patch: &Value) -> Result<Value, DbError> {
        let mut cur = self.settings_get()?;
        if !cur.is_object() {
            cur = Value::Object(Default::default());
        }
        if let (Some(m), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                m.insert(k.clone(), v.clone());
            }
        }
        let s = serde_json::to_string(&cur)?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (id, data) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![s],
        )?;
        Ok(cur)
    }

    pub fn kv_list(
        &self,
        scope: &str,
    ) -> Result<std::collections::HashMap<String, String>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT key, value FROM kv WHERE scope = ?1")?;
        let mut map = std::collections::HashMap::new();
        for r in stmt
            .query_map(params![scope], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .filter_map(Result::ok)
        {
            map.insert(r.0, r.1);
        }
        Ok(map)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_connection(
        &self,
        provider: &str,
        auth_type: &str,
        name: Option<&str>,
        email: Option<&str>,
        priority: Option<i64>,
        data: &Value,
    ) -> Result<super::ProviderConnection, DbError> {
        let t = now();
        let c = super::ProviderConnection {
            id: uuid::Uuid::new_v4().to_string(),
            provider: provider.into(),
            auth_type: auth_type.into(),
            name: name.map(String::from),
            email: email.map(String::from),
            priority,
            is_active: true,
            data: data.clone(),
            created_at: t.clone(),
            updated_at: t,
        };
        self.upsert_connection(&c)?;
        Ok(c)
    }

    pub fn update_connection_fields(
        &self,
        id: &str,
        patch: &Value,
    ) -> Result<Option<super::ProviderConnection>, DbError> {
        let Some(mut c) = self.get_connection(id)? else {
            return Ok(None);
        };
        if let Some(o) = patch.as_object() {
            for (k, v) in o {
                match k.as_str() {
                    "name" => c.name = v.as_str().map(String::from),
                    "email" => c.email = v.as_str().map(String::from),
                    "priority" => c.priority = v.as_i64(),
                    "isActive" => {
                        if let Some(b) = v.as_bool() {
                            c.is_active = b;
                        }
                    }
                    _ => {
                        if let Some(d) = c.data.as_object_mut() {
                            d.insert(k.clone(), v.clone());
                        }
                    }
                }
            }
        }
        c.updated_at = now();
        self.upsert_connection(&c)?;
        Ok(Some(c))
    }

    fn blob_list(&self, table: &str) -> Result<Vec<Value>, DbError> {
        let table = Self::checked_table(table)?;
        let conn = self.lock()?;
        let sql = format!("SELECT id, data FROM {table} ORDER BY id");
        let mut stmt = conn.prepare(&sql)?;
        let out: Vec<Value> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .filter_map(Result::ok)
            .map(|(id, data)| {
                let mut v: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
                if let Some(o) = v.as_object_mut() {
                    o.entry("id").or_insert(Value::String(id));
                }
                v
            })
            .collect();
        Ok(out)
    }

    fn blob_get(&self, table: &str, id: &str) -> Result<Option<Value>, DbError> {
        let table = Self::checked_table(table)?;
        let conn = self.lock()?;
        let sql = format!("SELECT data FROM {table} WHERE id = ?1");
        let v: Option<String> = conn.query_row(&sql, params![id], |r| r.get(0)).ok();
        Ok(v.map(|data| {
            let mut v: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.entry("id").or_insert(Value::String(id.to_string()));
            }
            v
        }))
    }

    fn blob_insert(&self, table: &str, mut data: Value) -> Result<Value, DbError> {
        let table = Self::checked_table(table)?;
        let id = data
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        if let Some(o) = data.as_object_mut() {
            o.insert("id".into(), Value::String(id.clone()));
        }
        let t = now();
        let s = serde_json::to_string(&data)?;
        let conn = self.lock()?;
        if table == "providerNodes" {
            let name = data
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            conn.execute(
                "INSERT INTO providerNodes (id, name, data, createdAt, updatedAt) VALUES (?1,?2,?3,?4,?4)",
                params![id, name, s, t],
            )?;
        } else if table == "proxyPools" {
            conn.execute(
                "INSERT INTO proxyPools (id, data, createdAt, updatedAt) VALUES (?1,?2,?3,?3)",
                params![id, s, t],
            )?;
        } else {
            let sql = format!("INSERT INTO {table} (id, data, createdAt) VALUES (?1,?2,?3)");
            conn.execute(&sql, params![id, s, t])?;
        }
        Ok(data)
    }

    fn blob_update(&self, table: &str, id: &str, patch: &Value) -> Result<Option<Value>, DbError> {
        let Some(mut cur) = self.blob_get(table, id)? else {
            return Ok(None);
        };
        if let (Some(m), Some(p)) = (cur.as_object_mut(), patch.as_object()) {
            for (k, v) in p {
                if k != "id" {
                    m.insert(k.clone(), v.clone());
                }
            }
        }
        let table = Self::checked_table(table)?;
        let s = serde_json::to_string(&cur)?;
        let conn = self.lock()?;
        let col = if table == "requestDetails" {
            "timestamp"
        } else {
            "createdAt"
        };
        let _ = col;
        let sql = format!("UPDATE {table} SET data = ?1 WHERE id = ?2");
        conn.execute(&sql, params![s, id])?;
        Ok(Some(cur))
    }

    fn blob_delete(&self, table: &str, id: &str) -> Result<usize, DbError> {
        let table = Self::checked_table(table)?;
        let conn = self.lock()?;
        let sql = format!("DELETE FROM {table} WHERE id = ?1");
        Ok(conn.execute(&sql, params![id])?)
    }

    fn checked_table(table: &str) -> Result<&'static str, DbError> {
        Ok(match table {
            "providerNodes" => "providerNodes",
            "proxyPools" => "proxyPools",
            "requestDetails" => "requestDetails",
            _ => {
                return Err(DbError::Join(format!("unknown table: {table}")));
            }
        })
    }

    pub fn list_nodes(&self) -> Result<Vec<Value>, DbError> {
        self.blob_list("providerNodes")
    }
    pub fn get_node(&self, id: &str) -> Result<Option<Value>, DbError> {
        self.blob_get("providerNodes", id)
    }
    pub fn create_node(&self, data: Value) -> Result<Value, DbError> {
        self.blob_insert("providerNodes", data)
    }
    pub fn update_node(&self, id: &str, patch: &Value) -> Result<Option<Value>, DbError> {
        self.blob_update("providerNodes", id, patch)
    }
    pub fn delete_node(&self, id: &str) -> Result<usize, DbError> {
        self.blob_delete("providerNodes", id)
    }

    pub fn list_pools(&self) -> Result<Vec<Value>, DbError> {
        self.blob_list("proxyPools")
    }
    pub fn get_pool(&self, id: &str) -> Result<Option<Value>, DbError> {
        self.blob_get("proxyPools", id)
    }
    pub fn create_pool(&self, data: Value) -> Result<Value, DbError> {
        self.blob_insert("proxyPools", data)
    }
    pub fn update_pool(&self, id: &str, patch: &Value) -> Result<Option<Value>, DbError> {
        self.blob_update("proxyPools", id, patch)
    }
    pub fn delete_pool(&self, id: &str) -> Result<usize, DbError> {
        self.blob_delete("proxyPools", id)
    }

    pub fn create_api_key(&self, name: &str, machine_id: &str) -> Result<Value, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let key = format!("sk-{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO apiKeys (id, key, name, machineId, isActive, createdAt) VALUES (?1,?2,?3,?4,1,?5)",
            params![id, key, name, machine_id, now()],
        )?;
        Ok(serde_json::json!({"id": id, "key": key, "name": name, "machineId": machine_id}))
    }

    pub fn list_api_key_rows(&self) -> Result<Vec<Value>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, key, name, machineId, isActive, createdAt FROM apiKeys ORDER BY createdAt",
        )?;
        let out: Vec<Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "key": r.get::<_, String>(1)?,
                    "name": r.get::<_, Option<String>>(2)?,
                    "machineId": r.get::<_, Option<String>>(3)?,
                    "isActive": r.get::<_, i64>(4)? != 0,
                    "createdAt": r.get::<_, String>(5)?,
                }))
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(out)
    }

    pub fn get_api_key_row(&self, id: &str) -> Result<Option<Value>, DbError> {
        let conn = self.lock()?;
        let v: Option<ApiKeyRow> = conn
            .query_row(
                "SELECT id, key, name, machineId, isActive, createdAt FROM apiKeys WHERE id = ?1",
                params![id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            )
            .ok();
        Ok(v.map(|(id, key, name, machine, active, created)| {
            serde_json::json!({"id": id, "key": key, "name": name, "machineId": machine, "isActive": active != 0, "createdAt": created})
        }))
    }

    pub fn set_api_key_active(&self, id: &str, active: bool) -> Result<Option<Value>, DbError> {
        let conn = self.lock()?;
        let n = conn.execute(
            "UPDATE apiKeys SET isActive = ?1 WHERE id = ?2",
            params![i64::from(active), id],
        )?;
        drop(conn);
        if n == 0 {
            return Ok(None);
        }
        self.get_api_key_row(id)
    }

    pub fn delete_api_key(&self, id: &str) -> Result<usize, DbError> {
        let conn = self.lock()?;
        Ok(conn.execute("DELETE FROM apiKeys WHERE id = ?1", params![id])?)
    }

    pub fn usage_recent(&self, limit: i64) -> Result<Vec<Value>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT timestamp, provider, model, connectionId, apiKey, endpoint, promptTokens, completionTokens, cost, status FROM usageHistory ORDER BY id DESC LIMIT ?1")?;
        let out: Vec<Value> = stmt
            .query_map(params![limit], |r| {
                Ok(serde_json::json!({
                    "timestamp": r.get::<_, String>(0)?,
                    "provider": r.get::<_, Option<String>>(1)?,
                    "model": r.get::<_, Option<String>>(2)?,
                    "connectionId": r.get::<_, Option<String>>(3)?,
                    "apiKey": r.get::<_, Option<String>>(4)?,
                    "endpoint": r.get::<_, Option<String>>(5)?,
                    "promptTokens": r.get::<_, i64>(6)?,
                    "completionTokens": r.get::<_, i64>(7)?,
                    "cost": r.get::<_, f64>(8)?,
                    "status": r.get::<_, Option<String>>(9)?,
                }))
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(out)
    }

    pub fn usage_chart(&self, limit: i64) -> Result<Value, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT substr(timestamp,1,10) AS day, COUNT(*), COALESCE(SUM(promptTokens),0), COALESCE(SUM(completionTokens),0), COALESCE(SUM(cost),0)
             FROM usageHistory GROUP BY day ORDER BY day DESC LIMIT ?1",
        )?;
        let mut days: Vec<Value> = stmt
            .query_map(params![limit], |r| {
                Ok(serde_json::json!({
                    "date": r.get::<_, String>(0)?,
                    "requests": r.get::<_, i64>(1)?,
                    "promptTokens": r.get::<_, i64>(2)?,
                    "completionTokens": r.get::<_, i64>(3)?,
                    "cost": r.get::<_, f64>(4)?,
                }))
            })?
            .filter_map(Result::ok)
            .collect();
        days.reverse();
        Ok(Value::Array(days))
    }

    pub fn usage_by_provider(&self) -> Result<Value, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT provider, COUNT(*), COALESCE(SUM(promptTokens),0), COALESCE(SUM(completionTokens),0), COALESCE(SUM(cost),0)
             FROM usageHistory GROUP BY provider ORDER BY COUNT(*) DESC",
        )?;
        let rows: Vec<Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "provider": r.get::<_, Option<String>>(0)?,
                    "requests": r.get::<_, i64>(1)?,
                    "promptTokens": r.get::<_, i64>(2)?,
                    "completionTokens": r.get::<_, i64>(3)?,
                    "cost": r.get::<_, f64>(4)?,
                }))
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(Value::Array(rows))
    }

    pub fn request_details_list(&self, limit: i64) -> Result<Vec<Value>, DbError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare("SELECT id, timestamp, provider, model, connectionId, status, data FROM requestDetails ORDER BY timestamp DESC LIMIT ?1")?;
        let out: Vec<Value> = stmt
            .query_map(params![limit], |r| {
                let data: String = r.get(6)?;
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "timestamp": r.get::<_, String>(1)?,
                    "provider": r.get::<_, Option<String>>(2)?,
                    "model": r.get::<_, Option<String>>(3)?,
                    "connectionId": r.get::<_, Option<String>>(4)?,
                    "status": r.get::<_, Option<String>>(5)?,
                    "data": serde_json::from_str::<Value>(&data).unwrap_or(Value::Null),
                }))
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(out)
    }
}
