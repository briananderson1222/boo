# AGENTS.md

## Project: Boo

Cross-platform Rust scheduler daemon that fires kiro-cli prompts on cron/at/every schedules with heartbeat-based missed schedule recovery.

## Goals

1. **Reliable scheduled AI tasks** — cron, one-shot, and interval scheduling that survives sleep/wake, crashes, and reboots
2. **Cross-platform** — macOS, Linux, Windows from a single Rust binary
3. **Heartbeat pattern** — inspired by OpenClaw: periodic tick checks overdue jobs, coalesces missed runs
4. **Minimal footprint** — single ~2MB binary, no runtime dependencies, no GUI framework
5. **Developer-friendly** — CLI-first management, JSON persistence, property-based testing

## Architecture

```
src/
├── main.rs           # CLI entry point (clap) — 13 user commands + hidden internal-notify
├── scheduler.rs      # Heartbeat loop, job spawning, retry loop, delete-after-run, batched start notifications
├── store.rs          # Atomic JSON persistence with file locking (single lock scope per mutation)
├── executor.rs       # Subprocess spawning, stdin piping, concurrent stdout/stderr capture, timeout + kill
├── cron_eval.rs      # Schedule evaluation (cron/at/every), overdue detection, missed count
├── job.rs            # Job + RunRecord models with schedule types and retry/notification fields
├── config.rs         # Global config (~/.boo/config.json), warns on malformed config
├── clock.rs          # Clock trait with Clone bound (SystemClock + MockClock for testing)
├── notifier.rs       # Desktop notifications via child process, start/stop split, click-to-open artifact
├── installer.rs      # Platform-specific auto-start (launchd/systemd/Windows)
├── error.rs          # Error types
└── lib.rs            # Module re-exports

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
- **Notification via child process**: macOS suppresses notifications from backgrounded processes. Daemon spawns `boo internal-notify` as child
- **Clickable notifications**: `--open` arg on internal-notify opens artifact file via system handler
- **Batched start notifications**: Multiple jobs firing in same tick get one grouped notification
- **Retry with delay**: Configurable retry count and delay per job, each attempt logged
- **Delete-after-run**: One-shot `--at` jobs auto-remove after success
- **Per-job workspace directories**: `~/.boo/workspace/<name>/` isolates kiro-cli sessions per job
- **Natural language --at parsing**: Falls back to kiro-cli with `--trust-tools=` for safe LLM parsing, always confirms
- **BOO_NON_INTERACTIVE=1**: Env var set on spawned kiro-cli so agents can detect daemon context
- **Duplicate job name prevention**: `boo add` rejects duplicates
- **Working directory validation**: `boo add` verifies dir exists
- **PID alive check**: `kill(pid, 0)` on Unix

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
| `chrono` / `chrono-tz` | Time handling |
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
- **DRY**: consolidate shared logic into `lib.rs` or shared functions. Check for existing implementations before writing new code. If similar logic exists in multiple places, refactor into a single source of truth.
- **Every code change must include corresponding tests.** No exceptions. If a feature is added or a bug is fixed, a test proving it works must accompany the change.

## Maintaining This File

Update AGENTS.md whenever:
- A new module is added or an existing one changes purpose
- A key design decision is made or reversed
- The testing strategy changes (new crate, new pattern)
- Dependencies are added or removed

Keep it concise — this is an orientation document, not a spec. If a section grows beyond a few paragraphs, the detail belongs in code comments or README.md instead.

## Relationship to Other Documentation

AGENTS.md is an internal reference for developers and AI agents working on this codebase. It is **not** a source of truth for users. Do not link to AGENTS.md from README.md, CLI help text, or any user-facing documentation. User-facing docs should be self-contained. README.md or other docs may mention that AGENTS.md exists as a resource for contributors, but should never depend on it for content.
