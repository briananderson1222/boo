use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Global configuration stored at ~/.boo/config.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the kiro-cli binary (auto-detected on install). Used by the
    /// `kiro` runner.
    #[serde(default = "default_kiro_path")]
    pub kiro_cli_path: String,

    /// Path to the Claude Code CLI binary. Used by the `claude` runner.
    #[serde(default = "default_claude_path")]
    pub claude_cli_path: String,

    /// Path to the Codex CLI binary. Used by the `codex` runner.
    #[serde(default = "default_codex_path")]
    pub codex_cli_path: String,

    /// Path to the pi CLI binary. Used by the `pi` runner.
    #[serde(default = "default_pi_path")]
    pub pi_cli_path: String,

    /// Path to the opencode CLI binary. Used by the `opencode` runner.
    #[serde(default = "default_opencode_path")]
    pub opencode_cli_path: String,

    /// Launch command for the generic `acp` runner — an Agent Client Protocol
    /// agent, space-separated (e.g. "opencode acp" or "kiro-cli acp").
    #[serde(default)]
    pub acp_command: Option<String>,

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

    /// Webhook URL to POST job events to (e.g. "http://localhost:3141/scheduler/webhook")
    #[serde(default)]
    pub notify_webhook: Option<String>,
}

fn default_kiro_path() -> String {
    "kiro-cli".to_string()
}
fn default_claude_path() -> String {
    "claude".to_string()
}
fn default_codex_path() -> String {
    "codex".to_string()
}
fn default_pi_path() -> String {
    "pi".to_string()
}
fn default_opencode_path() -> String {
    "opencode".to_string()
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
            claude_cli_path: default_claude_path(),
            codex_cli_path: default_codex_path(),
            pi_cli_path: default_pi_path(),
            opencode_cli_path: default_opencode_path(),
            acp_command: None,
            default_timeout_secs: default_timeout(),
            max_log_runs: default_max_log_runs(),
            heartbeat_secs: default_heartbeat_secs(),
            terminal: None,
            notify_webhook: None,
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
                    Err(e) => eprintln!(
                        "Warning: malformed config at {}: {e}, using defaults",
                        path.display()
                    ),
                },
                Err(e) => eprintln!(
                    "Warning: cannot read config at {}: {e}, using defaults",
                    path.display()
                ),
            }
        }
        Self::default()
    }

    pub fn save(&self) -> crate::error::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            restrict_dir_permissions(parent);
        }
        let json = serde_json::to_string_pretty(self)?;
        // Config may hold webhook URLs, which are bearer-secret-equivalent
        write_private(&path, json.as_bytes())?;
        Ok(())
    }

    pub fn path() -> PathBuf {
        boo_dir().join("config.json")
    }
}

/// Restrict a directory to owner-only (0700) on Unix. No-op elsewhere.
pub fn restrict_dir_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Restrict a file to owner-only (0600) on Unix. No-op elsewhere.
pub fn restrict_file_permissions(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Write a file that holds sensitive data (prompts, webhook URLs) with
/// owner-only (0600) permissions on Unix. New files are created 0600 from the
/// start — no window where fresh content sits world-readable before a chmod.
pub fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(contents)?;
        // Covers a pre-existing (legacy 0644) file, whose mode create() leaves
        // untouched.
        restrict_file_permissions(path);
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, contents)
    }
}

/// Returns the boo data directory: $BOO_HOME if set, else ~/.boo/
pub fn boo_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BOO_HOME") {
        return PathBuf::from(dir);
    }
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
            notify_webhook: None,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.kiro_cli_path, config.kiro_cli_path);
        assert_eq!(loaded.default_timeout_secs, config.default_timeout_secs);
        assert_eq!(loaded.claude_cli_path, config.claude_cli_path);
        assert_eq!(loaded.codex_cli_path, config.codex_cli_path);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_private_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("boo-wp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.json");
        write_private(&path, b"{\"webhook\":\"secret\"}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "write_private must create 0600, got {mode:o}");
        // Overwriting an existing file keeps it 0600.
        write_private(&path, b"{}").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_old_config_without_new_paths_still_loads() {
        // A config.json written before the multi-runner fields existed must
        // still deserialize, filling the new paths with defaults.
        let json = r#"{"kiro_cli_path":"kiro-cli","default_timeout_secs":300,"max_log_runs":50,"heartbeat_secs":60}"#;
        let loaded: Config = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.claude_cli_path, "claude");
        assert_eq!(loaded.codex_cli_path, "codex");
        assert_eq!(loaded.pi_cli_path, "pi");
        assert_eq!(loaded.opencode_cli_path, "opencode");
    }
}
