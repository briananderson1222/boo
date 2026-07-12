use crate::executor::ExecutionResult;
use crate::job::{self, Job};
use crate::notification_service::{NotificationSender, NotifyRequest};

/// Webhook lifecycle events, serialized as {"event": "job.started", ...}.
pub enum WebhookEvent<'a> {
    Started,
    /// Run finished executing — reports job.completed or job.failed based
    /// on the exit status, with duration and resolved artifact.
    Finished(&'a ExecutionResult),
    /// Run errored before producing a result (timeout, spawn failure).
    Errored(&'a str),
}

fn webhook_payload(job: &Job, event: &WebhookEvent) -> serde_json::Value {
    let id = &job.id.to_string()[..8];
    match event {
        WebhookEvent::Started => serde_json::json!({
            "event": "job.started",
            "job": job.name,
            "id": id,
        }),
        WebhookEvent::Finished(result) => {
            let artifact = job
                .open_artifact
                .as_ref()
                .and_then(|a| job::resolve_artifact(&job.working_dir, a))
                .map(|p| p.to_string_lossy().to_string());
            serde_json::json!({
                "event": if result.success { "job.completed" } else { "job.failed" },
                "job": job.name,
                "id": id,
                "success": result.success,
                "duration_secs": result.duration_secs,
                "artifact": artifact,
            })
        }
        WebhookEvent::Errored(error) => serde_json::json!({
            "event": "job.failed",
            "job": job.name,
            "id": id,
            "error": error,
        }),
    }
}

/// POST a webhook event and wait for delivery. Use from CLI paths that exit
/// right after — a spawned task would be dropped before the request leaves.
pub async fn send_webhook_event(url: &str, job: &Job, event: WebhookEvent<'_>) {
    if cfg!(test) {
        return;
    }
    let body = webhook_payload(job, &event);
    if let Err(e) = webhook_post(url, &body).await {
        eprintln!("Webhook delivery to {url} failed: {e}");
    }
}

/// Fire-and-forget webhook event for the long-lived daemon.
pub fn spawn_webhook_event(url: &str, job: &Job, event: WebhookEvent<'_>) {
    if cfg!(test) {
        return;
    }
    let url = url.to_string();
    let body = webhook_payload(job, &event);
    tokio::spawn(async move {
        if let Err(e) = webhook_post(&url, &body).await {
            eprintln!("Webhook delivery to {url} failed: {e}");
        }
    });
}

async fn webhook_post(url: &str, body: &serde_json::Value) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    client
        .post(url)
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

/// Build summary and body strings for a job result notification.
fn format_notification(job: &Job, result: &ExecutionResult) -> (String, String) {
    let summary = if result.success {
        format!(
            "✓ Job '{}' completed ({:.1}s)",
            job.name, result.duration_secs
        )
    } else {
        let code = result
            .exit_code
            .map(|c| format!("exit {c}"))
            .unwrap_or("killed".into());
        format!(
            "✗ Job '{}' failed ({}, {:.1}s)",
            job.name, code, result.duration_secs
        )
    };
    let body = result
        .response
        .as_deref()
        .map(|r| {
            // Prefer last "Summary:" line if present, otherwise use last non-empty line
            let trimmed = r.trim();
            let last_meaningful = trimmed
                .lines()
                .rev()
                .find(|l| l.starts_with("Summary:"))
                .or_else(|| trimmed.lines().rev().find(|l| !l.trim().is_empty()))
                .unwrap_or(trimmed);
            last_meaningful.trim().chars().take(200).collect::<String>()
        })
        .unwrap_or_default();
    (summary, body)
}

/// Send a completion/failure notification. Optionally opens an artifact on click.
pub fn notify(job: &Job, result: &ExecutionResult) {
    let (summary, body) = format_notification(job, result);
    let open_path = job
        .open_artifact
        .as_ref()
        .and_then(|a| job::resolve_artifact(&job.working_dir, a));
    spawn_notify(
        &summary,
        &body,
        open_path
            .as_ref()
            .map(|p| p.to_string_lossy().as_ref().to_owned())
            .as_deref(),
        Some(&job.working_dir.to_string_lossy()),
        Some(&job.name),
    );
}

/// Send a notification using the daemon's sender if available, otherwise subprocess.
pub fn send_notification(job: &Job, result: &ExecutionResult, sender: &Option<NotificationSender>) {
    let (summary, body) = format_notification(job, result);
    let open_path = job
        .open_artifact
        .as_ref()
        .and_then(|a| job::resolve_artifact(&job.working_dir, a))
        .or_else(|| {
            if !result.success && result.output_path.exists() {
                Some(result.output_path.clone())
            } else {
                None
            }
        });

    if let Some(s) = sender {
        s.send(NotifyRequest {
            summary,
            body,
            open: open_path.map(|p| p.to_string_lossy().to_string()),
            working_dir: Some(job.working_dir.to_string_lossy().to_string()),
            job_name: Some(job.name.clone()),
        });
    } else {
        spawn_notify(
            &summary,
            &body,
            open_path
                .as_ref()
                .map(|p| p.to_string_lossy().as_ref().to_owned())
                .as_deref(),
            Some(&job.working_dir.to_string_lossy()),
            Some(&job.name),
        );
    }
}

/// Send an error/timeout notification for a job.
pub fn notify_error(job: &Job, error: &str) {
    let summary = format!("✗ Job '{}' error", job.name);
    spawn_notify(
        &summary,
        error,
        None,
        Some(&job.working_dir.to_string_lossy()),
        Some(&job.name),
    );
}

/// Send a start notification for one or more jobs.
pub fn notify_start(job_names: &[&str]) {
    let (summary, body) = if job_names.len() == 1 {
        (
            format!("🚀 Job '{}' starting...", job_names[0]),
            format!("Run 'boo disable {}' to pause", job_names[0]),
        )
    } else {
        (
            format!("🚀 {} jobs starting", job_names.len()),
            job_names.join(", "),
        )
    };
    spawn_notify(&summary, &body, None, None, None);
}

/// Spawn the internal-notify child process.
fn spawn_notify(
    summary: &str,
    body: &str,
    open: Option<&str>,
    working_dir: Option<&str>,
    job_name: Option<&str>,
) {
    if cfg!(test) || std::env::var_os("BOO_NO_NOTIFY").is_some() {
        return;
    }

    // Prefer the .app bundle binary (required for native notifications on macOS)
    let exe = {
        #[cfg(target_os = "macos")]
        {
            let bundle =
                dirs::home_dir().map(|h| h.join("Applications/Boo.app/Contents/MacOS/boo"));
            match bundle {
                Some(p) if p.exists() => p,
                _ => std::env::current_exe().unwrap_or_else(|_| "boo".into()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            std::env::current_exe().unwrap_or_else(|_| "boo".into())
        }
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["internal-notify", summary, body])
        .stderr(std::process::Stdio::null());
    if let Some(path) = open {
        cmd.args(["--open", path]);
    }
    if let Some(dir) = working_dir {
        cmd.args(["--working-dir", dir]);
    }
    if let Some(name) = job_name {
        cmd.args(["--job-name", name]);
    }
    let _ = cmd.spawn();
}

/// Called by the hidden `internal-notify` subcommand. Runs on the main thread (required for macOS).
pub fn send_and_exit(
    summary: &str,
    body: &str,
    open: Option<&str>,
    _working_dir: Option<&str>,
    job_name: Option<&str>,
) {
    use user_notify::{
        NotificationBuilder, NotificationCategory, NotificationCategoryAction,
        NotificationResponseAction,
    };

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let manager = rt.block_on(async {
        let m = user_notify::get_notification_manager("com.boo.scheduler".into(), None);
        let _ = m.first_time_ask_for_notification_permission().await;
        m
    });

    // Set up click + inline reply callback
    let open_path = open.map(|s| s.to_string());
    let name = job_name.map(|s| s.to_string());
    let (tx, rx) = std::sync::mpsc::channel::<()>();

    let _ = manager.register(
        Box::new(move |response| {
            match &response.action {
                NotificationResponseAction::Default => {
                    if let Some(ref path) = open_path {
                        open_file(path);
                    }
                }
                NotificationResponseAction::Other(id) if id == "reply" => {
                    if let Some(text) = &response.user_text {
                        let text = text.trim();
                        if !text.is_empty() {
                            if let Some(ref n) = name {
                                open_terminal_resume(n, Some(text), false);
                            }
                        }
                    }
                }
                _ => {}
            }
            let _ = tx.send(());
        }),
        vec![NotificationCategory {
            identifier: "boo-job".into(),
            actions: vec![NotificationCategoryAction::TextInputAction {
                identifier: "reply".into(),
                title: "Reply".into(),
                input_button_title: "Send".into(),
                input_placeholder: "Follow up...".into(),
            }],
        }],
    );

    // Send notification
    let sent = rt.block_on(async {
        let n = NotificationBuilder::new()
            .title(summary)
            .body(body)
            .set_category_id("boo-job");
        manager.send_notification(n).await
    });

    if sent.is_err() {
        if let Some(path) = open {
            open_file(path);
        }
        std::process::exit(0);
    }

    // Run the macOS run loop so delegate callbacks are delivered
    #[cfg(target_os = "macos")]
    {
        use std::time::{Duration, Instant};
        extern "C" {
            fn CFRunLoopRunInMode(
                mode: *const std::ffi::c_void,
                seconds: f64,
                return_after: u8,
            ) -> i32;
            static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 0);
            }
            if rx.try_recv().is_ok() {
                break;
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = rx.recv_timeout(std::time::Duration::from_secs(120));
    }

    std::mem::forget(manager);
    std::process::exit(0);
}

/// Open a terminal and run `boo resume`. Used by notification reply and URL scheme.
pub fn open_terminal_resume(job_name: &str, prompt: Option<&str>, previous: bool) {
    let boo_bin = std::env::current_exe().unwrap_or_else(|_| "boo".into());
    let boo = boo_bin.to_string_lossy();

    let mut args = format!(
        "'{}' resume '{}'",
        boo.replace('\'', "'\\''"),
        job_name.replace('\'', "'\\''")
    );
    if previous {
        args.push_str(" --previous");
    }
    if let Some(p) = prompt {
        args.push_str(&format!(" '{}'", p.replace('\'', "'\\''")));
    }

    open_terminal_with_command(&args, &format!("resume-{}", std::process::id()));
}

/// Open a new terminal window and run a fresh interactive kiro-cli session.
/// Used by `boo run --interactive --new-window` for orchestrator handoffs.
pub fn open_terminal_run(
    job_name: &str,
    agent: Option<&str>,
    prompt: &str,
    working_dir: &std::path::Path,
) {
    let config = crate::config::Config::load();
    let kiro = &config.kiro_cli_path;

    let mut args = format!(
        "cd '{}' && '{}' chat",
        working_dir.to_string_lossy().replace('\'', "'\\''"),
        kiro.replace('\'', "'\\''")
    );
    if let Some(a) = agent {
        args.push_str(&format!(" --agent '{}'", a.replace('\'', "'\\''")));
    }
    args.push_str(&format!(" -- '{}'", prompt.replace('\'', "'\\''")));

    open_terminal_with_command(&args, job_name);
}

#[allow(unused_variables)]
fn open_terminal_with_command(args: &str, label: &str) {
    #[cfg(target_os = "macos")]
    {
        let config = crate::config::Config::load();
        let terminal = config.terminal.as_deref().unwrap_or_else(|| {
            for app in ["iTerm", "Ghostty", "Alacritty", "kitty", "WezTerm"] {
                if std::path::Path::new(&format!("/Applications/{app}.app")).exists() {
                    return app;
                }
            }
            "Terminal"
        });
        // iTerm and Terminal.app use AppleScript to avoid .command file session restoration loops.
        // Use "write text" so the session is a normal shell — iTerm won't re-run the command on restart.
        if terminal == "iTerm" {
            let escaped = args.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                "tell application \"iTerm\"\n\tactivate\n\tset newWindow to (create window with default profile)\n\ttell current session of newWindow\n\t\twrite text \"{escaped}\"\n\tend tell\nend tell"
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .spawn();
        } else if terminal == "Terminal" {
            let escaped = args.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                "tell application \"Terminal\"\n\tactivate\n\tdo script \"{escaped}\"\nend tell"
            );
            let _ = std::process::Command::new("osascript")
                .args(["-e", &script])
                .spawn();
        } else {
            let tmp = crate::config::boo_dir().join(format!("handoff-{}.command", label));
            let _ = std::fs::write(&tmp, format!("#!/bin/sh\nrm -f \"$0\"\nexec {args}\n"));
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
            let _ = std::process::Command::new("open")
                .args(["-a", terminal])
                .arg(&tmp)
                .status();
        }
    }

    #[cfg(target_os = "linux")]
    {
        let terminals = [
            ("x-terminal-emulator", vec!["-e"]),
            ("gnome-terminal", vec!["--"]),
            ("xterm", vec!["-e"]),
        ];
        for (term, term_args) in &terminals {
            let mut c = std::process::Command::new(term);
            c.args(term_args).args(["sh", "-c", args]);
            if c.spawn().is_ok() {
                return;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "cmd", "/K", args])
            .spawn();
    }
}

/// Open a file with the system default handler.
pub fn open_file(path: &str) {
    let path = std::path::Path::new(path);
    if !path.exists() {
        return;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::ExecutionResult;
    use crate::job::Job;

    fn job() -> Job {
        Job::new("nightly", "0 0 * * *", "do the thing", std::env::temp_dir())
    }

    fn result(success: bool, exit_code: Option<i32>, response: Option<&str>) -> ExecutionResult {
        ExecutionResult {
            exit_code,
            success,
            duration_secs: 1.25,
            output_path: std::env::temp_dir().join("x.log"),
            response: response.map(|s| s.to_string()),
        }
    }

    #[test]
    fn format_notification_success_summary_and_body() {
        let (summary, body) = format_notification(
            &job(),
            &result(true, Some(0), Some("noise\nSummary: all good")),
        );
        assert!(summary.starts_with("✓ Job 'nightly' completed"));
        assert!(summary.contains("1.2s") || summary.contains("1.3s"));
        // Prefers the "Summary:" line over the last line
        assert_eq!(body, "Summary: all good");
    }

    #[test]
    fn format_notification_body_falls_back_to_last_nonempty_line() {
        let (_s, body) =
            format_notification(&job(), &result(true, Some(0), Some("first\nlast\n\n")));
        assert_eq!(body, "last");
    }

    #[test]
    fn format_notification_failure_reports_exit_code() {
        let (summary, _b) = format_notification(&job(), &result(false, Some(2), None));
        assert!(summary.starts_with("✗ Job 'nightly' failed"));
        assert!(summary.contains("exit 2"));
    }

    #[test]
    fn format_notification_failure_without_code_says_killed() {
        let (summary, _b) = format_notification(&job(), &result(false, None, None));
        assert!(summary.contains("killed"));
    }

    #[test]
    fn webhook_payload_started() {
        let j = job();
        let v = webhook_payload(&j, &WebhookEvent::Started);
        assert_eq!(v["event"], "job.started");
        assert_eq!(v["job"], "nightly");
        assert_eq!(v["id"], j.id.to_string()[..8]);
    }

    #[test]
    fn webhook_payload_finished_success_is_completed() {
        let v = webhook_payload(
            &job(),
            &WebhookEvent::Finished(&result(true, Some(0), None)),
        );
        assert_eq!(v["event"], "job.completed");
        assert_eq!(v["success"], true);
        assert_eq!(v["duration_secs"], 1.25);
    }

    #[test]
    fn webhook_payload_finished_failure_is_failed() {
        let v = webhook_payload(
            &job(),
            &WebhookEvent::Finished(&result(false, Some(1), None)),
        );
        assert_eq!(v["event"], "job.failed");
        assert_eq!(v["success"], false);
    }

    #[test]
    fn webhook_payload_errored_carries_message() {
        let v = webhook_payload(&job(), &WebhookEvent::Errored("timed out"));
        assert_eq!(v["event"], "job.failed");
        assert_eq!(v["error"], "timed out");
    }
}
