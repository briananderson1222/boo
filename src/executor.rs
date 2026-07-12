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
    crate::strip_ansi(&String::from_utf8_lossy(s))
        .trim()
        .to_string()
}

/// A runner adapts a boo `Job` to a specific agent/shell CLI: it builds the
/// non-interactive command to run and supplies the prompt (usually via stdin).
///
/// This is the seam that makes boo harness-neutral. Each runner maps boo's
/// generic job fields — `prompt`, `model`, `trust_all_tools`/`trust_tools`,
/// `agent` — onto that CLI's own flags. Adding a new agent CLI is: implement
/// this trait, register it in [`get_runner`], and add its name to
/// [`VALID_RUNNERS`].
///
/// Note: this covers the scheduled/batch execution path only. Interactive
/// resume (`boo run --interactive`, `boo://resume`) and natural-language
/// `--at` parsing are still kiro-cli specific (see `main.rs`).
pub trait Runner: Send + Sync {
    fn build_command(&self, job: &Job, config: &Config) -> Command;

    /// Bytes to pipe to the child's stdin. Defaults to the job prompt — how
    /// every agent runner feeds it. `ShellRunner` overrides this to `None`.
    fn stdin_bytes(&self, job: &Job) -> Option<Vec<u8>> {
        Some(job.prompt.as_bytes().to_vec())
    }
}

/// Set the working directory and boo-context env vars that every runner needs.
fn apply_job_env(cmd: &mut Command, job: &Job) {
    cmd.current_dir(&job.working_dir);
    cmd.env("BOO_NON_INTERACTIVE", "1");
    cmd.env("BOO_JOB_NAME", &job.name);
}

/// Runs prompts via kiro-cli.
pub struct KiroRunner;

impl Runner for KiroRunner {
    fn build_command(&self, job: &Job, config: &Config) -> Command {
        let mut cmd = Command::new(&config.kiro_cli_path);
        cmd.args(["chat", "--no-interactive", "--wrap", "never"]);
        if job.trust_all_tools {
            cmd.arg("--trust-all-tools");
        }
        if let Some(ref tools) = job.trust_tools {
            cmd.args(["--trust-tools", tools]);
        }
        if let Some(ref agent) = job.agent {
            cmd.args(["--agent", agent]);
        }
        if let Some(ref model) = job.model {
            cmd.args(["--model", model]);
        }
        apply_job_env(&mut cmd, job);
        cmd
    }
}

/// Runs prompts via the Claude Code CLI (`claude -p`, headless print mode).
///
/// Field mapping: `model` → `--model`; `trust_all_tools` →
/// `--dangerously-skip-permissions`; `trust_tools` (comma/space list) →
/// `--allowedTools` (tool names are Claude Code's, e.g. `Read`, `Bash(git*)`,
/// so provide runner-appropriate values). `agent` has no Claude Code CLI
/// equivalent and is ignored (Claude Code agents are file-based under
/// `.claude/agents/`).
pub struct ClaudeCodeRunner;

impl Runner for ClaudeCodeRunner {
    fn build_command(&self, job: &Job, config: &Config) -> Command {
        let mut cmd = Command::new(&config.claude_cli_path);
        cmd.args(["-p", "--output-format", "text"]);
        if let Some(ref model) = job.model {
            cmd.args(["--model", model]);
        }
        if job.trust_all_tools {
            cmd.arg("--dangerously-skip-permissions");
        } else if let Some(ref tools) = job.trust_tools {
            // --allowedTools takes a space-separated list; accept either
            // comma- or space-separated input and pass each as a value.
            cmd.arg("--allowedTools");
            cmd.args(tools.split([',', ' ']).filter(|s| !s.is_empty()));
        }
        apply_job_env(&mut cmd, job);
        cmd
    }
}

/// Runs prompts via the Codex CLI (`codex exec`, non-interactive).
///
/// Field mapping: `model` → `-m`; `trust_all_tools` →
/// `--dangerously-bypass-approvals-and-sandbox`, otherwise the sandbox is
/// `workspace-write` so a scheduled job can write in its working dir without
/// approval prompts. `trust_tools` and `agent` have no direct Codex exec
/// equivalent and are ignored. The prompt is read from stdin (`exec -`).
pub struct CodexRunner;

impl Runner for CodexRunner {
    fn build_command(&self, job: &Job, config: &Config) -> Command {
        let mut cmd = Command::new(&config.codex_cli_path);
        cmd.args(["exec", "--skip-git-repo-check"]);
        if let Some(ref model) = job.model {
            cmd.args(["-m", model]);
        }
        if job.trust_all_tools {
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
        } else {
            cmd.args(["--sandbox", "workspace-write"]);
        }
        // "-" makes codex read the prompt from stdin.
        cmd.arg("-");
        apply_job_env(&mut cmd, job);
        cmd
    }
}

/// Runs raw shell commands.
pub struct ShellRunner;

