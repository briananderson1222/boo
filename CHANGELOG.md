# Changelog

All notable changes to this project will be documented in this file.

## [0.6.0] - 2026-07-12

Audit-driven hardening pass across correctness, security, dependencies, and docs.

### Bug Fixes
- `boo kill` now signals the actual job's process group. Active runs recorded the daemon's own PID, so kill hit the daemon and orphaned the child.
- Concurrent `boo edit`/`boo disable` during a run are no longer reverted: post-run bookkeeping re-reads under the lock and updates only `last_run`.
- Timed-out or spawn-failed runs now write a run record, advance `last_run`, and respect retry — previously they refired every heartbeat forever with no log entry.
- `boo edit --at` on a previously-run job fires again (schedule edits now reset `last_run` appropriately).
- `missed_count` no longer counts the current fire (on-time runs report 0); `every` jobs report real missed intervals instead of a hardcoded 0.
- `boo add --every "5µ"` and other multi-byte inputs error cleanly instead of panicking.
- Webhooks actually speak HTTPS (via `reqwest`/rustls) instead of writing plaintext to port 443; delivery is awaited on CLI exit and failures are logged.
- Windows CI is green: test isolation via `BOO_HOME` (the `dirs` crate ignores `HOME`/`USERPROFILE` on Windows).

### Features
- `--timezone` is now honored: cron/at schedules evaluate in the job's IANA timezone with DST, defaulting to UTC. Unknown zones are rejected.
- `--allow-overlap` and `--allow-url-trigger` flags on `add`/`edit`; `edit` can toggle `--delete-after-run`.

### Security
- `boo://run` and `boo://resume` require per-job `allow_url_trigger` (default off): any web page can open such links, so a job must opt in.
- On Unix, `~/.boo` is created `0700` and job/config/log files are written `0600` (prompts, webhook secrets, and transcripts were world-readable).
- Daemon handles SIGTERM (not just SIGINT) so `launchctl unload`/`systemctl stop` run the graceful drain and pid cleanup.
- `strip_ansi` strips OSC sequences, which were leaking terminal-title payloads into `.response` files.
- CI: `release.yml` `contents: write` scoped to the release job; third-party actions pinned to commit SHAs; Dependabot added for monthly action/cargo updates.

### Dependencies
- Dropped unmaintained `fs2` for std file locking; bumped `windows` 0.59 → 0.62; added `chrono-tz` and `reqwest`; removed now-unused `url`.
- `Cargo.lock` is now committed (reproducible builds, auditable dependencies).

### Refactor
- `cmd_add`/`cmd_edit` take typed `AddArgs`/`EditArgs` structs instead of 20+ positional parameters.
- Single `kill_process_group` and `WebhookEvent` helpers replace duplicated unsafe-kill and webhook-JSON blocks. `--runner` is validated against `{kiro, shell}`.

### CI recipes (feat/kiro-ci-recipes-bootstrap)
- Fixed `/open-issue <n>` selecting the wrong finding, the dead "no review found" guard, and the missing `code-review` label.
- Agent configs point at Rust sources; the reusable-action step passes inputs via `env:` (script-injection hardening); fork PRs skip cleanly; shared workflow scaffolding consolidated into a reusable workflow.

## [0.5.0] - 2026-04-07

### Features
- `boo run --interactive --new-window`: open a new terminal window for interactive sessions, enabling orchestrator handoffs
- Shared `open_terminal_with_command` helper consolidates terminal launch logic for both resume and run
- Active run tracking: `~/.boo/runs/<job-id>.active` files track in-flight jobs with PID and start time
- `boo status` now shows active runs with PID, source (manual/daemon), and elapsed time; running jobs marked with ▶
- `boo wait <job>`: poll until an active run completes, print result, exit 1 on failure
- `boo status --format json` includes `active_runs`, `running`, `pid`, `running_since` fields
- `boo list` now shows running status: ▶ prefix with elapsed time in table, `running`/`pid`/`running_since` in JSON
- `boo running`: show only currently active runs with PID, elapsed time, and source (manual/daemon)
- `boo kill <job>`: terminate an active run by name or ID, kills process group and cleans up tracking
- `boo clean`: remove completed one-shot jobs (expired `--at` jobs). Supports `--dry-run` and `--keep-logs`

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
