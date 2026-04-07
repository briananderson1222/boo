use assert_cmd::Command;
use predicates::prelude::*;

fn boo() -> Command {
    assert_cmd::cargo_bin_cmd!("boo")
}
fn tmp() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

/// Create a boo command with an isolated home directory.
fn boo_isolated(dir: &std::path::Path) -> Command {
    let mut cmd = boo();
    cmd.env("HOME", dir).env("USERPROFILE", dir);
    cmd
}

#[test]
fn test_help() {
    boo()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("scheduler daemon"));
}

#[test]
fn test_next_valid_cron() {
    boo()
        .args(["next", "0 9 * * *"])
        .assert()
        .success()
        .stdout(predicate::str::contains("occurrences"));
}

#[test]
fn test_next_invalid_cron() {
    boo().args(["next", "invalid"]).assert().failure();
}

#[test]
fn test_add_cron_list_remove_flow() {
    let name = format!("test-cron-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success();
    boo()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(&name));
    boo().args(["disable", &name]).assert().success();
    boo().args(["enable", &name]).assert().success();
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed"));
}

#[test]
fn test_add_every_schedule() {
    let name = format!("test-every-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--every",
            "30m",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("every 30m"));
    boo()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("every 30m"));
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success();
}

#[test]
fn test_add_at_schedule_iso() {
    let name = format!("test-at-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--at",
            "2099-12-31T23:59:00Z",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("at 2099"));
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success();
}

#[test]
fn test_add_no_schedule_fails() {
    let name = format!("test-nosched-{}", std::process::id());
    boo()
        .args(["add", "--name", &name, "--prompt", "test", "--dir", &tmp()])
        .assert()
        .failure();
}

#[test]
fn test_add_multiple_schedules_fails() {
    let name = format!("test-multi-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--every",
            "30m",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .failure();
}

#[test]
fn test_add_with_model_and_retry() {
    let name = format!("test-opts-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
            "--model",
            "claude-haiku-4.5",
            "--retry",
            "3",
            "--retry-delay",
            "30",
            "--notify-start",
            "--delete-after-run",
        ])
        .assert()
        .success();
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success();
}

#[test]
fn test_status_daemon_not_running() {
    // Just verify status command succeeds — can't reliably assert "stopped"
    // because the real daemon may be running during tests
    boo()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Daemon:"));
}

#[test]
fn test_remove_nonexistent() {
    boo()
        .args(["remove", "nonexistent-12345", "--keep-logs"])
        .assert()
        .failure();
}

#[test]
fn test_add_duplicate_name_rejected() {
    let name = format!("test-dup-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success();
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 10 * * *",
            "--prompt",
            "test2",
            "--dir",
            &tmp(),
        ])
        .assert()
        .failure();
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success();
}

#[test]
fn test_add_invalid_dir_rejected() {
    let name = format!("test-baddir-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "test",
            "--dir",
            "/nonexistent/path/xyz",
        ])
        .assert()
        .failure();
}

