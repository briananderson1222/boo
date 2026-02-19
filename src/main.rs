use clap::{Parser, Subcommand};
use boo::clock::SystemClock;
use boo::config::Config;
use boo::cron_eval;
use boo::executor;
use boo::installer;
use boo::job::Job;
use boo::scheduler::Scheduler;
use boo::store::JobStore;
use chrono::Utc;
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
        #[arg(long)]
        cron: String,
        #[arg(long)]
        prompt: String,
        #[arg(long, default_value = ".")]
        dir: PathBuf,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        timezone: Option<String>,
    },
    /// Remove a job by ID or name
    Remove {
        target: String,
        /// Delete run history without prompting
        #[arg(long)]
        delete_logs: bool,
        /// Keep run history without prompting
        #[arg(long)]
        keep_logs: bool,
    },
    /// List all jobs with next fire times
    List,
    /// Enable a job
    Enable { target: String },
    /// Disable a job
    Disable { target: String },
    /// Show daemon status and next fire times
    Status,
    /// Run a job immediately (output to terminal)
    Run { target: String },
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
        /// Show the clean response output of the most recent run
        #[arg(long)]
        output: bool,
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
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

async fn run(cli: Cli) -> boo::error::Result<()> {
    match cli.command {
        Commands::Daemon => cmd_daemon().await,
        Commands::Add { name, cron, prompt, dir, agent, timeout, timezone } =>
            cmd_add(name, cron, prompt, dir, agent, timeout, timezone),
        Commands::Remove { target, delete_logs, keep_logs } => cmd_remove(&target, delete_logs, keep_logs),
        Commands::List => cmd_list(),
        Commands::Enable { target } => cmd_set_enabled(&target, true),
        Commands::Disable { target } => cmd_set_enabled(&target, false),
        Commands::Status => cmd_status(),
        Commands::Run { target } => cmd_run(&target).await,
        Commands::Next { cron_expr, count } => cmd_next(&cron_expr, count),
        Commands::Logs { target, count, output } => cmd_logs(&target, count, output),
        Commands::Install => cmd_install(),
        Commands::Uninstall => cmd_uninstall(),
        Commands::_Notify { summary, body } => {
            boo::notifier::send_and_exit(&summary, &body);
            Ok(())
        }
    }
}

// --- Async commands (actually need async) ---

async fn cmd_daemon() -> boo::error::Result<()> {
    use fs2::FileExt;
    use std::fs::File;

    let boo_dir = boo::config::boo_dir();
    std::fs::create_dir_all(&boo_dir)?;

    let lock_path = boo_dir.join("daemon.lock");
    let pid_path = boo_dir.join("daemon.pid");

    let lock_file = File::create(&lock_path)?;
    lock_file.try_lock_exclusive().map_err(|_| {
        // Read the existing PID to show in error
        let existing_pid = std::fs::read_to_string(&pid_path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0);
        boo::error::BooError::DaemonAlreadyRunning(existing_pid)
    })?;

    std::fs::write(&pid_path, process::id().to_string())?;

    let config = Config::load();
    let scheduler = Arc::new(Scheduler::new(SystemClock, config, None));

    let s = Arc::clone(&scheduler);
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        s.trigger_shutdown();
    });

    scheduler.run().await;
    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

async fn cmd_run(target: &str) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;
    let config = Config::load();

    println!("Running job '{}'...", job.name);

    // Create log directory and path
    let log_dir = boo::config::boo_dir().join("runs").join(job.id.to_string());
    std::fs::create_dir_all(&log_dir)?;
    let now = Utc::now();
    let log_path = log_dir.join(format!("manual_{}_{:03}.log", now.format("%Y%m%d_%H%M%S"), now.timestamp_subsec_millis()));

    let result = executor::execute_job(&job, &config, &log_path).await?;

    // Save run record
    let record = boo::job::RunRecord {
        job_id: job.id,
        job_name: job.name.clone(),
        fired_at: now,
        scheduled_for: now,
        missed_count: 0,
        duration_secs: result.duration_secs,
        exit_code: result.exit_code,
        success: result.success,
        output_path: result.output_path.clone(),
        manual: true,
    };
    store.append_run_record(&record)?;

    println!("Job completed: success={}, duration={:.2}s", result.success, result.duration_secs);
    if let Some(ref response) = result.response {
        println!("\n{response}");
    }
    Ok(())
}

// --- Sync commands (no async needed) ---

