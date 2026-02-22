# Changelog

All notable changes to this project will be documented in this file.

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
