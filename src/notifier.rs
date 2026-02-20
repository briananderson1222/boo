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

    spawn_notify(&summary, &body, Some(&open_path.to_string_lossy()), Some(&job.working_dir.to_string_lossy()));
}

/// Send an error/timeout notification for a job.
pub fn notify_error(job: &Job, error: &str) {
    let summary = format!("✗ Job '{}' error", job.name);
    spawn_notify(&summary, error, None, Some(&job.working_dir.to_string_lossy()));
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
    spawn_notify(&summary, &body, None, None);
}

/// Spawn the internal-notify child process.
fn spawn_notify(summary: &str, body: &str, open: Option<&str>, working_dir: Option<&str>) {
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
    if let Some(dir) = working_dir {
        cmd.args(["--working-dir", dir]);
    }
    let _ = cmd.spawn();
}

/// Called by the hidden `internal-notify` subcommand. Runs on the main thread (required for macOS).
pub fn send_and_exit(summary: &str, body: &str, open: Option<&str>, working_dir: Option<&str>) {
    use user_notify::{NotificationBuilder, NotificationCategory, NotificationCategoryAction, NotificationResponseAction};

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
    let work_dir = working_dir.map(|s| s.to_string());
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
                    if let (Some(text), Some(ref dir)) = (&response.user_text, &work_dir) {
                        let text = text.trim();
                        if !text.is_empty() {
                            resume_with_prompt(dir, text);
                        }
                    }
                }
                _ => {}
            }
            let _ = tx.send(());
        }),
        vec![NotificationCategory {
            identifier: "boo-job".into(),
            actions: vec![
                NotificationCategoryAction::TextInputAction {
                    identifier: "reply".into(),
                    title: "Reply".into(),
                    input_button_title: "Send".into(),
                    input_placeholder: "Follow up...".into(),
                },
            ],
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
        if let Some(path) = open { open_file(path); }
        std::process::exit(0);
    }

    // Run the macOS run loop so delegate callbacks are delivered
    #[cfg(target_os = "macos")]
    {
        use std::time::{Duration, Instant};
        extern "C" {
            fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after: u8) -> i32;
            static kCFRunLoopDefaultMode: *const std::ffi::c_void;
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        while Instant::now() < deadline {
            unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 0); }
            if rx.try_recv().is_ok() { break; }
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = rx.recv_timeout(std::time::Duration::from_secs(120));
    }

    std::mem::forget(manager);
    std::process::exit(0);
}

/// Open a terminal and run `boo resume` with the reply prompt.
fn resume_with_prompt(working_dir: &str, prompt: &str) {
    let config = crate::config::Config::load();
    let boo_bin = std::env::current_exe().unwrap_or_else(|_| "boo".into());
    let boo = boo_bin.to_string_lossy();

    // Find the job name from working_dir (last path component)
    let job_name = std::path::Path::new(working_dir)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        let terminal = config.terminal.as_deref().unwrap_or_else(|| {
            for app in &["iTerm", "Ghostty", "Alacritty", "kitty", "WezTerm"] {
                if std::path::Path::new(&format!("/Applications/{app}.app")).exists() {
                    return app;
                }
            }
            "Terminal"
        });
        let tmp = crate::config::boo_dir().join("reply.command");
        let script = format!("#!/bin/sh\nexec '{}' resume '{}' '{}'\n",
            boo.replace('\'', "'\\''"),
            job_name.replace('\'', "'\\''"),
            prompt.replace('\'', "'\\''"),
        );
        let _ = std::fs::write(&tmp, &script);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
        if terminal == "Terminal" {
            let script = format!("tell application \"Terminal\"\n\tactivate\n\tdo script \"'{}'\"\nend tell",
                tmp.to_string_lossy().replace('\'', "'\\''"));
            let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
        } else {
            let _ = std::process::Command::new("open").args(["-a", terminal]).arg(&tmp).spawn();
        }
    }

    #[cfg(target_os = "linux")]
    {
        let cmd = format!("exec '{}' resume '{}' '{}'", boo, job_name, prompt.replace('\'', "'\\''"));
        let terminals = [("x-terminal-emulator", vec!["-e"]), ("gnome-terminal", vec!["--"]), ("xterm", vec!["-e"])];
        for (term, args) in &terminals {
            let mut c = std::process::Command::new(term);
            c.args(args).args(["sh", "-c", &cmd]);
            if c.spawn().is_ok() { return; }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let cmd = format!("\"{}\" resume \"{}\" \"{}\"", boo, job_name, prompt);
        let _ = std::process::Command::new("cmd").args(["/C", "start", "cmd", "/K", &cmd]).spawn();
    }
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
