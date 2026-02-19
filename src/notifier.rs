use crate::executor::ExecutionResult;
use crate::job::Job;

/// Send notification based on result success/failure.
/// Spawns a short-lived child process to deliver the notification,
/// which avoids macOS suppressing notifications from backgrounded daemons.
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

    // Spawn ourselves with a hidden notify subcommand so the notification
    // comes from a fresh foreground process context.
    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe {
        let _ = std::process::Command::new(exe)
            .args(["internal-notify", &summary, &body])
            .spawn();
        return;
    }

    // Fallback: send directly (may not show on macOS when backgrounded)
    let _ = notify_rust::Notification::new()
        .appname("boo")
        .summary(&summary)
        .body(&body)
        .sound_name("default")
        .show();
}

/// Called by the hidden `_notify` subcommand. Sends notification and exits.
pub fn send_and_exit(summary: &str, body: &str) {
    let _ = notify_rust::Notification::new()
        .appname("boo")
        .summary(summary)
        .body(body)
        .sound_name("default")
        .show();
}
