use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global configuration stored at ~/.boo/config.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to kiro-cli binary (auto-detected on install)
    #[serde(default = "default_kiro_path")]
    pub kiro_cli_path: String,

    /// Default job timeout in seconds
    #[serde(default = "default_timeout")]
    pub default_timeout_secs: u64,

    /// Max log files to keep per job
    #[serde(default = "default_max_log_runs")]
    pub max_log_runs: usize,

    /// Heartbeat interval in seconds
    #[serde(default = "default_heartbeat_secs")]
    pub heartbeat_secs: u64,

    /// Terminal app for interactive sessions (e.g. "iTerm", "Ghostty", "Terminal")
    #[serde(default)]
    pub terminal: Option<String>,
}

fn default_kiro_path() -> String {
    "kiro-cli".to_string()
}
fn default_timeout() -> u64 {
    300
}
fn default_max_log_runs() -> usize {
    50
}
fn default_heartbeat_secs() -> u64 {
    60
}

impl Default for Config {
    fn default() -> Self {
        Self {
            kiro_cli_path: default_kiro_path(),
            default_timeout_secs: default_timeout(),
            max_log_runs: default_max_log_runs(),
            heartbeat_secs: default_heartbeat_secs(),
            terminal: None,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = Self::path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(s) => match serde_json::from_str(&s) {
                    Ok(c) => return c,
                    Err(e) => eprintln!("Warning: malformed config at {}: {e}, using defaults", path.display()),
                },
                Err(e) => eprintln!("Warning: cannot read config at {}: {e}, using defaults", path.display()),
            }
        }
        Self::default()
    }

    pub fn save(&self) -> crate::error::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn path() -> PathBuf {
        boo_dir().join("config.json")
    }
}

/// Returns ~/.boo/ directory path
pub fn boo_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".boo")
}

/// Returns ~/.boo/runs/ directory path
pub fn runs_dir() -> PathBuf {
    boo_dir().join("runs")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.kiro_cli_path, "kiro-cli");
        assert_eq!(config.default_timeout_secs, 300);
        assert_eq!(config.max_log_runs, 50);
        assert_eq!(config.heartbeat_secs, 60);
    }
    
    #[test]
    fn test_config_serialization_roundtrip() {
        let config = Config {
            kiro_cli_path: "/usr/local/bin/kiro-cli".to_string(),
            default_timeout_secs: 600,
            max_log_runs: 100,
            heartbeat_secs: 30,
            terminal: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.kiro_cli_path, config.kiro_cli_path);
        assert_eq!(loaded.default_timeout_secs, config.default_timeout_secs);
    }
}
