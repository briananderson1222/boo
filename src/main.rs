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
        prompt: String,
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
    Status,
    /// Run a job immediately (output to terminal)
    Run {
        target: String,
        /// Suppress notifications
        #[arg(long)]
        no_notify: bool,
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
    },
    /// Resume an interactive kiro-cli session (to follow up on a previous run)
    Resume { target: Option<String> },
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
    },
}

fn main() {
    let cli = Cli::parse();

    // Handle notification subprocess on main thread (required for macOS notification delegate)
    if let Commands::_Notify { summary, body, open, working_dir } = cli.command {
        boo::notifier::send_and_exit(&summary, &body, open.as_deref(), working_dir.as_deref());
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

async fn run(cli: Cli) -> boo::error::Result<()> {
    match cli.command {
        Commands::Daemon => cmd_daemon().await,
        Commands::Add { name, cron, at, every, prompt, dir, agent, model, timeout,
                        timezone, delete_after_run, open_artifact, retry, retry_delay, notify_start } =>
            cmd_add(name, cron, at, every, prompt, dir, agent, model, timeout,
                    timezone, delete_after_run, open_artifact, retry, retry_delay, notify_start).await,
        Commands::Remove { target, delete_logs, keep_logs } => cmd_remove(&target, delete_logs, keep_logs),
        Commands::List { format } => cmd_list(&format),
        Commands::Enable { target } => cmd_set_enabled(&target, true),
        Commands::Disable { target } => cmd_set_enabled(&target, false),
        Commands::Status => cmd_status(),
        Commands::Run { target, no_notify } => cmd_run(&target, no_notify).await,
        Commands::Next { cron_expr, count } => cmd_next(&cron_expr, count),
        Commands::Logs { target, count, output } => cmd_logs(&target, count, output),
        Commands::Resume { target } => cmd_resume(target.as_deref()),
        Commands::Install => cmd_install(),
        Commands::Uninstall => cmd_uninstall(),
        Commands::_Notify { .. } => unreachable!("handled before tokio runtime"),
    }
}

// --- Async commands ---

async fn cmd_daemon() -> boo::error::Result<()> {
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
    let scheduler = Arc::new(Scheduler::new(SystemClock, config, None));
    let s = Arc::clone(&scheduler);
    tokio::spawn(async move { let _ = tokio::signal::ctrl_c().await; s.trigger_shutdown(); });

    scheduler.run().await;
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

async fn cmd_run(target: &str, no_notify: bool) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;
    let config = Config::load();

    if !no_notify && job.notify_start {
        notifier::notify_start(&[&job.name]);
    }

    println!("Running job '{}'...", job.name);
    let log_dir = boo::config::boo_dir().join("runs").join(job.id.to_string());
    std::fs::create_dir_all(&log_dir)?;
    let now = Utc::now();
    let log_path = log_dir.join(format!("manual_{}_{:03}.log", now.format("%Y%m%d_%H%M%S"), now.timestamp_subsec_millis()));

    match executor::execute_job(&job, &config, &log_path).await {
        Ok(result) => {
            let record = boo::job::RunRecord {
                job_id: job.id, job_name: job.name.clone(), fired_at: now, scheduled_for: now,
                missed_count: 0, duration_secs: result.duration_secs, exit_code: result.exit_code,
                success: result.success, output_path: result.output_path.clone(), manual: true,
            };
            store.append_run_record(&record)?;
            if !no_notify { notifier::notify(&job, &result); }
            println!("Job completed: success={}, duration={:.2}s", result.success, result.duration_secs);
            if let Some(ref response) = result.response {
                println!("\n{response}");
            }
            Ok(())
        }
        Err(e) => {
            if !no_notify { notifier::notify_error(&job, &e.to_string()); }
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_add(
    name: String, cron: Option<String>, at: Option<String>, every: Option<String>,
    prompt: String, dir: Option<PathBuf>, agent: Option<String>, model: Option<String>,
    timeout: Option<u64>, timezone: Option<String>, delete_after_run: bool,
    open_artifact: Option<String>, retry: u32, retry_delay: u64, notify_start: bool,
) -> boo::error::Result<()> {
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

    let mut job = Job::new(&name, "", &prompt, dir);
    job.agent = agent;
    job.model = model;
    job.timeout_secs = timeout;
    job.timezone = timezone;
    job.delete_after_run = delete_after_run;
    job.open_artifact = open_artifact;
    job.retry_count = retry;
    job.retry_delay_secs = retry_delay;
    job.notify_start = notify_start;

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
            let items: Vec<_> = rows.iter().map(|r| serde_json::json!({
                "id": r.0, "name": r.1, "schedule": r.2, "enabled": r.3,
                "next_fire": r.4, "last_run": r.5, "artifact": r.6,
                "artifact_file": r.7, "working_dir": r.8,
            })).collect();
            println!("{}", serde_json::to_string_pretty(&items).unwrap());
        }
        "csv" => {
            println!("id,name,schedule,enabled,next_fire,last_run,artifact,artifact_file,working_dir");
            for r in &rows {
                println!("{},{},{},{},{},{},{},{},{}",
                    r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7.as_deref().unwrap_or(""), r.8);
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

fn cmd_status() -> boo::error::Result<()> {
    let boo_dir = boo::config::boo_dir();
    let running = is_daemon_running(&boo_dir.join("daemon.pid"));
    println!("Daemon: {}", if running { "running" } else { "stopped" });

    let store = JobStore::new()?;
    let jobs: Vec<_> = store.load_jobs()?.into_iter().filter(|j| j.enabled).collect();
    if jobs.is_empty() {
        println!("No enabled jobs");
        return Ok(());
    }
    let now = Utc::now();
    println!("\nNext fire times:");
    for job in jobs {
        match cron_eval::next_fire_time(&job, now) {
            Some(next) => println!("  {} - {} ({})", job.name, next.format("%Y-%m-%d %H:%M:%S UTC"), job.schedule_display()),
            None => println!("  {} - done", job.name),
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

fn cmd_logs(target: &str, count: usize, output: bool) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;
    let records = store.load_run_records(job.id, count)?;
    if records.is_empty() {
        println!("No run records for job '{}'", job.name);
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

fn cmd_resume(target: Option<&str>) -> boo::error::Result<()> {
    let config = Config::load();
    let (dir, agent) = if let Some(t) = target {
        let store = JobStore::new()?;
        let job = resolve_job(&store, t)?;
        (job.working_dir.clone(), job.agent.clone())
    } else {
        (boo::config::boo_dir().join("workspace"), None)
    };
    println!("Opening session picker (dir: {})", dir.display());
    let mut cmd = std::process::Command::new(&config.kiro_cli_path);
    cmd.args(["chat", "--resume-picker"]);
    if let Some(ref a) = agent { cmd.args(["--agent", a]); }
    cmd.current_dir(&dir);
    let status = cmd.status().map_err(boo::error::BooError::Io)?;
    if !status.success() {
        return Err(boo::error::BooError::Other("kiro-cli session picker exited with error".into()));
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

// --- Helpers ---

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
    let pid_str = match std::fs::read_to_string(pid_path) {
        Ok(s) => s, Err(_) => return false,
    };
    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p, Err(_) => return false,
    };
    #[cfg(unix)]
    { unsafe { libc::kill(pid, 0) == 0 } }
    #[cfg(not(unix))]
    { true }
}