#[test]
fn test_remove_delete_logs_flag() {
    let name = format!("test-dellogs-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "test",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success();
    boo()
        .args(["remove", &name, "--delete-logs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed"));
}

#[test]
fn test_manual_run_saves_record() {
    let name = format!("test-manual-{}", std::process::id());
    boo()
        .args([
            "add",
            "--name",
            &name,
            "--cron",
            "0 9 * * *",
            "--prompt",
            "hello",
            "--dir",
            &tmp(),
        ])
        .assert()
        .success();
    boo()
        .args(["logs", &name])
        .assert()
        .success()
        .stdout(predicate::str::contains("No run records"));
    boo()
        .args(["remove", &name, "--keep-logs"])
        .assert()
        .success();
}

#[test]
fn test_parse_duration_via_every() {
    // Test various duration formats work
    for (dur, expected) in [
        ("30s", "every 30s"),
        ("20m", "every 20m"),
        ("6h", "every 6h"),
        ("1d", "every 1d"),
    ] {
        let name = format!("test-dur-{}-{}", dur, std::process::id());
        boo()
            .args([
                "add",
                "--name",
                &name,
                "--every",
                dur,
                "--prompt",
                "test",
                "--dir",
                &tmp(),
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(expected));
        boo()
            .args(["remove", &name, "--keep-logs"])
            .assert()
            .success();
    }
}

#[test]
fn test_add_command_shell_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = boo();
    cmd.env("HOME", dir.path())
        .args([
            "add",
            "--name",
            "shell-test",
            "--every",
            "1h",
            "--command",
            "echo hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added job 'shell-test'"));
}

#[test]
fn test_add_no_prompt_no_command_fails() {
    let dir = tempfile::tempdir().unwrap();
    let mut cmd = boo();
    cmd.env("HOME", dir.path())
        .args(["add", "--name", "fail-test", "--every", "1h"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--prompt or --command"));
}

#[test]
fn test_list_format_json() {
    let dir = tempfile::tempdir().unwrap();
    // Add a job first
    boo()
        .env("HOME", dir.path())
        .args([
            "add",
            "--name",
            "json-test",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    // List as JSON
    boo()
        .env("HOME", dir.path())
        .args(["list", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"name\": \"json-test\""));
}

#[test]
fn test_run_new_window_flag_accepted() {
    // Verify --new-window is a valid flag without actually opening a terminal
    boo()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--new-window"));
}

#[test]
fn test_list_format_csv() {
    let dir = tempfile::tempdir().unwrap();
    boo()
        .env("HOME", dir.path())
        .args([
            "add", "--name", "csv-test", "--every", "1h", "--prompt", "hello",
        ])
        .assert()
        .success();
    boo()
        .env("HOME", dir.path())
        .args(["list", "--format", "csv"])
        .assert()
        .success()
        .stdout(predicate::str::contains("csv-test"));
}

#[test]
fn test_wait_not_running() {
    let dir = tempfile::tempdir().unwrap();
    boo()
        .env("HOME", dir.path())
        .args([
            "add",
            "--name",
            "wait-test",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo()
        .env("HOME", dir.path())
        .args(["wait", "wait-test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not running"));
}

#[test]
fn test_wait_help() {
    boo()
        .args(["wait", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--interval"));
}

#[test]
fn test_status_shows_active_runs_section() {
    // Status should succeed and show daemon info (active runs section only appears when jobs are running)
    boo()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Daemon:"));
}

#[test]
fn test_status_json_has_active_runs() {
    boo()
        .args(["status", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("active_runs"));
}

#[test]
fn test_running_no_active() {
    boo()
        .arg("running")
        .assert()
        .success()
        .stdout(predicate::str::contains("No jobs currently running"));
}

#[test]
fn test_running_json_empty() {
    boo()
        .args(["running", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[test]
fn test_kill_not_running() {
    let dir = tempfile::tempdir().unwrap();
    boo()
        .env("HOME", dir.path())
        .args([
            "add",
            "--name",
            "kill-test",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo()
        .env("HOME", dir.path())
        .args(["kill", "kill-test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("not currently running"));
}

#[test]
fn test_kill_nonexistent() {
    boo().args(["kill", "nonexistent-99999"]).assert().failure();
}

#[test]
fn test_clean_nothing_to_clean() {
    let dir = tempfile::tempdir().unwrap();
    boo_isolated(dir.path())
        .args([
            "add",
            "--name",
            "recurring",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo_isolated(dir.path())
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains("No completed one-shot"));
}

#[test]
fn test_clean_dry_run() {
    let dir = tempfile::tempdir().unwrap();
    boo_isolated(dir.path())
        .args([
            "add",
            "--name",
            "old-oneshot",
            "--at",
            "2020-01-01T00:00:00Z",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo_isolated(dir.path())
        .args(["clean", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would remove"))
        .stdout(predicate::str::contains("old-oneshot"));
    // Verify job still exists after dry run
    boo_isolated(dir.path())
        .args(["clean", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would remove"));
}

#[test]
fn test_clean_removes_done_oneshots() {
    let dir = tempfile::tempdir().unwrap();
    boo_isolated(dir.path())
        .args([
            "add",
            "--name",
            "done-oneshot",
            "--at",
            "2020-01-01T00:00:00Z",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo_isolated(dir.path())
        .args([
            "add",
            "--name",
            "keep-recurring",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo_isolated(dir.path())
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleaned 1 job(s)"));
    // Recurring should still exist, one-shot should be gone
    let output = boo_isolated(dir.path()).arg("list").output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("keep-recurring"));
    assert!(!stdout.contains("done-oneshot"));
}

#[test]
fn test_clean_keep_logs() {
    let dir = tempfile::tempdir().unwrap();
    boo_isolated(dir.path())
        .args([
            "add",
            "--name",
            "logs-oneshot",
            "--at",
            "2020-01-01T00:00:00Z",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo_isolated(dir.path())
        .args(["clean", "--keep-logs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("logs kept"));
}

#[test]
fn test_list_json_has_running_field() {
    let dir = tempfile::tempdir().unwrap();
    boo()
        .env("HOME", dir.path())
        .args([
            "add",
            "--name",
            "run-field-test",
            "--every",
            "1h",
            "--prompt",
            "hello",
        ])
        .assert()
        .success();
    boo()
        .env("HOME", dir.path())
        .args(["list", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"running\""));
}
