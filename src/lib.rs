pub mod clock;
pub mod config;
pub mod cron_eval;
pub mod error;
pub mod executor;
pub mod installer;
pub mod job;
pub mod notification_service;
pub mod notifier;
pub mod scheduler;
pub mod store;

/// Check if a process with the given PID is alive.
pub fn is_pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION};
        unsafe {
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) {
                let _ = CloseHandle(handle);
                return true;
            }
        }
        false
    }
}

/// Kill a process group (the process and all its descendants).
///
/// When `graceful` is true, sends SIGTERM first, waits up to 2s, then SIGKILLs
/// if anything survives. When false, SIGKILLs immediately. On Windows this
/// shells out to `taskkill /T /F` (always forceful).
pub fn kill_process_group(pid: u32, graceful: bool) {
    #[cfg(unix)]
    {
        unsafe {
            if graceful {
                if libc::killpg(pid as i32, libc::SIGTERM) != 0 {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            } else {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        if graceful {
            std::thread::sleep(std::time::Duration::from_secs(2));
            if is_pid_alive(pid) {
                unsafe {
                    libc::killpg(pid as i32, libc::SIGKILL);
                }
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = graceful;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

/// Strip ANSI escape sequences (CSI and OSC) and BEL characters from text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                // CSI sequence: ESC [ ... <final alpha byte>
                Some(&'[') => {
                    chars.next();
                    for nc in chars.by_ref() {
                        if nc.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                // OSC sequence: ESC ] ... terminated by BEL or ESC \
                Some(&']') => {
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '\x07' {
                            break;
                        }
                        if nc == '\x1b' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if c != '\x07' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
pub mod test_helpers {
    use crate::config::Config;

    pub fn test_config() -> Config {
        Config {
            kiro_cli_path: "echo".to_string(),
            default_timeout_secs: 5,
            max_log_runs: 10,
            heartbeat_secs: 60,
            terminal: None,
            notify_webhook: None,
        }
    }
}
