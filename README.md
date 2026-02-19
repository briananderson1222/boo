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
# Add a scheduled job
boo add --name "morning-brief" \
  --cron "0 9 * * 1-5" \
  --prompt "Check my calendar for today and summarize my meetings" \
  --dir ~/projects

# Preview when it will fire
boo next "0 9 * * 1-5"

# Test it immediately (output to terminal)
boo run morning-brief

# Start the daemon
boo daemon

# Or install as auto-start service (survives reboots)
boo install
```

## Using Kiro Agents

Jobs can target specific kiro-cli agents with `--agent`:

```bash
boo add --name "calendar-check" \
  --cron "0 8 * * 1-5" \
  --prompt "Check my calendar for today and give me a brief summary of meetings and any prep I should do" \
  --dir ~/projects \
  --agent sales-sa \
  --timeout 180
```

The agent has access to all its configured tools and can delegate to subagents. Set a longer `--timeout` for complex agent tasks that involve multiple tool calls.

## Commands

| Command | Description |
|---------|-------------|
| `boo daemon` | Start the scheduler (foreground) |
| `boo add` | Add a scheduled job |
| `boo remove <name\|id>` | Remove a job |
| `boo list` | List all jobs with next fire times |
| `boo enable <name\|id>` | Enable a job |
| `boo disable <name\|id>` | Disable a job |
| `boo status` | Show daemon status and upcoming fires |
| `boo run <name\|id>` | Fire a job immediately (output to terminal) |
| `boo next "<cron>"` | Preview next N occurrences of a cron expression |
| `boo logs <name\|id>` | Show run history |
| `boo logs <name\|id> --output` | Print the clean response from the most recent run |
| `boo resume [name\|id]` | Resume an interactive kiro-cli session to follow up on a previous run |
| `boo install` | Register as auto-start service |
| `boo uninstall` | Remove auto-start service |

### Adding Jobs

```bash
boo add --name <NAME> --cron <CRON> --prompt <PROMPT> [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--name` | Job name (must be unique) | required |
| `--cron` | Cron expression (5-field) | required |
| `--prompt` | Prompt text sent to kiro-cli | required |
| `--dir` | Working directory for kiro-cli | `~/.boo/workspace/<name>` |
| `--agent` | Kiro agent to use | default agent |
| `--timeout` | Max seconds before killing the job | 300 |
| `--timezone` | IANA timezone for cron evaluation | UTC |

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

### Resuming Sessions

Every job run (both scheduled and manual) creates a kiro-cli session that can be resumed for follow-up:

```bash
boo resume wrap-up-day    # opens session picker scoped to that job
boo resume                # opens picker for default workspace
```

Each job runs in its own directory (`~/.boo/workspace/<job-name>/`), so sessions are isolated per job.

### Manual Runs

`boo run <job>` executes a job immediately and saves the same log files and run records as the daemon. Manual runs are tagged with `manual` type in `boo logs`:

```
Fired At             OK       Duration   Missed   Type
--------------------------------------------------------
2026-02-19 16:34:16  yes      52.35s     1        cron
2026-02-19 23:45:03  yes      151.15s    0        manual
```

### Removing Jobs

`boo remove` prompts to delete run history. Use flags to skip the prompt:

```bash
boo remove my-job                # prompts: "Delete logs too? [y/N]"
boo remove my-job --delete-logs  # deletes job + all logs
boo remove my-job --keep-logs    # deletes job, keeps logs
```

## How It Works

### Heartbeat Pattern

The daemon runs a timer loop (default every 30 seconds). On each tick:

1. Load all enabled jobs from `~/.boo/jobs.json`
2. For each job, check if `next_occurrence(cron, last_run) <= now`
3. If overdue, spawn kiro-cli as a subprocess with the prompt piped via stdin
4. Record the result and update `last_run`

### Missed Schedule Recovery

Job state (including `last_run` time) is persisted to disk. When the system wakes from sleep or the daemon restarts, the next heartbeat tick detects overdue jobs and fires them. Multiple missed occurrences are coalesced into a single run — the `missed_count` is recorded in the run metadata for visibility.

### Security

- Prompts are piped via **stdin**, not passed as CLI arguments (not visible in `ps aux`)
- Output files are stored in `~/.boo/runs/` with standard file permissions

### Notifications

Desktop notifications are sent when jobs complete or fail. On macOS, notifications are delivered via a child process to work around macOS suppressing notifications from backgrounded daemons. The notification includes the job name, duration, and a preview of the response.

## Output Files

Each job run produces two files in `~/.boo/runs/<job-id>/`:

| File | Contents |
|------|----------|
| `<timestamp>.log` | Full kiro-cli output (stdout + stderr, raw) |
| `<timestamp>.response` | Clean response only (stdout, ANSI codes stripped) |

View the clean response with:
```bash
boo logs my-job --output
```

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

| Field | Description | Default |
|-------|-------------|---------|
| `kiro_cli_path` | Path to kiro-cli binary | `kiro-cli` |
| `default_timeout_secs` | Default job timeout | 300 (5 min) |
| `max_log_runs` | Max run records kept per job | 50 |
| `heartbeat_secs` | Seconds between heartbeat ticks | 30 |

## Cross-Platform Support

| Platform | Auto-start Mechanism | Restart on Crash | Sleep/Wake Recovery |
|----------|---------------------|-----------------|-------------------|
| macOS | launchd (`~/Library/LaunchAgents/`) | `KeepAlive` | ✅ Heartbeat detects overdue |
| Linux | systemd user service (crontab fallback) | `Restart=always` | ✅ Heartbeat detects overdue |
| Windows | Startup batch file | Manual | ✅ Heartbeat detects overdue |

## File Layout

```
~/.boo/
├── config.json          # Global settings
├── jobs.json            # Job definitions (atomic writes, file-locked)
├── jobs.lock            # Advisory lock for concurrent CLI + daemon access
├── daemon.pid           # Running daemon PID
├── daemon.lock          # Prevents duplicate daemon instances
└── runs/
    └── <job-id>/
        ├── 20260219_160820_855.log        # Full output
        └── 20260219_160820_855.response   # Clean response
```

## Development

```bash
cargo test              # 40 tests (unit, property-based, CLI integration)
cargo clippy            # Zero warnings
cargo build --release   # 2.1MB binary
```

See [AGENTS.md](AGENTS.md) for architecture details and contributor guidance.

## License

MIT
