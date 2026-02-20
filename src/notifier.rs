use crate::executor::ExecutionResult;
use crate::job::{self, Job};

/// Send a completion/failure notification. Optionally opens an artifact on click.
pub fn notify(job: &Job, result: &ExecutionResult) {
    let summary = if result.success {
        format!("✓ Job '{}' completed ({:.1}s)", job.name, result.duration_secs)
    } else {
        let code = result.exit_code.map(|c| format!("exit {c}")).unwrap_or("killed".into());
        format!("✗ Job '{}' failed ({}, {:.1}s)", job.name, code, result.duration_secs)
    };

    let body = result.response.as_deref()
        .map(|r| r.chars().take(200).collect::<String>())
        .unwrap_or_default();

    let open_path = job.open_artifact.as_ref()
        .and_then(|a| job::resolve_artifact(&job.working_dir, a))
        .unwrap_or_else(|| result.output_path.with_extension("response"));

    spawn_notify(&summary, &body, Some(&open_path.to_string_lossy()));
}

/// Send an error/timeout notification for a job.
pub fn notify_error(job: &Job, error: &str) {
    let summary = format!("✗ Job '{}' error", job.name);
    spawn_notify(&summary, error, None);
}

/// Send a batched start notification for multiple jobs.
pub fn notify_start(job_names: &[&str]) {
    let (summary, body) = if job_names.len() == 1 {
        (format!("🚀 Job '{}' starting...", job_names[0]),
         format!("Run 'boo disable {}' to pause", job_names[0]))
    } else {
        (format!("🚀 {} jobs starting", job_names.len()),
         job_names.join(", "))
    };
    spawn_notify(&summary, &body, None);
}

/// Spawn the internal-notify child process.
fn spawn_notify(summary: &str, body: &str, open: Option<&str>) {
    if cfg!(test) || std::env::var_os("BOO_NO_NOTIFY").is_some() { return; }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["internal-notify", summary, body])
        .stderr(std::process::Stdio::null());
    if let Some(path) = open {
        cmd.args(["--open", path]);
    }
    let _ = cmd.spawn();
}

/// Called by the hidden `internal-notify` subcommand.
pub async fn send_and_exit(summary: &str, body: &str, open: Option<&str>) {
    send_impl(summary, body, open).await;
}

async fn send_impl(summary: &str, body: &str, open: Option<&str>) {
    use user_notify::NotificationBuilder;

    let manager = user_notify::get_notification_manager("com.boo.scheduler".into(), None);

    match manager.first_time_ask_for_notification_permission().await {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("Notification permission denied. Enable in System Settings > Notifications > Boo");
            if let Some(path) = open { open_file(path); }
            return;
        }
        Err(e) => {
            eprintln!("Permission request error: {e}");
            if let Some(path) = open { open_file(path); }
            return;
        }
    }

    let n = NotificationBuilder::new()
        .title(summary)
        .body(body);

    if let Err(e) = manager.send_notification(n).await {
        eprintln!("Notification error: {e}");
    }

    // Open artifact directly (click-to-open requires MainThreadMarker, future improvement)
    if let Some(path) = open {
        open_file(path);
    }

    std::mem::forget(manager);
    std::process::exit(0);
}

/// Open a file with the system default handler.
fn open_file(path: &str) {
    let path = std::path::Path::new(path);
    if !path.exists() { return; }

    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(path).spawn(); }

    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(path).spawn(); }

    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("cmd").args(["/C", "start", "", &path.to_string_lossy()]).spawn(); }
}
