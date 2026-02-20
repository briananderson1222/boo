# Boo

Cross-platform scheduler daemon that fires [kiro-cli](https://kiro.dev/cli/) prompts on cron schedules. Think of it as a personal cron for AI tasks — schedule prompts that run automatically, survive sleep/wake cycles, and catch up on missed runs.

Inspired by [OpenClaw's heartbeat technique](https://zenvanriel.nl/ai-engineer-blog/openclaw-cron-jobs-proactive-ai-guide/) for proactive AI automation.

## Install

```bash
cargo build --release
cp target/release/boo /usr/local/bin/
```

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
| `boo next "<cron>"` | Preview next N occurrences of a cron expression |
| `boo logs <name\|id>` | Show run history |
| `boo logs <name\|id> --output` | Print the clean response from the most recent run |
| `boo resume [name\|id]` | Resume an interactive kiro-cli session to follow up |
| `boo install` | Register as auto-start service |
| `boo uninstall` | Remove auto-start service |

### Job Options

| Option | Description | Default |
|--------|-------------|---------|
| `--name` | Job name (must be unique) | required |
| `--cron` / `--at` / `--every` | Schedule (exactly one required) | required |
| `--prompt` | Prompt text sent to kiro-cli | required |
| `--dir` | Working directory for kiro-cli | `~/.boo/workspace/<name>` |
| `--agent` | Kiro agent to use | default agent |
| `--model` | Kiro model override | agent default |
| `--timeout` | Max seconds before killing the job | 300 |
| `--retry` | Max retry attempts on failure | 0 |
| `--retry-delay` | Seconds between retries | 60 |
| `--delete-after-run` | Auto-delete job after success | false |
| `--open-artifact` | File/glob to open on notification click (relative to dir) | `.response` file |
| `--notify-start` | Send notification when job starts | false |

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

The daemon runs a timer loop (default every 30 seconds). On each tick:

1. Load all enabled jobs
2. For each job, check if overdue based on schedule type
3. If overdue, spawn kiro-cli with the prompt piped via stdin
4. Record the result and update `last_run`

### Missed Schedule Recovery

Job state is persisted to disk. When the system wakes from sleep, the next heartbeat detects overdue jobs and fires them. Missed occurrences are coalesced into a single run with `missed_count` metadata.

### Retry on Failure

Jobs with `--retry N` will retry up to N times with `--retry-delay` seconds between attempts. Each attempt is logged. Notifications indicate retry progress.

### Notifications

- **Start notification** (opt-in via `--notify-start`): batched if multiple jobs fire simultaneously
- **Completion notification**: includes response preview, click to open artifact
- **Failure notification**: includes exit code and retry status

Notifications are delivered via a child process to work around macOS suppressing notifications from backgrounded daemons.

### Clickable Artifacts

When a notification is clicked, it opens:
1. The `--open-artifact` file (relative to job's working directory), or
2. The `.response` file from that run (default)

Artifact patterns support globs (e.g. `daily-*.html`) — the newest matching file is resolved at notification time. This is useful when agents generate files with dynamic names like timestamps.

### Output Formats

```bash
boo list                  # Default table view
boo list --format json    # JSON (pipe to jq)
boo list --format csv     # CSV
```

### Session Resume

Every job run creates a kiro-cli session. Resume any past session for follow-up:

```bash
boo resume wrap-up-day    # picker scoped to that job
boo resume                # picker for default workspace
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
  "heartbeat_secs": 30
}
```

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
cargo test              # 60 tests (unit, property-based, CLI integration)
cargo clippy            # Zero warnings
cargo build --release   # ~2MB binary
```

See [AGENTS.md](AGENTS.md) for architecture details and contributor guidance.

## License

MIT
