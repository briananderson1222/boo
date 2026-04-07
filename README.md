# Boo

Cross-platform scheduler daemon that fires [kiro-cli](https://kiro.dev/cli/) prompts on cron schedules. Think of it as a personal cron for AI tasks — schedule prompts that run automatically, survive sleep/wake cycles, and catch up on missed runs.

Inspired by [OpenClaw's heartbeat technique](https://zenvanriel.nl/ai-engineer-blog/openclaw-cron-jobs-proactive-ai-guide/) for proactive AI automation.

## Install

```bash
cargo build --release
cp target/release/boo /usr/local/bin/
```

> **Note:** After building a new version, re-run `boo install` to update the `.app` bundles used for notifications and URL scheme handling.

## Quick Start

```bash
# Recurring job with cron
boo add --name "morning-brief" \
  --cron "0 9 * * 1-5" \
  --prompt "Check my calendar and summarize today's meetings" \
  --agent sales-sa --timeout 180

# One-shot reminder
boo add --name "remind-prep" \
  --at "2026-02-20T15:00:00Z" \
  --prompt "Review the Quick Suite email before your meeting" \
  --delete-after-run

# Interval job
boo add --name "inbox-check" \
  --every "30m" \
  --prompt "Flag any urgent emails"

# Test it
boo run morning-brief

# Start the daemon
boo daemon

# Or install as auto-start service
boo install
```

## Schedule Types

Three mutually exclusive schedule types:

| Type | Flag | Example | Use Case |
|------|------|---------|----------|
| Cron | `--cron` | `"0 9 * * 1-5"` | Recurring at specific times |
| At | `--at` | `"2026-02-20T15:00:00Z"` or `"tomorrow 9am"` | One-shot, fire once |
| Every | `--every` | `"30m"`, `"6h"`, `"1d"` | Recurring at fixed intervals |

Natural language `--at` values (like "tomorrow 9am") are parsed via kiro-cli with confirmation.

## Using Kiro Agents

Jobs can target specific kiro-cli agents with `--agent` and models with `--model`:

```bash
boo add --name "calendar-check" \
  --cron "0 8 * * 1-5" \
  --prompt "Check my calendar for today" \
  --agent sales-sa \
  --model claude-sonnet-4.5 \
  --timeout 180
```

## Commands

| Command | Description |
|---------|-------------|
| `boo daemon` | Start the scheduler (foreground) |
| `boo add` | Add a scheduled job |
| `boo remove <name\|id>` | Remove a job |
| `boo list` | List all jobs (supports `--format json\|csv\|table`) |
| `boo enable/disable <name\|id>` | Toggle a job |
| `boo status` | Show daemon status and upcoming fires |
| `boo run <name\|id>` | Fire a job immediately (with notifications, use `--no-notify` to suppress) |
| `boo run --follow` | Print only response content for programmatic use |
| `boo run --interactive` | Launch foreground kiro-cli session for a job |
| `boo run --interactive --new-window` | Open a new terminal window for the session |
| `boo next "<cron>"` | Preview next N occurrences of a cron expression |
| `boo logs <name\|id>` | Show run history |
| `boo logs <name\|id> --output` | Print the clean response from the most recent run |
| `boo resume [name\|id]` | Resume an interactive kiro-cli session to follow up |
| `boo stats [name\|id]` | Show run statistics (24h/7d/30d windows, supports `--format json\|csv\|table`) |
| `boo edit <name\|id>` | Edit an existing job's settings |
| `boo wait <name\|id>` | Wait for an active job run to complete |
| `boo install` | Register as auto-start service (re-run after building new version) |
| `boo uninstall` | Remove auto-start service |

### Job Options

| Option | Description | Default |
|--------|-------------|---------|
| `--name` | Job name (must be unique) | required |
| `--cron` / `--at` / `--every` | Schedule (exactly one required) | required |
| `--prompt` | Prompt text sent to kiro-cli | required (unless `--command`) |
| `--command` | Raw shell command (implies `--runner shell`) | — |
| `--runner` | Runner type: `kiro` (default), `shell` | `kiro` |
| `--dir` | Working directory for kiro-cli | `~/.boo/workspace/<name>` |
| `--agent` | Kiro agent to use | default agent |
| `--model` | Kiro model override | agent default |
| `--timeout` | Max seconds before killing the job | 300 |
| `--retry` | Max retry attempts on failure | 0 |
| `--retry-delay` | Seconds between retries | 60 |
| `--delete-after-run` | Auto-delete job after success | false |
| `--open-artifact` | File/glob to open on notification click (relative to dir) | `.response` file |
| `--notify-start` | Send notification when job starts | false |
| `--trust-all-tools` | Pass `--trust-all-tools` to kiro-cli | false |
| `--trust-tools` | Trust only these tools (comma-separated) | — |
| `--runner` | Runner type: `kiro` (default), `shell` | `kiro` |
| `--description` | Human-readable description of what this job does | — |

### Cron Expressions

Standard 5-field cron syntax: `minute hour day-of-month month day-of-week`

```
* * * * *        Every minute
0 9 * * *        Daily at 9:00 AM
0 9 * * 1-5      Weekdays at 9:00 AM
*/30 * * * *     Every 30 minutes
0 8,17 * * *     At 8:00 AM and 5:00 PM
0 0 1 * *        First day of each month at midnight
```

Use `boo next "<cron>"` to preview when a cron expression will fire.

## How It Works

### Heartbeat Pattern

The daemon runs a timer loop (default every 60 seconds). On each tick:

1. Load all enabled jobs
2. For each job, check if overdue based on schedule type
3. If overdue, spawn kiro-cli with the prompt piped via stdin
4. Record the result and update `last_run`

### Missed Schedule Recovery

Job state is persisted to disk. When the system wakes from sleep, the next heartbeat detects overdue jobs and fires them. Missed occurrences are coalesced into a single run with `missed_count` metadata.

### Retry on Failure

Jobs with `--retry N` will retry up to N times with `--retry-delay` seconds between attempts. Each attempt is logged. Notifications indicate retry progress.

### Notifications

- **Start notification** (opt-in via `--notify-start`): sent when job begins
- **Completion notification**: includes response preview, click to open artifact
- **Failure notification**: includes exit code and retry status

Notifications are delivered natively via the `user-notify` crate. On macOS, the daemon sends notifications directly from the main thread for reliable callback handling (click-to-open and inline reply).

### Clickable Artifacts

When a notification is clicked, it opens the `--open-artifact` file (relative to job's working directory) if the glob resolves to an existing file.

Artifact patterns support globs (e.g. `daily-*.html`) — the newest matching file is resolved at notification time. This is useful when agents generate files with dynamic names like timestamps.

### Inline Reply

Notifications include a "Reply" action. Type a follow-up message and it opens an interactive `boo resume <job> "<text>"` session in your terminal (auto-detects iTerm, Ghostty, Alacritty, kitty, WezTerm, or Terminal.app — configurable via `terminal` in config.json).

### Output Formats

```bash
boo list                  # Default table view
boo list --format json    # JSON (pipe to jq)
boo list --format csv     # CSV
```

### Session Resume

Every job run creates a kiro-cli session. Resume any past session for follow-up:

```bash
boo resume good-morning                    # Resume latest session
boo resume good-morning "follow up text"   # Resume with a prompt
boo resume good-morning --previous         # Pick from previous sessions
boo resume                                 # Resume in default workspace
```

### Shell Commands

Not everything needs an AI agent. Use `--command` for raw shell jobs:

```bash
boo add --name "backup" --every "1d" --command "rsync -a ~/docs /backup/"
boo add --name "cleanup" --cron "0 3 * * 0" --command "find /tmp -mtime +7 -delete"
```

### URL Scheme

`boo install` registers the `boo://` URL scheme. Use deep links in HTML artifacts or bookmarks:

```
boo://resume/good-morning?prompt=tell%20me%20more    # Resume with prompt
boo://run/catch-up-emails                             # Trigger a job
boo://open/good-morning                               # Open latest artifact
```

On macOS, a small Swift helper (BooURL.app) is compiled at install time to handle URL events. Requires Xcode Command Line Tools.

### Editing Jobs

Modify any setting on an existing job without remove/re-add:

```bash
boo edit my-job --timeout 600 --retry 5
boo edit my-job --cron "0 8 * * 1-5"     # Change schedule
boo edit my-job --description "Daily standup prep"
```

### Removing Jobs

```bash
boo remove my-job                # prompts: "Delete logs too? [y/N]"
boo remove my-job --delete-logs  # deletes job + all logs
boo remove my-job --keep-logs    # deletes job, keeps logs
```

### Security

- Prompts are piped via **stdin**, not passed as CLI arguments (not visible in `ps aux`)
- `BOO_NON_INTERACTIVE=1` env var is set on all spawned kiro-cli processes so agents can detect daemon context
- `BOO_JOB_NAME` env var is set to the job name so agents know which job they're running as
- Output files are stored in `~/.boo/runs/` with standard file permissions

## Output Files

Each run produces two files in `~/.boo/runs/<job-id>/`:

| File | Contents |
|------|----------|
| `<timestamp>.log` | Full kiro-cli output (stdout + stderr) |
| `<timestamp>.response` | Clean response only (ANSI stripped) |

## Configuration

Optional config at `~/.boo/config.json`:

```json
{
  "kiro_cli_path": "/usr/local/bin/kiro-cli",
  "default_timeout_secs": 300,
  "max_log_runs": 50,
  "heartbeat_secs": 60,
  "terminal": "iTerm",
  "notify_webhook": "https://hooks.example.com/boo"
}
```

| Key | Description | Default |
|-----|-------------|---------|
| `kiro_cli_path` | Path to kiro-cli binary | `kiro-cli` (from PATH) |
| `default_timeout_secs` | Default job timeout | 300 |
| `max_log_runs` | Max log files kept per job | 50 |
| `heartbeat_secs` | Daemon tick interval | 60 |
| `terminal` | Preferred terminal for resume/new-window | auto-detect |
| `notify_webhook` | URL for fire-and-forget HTTP POST on job lifecycle events | — |

## Cross-Platform Support

| Platform | Auto-start | Restart on Crash |
|----------|-----------|-----------------|
| macOS | launchd (`~/Library/LaunchAgents/`) | `KeepAlive` |
| Linux | systemd user service (crontab fallback) | `Restart=always` |
| Windows | Startup batch file | Manual |

## File Layout

```
~/.boo/
├── config.json          # Global settings
├── jobs.json            # Job definitions (atomic writes, file-locked)
├── jobs.lock            # Advisory lock for concurrent CLI + daemon access
├── daemon.pid           # Running daemon PID
├── daemon.lock          # Prevents duplicate daemon instances
├── workspace/
│   └── <job-name>/      # Per-job working directory (kiro-cli sessions isolated here)
└── runs/
    ├── <job-id>.jsonl   # Run metadata (timestamp, duration, success, missed count)
    └── <job-id>/
        ├── 20260219_160820_855.log        # Full output
        └── 20260219_160820_855.response   # Clean response
```

## Development

```bash
cargo test              # 75 tests (unit, property-based, CLI integration)
cargo clippy            # Zero warnings
cargo build --release   # ~3.5MB binary
```

See [AGENTS.md](AGENTS.md) for architecture details and contributor guidance.

## License

MIT
