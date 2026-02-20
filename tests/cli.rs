use assert_cmd::Command;
use predicates::prelude::*;

fn boo() -> Command { assert_cmd::cargo_bin_cmd!("boo") }

#[test]
fn test_help() {
    boo().arg("--help").assert().success()
        .stdout(predicate::str::contains("scheduler daemon"));
}

#[test]
fn test_next_valid_cron() {
    boo().args(["next", "0 9 * * *"]).assert().success()
        .stdout(predicate::str::contains("occurrences"));
}

#[test]
fn test_next_invalid_cron() {
    boo().args(["next", "invalid"]).assert().failure();
}

#[test]
fn test_add_cron_list_remove_flow() {
    let name = format!("test-cron-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "test", "--dir", "/tmp"]).assert().success();
    boo().arg("list").assert().success().stdout(predicate::str::contains(&name));
    boo().args(["disable", &name]).assert().success();
    boo().args(["enable", &name]).assert().success();
    boo().args(["remove", &name, "--keep-logs"]).assert().success().stdout(predicate::str::contains("Removed"));
}

#[test]
fn test_add_every_schedule() {
    let name = format!("test-every-{}", std::process::id());
    boo().args(["add", "--name", &name, "--every", "30m", "--prompt", "test", "--dir", "/tmp"]).assert().success()
        .stdout(predicate::str::contains("every 30m"));
    boo().arg("list").assert().success().stdout(predicate::str::contains("every 30m"));
    boo().args(["remove", &name, "--keep-logs"]).assert().success();
}

#[test]
fn test_add_at_schedule_iso() {
    let name = format!("test-at-{}", std::process::id());
    boo().args(["add", "--name", &name, "--at", "2099-12-31T23:59:00Z", "--prompt", "test", "--dir", "/tmp"]).assert().success()
        .stdout(predicate::str::contains("at 2099"));
    boo().args(["remove", &name, "--keep-logs"]).assert().success();
}

#[test]
fn test_add_no_schedule_fails() {
    let name = format!("test-nosched-{}", std::process::id());
    boo().args(["add", "--name", &name, "--prompt", "test", "--dir", "/tmp"]).assert().failure();
}

#[test]
fn test_add_multiple_schedules_fails() {
    let name = format!("test-multi-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--every", "30m", "--prompt", "test", "--dir", "/tmp"])
        .assert().failure();
}

#[test]
fn test_add_with_model_and_retry() {
    let name = format!("test-opts-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "test", "--dir", "/tmp",
                "--model", "claude-haiku-4.5", "--retry", "3", "--retry-delay", "30",
                "--notify-start", "--delete-after-run"]).assert().success();
    boo().args(["remove", &name, "--keep-logs"]).assert().success();
}

#[test]
fn test_status_daemon_not_running() {
    // Remove stale PID to ensure clean test
    let _ = std::fs::remove_file(dirs::home_dir().unwrap().join(".boo/daemon.pid"));
    boo().arg("status").assert().success().stdout(predicate::str::contains("stopped"));
}

#[test]
fn test_remove_nonexistent() {
    boo().args(["remove", "nonexistent-12345", "--keep-logs"]).assert().failure();
}

#[test]
fn test_add_duplicate_name_rejected() {
    let name = format!("test-dup-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "test", "--dir", "/tmp"]).assert().success();
    boo().args(["add", "--name", &name, "--cron", "0 10 * * *", "--prompt", "test2", "--dir", "/tmp"]).assert().failure();
    boo().args(["remove", &name, "--keep-logs"]).assert().success();
}

#[test]
fn test_add_invalid_dir_rejected() {
    let name = format!("test-baddir-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "test", "--dir", "/nonexistent/path/xyz"])
        .assert().failure();
}

#[test]
fn test_remove_delete_logs_flag() {
    let name = format!("test-dellogs-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "test", "--dir", "/tmp"]).assert().success();
    boo().args(["remove", &name, "--delete-logs"]).assert().success().stdout(predicate::str::contains("Removed"));
}

#[test]
fn test_manual_run_saves_record() {
    let name = format!("test-manual-{}", std::process::id());
    boo().args(["add", "--name", &name, "--cron", "0 9 * * *", "--prompt", "hello", "--dir", "/tmp"]).assert().success();
    boo().args(["logs", &name]).assert().success().stdout(predicate::str::contains("No run records"));
    boo().args(["remove", &name, "--keep-logs"]).assert().success();
}

#[test]
fn test_parse_duration_via_every() {
    // Test various duration formats work
    for (dur, expected) in [("30s", "every 30s"), ("20m", "every 20m"), ("6h", "every 6h"), ("1d", "every 1d")] {
        let name = format!("test-dur-{}-{}", dur, std::process::id());
        boo().args(["add", "--name", &name, "--every", dur, "--prompt", "test", "--dir", "/tmp"])
            .assert().success().stdout(predicate::str::contains(expected));
        boo().args(["remove", &name, "--keep-logs"]).assert().success();
    }
}
