use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub id: Uuid,
    pub name: String,
    pub cron_expr: String,
    pub timezone: Option<String>,
    pub prompt: String,
    pub working_dir: PathBuf,
    pub agent: Option<String>,
    pub enabled: bool,
    pub allow_overlap: bool,
    pub timeout_secs: Option<u64>,
    pub last_run: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    /// One-shot schedule: fire once at this time, then optionally delete.
    #[serde(default)]
    pub at_time: Option<DateTime<Utc>>,
    /// Interval schedule: fire every N seconds.
    #[serde(default)]
    pub every_secs: Option<u64>,
    /// Auto-delete job after successful execution (useful for one-shot --at jobs).
    #[serde(default)]
    pub delete_after_run: bool,
    /// Override kiro-cli model for this job.
    #[serde(default)]
    pub model: Option<String>,
    /// File to open when notification is clicked (relative to working_dir).
    #[serde(default)]
    pub open_artifact: Option<String>,
    /// Max retry attempts on failure (0 = no retry).
    #[serde(default)]
    pub retry_count: u32,
    /// Seconds between retries.
    #[serde(default = "default_retry_delay")]
    pub retry_delay_secs: u64,
    /// Send a start notification when this job begins.
    #[serde(default)]
    pub notify_start: bool,
    /// Pass --trust-all-tools to kiro-cli (opt-in).
    #[serde(default)]
    pub trust_all_tools: bool,
    /// Runner type: "kiro" (default), "shell", or future CLI names.
    #[serde(default)]
    pub runner: Option<String>,
    /// Raw shell command (shortcut for runner=shell). Mutually exclusive with prompt for shell jobs.
    #[serde(default)]
    pub command: Option<String>,
}

fn default_retry_delay() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub job_id: Uuid,
    pub job_name: String,
    pub fired_at: DateTime<Utc>,
    pub scheduled_for: DateTime<Utc>,
    pub missed_count: u32,
    pub duration_secs: f64,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub output_path: PathBuf,
    #[serde(default)]
    pub manual: bool,
}

impl Job {
    pub fn new(
        name: impl Into<String>,
        cron_expr: impl Into<String>,
        prompt: impl Into<String>,
        working_dir: PathBuf,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            cron_expr: cron_expr.into(),
            timezone: None,
            prompt: prompt.into(),
            working_dir,
            agent: None,
            enabled: true,
            allow_overlap: false,
            timeout_secs: None,
            last_run: None,
            created_at: Utc::now(),
            at_time: None,
            every_secs: None,
            delete_after_run: false,
            model: None,
            open_artifact: None,
            retry_count: 0,
            retry_delay_secs: default_retry_delay(),
            notify_start: false,
            trust_all_tools: false,
            runner: None,
            command: None,
        }
    }

    /// Human-readable schedule description for display.
    pub fn schedule_display(&self) -> String {
        if let Some(at) = self.at_time {
            format!("at {}", at.format("%Y-%m-%d %H:%M"))
        } else if let Some(secs) = self.every_secs {
            if secs >= 86400 { format!("every {}d", secs / 86400) }
            else if secs >= 3600 { format!("every {}h", secs / 3600) }
            else if secs >= 60 { format!("every {}m", secs / 60) }
            else { format!("every {}s", secs) }
        } else {
            format!("cron {}", self.cron_expr)
        }
    }
}

/// Resolve an artifact pattern (possibly a glob) to the newest matching file in a directory.
/// Returns None if no match found. For literal paths, checks existence directly.
pub fn resolve_artifact(working_dir: &Path, pattern: &str) -> Option<PathBuf> {
    let full = working_dir.join(pattern);
    // If it's a literal path (no glob chars), just check existence
    if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('[') {
        return if full.exists() { Some(full) } else { None };
    }
    // Glob: find newest matching file
    glob::glob(&full.to_string_lossy()).ok()?
        .filter_map(|e| e.ok())
        .filter(|p| p.is_file())
        .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn job_serialization_roundtrip(
            name in "\\PC*",
            cron_expr in "\\PC*",
            prompt in "\\PC*",
            working_dir in "\\PC*",
        ) {
            let job = Job::new(name, cron_expr, prompt, PathBuf::from(working_dir));
            let serialized = serde_json::to_string(&job).unwrap();
            let deserialized: Job = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(job, deserialized);
        }

        #[test]
        fn job_new_defaults(name in "\\PC*") {
            let job = Job::new(name, "* * * * *", "test", std::env::temp_dir());
            prop_assert!(job.enabled);
            prop_assert!(!job.allow_overlap);
            prop_assert!(!job.delete_after_run);
            prop_assert!(!job.notify_start);
            prop_assert_eq!(job.retry_count, 0);
            prop_assert!(job.at_time.is_none());
            prop_assert!(job.every_secs.is_none());
            prop_assert!(job.model.is_none());
        }
    }

    #[test]
    fn test_resolve_artifact_literal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("report.html"), "test").unwrap();
        assert!(resolve_artifact(dir.path(), "report.html").is_some());
        assert!(resolve_artifact(dir.path(), "missing.html").is_none());
    }

    #[test]
    fn test_resolve_artifact_glob() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("daily-2026-01-01.html"), "old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(dir.path().join("daily-2026-01-02.html"), "new").unwrap();
        let result = resolve_artifact(dir.path(), "daily-*.html");
        assert!(result.is_some());
        assert!(result.unwrap().to_string_lossy().contains("2026-01-02"));
    }

    #[test]
    fn test_resolve_artifact_no_match() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve_artifact(dir.path(), "*.xyz").is_none());
    }
}
