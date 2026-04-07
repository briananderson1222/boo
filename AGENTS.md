# AGENTS.md

## Project: Boo

Cross-platform Rust scheduler daemon that fires kiro-cli prompts on cron/at/every schedules with heartbeat-based missed schedule recovery.

## Goals

1. **Reliable scheduled AI tasks** — cron, one-shot, and interval scheduling that survives sleep/wake, crashes, and reboots
2. **Cross-platform** — macOS, Linux, Windows from a single Rust binary
3. **Heartbeat pattern** — inspired by OpenClaw: periodic tick checks overdue jobs, coalesces missed runs
4. **Minimal footprint** — single ~3.5MB binary, no runtime dependencies, no GUI framework
5. **Developer-friendly** — CLI-first management, JSON persistence, property-based testing

## Architecture

```
src/
├── main.rs           # CLI entry point (clap) — 16 user commands + hidden internal-notify
├── scheduler.rs      # Heartbeat loop, job spawning, retry loop, delete-after-run, notification integration
├── store.rs          # Atomic JSON persistence with file locking (single lock scope per mutation)
├── executor.rs       # Runner trait (KiroRunner, ShellRunner), subprocess spawning, stdin piping, timeout + kill
├── cron_eval.rs      # Schedule evaluation (cron/at/every), overdue detection, missed count
├── job.rs            # Job + RunRecord models with schedule types, runner/command, retry/notification fields
├── config.rs         # Global config (~/.boo/config.json), warns on malformed config
├── clock.rs          # Clock trait with Clone bound (SystemClock + MockClock for testing)
├── notifier.rs       # Notification formatting, subprocess fallback for boo run, open_file/open_terminal_resume/open_terminal_run
├── notification_service.rs  # Daemon notification thread: CFRunLoop + user-notify manager, click/reply callbacks
├── installer.rs      # Platform-specific auto-start (launchd/systemd/Windows), .app bundles, URL scheme
├── error.rs          # Error types
└── lib.rs            # Module re-exports, shared strip_ansi

tests/
└── cli.rs            # End-to-end CLI integration tests (assert_cmd)
```

## Key Design Decisions

- **Three schedule types**: cron (recurring), at (one-shot), every (interval) — mutually exclusive, stored as optional fields for backward compat
- **Atomic file operations**: All store mutations use a single file lock scope with tmp+rename
- **Stdin piping**: Prompts sent via stdin, not CLI args (not visible in `ps aux`)
- **Concurrent stdout/stderr capture**: `tokio::try_join!` prevents pipe deadlock
- **Child process kill on timeout**: No orphan processes
- **Separate stdout/stderr**: stdout = response, stderr = chrome. `.response` file is ANSI-stripped stdout
- **Clock trait**: Enables deterministic testing with MockClock
- **Coalesced missed runs**: Fire once with `missed_count` metadata (capped at 1000)
- **Daemon-direct notifications**: Daemon sends notifications from main thread (CFRunLoop) for reliable click/reply callbacks. `boo run` falls back to subprocess
- **Clickable notifications**: Click opens artifact file; inline reply opens terminal with `boo resume`
- **Runner trait**: Extensible execution — KiroRunner (kiro-cli), ShellRunner (raw commands), future CLIs add a trait impl
- **URL scheme**: `boo://` deep links handled by BooURL.app (Swift, compiled at install time)
- **BOO_JOB_NAME env var**: Set on spawned kiro-cli so agents know which job they're running as
- **Batched start notifications**: Multiple jobs firing in same tick send individual start notifications (one per job)
- **Retry with delay**: Configurable retry count and delay per job, each attempt logged
- **Delete-after-run**: One-shot `--at` jobs auto-remove after success
- **Per-job workspace directories**: `~/.boo/workspace/<name>/` isolates kiro-cli sessions per job
- **Natural language --at parsing**: Falls back to kiro-cli with `--trust-tools=` for safe LLM parsing, always confirms
- **BOO_NON_INTERACTIVE=1**: Env var set on spawned kiro-cli so agents can detect daemon context
- **Duplicate job name prevention**: `boo add` rejects duplicates
- **Working directory validation**: `boo add` verifies dir exists
- **PID alive check**: `kill(pid, 0)` on Unix
- **Terminal handoff**: `boo run --interactive --new-window` opens a new terminal window via `open_terminal_run`, shared `open_terminal_with_command` helper used by both resume and run
- **Reserved fields**: `Job.timezone` (stored, not yet used by cron_eval — always UTC) and `Job.allow_overlap` (checked by scheduler, but no CLI flag to enable)

## Testing Strategy

- **proptest** for property-based testing (serialization roundtrips, cron evaluation invariants, store consistency)
- **tempfile** for test isolation (no tests touch real `~/.boo`)
- **assert_cmd** for CLI integration tests (schedule types, mutual exclusion, flags, duration parsing, full lifecycle)
- **MockClock** for deterministic scheduler tests (overdue detection for cron/at/every, delete-after-run, skip disabled)
- **`echo` as kiro-cli substitute** in tests

## Dependencies

| Crate | Purpose |
|-------|---------|
| `croner` | Cron expression parsing |
| `tokio` | Async runtime, timers, subprocess |
| `clap` | CLI argument parsing (derive) |
| `serde` / `serde_json` | JSON persistence |
| `chrono` | Time handling |
| `uuid` | Job IDs |
| `user-notify` | Native desktop notifications (cross-platform) |
| `glob` | Glob pattern matching for artifact resolution |
| `fs2` | File locking |
| `dirs` | Cross-platform config directories |
| `libc` | PID alive check (Unix only) |
| `thiserror` | Error type derivation |

Dev dependencies: `proptest`, `tempfile`, `assert_cmd`, `predicates`

## Conventions

- Sync functions for CLI commands that don't need async (only `daemon`, `run`, and `add` are async)
- `impl Into<String>` for constructor ergonomics
- All public store methods are atomic (single lock scope for read-modify-write)
- Errors in spawned job tasks are caught with `eprintln!`, never crash the daemon
- `&Path` over `&PathBuf` in function signatures
- No `unwrap()` in production code (only in tests and MockClock mutex access)
- **Clean warnings before committing**: Run `cargo fix --lib -p boo --tests --allow-dirty` (or manually resolve) to eliminate unused imports and other warnings. The CI bar is zero warnings.
- **DRY**: consolidate shared logic into `lib.rs` or shared functions. Check for existing implementations before writing new code. If similar logic exists in multiple places, refactor into a single source of truth.
- **Every code change must include corresponding tests.** No exceptions. If a feature is added or a bug is fixed, a test proving it works must accompany the change.
- **Version bump after release**: After tagging a release, immediately bump `Cargo.toml` version to the next minor (e.g. 0.2.0 → 0.3.0) so the working tree always reflects in-progress work.
- **Changelog**: Update `CHANGELOG.md` when tagging a release. The annotated tag message should match the changelog entry. GitHub release notes are populated from the tag message.

## Maintaining This File

Update AGENTS.md whenever:
- A new module is added or an existing one changes purpose
- A key design decision is made or reversed
- The testing strategy changes (new crate, new pattern)
- Dependencies are added or removed

Keep it concise — this is an orientation document, not a spec. If a section grows beyond a few paragraphs, the detail belongs in code comments or README.md instead.

## Relationship to Other Documentation

AGENTS.md is an internal reference for developers and AI agents working on this codebase. It is **not** a source of truth for users. Do not link to AGENTS.md from README.md, CLI help text, or any user-facing documentation. User-facing docs should be self-contained. README.md or other docs may mention that AGENTS.md exists as a resource for contributors, but should never depend on it for content.
