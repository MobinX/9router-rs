use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub port: u16,
    pub host: String,
    pub no_browser: bool,
    pub data_dir: String,
    pub log_level: String,
}

impl Default for Settings {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        Self {
            port: std::env::var("PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(20128),
            host: std::env::var("HOSTNAME").unwrap_or_else(|_| "0.0.0.0".into()),
            no_browser: false,
            data_dir: format!("{home}/.9router"),
            log_level: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        }
    }
}

impl Settings {
    pub fn db_path(&self) -> String {
        format!("{}/db/data.sqlite", self.data_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_sane() {
        let s = Settings::default();
        assert_eq!(s.port, 20128);
        assert!(s.db_path().ends_with("data.sqlite"));
    }
}