fn cmd_add(name: String, cron: String, prompt: String, dir: PathBuf,
           agent: Option<String>, timeout: Option<u64>, timezone: Option<String>,
) -> boo::error::Result<()> {
    cron_eval::next_occurrence(&cron, Utc::now())?;

    // Validate working directory exists
    if !dir.exists() {
        return Err(boo::error::BooError::Other(format!(
            "Working directory does not exist: {}", dir.display()
        )));
    }

    // Check for duplicate name
    let store = JobStore::new()?;
    let existing = store.load_jobs()?;
    if existing.iter().any(|j| j.name == name) {
        return Err(boo::error::BooError::Other(format!(
            "Job with name '{}' already exists", name
        )));
    }

    let mut job = Job::new(name, cron, prompt, dir);
    job.agent = agent;
    job.timeout_secs = timeout;
    job.timezone = timezone;
    store.add_job(job.clone())?;
    println!("Added job '{}' with ID {}", job.name, job.id);
    Ok(())
}

fn cmd_remove(target: &str, delete_logs: bool, keep_logs: bool) -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let job = resolve_job(&store, target)?;

    let records = store.load_run_records(job.id, 1)?;
    if !records.is_empty() && !keep_logs {
        let should_delete = if delete_logs {
            true
        } else {
            eprint!("Job '{}' has run history. Delete logs too? [y/N] ", job.name);
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            input.trim().eq_ignore_ascii_case("y")
        };
        if should_delete {
            let runs_dir = boo::config::boo_dir().join("runs").join(job.id.to_string());
            let jsonl = boo::config::boo_dir().join("runs").join(format!("{}.jsonl", job.id));
            let _ = std::fs::remove_dir_all(&runs_dir);
            let _ = std::fs::remove_file(&jsonl);
            eprintln!("Deleted run history.");
        }
    }

    store.remove_job(job.id)?;
    println!("Removed job '{}' ({})", job.name, job.id);
    Ok(())
}

fn cmd_list() -> boo::error::Result<()> {
    let store = JobStore::new()?;
    let jobs = store.load_jobs()?;
    if jobs.is_empty() {
        println!("No jobs configured");
        return Ok(());
    }
    println!("{:<8} {:<20} {:<15} {:<8} {:<20}", "ID", "Name", "Cron", "Enabled", "Next Fire");
    println!("{}", "-".repeat(80));
    for job in jobs {
        let id_short = &job.id.to_string()[..8];
        let enabled = if job.enabled { "yes" } else { "no" };
        let next = if job.enabled {
            cron_eval::next_occurrence(&job.cron_expr, Utc::now())
                .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                .unwrap_or_else(|_| "invalid cron".into())
        } else {
            "disabled".into()
        };
        println!("{:<8} {:<20} {:<15} {:<8} {:<20}", id_short, job.name, job.cron_expr, enabled, next);
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
    let pid_path = boo_dir.join("daemon.pid");

    let running = is_daemon_running(&pid_path);
    println!("Daemon: {}", if running { "running" } else { "stopped" });

    let store = JobStore::new()?;
    let jobs: Vec<_> = store.load_jobs()?.into_iter().filter(|j| j.enabled).collect();
    if jobs.is_empty() {
        println!("No enabled jobs");
        return Ok(());
    }
    println!("\nNext fire times:");
    for job in jobs {
        match cron_eval::next_occurrence(&job.cron_expr, Utc::now()) {
            Ok(next) => println!("  {} - {}", job.name, next.format("%Y-%m-%d %H:%M:%S UTC")),
            Err(_) => println!("  {} - invalid cron expression", job.name),
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
        // Show the clean response from the most recent run
        let latest = &records[records.len() - 1];
        let response_path = latest.output_path.with_extension("response");
        match std::fs::read_to_string(&response_path) {
            Ok(content) => println!("{content}"),
            Err(_) => {
                // Fall back to full log with ANSI stripped
                match std::fs::read_to_string(&latest.output_path) {
                    Ok(content) => println!("{content}"),
                    Err(e) => println!("Could not read output: {e}"),
                }
            }
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
    let jobs = store.load_jobs()?;
    jobs.into_iter().find(|j| j.name == target)
        .ok_or_else(|| boo::error::BooError::Other(format!("Job not found: {target}")))
}

/// Check if daemon is actually running by verifying the PID is alive.
fn is_daemon_running(pid_path: &std::path::Path) -> bool {
    let pid_str = match std::fs::read_to_string(pid_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pid: i32 = match pid_str.trim().parse() {
        Ok(p) => p,
        Err(_) => return false,
    };
    // kill(pid, 0) checks if process exists without sending a signal
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid, 0) == 0 }
    }
    #[cfg(not(unix))]
    {
        // On non-Unix, fall back to PID file existence
        true
    }
}
