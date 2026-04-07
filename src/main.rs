use clap::{Parser, Subcommand};
use boo::clock::SystemClock;
use boo::config::Config;
use boo::cron_eval;
use boo::executor;
use boo::installer;
use boo::job::Job;
use boo::notifier;
use boo::scheduler::Scheduler;
use boo::store::JobStore;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "boo", about = "Cross-platform scheduler daemon for kiro-cli prompts")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Start the scheduler daemon
    Daemon,
    /// Add a new scheduled job
    Add {
        #[arg(long)]
        name: String,
        /// Cron expression (e.g. "0 9 * * 1-5")
        #[arg(long, group = "schedule")]
        cron: Option<String>,
        /// One-shot time (ISO 8601 or natural language like "tomorrow 9am")
        #[arg(long, group = "schedule")]
        at: Option<String>,
        /// Interval (e.g. "30m", "6h", "1d")
        #[arg(long, group = "schedule")]
        every: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        /// Raw shell command (shortcut for --runner shell)
        #[arg(long, conflicts_with = "prompt")]
        command: Option<String>,
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        timezone: Option<String>,
        /// Auto-delete job after successful execution
        #[arg(long)]
        delete_after_run: bool,
        /// File to open when notification is clicked (relative to working dir)
        #[arg(long)]
        open_artifact: Option<String>,
        /// Max retry attempts on failure
        #[arg(long, default_value = "0")]
        retry: u32,
        /// Seconds between retries
        #[arg(long, default_value = "60")]
        retry_delay: u64,
        /// Send a start notification when this job begins
        #[arg(long)]
        notify_start: bool,
        /// Pass --trust-all-tools to kiro-cli
        #[arg(long)]
        trust_all_tools: bool,
        /// Trust only these tools (comma-separated). Example: --trust-tools=write,shell
        #[arg(long)]
        trust_tools: Option<String>,
        /// Runner type: kiro (default), shell, or future CLI names
        #[arg(long)]
        runner: Option<String>,
        /// Human-readable description of what this job does
        #[arg(long)]
        description: Option<String>,
    },
    /// Remove a job by ID or name
    Remove {
        target: String,
        #[arg(long)]
        delete_logs: bool,
        #[arg(long)]
        keep_logs: bool,
    },
    /// List all jobs with next fire times
    List {
        /// Output format: table (default), json, csv
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Enable a job
    Enable { target: String },
    /// Disable a job
    Disable { target: String },
    /// Show daemon status and next fire times
    Status {
        /// Output format: table (default), json
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Run a job immediately (output to terminal)
    Run {
        target: String,
        /// Suppress notifications
        #[arg(long)]
        no_notify: bool,
        /// Print only the response content (no status messages), for programmatic use
        #[arg(long)]
        follow: bool,
        /// Launch an interactive session in the foreground instead of running non-interactively
        #[arg(long)]
        interactive: bool,
        /// Open a new terminal window for the session (use with --interactive)
        #[arg(long)]
        new_window: bool,
        /// Pass --trust-all-tools to kiro-cli
        #[arg(long)]
        trust_all_tools: bool,
        /// Trust only these tools (comma-separated). Example: --trust-tools=write,shell
        #[arg(long)]
        trust_tools: Option<String>,
    },
    /// Preview next N occurrences of a cron expression
    Next {
        cron_expr: String,
        #[arg(short, long, default_value = "5")]
        count: usize,
    },
    /// Show recent run logs for a job
    Logs {
        target: String,
        #[arg(short, long, default_value = "10")]
        count: usize,
        #[arg(long)]
        output: bool,
        /// Output format: table (default), json
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Resume an interactive kiro-cli session (to follow up on a previous run)
    Resume {
        target: Option<String>,
        /// Optional follow-up prompt to send immediately
        prompt: Option<String>,
        /// Show session picker instead of resuming latest
        #[arg(long)]
        previous: bool,
    },
    /// Show run statistics for jobs
    Stats {
        /// Job name or ID (omit for all jobs)
        target: Option<String>,
        /// Output format: table (default), json, csv
        #[arg(long, default_value = "table")]
        format: String,
    },
    /// Edit an existing job's settings
    Edit {
        target: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cron: Option<String>,
        #[arg(long)]
        at: Option<String>,
        #[arg(long)]
        every: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        timezone: Option<String>,
        #[arg(long)]
        open_artifact: Option<String>,
        #[arg(long)]
        retry: Option<u32>,
        #[arg(long)]
        retry_delay: Option<u64>,
        #[arg(long)]
        notify_start: Option<bool>,
        #[arg(long)]
        trust_all_tools: Option<bool>,
        /// Trust only these tools (comma-separated, or empty string to clear)
        #[arg(long)]
        trust_tools: Option<String>,
        #[arg(long)]
        runner: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Wait for an active job run to complete
    Wait {
        target: String,
        /// Poll interval in seconds
        #[arg(long, default_value = "2")]
        interval: u64,
    },
    /// Install boo as auto-start service
    Install,
    /// Remove boo from auto-start
    Uninstall,
    /// Internal: send a notification (used by daemon)
    #[command(hide = true, name = "internal-notify")]
    _Notify {
        summary: String,
        body: String,
        #[arg(long)]
        open: Option<String>,
        /// Working directory for inline reply resume
        #[arg(long)]
        working_dir: Option<String>,
        /// Job name for inline reply resume
        #[arg(long)]
        job_name: Option<String>,
    },
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Handle boo:// URL scheme (launched by OS when clicking boo:// links)
    if args.len() == 2 && args[1].starts_with("boo://") {
        if let Err(e) = handle_url(&args[1]) {
            eprintln!("Error handling URL: {e}");
            process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    // Handle notification subprocess on main thread (required for macOS notification delegate)
    if let Commands::_Notify { summary, body, open, working_dir, job_name } = cli.command {
        boo::notifier::send_and_exit(&summary, &body, open.as_deref(), working_dir.as_deref(), job_name.as_deref());
        return;
    }

    // Handle daemon on main thread (notification service needs main thread for macOS)
    if let Commands::Daemon = &cli.command {
        if let Err(e) = cmd_daemon_blocking() {
            eprintln!("Error: {e}");
            process::exit(1);
        }
        return;
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            if let Err(e) = run(cli).await {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        });
}

/// Handle boo:// URL scheme.
/// Format: boo://resume/<job>?prompt=<text>&previous=true
///         boo://run/<job>
///         boo://open/<job>
fn handle_url(url: &str) -> boo::error::Result<()> {
    let url = url.strip_prefix("boo://").unwrap_or(url);
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();

    let params: std::collections::HashMap<&str, String> = query.split('&')
        .filter(|s| !s.is_empty())
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k, urldecode(v)))
        .collect();

    match parts.first().copied() {
        Some("resume") => {
            let target = parts.get(1).copied();
            let prompt = params.get("prompt").map(|s| s.as_str());
            let previous = params.get("previous").is_some_and(|v| v == "true");
            // URL scheme launches without a terminal — need to open one
            if let Some(t) = target {
                let store = JobStore::new()?;
                let _ = resolve_job(&store, t)?; // validate job exists
                boo::notifier::open_terminal_resume(t, prompt, previous);
                Ok(())
            } else {
                cmd_resume(target, prompt, previous)
            }
        }
        Some("run") => {
            let target = parts.get(1).ok_or_else(|| boo::error::BooError::Other("Missing job name in URL".into()))?;
            tokio::runtime::Builder::new_current_thread()
                .enable_all().build().unwrap()
                .block_on(cmd_run(target, false, false, false, false, false, None))
        }
        Some("open") => {
            let target = parts.get(1).ok_or_else(|| boo::error::BooError::Other("Missing job name in URL".into()))?;
            let store = JobStore::new()?;
            let job = resolve_job(&store, target)?;
            if let Some(ref artifact) = job.open_artifact {
                if let Some(path) = boo::job::resolve_artifact(&job.working_dir, artifact) {
                    #[cfg(target_os = "macos")]
                    { let _ = std::process::Command::new("open").arg(&path).spawn(); }
                    #[cfg(target_os = "linux")]
                    { let _ = std::process::Command::new("xdg-open").arg(&path).spawn(); }
                    #[cfg(target_os = "windows")]
                    { let _ = std::process::Command::new("cmd").args(["/C", "start", "", &path.to_string_lossy()]).spawn(); }
                }
            }
            Ok(())
        }
        _ => Err(boo::error::BooError::Other(format!("Unknown URL action: {url}")))
    }
}

fn urldecode(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().and_then(|c| (c as char).to_digit(16));
            let lo = chars.next().and_then(|c| (c as char).to_digit(16));
            if let (Some(h), Some(l)) = (hi, lo) {
                bytes.push((h * 16 + l) as u8);
            }
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

async fn run(cli: Cli) -> boo::error::Result<()> {
    match cli.command {
        Commands::Daemon => unreachable!("handled before tokio runtime"),
        Commands::Add { name, cron, at, every, prompt, command, dir, agent, model, timeout,
                        timezone, delete_after_run, open_artifact, retry, retry_delay, notify_start, trust_all_tools, trust_tools, runner, description } =>
            cmd_add(name, cron, at, every, prompt, command, dir, agent, model, timeout,
                    timezone, delete_after_run, open_artifact, retry, retry_delay, notify_start, trust_all_tools, trust_tools, runner, description).await,
        Commands::Remove { target, delete_logs, keep_logs } => cmd_remove(&target, delete_logs, keep_logs),
        Commands::Edit { target, name, cron, at, every, prompt, command, dir, agent, model,
                         timeout, timezone, open_artifact, retry, retry_delay, notify_start, trust_all_tools, trust_tools, runner, description } =>
            cmd_edit(&target, name, cron, at, every, prompt, command, dir, agent, model,
                     timeout, timezone, open_artifact, retry, retry_delay, notify_start, trust_all_tools, trust_tools, runner, description).await,
        Commands::List { format } => cmd_list(&format),
        Commands::Enable { target } => cmd_set_enabled(&target, true),
        Commands::Disable { target } => cmd_set_enabled(&target, false),
        Commands::Status { format } => cmd_status(&format),
        Commands::Run { target, no_notify, follow, interactive, new_window, trust_all_tools, trust_tools } =>
            cmd_run(&target, no_notify, follow, interactive, new_window, trust_all_tools, trust_tools).await,
        Commands::Next { cron_expr, count } => cmd_next(&cron_expr, count),
        Commands::Logs { target, count, output, format } => cmd_logs(&target, count, output, &format),
        Commands::Resume { target, prompt, previous } => cmd_resume(target.as_deref(), prompt.as_deref(), previous),
        Commands::Stats { target, format } => cmd_stats(target.as_deref(), &format),
        Commands::Wait { target, interval } => cmd_wait(&target, interval).await,
        Commands::Install => cmd_install(),
        Commands::Uninstall => cmd_uninstall(),
        Commands::_Notify { .. } => unreachable!("handled before tokio runtime"),
    }
}

// --- Async commands ---

/// Notification service must run on main thread (macOS requirement).
/// Daemon runs tokio on a background thread.
fn cmd_daemon_blocking() -> boo::error::Result<()> {
    use fs2::FileExt;
    use std::fs::File;

    let boo_dir = boo::config::boo_dir();
    std::fs::create_dir_all(&boo_dir)?;
    let lock_path = boo_dir.join("daemon.lock");
    let pid_path = boo_dir.join("daemon.pid");

    let lock_file = File::create(&lock_path)?;
    lock_file.try_lock_exclusive().map_err(|_| {
        let existing_pid = std::fs::read_to_string(&pid_path)
            .ok().and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
        boo::error::BooError::DaemonAlreadyRunning(existing_pid)
    })?;
    std::fs::write(&pid_path, process::id().to_string())?;

    let config = Config::load();
    let notification_sender = boo::notification_service::NotificationSender::start_on_main_thread();
    let scheduler = Arc::new(Scheduler::new(SystemClock, config, None).with_notification_sender(notification_sender.clone()));

    // Run tokio + scheduler on a background thread
    let s = scheduler.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all().build().unwrap();
        rt.block_on(async {
            let s2 = s.clone();
            tokio::spawn(async move { let _ = tokio::signal::ctrl_c().await; s2.trigger_shutdown(); });
            s.run().await;
        });
        let _ = std::fs::remove_file(&pid_path);
        std::process::exit(0);
    });

    // Main thread: pump CFRunLoop for notification callbacks
    notification_sender.run_loop();
    Ok(())
}

async fn cmd_run(target: &str, no_notify: bool, follow: bool, interactive: bool, new_window: bool, trust_all_tools: bool, trust_tools: Option<String>) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let mut job = resolve_job(&store, target)?;
    if trust_all_tools { job.trust_all_tools = true; }
    if let Some(ref tools) = trust_tools { job.trust_tools = Some(tools.clone()); }

    if interactive && new_window {
        // Open in a new terminal window and return the job ID for tracking
        notifier::open_terminal_run(&job.name, job.agent.as_deref(), &job.prompt, &job.working_dir);
        println!("{}", job.id);
        return Ok(());
    }

    if interactive {
        return launch_interactive_session(&job.working_dir, job.agent.as_deref(), Some(&job.prompt), None);
    }

    let config = Config::load();

    if !no_notify && job.notify_start {
        notifier::notify_start(&[&job.name]);
    }
    if let Some(ref url) = config.notify_webhook {
        notifier::notify_webhook(url, serde_json::json!({
            "event": "job.started", "job": job.name, "id": job.id.to_string()[..8],
        }));
    }

    if !follow { println!("Running job '{}'...", job.name); }
    let log_dir = boo::config::boo_dir().join("runs").join(job.id.to_string());
    std::fs::create_dir_all(&log_dir)?;
    let now = Utc::now();
    let log_path = log_dir.join(format!("manual_{}_{:03}.log", now.format("%Y%m%d_%H%M%S"), now.timestamp_subsec_millis()));

    // Track active run
    let active = boo::store::ActiveRun {
        job_id: job.id, job_name: job.name.clone(),
        pid: process::id(), started_at: now, manual: true,
    };
    let _ = store.write_active_run(&active);

    match executor::execute_job(&job, &config, &log_path).await {
        Ok(result) => {
            store.remove_active_run(job.id);
            let record = boo::job::RunRecord {
                job_id: job.id, job_name: job.name.clone(), fired_at: now, scheduled_for: now,
                missed_count: 0, duration_secs: result.duration_secs, exit_code: result.exit_code,
                success: result.success, output_path: result.output_path.clone(), manual: true,
            };
            store.append_run_record(&record)?;
            if !no_notify { notifier::notify(&job, &result); }
            if let Some(ref url) = config.notify_webhook {
                let artifact = job.open_artifact.as_ref()
                    .and_then(|a| boo::job::resolve_artifact(&job.working_dir, a))
                    .map(|p| p.to_string_lossy().to_string());
                notifier::notify_webhook(url, serde_json::json!({
                    "event": if result.success { "job.completed" } else { "job.failed" },
                    "job": job.name, "id": job.id.to_string()[..8],
                    "success": result.success, "duration_secs": result.duration_secs,
                    "artifact": artifact,
                }));
            }
            if follow {
                if let Some(ref response) = result.response {
                    print!("{response}");
                }
                if !result.success { process::exit(1); }
            } else {
                println!("Job completed: success={}, duration={:.2}s", result.success, result.duration_secs);
                if let Some(ref response) = result.response {
                    println!("\n{response}");
                }
            }
            Ok(())
        }
        Err(e) => {
            store.remove_active_run(job.id);
            if !no_notify { notifier::notify_error(&job, &e.to_string()); }
            if let Some(ref url) = config.notify_webhook {
                notifier::notify_webhook(url, serde_json::json!({
                    "event": "job.failed", "job": job.name, "id": job.id.to_string()[..8],
                    "error": e.to_string(),
                }));
            }
            if follow { eprintln!("{e}"); process::exit(1); }
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_add(
    name: String, cron: Option<String>, at: Option<String>, every: Option<String>,
    prompt: Option<String>, command: Option<String>, dir: Option<PathBuf>, agent: Option<String>, model: Option<String>,
    timeout: Option<u64>, timezone: Option<String>, delete_after_run: bool,
    open_artifact: Option<String>, retry: u32, retry_delay: u64, notify_start: bool,
    trust_all_tools: bool, trust_tools: Option<String>, runner: Option<String>,
    description: Option<String>,
) -> boo::error::Result<()> {
    if prompt.is_none() && command.is_none() {
        return Err(boo::error::BooError::Other("Must specify --prompt or --command".into()));
    }

    // Require exactly one schedule type
    let schedule_count = cron.is_some() as u8 + at.is_some() as u8 + every.is_some() as u8;
    if schedule_count == 0 {
        return Err(boo::error::BooError::Other(
            "Must specify one of --cron, --at, or --every".into()));
    }

    let dir = dir.unwrap_or_else(|| {
        let ws = boo::config::boo_dir().join("workspace").join(&name);
        let _ = std::fs::create_dir_all(&ws);
        ws
    });
    if !dir.exists() {
        return Err(boo::error::BooError::Other(format!(
            "Working directory does not exist: {}", dir.display())));
    }

    let store = JobStore::new()?;
    if store.load_jobs()?.iter().any(|j| j.name == name) {
        return Err(boo::error::BooError::Other(format!(
            "Job with name '{}' already exists", name)));
    }

    let prompt_str = prompt.as_deref().unwrap_or("");
    let mut job = Job::new(&name, "", prompt_str, dir);
    job.agent = agent;
    job.model = model;
    job.timeout_secs = timeout;
    job.timezone = timezone;
    job.delete_after_run = delete_after_run;
    job.open_artifact = open_artifact;
    job.retry_count = retry;
    job.retry_delay_secs = retry_delay;
    job.notify_start = notify_start;
    job.trust_all_tools = trust_all_tools;
    job.trust_tools = trust_tools;
    job.runner = if command.is_some() && runner.is_none() { Some("shell".into()) } else { runner };
    job.command = command;
    job.description = description;

    if let Some(cron_str) = cron {
        cron_eval::next_occurrence(&cron_str, Utc::now())?;
        job.cron_expr = cron_str;
    } else if let Some(at_str) = at {
        let at_time = parse_at_time(&at_str).await?;
        job.at_time = Some(at_time);
    } else if let Some(every_str) = every {
        job.every_secs = Some(parse_duration(&every_str)?);
    }

    store.add_job(job.clone())?;
    println!("Added job '{}' ({}) with ID {}", job.name, job.schedule_display(), job.id);
    Ok(())
}

// --- Sync commands ---

fn cmd_remove(target: &str, delete_logs: bool, keep_logs: bool) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;
    let records = store.load_run_records(job.id, 1)?;
    if !records.is_empty() && !keep_logs {
        let should_delete = if delete_logs { true } else {
            eprint!("Job '{}' has run history. Delete logs too? [y/N] ", job.name);
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            input.trim().eq_ignore_ascii_case("y")
        };
        if should_delete {
            let _ = std::fs::remove_dir_all(boo::config::boo_dir().join("runs").join(job.id.to_string()));
            let _ = std::fs::remove_file(boo::config::boo_dir().join("runs").join(format!("{}.jsonl", job.id)));
            eprintln!("Deleted run history.");
        }
    }
    store.remove_job(job.id)?;
    println!("Removed job '{}' ({})", job.name, job.id);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_edit(
    target: &str, name: Option<String>, cron: Option<String>, at: Option<String>,
    every: Option<String>, prompt: Option<String>, command: Option<String>,
    dir: Option<PathBuf>, agent: Option<String>, model: Option<String>,
    timeout: Option<u64>, timezone: Option<String>, open_artifact: Option<String>,
    retry: Option<u32>, retry_delay: Option<u64>, notify_start: Option<bool>,
    trust_all_tools: Option<bool>, trust_tools: Option<String>, runner: Option<String>,
    description: Option<String>,
) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let mut job = resolve_job(&store, target)?;
    let mut changes = Vec::new();

    if let Some(ref new_name) = name {
        if store.load_jobs()?.iter().any(|j| j.name == *new_name && j.id != job.id) {
            return Err(boo::error::BooError::Other(format!(
                "Job with name '{}' already exists", new_name)));
        }
        let old_ws = boo::config::boo_dir().join("workspace").join(&job.name);
        let new_ws = boo::config::boo_dir().join("workspace").join(new_name);
        if job.working_dir == old_ws && old_ws.exists() {
            std::fs::rename(&old_ws, &new_ws)?;
            job.working_dir = new_ws;
            changes.push(format!("working_dir → {}", job.working_dir.display()));
        }
        job.name = new_name.clone();
        changes.push(format!("name → {new_name}"));
    }
    if let Some(v) = cron {
        cron_eval::next_occurrence(&v, Utc::now())?;
        job.cron_expr = v.clone();
        job.at_time = None;
        job.every_secs = None;
        changes.push(format!("cron → {v}"));
    } else if let Some(v) = at {
        let at_time = parse_at_time(&v).await?;
        job.at_time = Some(at_time);
        job.cron_expr = String::new();
        job.every_secs = None;
        changes.push(format!("at → {at_time}"));
    } else if let Some(v) = every {
        job.every_secs = Some(parse_duration(&v)?);
        job.cron_expr = String::new();
        job.at_time = None;
        changes.push(format!("every → {v}"));
    }
    if let Some(v) = prompt { job.prompt = v.clone(); changes.push(format!("prompt → {v}")); }
    if let Some(v) = command { job.command = Some(v.clone()); changes.push(format!("command → {v}")); }
    if let Some(v) = dir { job.working_dir = v.clone(); changes.push(format!("dir → {}", v.display())); }
    if let Some(v) = agent { job.agent = Some(v.clone()); changes.push(format!("agent → {v}")); }
    if let Some(v) = model { job.model = Some(v.clone()); changes.push(format!("model → {v}")); }
    if let Some(v) = timeout { job.timeout_secs = Some(v); changes.push(format!("timeout → {v}s")); }
    if let Some(v) = timezone { job.timezone = Some(v.clone()); changes.push(format!("timezone → {v}")); }
    if let Some(v) = open_artifact { job.open_artifact = Some(v.clone()); changes.push(format!("open_artifact → {v}")); }
    if let Some(v) = retry { job.retry_count = v; changes.push(format!("retry → {v}")); }
    if let Some(v) = retry_delay { job.retry_delay_secs = v; changes.push(format!("retry_delay → {v}s")); }
    if let Some(v) = notify_start { job.notify_start = v; changes.push(format!("notify_start → {v}")); }
    if let Some(v) = trust_all_tools { job.trust_all_tools = v; changes.push(format!("trust_all_tools → {v}")); }
    if let Some(v) = trust_tools {
        if v.is_empty() { job.trust_tools = None; changes.push("trust_tools → (cleared)".into()); }
        else { job.trust_tools = Some(v.clone()); changes.push(format!("trust_tools → {v}")); }
    }
    if let Some(v) = runner { job.runner = Some(v.clone()); changes.push(format!("runner → {v}")); }
    if let Some(v) = description { job.description = Some(v.clone()); changes.push(format!("description → {v}")); }

    if changes.is_empty() {
        println!("No changes specified.");
        return Ok(());
    }

    store.update_job(&job)?;
    println!("Updated job '{}' ({}):", job.name, job.id);
    for c in &changes {
        println!("  {c}");
    }
    Ok(())
}

fn cmd_list(format: &str) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let jobs = store.load_jobs()?;
    if jobs.is_empty() {
        println!("No jobs configured");
        return Ok(());
    }
    let now = Utc::now();
    let home = dirs::home_dir().map(|h| h.to_string_lossy().to_string()).unwrap_or_default();

    // Pre-compute rows
    let rows: Vec<_> = jobs.iter().map(|job| {
        let id_short = job.id.to_string()[..8].to_string();
        let enabled = if job.enabled { "yes" } else { "no" }.to_string();
        let next = if !job.enabled {
            "disabled".into()
        } else {
            cron_eval::next_fire_time(job, now)
                .map(|t| t.format("%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|| "done".into())
        };
        let last_run = job.last_run
            .map(|t| t.format("%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".into());
        let (artifact_pattern, artifact_resolved) = match &job.open_artifact {
            Some(a) => match boo::job::resolve_artifact(&job.working_dir, a) {
                Some(p) => (a.clone(), Some(p.to_string_lossy().to_string())),
                None => (a.clone(), None),
            },
            None => ("-".into(), None),
        };
        let work_dir = job.working_dir.to_string_lossy().replace(&home, "~");
        (id_short, job.name.clone(), job.schedule_display(), enabled, next, last_run, artifact_pattern, artifact_resolved, work_dir)
    }).collect();

    match format {
        "json" => {
            let items: Vec<_> = jobs.iter().map(|job| {
                let next_fire = if !job.enabled { None } else { cron_eval::next_fire_time(job, now) };
                serde_json::json!({
                    "id": job.id.to_string()[..8],
                    "name": job.name,
                    "schedule": job.schedule_display(),
                    "schedule_human": cron_to_human(job),
                    "enabled": if job.enabled { "yes" } else { "no" },
                    "next_fire": next_fire.map(|t| t.to_rfc3339()),
                    "last_run": job.last_run.map(|t| t.to_rfc3339()),
                    "artifact": job.open_artifact.as_deref().unwrap_or("-"),
                    "artifact_file": job.open_artifact.as_ref().and_then(|a| boo::job::resolve_artifact(&job.working_dir, a).map(|p| p.to_string_lossy().to_string())),
                    "working_dir": job.working_dir.to_string_lossy().replace(&home, "~"),
                    "prompt": if job.prompt.is_empty() { None } else { Some(&job.prompt) },
                    "command": &job.command,
                    "agent": &job.agent,
                    "description": &job.description,
                })
            }).collect();
            println!("{}", serde_json::to_string_pretty(&items).unwrap());
        }
        "csv" => {
            println!("id,name,schedule,enabled,next_fire,last_run,artifact,artifact_file,working_dir");
            for r in &rows {
                fn csv(s: &str) -> String {
                    if s.contains(',') || s.contains('"') || s.contains('\n') {
                        format!("\"{}\"", s.replace('"', "\"\""))
                    } else { s.to_string() }
                }
                println!("{},{},{},{},{},{},{},{},{}",
                    csv(&r.0), csv(&r.1), csv(&r.2), csv(&r.3), csv(&r.4), csv(&r.5), csv(&r.6), csv(r.7.as_deref().unwrap_or("")), csv(&r.8));
            }
        }
        _ => {
            let aw = "Artifact".len().max(rows.iter().map(|r| r.6.len()).max().unwrap_or(0)) + 2;
            let ww = "Working Dir".len().max(rows.iter().map(|r| r.8.len()).max().unwrap_or(0)) + 2;

            println!("{:<8} {:<18} {:<16} {:<7} {:<18} {:<18} {:<aw$} {:<ww$} Latest File", "ID", "Name", "Schedule", "On", "Next Fire", "Last Run", "Artifact", "Working Dir");
            println!("{}", "-".repeat(87 + aw + ww + 20));
            for r in &rows {
                let file_col = r.7.as_deref().unwrap_or("-");
                println!("{:<8} {:<18} {:<16} {:<7} {:<18} {:<18} {:<aw$} {:<ww$} {}",
                    r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.8, file_col);
            }
        }
    }
    Ok(())
}

fn cmd_set_enabled(target: &str, enabled: bool) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let mut job = resolve_job(&store, target)?;
    job.enabled = enabled;
    store.update_job(&job)?;
    println!("{} job '{}'", if enabled { "Enabled" } else { "Disabled" }, job.name);
    Ok(())
}

fn cmd_status(format: &str) -> boo::error::Result<()> {
    let boo_dir = boo::config::boo_dir();
    let running = is_daemon_running(&boo_dir.join("daemon.pid"));

    let store = JobStore::new()?;
    let jobs: Vec<_> = store.load_jobs()?.into_iter().filter(|j| j.enabled).collect();
    let active_runs = store.list_active_runs();
    let now = Utc::now();

    if format == "json" {
        let next_fires: Vec<_> = jobs.iter().map(|job| {
            let active = active_runs.iter().find(|r| r.job_id == job.id);
            serde_json::json!({
                "name": job.name,
                "id": job.id.to_string(),
                "schedule": job.schedule_display(),
                "next_fire": cron_eval::next_fire_time(job, now).map(|t| t.to_rfc3339()),
                "running": active.is_some(),
                "pid": active.map(|a| a.pid),
                "running_since": active.map(|a| a.started_at.to_rfc3339()),
            })
        }).collect();
        let obj = serde_json::json!({
            "daemon_running": running,
            "enabled_jobs": jobs.len(),
            "active_runs": active_runs.len(),
            "jobs": next_fires,
        });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
        return Ok(());
    }

    println!("Daemon: {}", if running { "running" } else { "stopped" });

    if !active_runs.is_empty() {
        println!("\nActive runs:");
        for run in &active_runs {
            let elapsed = (now - run.started_at).num_seconds();
            let source = if run.manual { "manual" } else { "daemon" };
            println!("  {} - pid {} ({source}, {elapsed}s elapsed)", run.job_name, run.pid);
        }
    }

    if jobs.is_empty() {
        println!("No enabled jobs");
        return Ok(());
    }
    println!("\nNext fire times:");
    for job in jobs {
        let active = active_runs.iter().any(|r| r.job_id == job.id);
        let prefix = if active { "▶ " } else { "  " };
        match cron_eval::next_fire_time(&job, now) {
            Some(next) => println!("{prefix}{} - {} ({})", job.name, next.format("%Y-%m-%d %H:%M:%S UTC"), job.schedule_display()),
            None => println!("{prefix}{} - done", job.name),
        }
    }
    Ok(())
}

fn cmd_next(cron_expr: &str, count: usize) -> boo::error::Result<()> {
    let occurrences = cron_eval::next_n_occurrences(cron_expr, Utc::now(), count)?;
    println!("Next {count} occurrences of '{cron_expr}':");
    for (i, t) in occurrences.iter().enumerate() {
        println!("  {}: {}", i + 1, t.format("%Y-%m-%d %H:%M:%S UTC"));
    }
    Ok(())
}

fn cmd_logs(target: &str, count: usize, output: bool, format: &str) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;
    let records = store.load_run_records(job.id, count)?;
    if records.is_empty() {
        if format == "json" {
            println!("[]");
        } else {
            println!("No run records for job '{}'", job.name);
        }
        return Ok(());
    }
    if output {
        let latest = &records[records.len() - 1];
        let response_path = latest.output_path.with_extension("response");
        match std::fs::read_to_string(&response_path) {
            Ok(c) => println!("{c}"),
            Err(_) => match std::fs::read_to_string(&latest.output_path) {
                Ok(c) => println!("{c}"),
                Err(e) => println!("Could not read output: {e}"),
            },
        }
        return Ok(());
    }
    if format == "json" {
        let items: Vec<_> = records.iter().map(|r| {
            serde_json::json!({
                "job_id": r.job_id.to_string(),
                "job_name": r.job_name,
                "fired_at": r.fired_at.to_rfc3339(),
                "scheduled_for": r.scheduled_for.to_rfc3339(),
                "missed_count": r.missed_count,
                "duration_secs": r.duration_secs,
                "exit_code": r.exit_code,
                "success": r.success,
                "manual": r.manual,
                "output_path": r.output_path.to_string_lossy(),
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
        return Ok(());
    }
    println!("Recent runs for '{}':", job.name);
    println!("{:<20} {:<8} {:<10} {:<8} {:<6}", "Fired At", "OK", "Duration", "Missed", "Type");
    println!("{}", "-".repeat(56));
    for r in records {
        println!("{:<20} {:<8} {:<10} {:<8} {:<6}",
            r.fired_at.format("%Y-%m-%d %H:%M:%S"),
            if r.success { "yes" } else { "no" },
            format!("{:.2}s", r.duration_secs),
            r.missed_count,
            if r.manual { "manual" } else { "cron" });
    }
    Ok(())
}

fn launch_interactive_session(
    dir: &std::path::Path,
    agent: Option<&str>,
    prompt: Option<&str>,
    resume: Option<bool>, // None = fresh, Some(false) = --resume latest, Some(true) = --resume-picker
) -> boo::error::Result<()> {
    let config = Config::load();
    let mut cmd = std::process::Command::new(&config.kiro_cli_path);
    cmd.arg("chat");
    match resume {
        Some(true) => { cmd.arg("--resume-picker"); }
        Some(false) => { cmd.arg("--resume"); }
        None => {}
    }
    if let Some(a) = agent { cmd.args(["--agent", a]); }
    if let Some(p) = prompt { cmd.args(["--", p]); }
    cmd.current_dir(dir);
    let status = cmd.status().map_err(boo::error::BooError::Io)?;
    if !status.success() {
        return Err(boo::error::BooError::Other("kiro-cli session exited with error".into()));
    }
    Ok(())
}

fn cmd_resume(target: Option<&str>, prompt: Option<&str>, previous: bool) -> boo::error::Result<()> {
    let (dir, agent) = if let Some(t) = target {
        let store = JobStore::new()?;
        let job = resolve_job(&store, t)?;
        (job.working_dir.clone(), job.agent.clone())
    } else {
        (boo::config::boo_dir().join("workspace"), None)
    };
    launch_interactive_session(&dir, agent.as_deref(), prompt, Some(previous))
}

async fn cmd_wait(target: &str, interval: u64) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;

    // Check if currently running
    let active = store.get_active_run(job.id);
    if active.is_none() {
        // Not running — show last run result
        let records = store.load_run_records(job.id, 1)?;
        if let Some(last) = records.last() {
            println!("Job '{}' is not running. Last run: success={}, duration={:.2}s ({})",
                job.name, last.success, last.duration_secs, last.fired_at.format("%Y-%m-%d %H:%M:%S UTC"));
        } else {
            println!("Job '{}' is not running and has no run history.", job.name);
        }
        return Ok(());
    }

    let active = active.unwrap();
    println!("Waiting for '{}' (pid {})...", job.name, active.pid);

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        if store.get_active_run(job.id).is_none() {
            break;
        }
    }

    // Show result
    let records = store.load_run_records(job.id, 1)?;
    if let Some(last) = records.last() {
        let status = if last.success { "✓ succeeded" } else { "✗ failed" };
        println!("Job '{}' {status} in {:.2}s", job.name, last.duration_secs);
        if !last.success { std::process::exit(1); }
    } else {
        println!("Job '{}' finished (no run record found).", job.name);
    }
    Ok(())
}

fn cmd_install() -> boo::error::Result<()> {
    let path = installer::install()?;
    println!("Installed boo service at: {}", path.display());
    Ok(())
}

fn cmd_uninstall() -> boo::error::Result<()> {
    installer::uninstall()?;
    println!("Uninstalled boo service");
    Ok(())
}

fn cmd_stats(target: Option<&str>, format: &str) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let jobs = if let Some(t) = target {
        vec![resolve_job(&store, t)?]
    } else {
        store.load_jobs()?
    };

    if jobs.is_empty() {
        if format == "json" { println!("{{\"jobs\":[],\"total\":{{}}}}"); }
        else { println!("No jobs configured"); }
        return Ok(());
    }

    let now = Utc::now();
    let window_24h = now - chrono::Duration::hours(24);
    let window_7d = now - chrono::Duration::days(7);
    let window_30d = now - chrono::Duration::days(30);

    #[derive(Default)]
    struct Stats {
        total: u64, successes: u64, failures: u64, manual: u64,
        total_missed: u64, total_duration: f64, max_duration: f64,
        last_success: Option<DateTime<Utc>>, last_failure: Option<DateTime<Utc>>,
        runs_24h: u64, ok_24h: u64, fail_24h: u64,
        runs_7d: u64, ok_7d: u64, fail_7d: u64,
        runs_30d: u64, ok_30d: u64, fail_30d: u64,
    }

    let mut all_stats: Vec<(String, String, Stats)> = Vec::new();
    let mut global = Stats::default();

    for job in &jobs {
        let records = store.load_run_records(job.id, 10_000)?;
        let mut s = Stats::default();
        for r in &records {
            s.total += 1;
            if r.success { s.successes += 1; } else { s.failures += 1; }
            if r.manual { s.manual += 1; }
            s.total_missed += r.missed_count as u64;
            s.total_duration += r.duration_secs;
            if r.duration_secs > s.max_duration { s.max_duration = r.duration_secs; }
            if r.success { s.last_success = Some(s.last_success.map_or(r.fired_at, |prev: DateTime<Utc>| prev.max(r.fired_at))); }
            else { s.last_failure = Some(s.last_failure.map_or(r.fired_at, |prev: DateTime<Utc>| prev.max(r.fired_at))); }
            if r.fired_at >= window_24h { s.runs_24h += 1; if r.success { s.ok_24h += 1; } else { s.fail_24h += 1; } }
            if r.fired_at >= window_7d { s.runs_7d += 1; if r.success { s.ok_7d += 1; } else { s.fail_7d += 1; } }
            if r.fired_at >= window_30d { s.runs_30d += 1; if r.success { s.ok_30d += 1; } else { s.fail_30d += 1; } }
        }
        global.total += s.total; global.successes += s.successes; global.failures += s.failures;
        global.manual += s.manual; global.total_missed += s.total_missed;
        global.total_duration += s.total_duration;
        if s.max_duration > global.max_duration { global.max_duration = s.max_duration; }
        global.runs_24h += s.runs_24h; global.ok_24h += s.ok_24h; global.fail_24h += s.fail_24h;
        global.runs_7d += s.runs_7d; global.ok_7d += s.ok_7d; global.fail_7d += s.fail_7d;
        global.runs_30d += s.runs_30d; global.ok_30d += s.ok_30d; global.fail_30d += s.fail_30d;
        if let Some(t) = s.last_success { global.last_success = Some(global.last_success.map_or(t, |p: DateTime<Utc>| p.max(t))); }
        if let Some(t) = s.last_failure { global.last_failure = Some(global.last_failure.map_or(t, |p: DateTime<Utc>| p.max(t))); }
        all_stats.push((job.name.clone(), job.id.to_string(), s));
    }

    fn rate(ok: u64, total: u64) -> f64 { if total == 0 { 0.0 } else { ok as f64 / total as f64 * 100.0 } }
    fn avg(total_dur: f64, count: u64) -> f64 { if count == 0 { 0.0 } else { total_dur / count as f64 } }

    fn stats_json(s: &Stats) -> serde_json::Value {
        serde_json::json!({
            "total_runs": s.total, "successes": s.successes, "failures": s.failures,
            "manual_runs": s.manual, "total_missed": s.total_missed,
            "avg_duration": (avg(s.total_duration, s.total) * 100.0).round() / 100.0,
            "max_duration": (s.max_duration * 100.0).round() / 100.0,
            "success_rate": (rate(s.successes, s.total) * 10.0).round() / 10.0,
            "last_success": s.last_success.map(|t| t.to_rfc3339()),
            "last_failure": s.last_failure.map(|t| t.to_rfc3339()),
            "last_24h": { "runs": s.runs_24h, "successes": s.ok_24h, "failures": s.fail_24h },
            "last_7d": { "runs": s.runs_7d, "successes": s.ok_7d, "failures": s.fail_7d },
            "last_30d": { "runs": s.runs_30d, "successes": s.ok_30d, "failures": s.fail_30d },
        })
    }

    if format == "json" {
        let job_items: Vec<_> = all_stats.iter().map(|(name, id, s)| {
            let mut v = stats_json(s);
            v["name"] = serde_json::json!(name);
            v["id"] = serde_json::json!(id);
            v
        }).collect();
        let obj = serde_json::json!({ "jobs": job_items, "total": stats_json(&global) });
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
        return Ok(());
    }

    if format == "csv" {
        println!("name,runs,ok,fail,manual,missed,avg_time,max_time,success_rate");
        for (name, _, s) in &all_stats {
            println!("{},{},{},{},{},{},{:.2},{:.2},{:.1}",
                name, s.total, s.successes, s.failures, s.manual, s.total_missed,
                avg(s.total_duration, s.total), s.max_duration, rate(s.successes, s.total));
        }
        return Ok(());
    }

    // Table format
    println!("{:<18} {:>5} {:>4} {:>5} {:>6} {:>7} {:>9} {:>8}",
        "Job", "Runs", "OK", "Fail", "Missed", "Avg Time", "Max Time", "Success%");
    println!("{}", "-".repeat(78));
    for (name, _, s) in &all_stats {
        println!("{:<18} {:>5} {:>4} {:>5} {:>6} {:>7.1}s {:>8.1}s {:>7.1}%",
            name, s.total, s.successes, s.failures, s.total_missed,
            avg(s.total_duration, s.total), s.max_duration, rate(s.successes, s.total));
    }
    if all_stats.len() > 1 {
        println!("{}", "-".repeat(78));
        println!("{:<18} {:>5} {:>4} {:>5} {:>6} {:>7.1}s {:>8.1}s {:>7.1}%",
            "TOTAL", global.total, global.successes, global.failures, global.total_missed,
            avg(global.total_duration, global.total), global.max_duration, rate(global.successes, global.total));
    }
    Ok(())
}

// --- Helpers ---

/// Convert a job's schedule to a human-friendly description.
fn cron_to_human(job: &Job) -> String {
    if let Some(at) = job.at_time {
        return format!("Once at {}", at.format("%b %d, %I:%M %p UTC"));
    }
    if let Some(secs) = job.every_secs {
        return if secs >= 86400 { format!("Every {} day(s)", secs / 86400) }
        else if secs >= 3600 { format!("Every {} hour(s)", secs / 3600) }
        else if secs >= 60 { format!("Every {} minute(s)", secs / 60) }
        else { format!("Every {} second(s)", secs) };
    }
    let parts: Vec<&str> = job.cron_expr.split_whitespace().collect();
    if parts.len() != 5 { return job.cron_expr.clone(); }
    let (min, hour, _dom, _mon, dow) = (parts[0], parts[1], parts[2], parts[3], parts[4]);

    let time = match (hour, min) {
        (h, m) if !h.contains('*') && !h.contains('/') && !h.contains('-') && !m.contains('*') && !m.contains('/') => {
            let h: u32 = h.parse().unwrap_or(0);
            let m: u32 = m.parse().unwrap_or(0);
            let (h12, ampm) = if h == 0 { (12, "AM") } else if h < 12 { (h, "AM") } else if h == 12 { (12, "PM") } else { (h - 12, "PM") };
            format!("{h12}:{m:02} {ampm} UTC")
        }
        _ if min.starts_with("*/") && hour.contains('-') => {
            format!("Every {} min ({} UTC)", &min[2..], hour)
        }
        _ if min.starts_with("*/") => format!("Every {} min", &min[2..]),
        _ => format!("{hour}:{min} UTC"),
    };

    let days = match dow {
        "*" => "Daily".into(),
        "1-5" => "Weekdays".into(),
        "0,6" | "6,0" => "Weekends".into(),
        "0" | "7" => "Sundays".into(),
        "1" => "Mondays".into(),
        "2" => "Tuesdays".into(),
        "3" => "Wednesdays".into(),
        "4" => "Thursdays".into(),
        "5" => "Fridays".into(),
        "6" => "Saturdays".into(),
        d => format!("Days {d}"),
    };

    format!("{days} at {time}")
}

fn resolve_job(store: &JobStore, target: &str) -> boo::error::Result<Job> {
    if let Ok(uuid) = Uuid::parse_str(target) {
        return store.get_job(uuid);
    }
    store.load_jobs()?.into_iter().find(|j| j.name == target)
        .ok_or_else(|| boo::error::BooError::Other(format!("Job not found: {target}")))
}

/// Parse a duration string like "30s", "20m", "6h", "1d" into seconds.
pub fn parse_duration(s: &str) -> boo::error::Result<u64> {
    let s = s.trim();
    let (num, suffix) = s.split_at(s.len().saturating_sub(1));
    let n: u64 = num.parse().map_err(|_| boo::error::BooError::Other(format!("Invalid duration: {s}")))?;
    match suffix {
        "s" => Ok(n),
        "m" => Ok(n * 60),
        "h" => Ok(n * 3600),
        "d" => Ok(n * 86400),
        _ => Err(boo::error::BooError::Other(format!("Invalid duration suffix: {s}. Use s/m/h/d")))
    }
}

/// Parse an --at time string. Tries ISO 8601 first, then uses kiro-cli for natural language.
async fn parse_at_time(input: &str) -> boo::error::Result<DateTime<Utc>> {
    // Try ISO 8601 first
    if let Ok(dt) = input.parse::<DateTime<Utc>>() {
        return Ok(dt);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(input) {
        return Ok(dt.with_timezone(&Utc));
    }
    // Try common formats
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M") {
        return Ok(dt.and_utc());
    }

    // Natural language: use kiro-cli to parse
    let config = Config::load();
    let now = Utc::now();
    let prompt = format!(
        "Parse this time expression into ISO 8601 UTC format. Current time: {}. Input: '{}'. Reply with ONLY the ISO 8601 timestamp (e.g. 2026-02-20T16:00:00Z), nothing else.",
        now.to_rfc3339(), input
    );

    eprintln!("Parsing '{}' via AI...", input);

    let output = tokio::process::Command::new(&config.kiro_cli_path)
        .args(["chat", "--no-interactive", "--trust-tools=", "--wrap", "never", &prompt])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .map_err(boo::error::BooError::Io)?;

    let raw = String::from_utf8_lossy(&output.stdout);
    // Strip ANSI codes and find the timestamp
    let cleaned = boo::strip_ansi(&raw);
    let timestamp = cleaned.lines()
        .filter_map(|line| {
            let l = line.trim().trim_start_matches('>').trim();
            chrono::DateTime::parse_from_rfc3339(l).ok()
                .map(|dt| dt.with_timezone(&Utc))
        })
        .next()
        .ok_or_else(|| boo::error::BooError::Other(
            format!("Could not parse AI response as timestamp: {}", cleaned.trim())))?;

    // Confirm with user
    eprintln!("Parsed '{}' → {}", input, timestamp.format("%Y-%m-%d %H:%M:%S UTC"));
    eprint!("Confirm? [Y/n] ");
    let mut confirm = String::new();
    std::io::stdin().read_line(&mut confirm).ok();
    if confirm.trim().eq_ignore_ascii_case("n") {
        return Err(boo::error::BooError::Other("Cancelled by user".into()));
    }

    Ok(timestamp)
}

fn is_daemon_running(pid_path: &std::path::Path) -> bool {
    // Primary: check PID file
    if let Ok(pid_str) = std::fs::read_to_string(pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if boo::is_pid_alive(pid) { return true; }
        }
    }

    // Fallback: if daemon.pid is missing/stale, check if daemon.lock is held
    let lock_path = pid_path.with_file_name("daemon.lock");
    if let Ok(file) = std::fs::File::open(&lock_path) {
        use fs2::FileExt;
        if file.try_lock_exclusive().is_err() {
            return true; // lock held → daemon is running
        }
        let _ = file.unlock();
    }
    false
}