impl Runner for ShellRunner {
    fn build_command(&self, job: &Job, _config: &Config) -> Command {
        let mut cmd = if cfg!(target_os = "windows") {
            Command::new("cmd")
        } else {
            Command::new("sh")
        };

        let shell_cmd = job.command.as_deref().unwrap_or(&job.prompt);
        if cfg!(target_os = "windows") {
            cmd.args(["/C", shell_cmd]);
        } else {
            cmd.args(["-c", shell_cmd]);
        }

        apply_job_env(&mut cmd, job);
        cmd
    }

    fn stdin_bytes(&self, _job: &Job) -> Option<Vec<u8>> {
        None
    }
}

/// Known runner names. A `None`/unset runner defaults to kiro (or shell when a
/// raw command is set).
pub const VALID_RUNNERS: &[&str] = &["kiro", "claude", "codex", "shell"];

/// Validate a `--runner` value, so a typo like "shel" is rejected at add/edit
/// time instead of silently falling back to the kiro runner.
pub fn validate_runner(runner: &str) -> Result<()> {
    if VALID_RUNNERS.contains(&runner) {
        Ok(())
    } else {
        Err(BooError::Other(format!(
            "Unknown runner '{runner}'. Valid runners: {}",
            VALID_RUNNERS.join(", ")
        )))
    }
}

/// Get the appropriate runner for a job.
pub fn get_runner(job: &Job) -> Box<dyn Runner> {
    match job.runner.as_deref() {
        Some("shell") => Box::new(ShellRunner),
        Some("claude") => Box::new(ClaudeCodeRunner),
        Some("codex") => Box::new(CodexRunner),
        Some("kiro") => Box::new(KiroRunner),
        // No runner set: a raw --command implies shell, otherwise kiro.
        _ if job.command.is_some() => Box::new(ShellRunner),
        _ => Box::new(KiroRunner),
    }
}

