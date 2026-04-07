# Changelog

All notable changes to this project will be documented in this file.

## [0.5.0] - 2026-04-07

### Features
- `boo run --interactive --new-window`: open a new terminal window for interactive sessions, enabling orchestrator handoffs
- Shared `open_terminal_with_command` helper consolidates terminal launch logic for both resume and run
- Active run tracking: `~/.boo/runs/<job-id>.active` files track in-flight jobs with PID and start time
- `boo status` now shows active runs with PID, source (manual/daemon), and elapsed time; running jobs marked with ▶
- `boo wait <job>`: poll until an active run completes, print result, exit 1 on failure
- `boo status --format json` includes `active_runs`, `running`, `pid`, `running_since` fields

### Bug Fixes
- Ensure working directory and log directory exist before job execution
- Fix iTerm session restoration loop: use AppleScript `write text` for iTerm, `do script` for Terminal.app instead of `.command` files
- Stale `.active` files auto-cleaned when PID is no longer alive
- Gitignore backup files

### Code Quality
- DRY: shared `is_pid_alive` in lib.rs replaces duplicated PID checks in main.rs

### Docs
- README: add `edit`, `stats`, `wait` commands, `--trust-all-tools`/`--trust-tools`/`--runner`/`--description` options, `--follow`/`--interactive`/`--new-window` run flags, `notify_webhook` config, config reference table, editing jobs section
- AGENTS.md: update command count, notifier.rs description, add terminal handoff design decision

## [0.4.0] - 2026-03-05

### Bug Fixes
- Kill entire process group on timeout, preventing orphaned `kiro-cli-chat` processes that survive after the parent wrapper is killed
- Daemon status correctly reports stopped when pid file is missing

### Features
- `--trust-tools` flag for selective tool trust per job
- `--description` field for human-readable job descriptions

## [0.3.0] - 2026-02-26

### New Commands
- `boo edit`: modify existing job settings without remove/re-add
- `boo stats`: run statistics with 24h/7d/30d windows, JSON/CSV output

### Features
- `--trust-all-tools` opt-in flag per job (no longer forced)
- `notify_webhook`: fire-and-forget HTTP POST for job lifecycle events
- `boo run --follow`: print only response content for programmatic use
- `boo run --interactive`: launch foreground kiro-cli session for a job
- JSON output format for `status`, `logs`, and `list` commands
- Human-readable schedule descriptions in JSON output
- Notification body prefers `Summary:` line or last non-empty line

### Bug Fixes
- Add `~/.local/bin` to launchd PATH for kiro-cli and user tools
- Update Windows crate API for windows 0.59 compatibility
- Gate plist test to macOS only (was failing on Linux CI)
- Resolve clippy warnings (unused params, needless borrow)

### Code Quality
- Zero-warnings convention enforced
- Remove unused imports

## [0.2.0] - 2026-02-22

### Dependencies
- Upgrade croner 2→3
- Update clap 4.5.60
- Remove unused chrono-tz

### Bug Fixes
- Fix disk space leak in rotate_logs (old .log/.response files never cleaned up)
- Fix broken Linux/Windows notification replies (missing user_info)
- Fix job name derived from working_dir path (broke custom --dir)
- Fix is_daemon_running non-unix always returning true
- Fix urldecode multi-byte UTF-8 handling
- Fix CSV output escaping
- Fix redundant command storage in job.prompt
- Fix duplicate "Created BooURL.app" print

### Code Quality
- Remove dead code (execute_job_interactive, save_jobs, resume_with_prompt, open_file_pub)
- DRY notification formatting (format_notification)
- Shared test_config helper
- Thread job_name through notification chain
- Remove unused _working_dir param from open_terminal_resume

### CI
- Add macOS + Windows to test matrix
- Fix all cross-platform test issues

### Docs
- Fix README heartbeat default, binary size, notification description
- Fix AGENTS.md notification claims, dep table, reserved fields
- Add version bump convention

## [0.1.1] - 2026-02-21

### Bug Fixes
- Error notifications open log file on click
- Show retry count in error notifications
- Remove --require-mcp-startup flag

## [0.1.0] - 2026-02-19

Initial release. Cross-platform scheduler daemon for kiro-cli prompts with cron/at/every schedules, heartbeat-based missed schedule recovery, native notifications with click-to-open and inline reply, retry on failure, shell command support, and boo:// URL scheme.
