# Contributing to Boo

Thanks for your interest in improving Boo. This is a small Rust project and
contributions of all sizes are welcome.

## Getting started

```bash
git clone https://github.com/briananderson1222/boo
cd boo
cargo build
cargo test
```

Boo targets stable Rust (edition 2021). No nightly features are used.

## Before you open a pull request

Run the same checks CI runs — they must all pass:

```bash
cargo fmt --check        # formatting
cargo clippy --all-targets -- -D warnings   # lints (warnings are errors)
cargo test               # unit, property-based, and CLI integration tests
```

- **Format and lint are enforced.** `cargo fmt` before committing; the CI
  `fmt --check` and `clippy -D warnings` jobs will fail otherwise.
- **Add tests for behavior changes.** Scheduler/store/cron logic is covered by
  unit and property tests (`src/*.rs`), and CLI behavior by integration tests
  (`tests/cli.rs`). Use `BOO_HOME` to point a test at an isolated data dir —
  see the `boo_isolated` helper. Set `kiro_cli_path` to `echo` in a test config
  to exercise the executor end-to-end without kiro-cli installed.
- **Keep the worktree hermetic.** Tests must not touch the developer's real
  `~/.boo`. `BOO_HOME` is the authoritative override (`HOME`/`USERPROFILE` are
  ignored by `dirs::home_dir()` on Windows).

## Project layout

See [AGENTS.md](AGENTS.md) for the architecture, module responsibilities, and
design decisions. In short:

- `src/scheduler.rs` — the heartbeat tick loop and job firing
- `src/executor.rs` — spawning runners (kiro-cli / shell) and capturing output
- `src/store.rs` — the locked, atomic on-disk job store
- `src/cron_eval.rs` — schedule evaluation (cron/at/every, timezones, missed runs)
- `src/main.rs` — CLI definitions and command handlers

## Commit and PR conventions

- Write focused commits with a clear subject line (the existing history uses
  `fix:`, `feat:`, `chore:`, `docs:`, `refactor:`, `test:` prefixes).
- One logical change per pull request where practical.
- Update `CHANGELOG.md` under a new version heading for user-visible changes.
- Describe what you changed and how you verified it in the PR body.

## Reporting bugs and requesting features

Open an issue using the templates. For anything security-sensitive, please
follow [SECURITY.md](SECURITY.md) instead of filing a public issue.

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE).
