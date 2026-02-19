use thiserror::Error;

#[derive(Error, Debug)]
pub enum BooError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Cron parse error: {0}")]
    CronParse(String),

    #[error("Job not found: {0}")]
    JobNotFound(uuid::Uuid),

    #[error("Daemon already running (pid: {0})")]
    DaemonAlreadyRunning(u32),

    #[error("Daemon not running")]
    DaemonNotRunning,

    #[error("Job timed out after {0}s")]
    JobTimeout(u64),

    #[error("Job failed with exit code {0}")]
    JobFailed(i32),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, BooError>;
