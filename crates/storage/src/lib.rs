use rusqlite::{params, Connection};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