/// Execute a job, capturing output to log file.
///
/// `on_spawn` is invoked with the child's PID immediately after spawn so
/// callers can record the real process to signal for `boo kill`/`boo wait`
/// (recording the daemon's own PID here was a bug: the child lives in its
/// own process group).
pub async fn execute_job(
    job: &Job,
    config: &Config,
    log_path: &Path,
    on_spawn: Option<&(dyn Fn(u32) + Send + Sync)>,
) -> Result<ExecutionResult> {
    // Ensure working dir and log dir exist
    tokio::fs::create_dir_all(&job.working_dir)
        .await
        .map_err(BooError::Io)?;
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(BooError::Io)?;
    }

    let runner = get_runner(job);
    let start = Instant::now();
    let timeout_secs = job.timeout_secs.unwrap_or(config.default_timeout_secs);

    let mut cmd = runner.build_command(job, config);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Spawn in new process group so we can kill all descendants on timeout
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }

    let mut child = cmd.spawn().map_err(BooError::Io)?;

    if let (Some(callback), Some(pid)) = (on_spawn, child.id()) {
        callback(pid);
    }

    let result = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
        // Stdin write must happen inside the timeout: a child that never
        // reads stdin would otherwise block this task forever once the
        // prompt exceeds the pipe buffer.
        if let Some(bytes) = runner.stdin_bytes(job) {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(&bytes).await;
                drop(stdin);
            }
        } else {
            drop(child.stdin.take());
        }

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let copy_out = async {
            let mut buf = Vec::new();
            if let Some(out) = stdout {
                tokio::io::copy(&mut BufReader::new(out), &mut buf)
                    .await
                    .map_err(BooError::Io)?;
            }
            Ok::<_, BooError>(buf)
        };
        let copy_err = async {
            let mut buf = Vec::new();
            if let Some(err) = stderr {
                tokio::io::copy(&mut BufReader::new(err), &mut buf)
                    .await
                    .map_err(BooError::Io)?;
            }
            Ok::<_, BooError>(buf)
        };
        let (out_buf, err_buf) = tokio::try_join!(copy_out, copy_err)?;

        // Write full log (stdout + stderr)
        let log_file = tokio::fs::File::create(log_path)
            .await
            .map_err(BooError::Io)?;
        let mut writer = tokio::io::BufWriter::new(log_file);
        writer.write_all(&out_buf).await.map_err(BooError::Io)?;
        writer.write_all(&err_buf).await.map_err(BooError::Io)?;
        writer.flush().await.map_err(BooError::Io)?;

        // Also write clean response to .response file (stdout only, ANSI stripped)
        let response = strip_ansi(&out_buf);
        let response_path = log_path.with_extension("response");
        let _ = tokio::fs::write(&response_path, &response).await;
        // Logs/transcripts can contain secrets the agent encountered
        crate::config::restrict_file_permissions(log_path);
        crate::config::restrict_file_permissions(&response_path);

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
            // Kill entire process group (child + all descendants), forcefully
            if let Some(id) = child.id() {
                crate::kill_process_group(id, false);
            }
            let _ = child.kill().await;
            // Leave a log behind so the failure is visible in `boo logs`
            let _ = tokio::fs::write(
                log_path,
                format!("boo: job timed out after {timeout_secs}s; process killed\n"),
            )
            .await;
            Err(BooError::JobTimeout(timeout_secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Job;

    use tempfile::tempdir;

    fn test_job() -> Job {
        let mut job = Job::new("test", "* * * * *", "echo hello", std::env::temp_dir());
        job.timezone = Some("UTC".into());
        job.timeout_secs = Some(5);
        job
    }

    fn test_config() -> Config {
        crate::test_helpers::test_config()
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi(b"\x1b[38;5;141m> \x1b[0mHello\x07"), "> Hello");
        assert_eq!(strip_ansi(b"plain text"), "plain text");
        assert_eq!(strip_ansi(b"\x1b[1mBold\x1b[0m"), "Bold");
        // OSC title sequences must not leak their payload
        assert_eq!(
            strip_ansi(b"\x1b]0;secret title\x07real output"),
            "real output"
        );
        assert_eq!(strip_ansi(b"\x1b]0;title\x1b\\text"), "text");
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

    #[test]
    fn test_get_runner_default_is_kiro() {
        let job = test_job();
        let runner = get_runner(&job);
        // KiroRunner sends prompt via stdin
        assert!(runner.stdin_bytes(&job).is_some());
    }

    #[test]
    fn test_get_runner_shell_via_command() {
        let mut job = test_job();
        job.command = Some("echo hello".into());
        let runner = get_runner(&job);
        // ShellRunner has no stdin
        assert!(runner.stdin_bytes(&job).is_none());
    }

    #[test]
    fn test_get_runner_explicit_shell() {
        let mut job = test_job();
        job.runner = Some("shell".into());
        let runner = get_runner(&job);
        assert!(runner.stdin_bytes(&job).is_none());
    }

    /// Extract (program, args) from a built Command for assertions.
    fn cmd_parts(cmd: &Command) -> (String, Vec<String>) {
        let std = cmd.as_std();
        let prog = std.get_program().to_string_lossy().to_string();
        let args = std
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        (prog, args)
    }

    #[test]
    fn test_claude_runner_command_shape() {
        let mut job = test_job();
        job.runner = Some("claude".into());
        job.model = Some("claude-sonnet-4-5".into());
        job.trust_all_tools = true;
        let runner = get_runner(&job);
        let (prog, args) = cmd_parts(&runner.build_command(&job, &test_config()));
        assert!(prog.ends_with("echo")); // stubbed claude_cli_path
        assert!(args.contains(&"-p".to_string()));
        assert_eq!(
            args.iter()
                .position(|a| a == "--model")
                .map(|i| &args[i + 1]),
            Some(&"claude-sonnet-4-5".to_string())
        );
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        // Claude Code reads the prompt from stdin
        assert!(runner.stdin_bytes(&job).is_some());
    }

    #[test]
    fn test_claude_runner_allowed_tools() {
        let mut job = test_job();
        job.runner = Some("claude".into());
        job.trust_tools = Some("Read,Grep".into());
        let runner = get_runner(&job);
        let (_p, args) = cmd_parts(&runner.build_command(&job, &test_config()));
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"Read".to_string()));
        assert!(args.contains(&"Grep".to_string()));
        assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn test_codex_runner_command_shape() {
        let mut job = test_job();
        job.runner = Some("codex".into());
        job.model = Some("gpt-5-codex".into());
        let runner = get_runner(&job);
        let (prog, args) = cmd_parts(&runner.build_command(&job, &test_config()));
        assert!(prog.ends_with("echo")); // stubbed codex_cli_path
        assert!(args.contains(&"exec".to_string()));
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
        assert_eq!(
            args.iter().position(|a| a == "-m").map(|i| &args[i + 1]),
            Some(&"gpt-5-codex".to_string())
        );
        // Default (non-trust-all) job runs sandboxed and reads prompt from stdin
        assert!(args.contains(&"--sandbox".to_string()));
        assert!(args.contains(&"workspace-write".to_string()));
        assert!(args.contains(&"-".to_string()));
        assert!(runner.stdin_bytes(&job).is_some());
    }

    #[test]
    fn test_codex_runner_trust_all_bypasses_sandbox() {
        let mut job = test_job();
        job.runner = Some("codex".into());
        job.trust_all_tools = true;
        let runner = get_runner(&job);
        let (_p, args) = cmd_parts(&runner.build_command(&job, &test_config()));
        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(!args.contains(&"workspace-write".to_string()));
    }

    #[test]
    fn test_new_runners_validate() {
        assert!(validate_runner("claude").is_ok());
        assert!(validate_runner("codex").is_ok());
        assert!(validate_runner("kiro").is_ok());
        assert!(validate_runner("shell").is_ok());
        assert!(validate_runner("gemini").is_err());
    }

    #[tokio::test]
    async fn test_execute_job_with_echo() {
        let job = test_job();
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let result = execute_job(&job, &test_config(), &log_path, None).await;
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.response.is_some());
    }

    #[tokio::test]
    async fn test_execute_job_captures_output() {
        let job = test_job();
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let result = execute_job(&job, &test_config(), &log_path, None)
            .await
            .unwrap();
        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("chat"));
        assert!(result.success);
    }
}
