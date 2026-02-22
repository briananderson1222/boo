pub mod clock;
pub mod config;
pub mod cron_eval;
pub mod error;
pub mod executor;
pub mod installer;
pub mod job;
pub mod notifier;
pub mod notification_service;
pub mod scheduler;
pub mod store;

/// Strip ANSI escape sequences and BEL characters from text.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if nc.is_ascii_alphabetic() { break; }
                }
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
        }
    }
}
