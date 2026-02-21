use crate::config::Config;
use crate::error::{BooError, Result};
use crate::job::Job;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::Command;

pub struct ExecutionResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub duration_secs: f64,
    pub output_path: PathBuf,
    pub response: Option<String>,
}

/// Strip ANSI escape sequences from text.
fn strip_ansi(s: &[u8]) -> String {
    crate::strip_ansi(&String::from_utf8_lossy(s)).trim().to_string()
}

/// A runner knows how to build a command and prepare stdin for a job.
pub trait Runner: Send + Sync {
    fn build_command(&self, job: &Job, config: &Config) -> Command;
    fn stdin_bytes(&self, job: &Job) -> Option<Vec<u8>>;
}

/// Runs prompts via kiro-cli.
pub struct KiroRunner;

impl Runner for KiroRunner {
    fn build_command(&self, job: &Job, config: &Config) -> Command {
        let mut cmd = Command::new(&config.kiro_cli_path);
        cmd.args(["chat", "--no-interactive", "--trust-all-tools", "--wrap", "never"]);
        if let Some(ref agent) = job.agent { cmd.args(["--agent", agent]); }
        if let Some(ref model) = job.model { cmd.args(["--model", model]); }
        cmd.current_dir(&job.working_dir);
        cmd.env("BOO_NON_INTERACTIVE", "1");
        cmd.env("BOO_JOB_NAME", &job.name);
        cmd
    }

    fn stdin_bytes(&self, job: &Job) -> Option<Vec<u8>> {
        Some(job.prompt.as_bytes().to_vec())
    }
}

/// Runs raw shell commands.
pub struct ShellRunner;

impl Runner for ShellRunner {
    fn build_command(&self, job: &Job, _config: &Config) -> Command {
        let mut cmd = Command::new("sh");
        let shell_cmd = job.command.as_deref().unwrap_or(&job.prompt);
        cmd.args(["-c", shell_cmd]);
        cmd.current_dir(&job.working_dir);
        cmd.env("BOO_NON_INTERACTIVE", "1");
        cmd.env("BOO_JOB_NAME", &job.name);
        cmd
    }

    fn stdin_bytes(&self, _job: &Job) -> Option<Vec<u8>> { None }
}

/// Get the appropriate runner for a job.
pub fn get_runner(job: &Job) -> Box<dyn Runner> {
    match job.runner.as_deref() {
        Some("shell") => Box::new(ShellRunner),
        _ if job.command.is_some() => Box::new(ShellRunner),
        _ => Box::new(KiroRunner),
    }
}

/// Execute a job, capturing output to log file.
pub async fn execute_job(job: &Job, config: &Config, log_path: &Path) -> Result<ExecutionResult> {
    let runner = get_runner(job);
    let start = Instant::now();
    let timeout_secs = job.timeout_secs.unwrap_or(config.default_timeout_secs);

    let mut cmd = runner.build_command(job, config);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(BooError::Io)?;

    if let Some(bytes) = runner.stdin_bytes(job) {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&bytes).await;
            drop(stdin);
        }
    } else {
        drop(child.stdin.take());
    }

    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let copy_out = async {
            let mut buf = Vec::new();
            if let Some(out) = stdout {
                tokio::io::copy(&mut BufReader::new(out), &mut buf).await.map_err(BooError::Io)?;
            }
            Ok::<_, BooError>(buf)
        };
        let copy_err = async {
            let mut buf = Vec::new();
            if let Some(err) = stderr {
                tokio::io::copy(&mut BufReader::new(err), &mut buf).await.map_err(BooError::Io)?;
            }
            Ok::<_, BooError>(buf)
        };
        let (out_buf, err_buf) = tokio::try_join!(copy_out, copy_err)?;

        // Write full log (stdout + stderr)
        let log_file = tokio::fs::File::create(log_path).await.map_err(BooError::Io)?;
        let mut writer = tokio::io::BufWriter::new(log_file);
        writer.write_all(&out_buf).await.map_err(BooError::Io)?;
        writer.write_all(&err_buf).await.map_err(BooError::Io)?;
        writer.flush().await.map_err(BooError::Io)?;

        // Also write clean response to .response file (stdout only, ANSI stripped)
        let response = strip_ansi(&out_buf);
        let response_path = log_path.with_extension("response");
        let _ = tokio::fs::write(&response_path, &response).await;

        let status = child.wait().await.map_err(BooError::Io)?;
        Ok::<_, BooError>((status.code(), status.success(), response))
    })
    .await;

    let duration_secs = start.elapsed().as_secs_f64();

    match result {
        Ok(Ok((exit_code, success, response))) => Ok(ExecutionResult {
            exit_code,
            success,
            duration_secs,
            output_path: log_path.to_path_buf(),
            response: Some(response),
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            let _ = child.kill().await;
            Err(BooError::JobTimeout(timeout_secs))
        }
    }
}

/// Execute a job and print output to terminal (for `boo run` command)
pub async fn execute_job_interactive(job: &Job, config: &Config) -> Result<ExecutionResult> {
    let runner = get_runner(job);
    let start = Instant::now();
    let timeout_secs = job.timeout_secs.unwrap_or(config.default_timeout_secs);

    let mut cmd = runner.build_command(job, config);
    cmd.stdin(Stdio::piped());

    let mut child = cmd.spawn().map_err(BooError::Io)?;

    if let Some(bytes) = runner.stdin_bytes(job) {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&bytes).await;
            drop(stdin);
        }
    } else {
        drop(child.stdin.take());
    }

    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        let status = child.wait().await.map_err(BooError::Io)?;
        Ok::<_, BooError>((status.code(), status.success()))
    })
    .await;

    let duration_secs = start.elapsed().as_secs_f64();

    match result {
        Ok(Ok((exit_code, success))) => Ok(ExecutionResult {
            exit_code,
            success,
            duration_secs,
            output_path: PathBuf::new(),
            response: None,
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            let _ = child.kill().await;
            Err(BooError::JobTimeout(timeout_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Job;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_job() -> Job {
        let mut job = Job::new("test", "* * * * *", "echo hello", PathBuf::from("/tmp"));
        job.timezone = Some("UTC".into());
        job.timeout_secs = Some(5);
        job
    }

    fn test_config() -> Config {
            Config {
                kiro_cli_path: "echo".into(),
                default_timeout_secs: 5,
                max_log_runs: 10,
                heartbeat_secs: 60,
                terminal: None,
            }
        }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi(b"\x1b[38;5;141m> \x1b[0mHello\x07"), "> Hello");
        assert_eq!(strip_ansi(b"plain text"), "plain text");
        assert_eq!(strip_ansi(b"\x1b[1mBold\x1b[0m"), "Bold");
    }

    #[tokio::test]
    async fn test_build_command() {
        let job = test_job();
        let runner = get_runner(&job);
        let _cmd = runner.build_command(&job, &test_config());
    }

    #[tokio::test]
    async fn test_build_command_with_agent() {
        let mut job = test_job();
        job.agent = Some("test-agent".into());
        let runner = get_runner(&job);
        let _cmd = runner.build_command(&job, &test_config());
    }

    #[tokio::test]
    async fn test_execute_job_with_echo() {
        let job = test_job();
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let result = execute_job(&job, &test_config(), &log_path).await;
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.response.is_some());
    }

    #[tokio::test]
    async fn test_execute_job_captures_output() {
        let job = test_job();
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let result = execute_job(&job, &test_config(), &log_path).await.unwrap();
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("chat"));
        assert!(result.success);
    }
}
