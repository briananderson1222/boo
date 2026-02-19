# AGENTS.md

## Project: Boo

Cross-platform Rust scheduler daemon that fires kiro-cli prompts on cron schedules with heartbeat-based missed schedule recovery.

## Goals

1. **Reliable scheduled AI tasks** — cron-based scheduling that survives sleep/wake, crashes, and reboots
2. **Cross-platform** — macOS, Linux, Windows from a single Rust binary
3. **Heartbeat pattern** — inspired by OpenClaw's approach: periodic tick checks overdue jobs, coalesces missed runs
4. **Minimal footprint** — single ~2MB binary, no runtime dependencies, no GUI framework
5. **Developer-friendly** — CLI-first management, JSON persistence, property-based testing

## Architecture

```
src/
├── main.rs           # CLI entry point (clap) — 12 user commands + hidden internal-notify
├── scheduler.rs      # Heartbeat loop, job spawning, graceful shutdown
├── store.rs          # Atomic JSON persistence with file locking (single lock scope per mutation)
├── executor.rs       # Subprocess spawning, stdin piping, concurrent stdout/stderr capture, timeout + kill
├── cron_eval.rs      # Cron parsing (croner), overdue detection, missed count (capped at 1000)
├── job.rs            # Job + RunRecord models
├── config.rs         # Global config (~/.boo/config.json), warns on malformed config
├── clock.rs          # Clock trait with Clone bound (SystemClock + MockClock for testing)
├── notifier.rs       # Desktop notifications via child process (macOS daemon workaround)
├── installer.rs      # Platform-specific auto-start (launchd/systemd/Windows)
├── error.rs          # Error types
└── lib.rs            # Module re-exports

tests/
└── cli.rs            # End-to-end CLI integration tests (assert_cmd)
```

## Key Design Decisions

- **Atomic file operations**: All store mutations use a single file lock scope with tmp+rename to prevent corruption on crash or concurrent access
- **Stdin piping**: Prompts sent via stdin, not CLI args (security — not visible in `ps aux`)
- **Concurrent stdout/stderr capture**: `tokio::try_join!` reads both streams simultaneously to prevent pipe deadlock when kiro-cli writes large output
- **Child process kill on timeout**: `child.kill().await` prevents orphan processes
- **Separate stdout/stderr**: stdout = kiro-cli response, stderr = chrome/warnings. Response file (`.response`) contains ANSI-stripped stdout only
- **Clock trait**: Enables deterministic testing with MockClock; trait requires `Clone + Send + Sync`
- **Coalesced missed runs**: On wake from sleep, overdue jobs fire once with `missed_count` metadata (capped at 1000 iterations)
- **Notification via child process**: macOS suppresses notifications from backgrounded processes. The daemon spawns `boo internal-notify <summary> <body>` as a child process which delivers the notification from a fresh process context. Falls back to direct notify-rust call if the binary can't be found
- **Per-job workspace directories**: Default working directory is `~/.boo/workspace/<job-name>/`, giving each job isolated kiro-cli sessions. Code-specific jobs can override with `--dir`
- **Session resume**: `boo resume <job>` opens kiro-cli's session picker scoped to that job's workspace directory and agent
- **Manual run tracking**: `boo run` saves the same log/response/record as daemon runs, tagged with `manual: true` in RunRecord
- **Remove with cleanup**: `boo remove` prompts for log deletion, with `--delete-logs`/`--keep-logs` flags to skip the prompt
- **Working directory validation**: `boo add` verifies the directory exists before creating the job
- **PID alive check**: `boo status` uses `kill(pid, 0)` on Unix to verify the daemon is actually running, not just that a stale PID file exists

## Testing Strategy

- **proptest** for property-based testing (serialization roundtrips, cron evaluation invariants, store consistency)
- **tempfile** for test isolation (no tests touch real `~/.boo`)
- **assert_cmd** for CLI integration tests (full add→list→disable→enable→remove flow, cron preview, error cases)
- **MockClock** for deterministic scheduler tests (overdue detection, skip non-overdue, skip disabled, shutdown)
- **`echo` as kiro-cli substitute** in tests — config sets `kiro_cli_path = "echo"` so tests don't require kiro-cli

## Dependencies

| Crate | Purpose |
|-------|---------|
| `croner` | Cron expression parsing |
| `tokio` | Async runtime, timers, subprocess |
| `clap` | CLI argument parsing (derive) |
| `serde` / `serde_json` | JSON persistence |
| `chrono` / `chrono-tz` | Time handling |
| `uuid` | Job IDs |
| `notify-rust` | Desktop notifications (cross-platform) |
| `fs2` | File locking for concurrent access |
| `dirs` | Cross-platform config directories |
| `libc` | PID alive check (Unix only) |
| `thiserror` | Error type derivation |

Dev dependencies: `proptest`, `tempfile`, `assert_cmd`, `predicates`

## Conventions

- Sync functions for CLI commands that don't need async (only `daemon` and `run` are async)
- `impl Into<String>` for constructor ergonomics
- All public store methods are atomic (single lock scope for read-modify-write)
- Errors in spawned job tasks are caught with `eprintln!`, never crash the daemon
- `&Path` over `&PathBuf` in function signatures
- No `unwrap()` in production code (only in tests and MockClock mutex access)
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
