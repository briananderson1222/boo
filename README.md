# Boo

**A personal cron for AI agents.** Schedule prompts for [kiro-cli](https://kiro.dev/cli/), [Claude Code](https://docs.claude.com/en/docs/claude-code), or [Codex](https://developers.openai.com/codex/cli) — plus plain shell commands — and Boo runs them on time, survives sleep/wake and reboots, and catches up on anything it missed.

Inspired by [OpenClaw's heartbeat technique](https://zenvanriel.nl/ai-engineer-blog/openclaw-cron-jobs-proactive-ai-guide/) for proactive AI automation.

## Why Boo?

- **Set-and-forget AI tasks.** "Every weekday at 9am, summarize my calendar." "Every 30 minutes, flag urgent emails." Boo fires the prompt and captures the response.
- **Never misses.** Job state is on disk. Close your laptop, reboot, come back tomorrow — the next tick detects overdue jobs and runs them, recording how many occurrences were skipped.
- **Not locked to one tool.** The same job model drives kiro-cli, Claude Code, Codex, or a raw shell command — pick per job with `--runner`.
- **Native desktop notifications.** Click to open the result; reply inline to start a follow-up session.
- **One small binary.** Cross-platform Rust (~3.5 MB), no runtime dependencies, CLI-first.

## Install

Download a prebuilt binary for macOS, Linux, or Windows from the [latest release](https://github.com/briananderson1222/boo/releases/latest), or build from source:

```bash
cargo build --release
cp target/release/boo /usr/local/bin/
boo install          # optional: run as an auto-start service
```

> After building a new version, re-run `boo install` to refresh the notification / URL-handler app bundles.

## Quick start

```bash
# Recurring (cron): weekday mornings
boo add --name morning-brief \
  --cron "0 9 * * 1-5" \
  --prompt "Check my calendar and summarize today's meetings"

# One-shot reminder that deletes itself after running
boo add --name remind-prep \
  --at "tomorrow 3pm" \
  --prompt "Review the Quick Suite email before the meeting" \
  --delete-after-run

# Interval
boo add --name inbox-check --every 30m --prompt "Flag any urgent emails"

boo run morning-brief    # test it now
boo daemon               # run the scheduler in the foreground (or `boo install`)
```

## Scheduling

Every job has exactly one schedule type:

| Type | Flag | Example | Use for |
|------|------|---------|---------|
| Cron | `--cron` | `"0 9 * * 1-5"` | Recurring at specific times |
| At | `--at` | `"2026-02-20T15:00:00Z"` or `"tomorrow 9am"` | Fire once |
| Every | `--every` | `"30m"`, `"6h"`, `"1d"` | Recurring at a fixed interval |

Cron uses standard 5-field syntax and evaluates in UTC unless you pass `--timezone` (e.g. `America/New_York`, DST-aware). Natural-language `--at` values are parsed via kiro-cli with confirmation. Preview any expression with `boo next "<cron>"`.

<details>
<summary>Cron examples</summary>

```
* * * * *        Every minute
0 9 * * *        Daily at 9:00 AM
0 9 * * 1-5      Weekdays at 9:00 AM
*/30 * * * *     Every 30 minutes
0 8,17 * * *     At 8:00 AM and 5:00 PM
0 0 1 * *        First of each month at midnight
```
</details>

## Runners: kiro, Claude Code, Codex, or shell

`--runner` picks which tool executes the job. The prompt is piped to the tool's stdin; Boo maps its generic options (`--model`, `--trust-all-tools`, `--trust-tools`) onto each CLI's flags.

| Runner | CLI | Notes |
|--------|-----|-------|
| `kiro` *(default)* | `kiro-cli chat` | Full support incl. `--agent` and interactive resume |
| `claude` | `claude -p` (Claude Code headless) | `--trust-all-tools` → `--dangerously-skip-permissions`; `--trust-tools` → `--allowedTools`; `--agent` ignored |
| `codex` | `codex exec` | `--trust-all-tools` bypasses the sandbox, else `--sandbox workspace-write`; `--agent`/`--trust-tools` ignored |
| `pi` | `pi -p` ([pi](https://github.com/earendil-works/pi)) | `--trust-tools` → `--tools` (allowlist); `--agent` ignored |
| `opencode` | `opencode run` ([opencode](https://opencode.ai)) | `--model` uses `provider/model`; `--agent` → `--agent`; `--trust-all-tools` → `--auto`; prompt passed as an argument |
| `acp` | any [ACP](https://agentclientprotocol.com) agent | Generic — drives any Agent Client Protocol agent (opencode, kiro, …) over JSON-RPC. Set `acp_command` in config |
| `shell` | `sh -c` / `cmd /C` | Runs `--command` as a raw shell command — no AI needed |

```bash
boo add --name claude-brief --runner claude --every 1d \
  --prompt "Summarize yesterday's commits"

boo add --name codex-audit --runner codex --cron "0 7 * * 1" \
  --prompt "Audit dependencies for CVEs" --model gpt-5-codex

boo add --name backup --runner shell --every 1d \
  --command "rsync -a ~/docs /backup/"
```

The `acp` runner is generic: instead of a per-CLI adapter it speaks the [Agent Client Protocol](https://agentclientprotocol.com) (JSON-RPC over stdio) to **any** ACP agent — point `acp_command` at one and it works:

```bash
# ~/.boo/config.json → { "acp_command": "opencode acp" }   (or "kiro-cli acp")
boo add --name acp-brief --runner acp --every 1d --prompt "Summarize open PRs"
```

By default the ACP runner denies tool-permission requests (safe for unattended runs); `--trust-all-tools` approves them and `--trust-tools` approves matching tool names.

Binary paths are configurable (`kiro_cli_path` / `claude_cli_path` / `codex_cli_path` / `pi_cli_path` / `opencode_cli_path`); they default to the CLI name on `PATH`. Foreground interactive runs (`boo run --interactive`), session resume (`boo resume`), and natural-language `--at` parsing work for every runner — mapped to each CLI's own resume flags (kiro `--resume`, claude `--continue`, codex `resume --last`). Only `--new-window` and `boo://resume` deep links remain kiro-specific for now.

## Features you'll use

- **Missed-run catch-up.** After sleep or downtime, an overdue job fires once and reports `missed_count` (skipped occurrences); on-time runs report 0.
- **Retry.** `--retry N --retry-delay S` retries failed runs; every attempt is logged and notifications show progress.
- **Notifications.** Completion notifications preview the response and open the artifact on click; a "Reply" action opens a follow-up session in your terminal. Opt into start notifications with `--notify-start`.
- **Clickable artifacts.** `--open-artifact "daily-*.html"` opens the newest matching file when the notification is clicked — handy for agents that write timestamped output.
- **Session resume.** Every kiro run leaves a session you can pick up:
  ```bash
  boo resume morning-brief                  # latest session
  boo resume morning-brief "follow up"      # with a prompt
  boo resume morning-brief --previous       # choose from history
  ```
- **Deep links.** With the URL handler installed, `boo://run/<job>` and `boo://resume/<job>?prompt=…` trigger jobs from bookmarks or HTML. Because any page can open a link, these are **opt-in per job** via `--allow-url-trigger true`; `boo://open/<job>` (opens an artifact only) is not gated.

## Command reference

<details>
<summary>All commands</summary>

| Command | Description |
|---------|-------------|
| `boo daemon` | Start the scheduler (foreground) |
| `boo add` | Add a scheduled job |
| `boo edit <name\|id>` | Change an existing job's settings |
| `boo remove <name\|id>` | Remove a job (`--delete-logs` / `--keep-logs`) |
| `boo list` | List all jobs (`--format table\|json\|csv`) |
| `boo enable/disable <name\|id>` | Toggle a job |
| `boo status` | Daemon status and upcoming fires |
| `boo run <name\|id>` | Fire a job now (`--no-notify`, `--follow`, `--interactive`, `--new-window`) |
| `boo next "<cron>"` | Preview upcoming occurrences of a cron expression |
| `boo logs <name\|id>` | Run history (`--output` prints the latest clean response) |
| `boo resume [name\|id]` | Resume an interactive kiro-cli session |
| `boo stats [name\|id]` | Run statistics (24h/7d/30d, `--format`) |
| `boo running` | Currently active runs (PID, elapsed, source) |
| `boo wait <name\|id>` | Block until an active run finishes |
| `boo kill <name\|id>` | Terminate an active run (kills the process group) |
| `boo clean` | Remove completed one-shot jobs (`--dry-run`, `--keep-logs`) |
| `boo install` / `boo uninstall` | Manage the auto-start service |
</details>

<details>
<summary>Job options (<code>boo add</code> / <code>boo edit</code>)</summary>

| Option | Description | Default |
|--------|-------------|---------|
| `--name` | Job name (unique) | required |
| `--cron` / `--at` / `--every` | Schedule (exactly one) | required |
| `--prompt` | Prompt piped to the runner via stdin | required (unless `--command`) |
| `--command` | Raw shell command (implies `--runner shell`) | — |
| `--runner` | `kiro` (default), `claude`, `codex`, `pi`, `opencode`, `acp`, `shell` | `kiro` |
| `--dir` | Working directory for the job | `~/.boo/workspace/<name>` |
| `--agent` | Agent to use (kiro runner only) | default agent |
| `--model` | Model override, passed to the runner's CLI | runner default |
| `--timezone` | IANA timezone for cron/at (e.g. `America/New_York`) | UTC |
| `--timeout` | Seconds before the job is killed | 300 |
| `--retry` / `--retry-delay` | Retry attempts / seconds between them | 0 / 60 |
| `--delete-after-run` | Auto-delete after a successful run | false |
| `--open-artifact` | File/glob to open on notification click | `.response` |
| `--notify-start` | Notify when the job begins | false |
| `--trust-all-tools` | Grant all tools to the agent | false |
| `--trust-tools` | Trust only these tools (comma-separated) | — |
| `--allow-overlap` | Start a run while a prior one is still going | false |
| `--allow-url-trigger` | Let `boo://run` / `boo://resume` fire this job | false |
| `--description` | Human-readable description | — |
</details>

## Configuration

Optional `~/.boo/config.json`:

```json
{
  "kiro_cli_path": "kiro-cli",
  "claude_cli_path": "claude",
  "codex_cli_path": "codex",
  "pi_cli_path": "pi",
  "opencode_cli_path": "opencode",
  "default_timeout_secs": 300,
  "max_log_runs": 50,
  "heartbeat_secs": 60,
  "terminal": "iTerm",
  "notify_webhook": "https://hooks.example.com/boo"
}
```

| Key | Description | Default |
|-----|-------------|---------|
| `kiro_cli_path` / `claude_cli_path` / `codex_cli_path` / `pi_cli_path` / `opencode_cli_path` | Binary for each runner | CLI name on `PATH` |
| `acp_command` | Launch command for the `acp` runner (e.g. `"opencode acp"`, `"kiro-cli acp"`) | — |
| `default_timeout_secs` | Default job timeout | 300 |
| `max_log_runs` | Log files kept per job | 50 |
| `heartbeat_secs` | Daemon tick interval | 60 |
| `terminal` | Preferred terminal for resume / new-window | auto-detect |
| `notify_webhook` | URL POSTed a JSON body on `job.started` / `job.completed` / `job.failed` (`http`/`https`, TLS) | — |

---

## How it works

The daemon runs a heartbeat loop (default 60s). Each tick loads enabled jobs, checks each against its schedule, and for any that are overdue spawns the runner with the prompt on stdin, records the result, and updates `last_run`. Because all state is persisted, a missed window (sleep, reboot) is simply detected and caught up on the next tick.

## Platform support

| Platform | Auto-start | Restart on crash |
|----------|-----------|------------------|
| macOS | launchd (`~/Library/LaunchAgents/`) | `KeepAlive` |
| Linux | systemd user service (crontab fallback) | `Restart=always` |
| Windows | Task Scheduler (logon task) | manual |

On macOS the daemon sends notifications from the main thread (CFRunLoop) so click-to-open and inline reply work reliably. `boo://` deep links are handled by a small Swift helper compiled at install time (requires Xcode Command Line Tools).

## Output & storage

Each run writes to `~/.boo/runs/<job-id>/`: a `<timestamp>.log` (full stdout+stderr) and a `<timestamp>.response` (clean, ANSI-stripped stdout).

```
~/.boo/
├── config.json          # global settings
├── jobs.json            # job definitions (atomic, file-locked)
├── daemon.pid / .lock   # daemon state / single-instance guard
├── workspace/<name>/    # per-job working directory
└── runs/
    ├── <job-id>.jsonl   # run metadata (time, duration, success, missed count)
    └── <job-id>/…       # per-run .log and .response files
```

## Security

- Prompts are piped via **stdin**, never passed as CLI arguments (not visible in `ps aux`).
- On Unix, `~/.boo` is created `0700` and `jobs.json` / `config.json` / run logs are `0600` — they can hold prompts, webhook URLs, and agent output.
- `boo://run` / `boo://resume` are opt-in per job (`--allow-url-trigger`); webhook deliveries use TLS for `https://` URLs and log failures instead of dropping them.
- Spawned runners get `BOO_NON_INTERACTIVE=1` and `BOO_JOB_NAME` so agents can detect the daemon context and which job they are.

To report a vulnerability, see [SECURITY.md](SECURITY.md).

## Development

```bash
cargo test     # unit, property-based, and CLI integration tests
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Architecture and design decisions live in [AGENTS.md](AGENTS.md); contribution guidelines in [CONTRIBUTING.md](CONTRIBUTING.md).

## License

[MIT](LICENSE)
