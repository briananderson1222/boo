use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
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
        }
    }
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
            let job = Job::new(name, "* * * * *", "test", PathBuf::from("/tmp"));
            prop_assert!(job.enabled);
            prop_assert!(!job.allow_overlap);
        }
    }
}
