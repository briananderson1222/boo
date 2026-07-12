use crate::clock::Clock;
use crate::config::{runs_dir, Config};
use crate::cron_eval;
use crate::executor;
use crate::job::{Job, RunRecord};
use crate::notification_service::{NotificationSender, NotifyRequest};
use crate::notifier;
use crate::store::JobStore;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct Scheduler<C: Clock> {
    clock: C,
    config: Config,
    store_dir: Option<PathBuf>,
    running_jobs: Arc<Mutex<HashSet<Uuid>>>,
    shutdown: Arc<tokio::sync::Notify>,
    notification_sender: Option<NotificationSender>,
}

impl<C: Clock + 'static> Scheduler<C> {
    pub fn new(clock: C, config: Config, store_dir: Option<PathBuf>) -> Self {
        Self {
            clock,
            config,
            store_dir,
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
            shutdown: Arc::new(tokio::sync::Notify::new()),
            notification_sender: None,
        }
    }

    pub fn with_notification_sender(mut self, sender: NotificationSender) -> Self {
        self.notification_sender = Some(sender);
        self
    }

    pub async fn run(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.heartbeat_secs));
        loop {
            tokio::select! {
                _ = interval.tick() => { self.tick().await; }
                _ = self.shutdown.notified() => {
                    let _ = tokio::time::timeout(Duration::from_secs(30), async {
                        loop {
                            if self.running_jobs.lock().await.is_empty() { break; }
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }).await;
                    return;
                }
            }
        }
    }

    async fn tick(&self) {
        let store = match self.create_store() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to create store: {e}");
                return;
            }
        };
        let jobs = match store.load_jobs() {
            Ok(j) => j,
            Err(e) => {
                eprintln!("Failed to load jobs: {e}");
                return;
            }
        };

        let now = self.clock.now();
        let mut to_fire = Vec::new();

        for job in jobs {
            if !job.enabled || !cron_eval::is_overdue(&job, now) {
                continue;
            }
            let running = self.running_jobs.lock().await;
            if running.contains(&job.id) && !job.allow_overlap {
                continue;
            }
            drop(running);
            to_fire.push(job);
        }

        // Send start notifications for jobs with notify_start
        let start_names: Vec<&str> = to_fire
            .iter()
            .filter(|j| j.notify_start)
            .map(|j| j.name.as_str())
            .collect();
        if !start_names.is_empty() {
            if let Some(ref sender) = self.notification_sender {
                for name in &start_names {
                    sender.send(NotifyRequest {
                        summary: format!("🚀 Job '{}' starting...", name),
                        body: format!("Run 'boo disable {}' to pause", name),
                        open: None,
                        working_dir: None,
                        job_name: Some(name.to_string()),
                    });
                }
            } else {
                notifier::notify_start(&start_names);
            }
        }

        // Webhook: notify started for ALL fired jobs (not just notify_start ones)
        if let Some(ref url) = self.config.notify_webhook {
            for job in &to_fire {
                notifier::spawn_webhook_event(url, job, notifier::WebhookEvent::Started);
            }
        }

        for job in to_fire {
            self.spawn_job(job);
        }
    }

    fn spawn_job(&self, job: Job) {
        let config = self.config.clone();
        let store_dir = self.store_dir.clone();
        let running_jobs = self.running_jobs.clone();
        let clock = self.clock.clone();
        let sender = self.notification_sender.clone();
        let webhook_url = self.config.notify_webhook.clone();

        tokio::spawn(async move {
            {
                running_jobs.lock().await.insert(job.id);
            }

            let result = Self::execute_with_retry(
                job.clone(),
                config,
                store_dir.clone(),
                clock,
                sender.clone(),
                webhook_url.clone(),
            )
            .await;
            if let Err(e) = &result {
                let retries = job.retry_count;
                let msg = if retries > 0 {
                    format!("{} (after {} retries)", e, retries)
                } else {
                    e.to_string()
                };
                eprintln!("Job execution failed for {}: {msg}", job.name);

                // Find latest log file for click-to-open
                let log_dir = store_dir
                    .as_ref()
                    .map(|d| d.join("runs"))
                    .unwrap_or_else(crate::config::runs_dir)
                    .join(job.id.to_string());
                let latest_log = std::fs::read_dir(&log_dir)
                    .ok()
                    .and_then(|entries| {
                        entries
                            .filter_map(|e| e.ok())
                            .filter(|e| e.path().extension().is_some_and(|ext| ext == "log"))
                            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
                    })
                    .map(|e| e.path().to_string_lossy().to_string());

                if let Some(ref s) = sender {
                    s.send(NotifyRequest {
                        summary: format!("✗ Job '{}' failed", job.name),
                        body: msg.clone(),
                        open: latest_log,
                        working_dir: Some(job.working_dir.to_string_lossy().to_string()),
                        job_name: Some(job.name.clone()),
                    });
                } else {
                    notifier::notify_error(&job, &msg);
                }

                if let Some(ref url) = webhook_url {
                    notifier::spawn_webhook_event(url, &job, notifier::WebhookEvent::Errored(&msg));
                }
            }

            // Clean up active run tracking
            if let Ok(store) = Self::make_store(&store_dir) {
                store.remove_active_run(job.id);
            }

            {
                running_jobs.lock().await.remove(&job.id);
            }
        });
    }

    async fn execute_with_retry(
        job: Job,
        config: Config,
        store_dir: Option<PathBuf>,
        clock: C,
        sender: Option<NotificationSender>,
        webhook_url: Option<String>,
    ) -> crate::error::Result<()> {
        let max_attempts = job.retry_count + 1;
        let mut last_err = None;

        for attempt in 1..=max_attempts {
            match Self::execute_job_impl(&job, &config, &store_dir, &clock, &sender, &webhook_url)
                .await
            {
                Ok(success) => {
                    if success {
                        // Delete one-shot jobs after success
                        if job.delete_after_run {
                            let store = Self::make_store(&store_dir)?;
                            let _ = store.remove_job(job.id);
                        }
                        return Ok(());
                    }
                    // Job ran but failed (non-zero exit)
                    if attempt < max_attempts {
                        eprintln!(
                            "Job '{}' failed (attempt {attempt}/{max_attempts}), retrying in {}s",
                            job.name, job.retry_delay_secs
                        );
                        tokio::time::sleep(Duration::from_secs(job.retry_delay_secs)).await;
                    }
                    last_err = Some(crate::error::BooError::JobFailed(1));
                }
                Err(e) => {
                    if attempt < max_attempts {
                        eprintln!("Job '{}' error (attempt {attempt}/{max_attempts}): {e}, retrying in {}s",
                            job.name, job.retry_delay_secs);
                        tokio::time::sleep(Duration::from_secs(job.retry_delay_secs)).await;
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or(crate::error::BooError::Other("unknown error".into())))
    }

    /// Execute a single attempt. Returns Ok(true) on success, Ok(false) on job failure.
    async fn execute_job_impl(
        job: &Job,
        config: &Config,
        store_dir: &Option<PathBuf>,
        clock: &C,
        sender: &Option<NotificationSender>,
        webhook_url: &Option<String>,
    ) -> crate::error::Result<bool> {
        let store = Self::make_store(store_dir)?;
        let now = clock.now();
        let from_time = job.last_run.unwrap_or(job.created_at);

        // Compute scheduled_for based on schedule type
        let scheduled_for = if let Some(at) = job.at_time {
            at
        } else if let Some(every_secs) = job.every_secs {
            from_time + chrono::Duration::seconds(every_secs as i64)
        } else {
            cron_eval::next_occurrence(&job.cron_expr, from_time)?
        };

        let missed = if job.at_time.is_some() {
            0
        } else if let Some(every_secs) = job.every_secs {
            cron_eval::missed_count_every(every_secs, from_time, now)
        } else {
            cron_eval::missed_count(&job.cron_expr, from_time, now)
        };

        // Log directory
        let base_runs = store_dir
            .as_ref()
            .map(|d| d.join("runs"))
            .unwrap_or_else(runs_dir);
        let log_dir = base_runs.join(job.id.to_string());
        std::fs::create_dir_all(&log_dir)?;

        let ts = now.format("%Y%m%d_%H%M%S");
        let ms = now.timestamp_subsec_millis();
        let log_path = log_dir.join(format!("{ts}_{ms:03}.log"));

        // Track the real child PID on disk for boo status/wait/kill
        let active_store = Self::make_store(store_dir)?;
        let (active_id, active_name) = (job.id, job.name.clone());
        let on_spawn = move |pid: u32| {
            let _ = active_store.write_active_run(&crate::store::ActiveRun {
                job_id: active_id,
                job_name: active_name.clone(),
                pid,
                started_at: chrono::Utc::now(),
                manual: false,
            });
        };

        let started = std::time::Instant::now();
        let exec = executor::execute_job(job, config, &log_path, Some(&on_spawn)).await;

        let result = match exec {
            Ok(result) => result,
            Err(e) => {
                // Timeouts and spawn failures must still leave a run record
                // and advance last_run — otherwise the job stays overdue and
                // refires every heartbeat forever, invisibly.
                let record = RunRecord {
                    job_id: job.id,
                    job_name: job.name.clone(),
                    fired_at: now,
                    scheduled_for,
                    missed_count: missed,
                    duration_secs: started.elapsed().as_secs_f64(),
                    exit_code: None,
                    success: false,
                    output_path: log_path.clone(),
                    manual: false,
                };
                let _ = store.append_run_record(&record);
                let _ = store.rotate_logs(job.id, config.max_log_runs);
                let _ = store.set_last_run(job.id, now);
                return Err(e);
            }
        };

        let record = RunRecord {
            job_id: job.id,
            job_name: job.name.clone(),
            fired_at: now,
            scheduled_for,
            missed_count: missed,
            duration_secs: result.duration_secs,
            exit_code: result.exit_code,
            success: result.success,
            output_path: result.output_path.clone(),
            manual: false,
        };
        store.append_run_record(&record)?;
        store.rotate_logs(job.id, config.max_log_runs)?;

        store.set_last_run(job.id, now)?;

        notifier::send_notification(job, &result, sender);

        if let Some(ref url) = webhook_url {
            notifier::spawn_webhook_event(url, job, notifier::WebhookEvent::Finished(&result));
        }

        Ok(result.success)
    }

    fn make_store(store_dir: &Option<PathBuf>) -> crate::error::Result<JobStore> {
        if let Some(dir) = store_dir {
            JobStore::with_dir(dir.clone())
        } else {
            JobStore::new()
        }
    }

    pub fn trigger_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    fn create_store(&self) -> crate::error::Result<JobStore> {
        Self::make_store(&self.store_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::MockClock;
    use crate::job::Job;
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_config() -> Config {
        crate::test_helpers::test_config()
    }

    #[test]
    fn test_scheduler_construction() {
        let tmp = TempDir::new().unwrap();
        let scheduler = Scheduler::new(
            MockClock::new(Utc::now()),
            test_config(),
            Some(tmp.path().to_path_buf()),
        );
        assert!(scheduler.running_jobs.try_lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_tick_fires_overdue_cron_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("fire-test", "* * * * *", "hello", std::env::temp_dir());
        job.last_run = Some(now - chrono::Duration::minutes(2));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let records = JobStore::with_dir(dir)
            .unwrap()
            .load_run_records(job.id, 10)
            .unwrap();
        assert!(
            !records.is_empty(),
            "Expected run record for overdue cron job"
        );
    }

    #[tokio::test]
    async fn test_tick_fires_overdue_every_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("every-test", "", "hello", std::env::temp_dir());
        job.every_secs = Some(60);
        job.last_run = Some(now - chrono::Duration::minutes(2));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let records = JobStore::with_dir(dir)
            .unwrap()
            .load_run_records(job.id, 10)
            .unwrap();
        assert!(
            !records.is_empty(),
            "Expected run record for overdue every job"
        );
    }

    #[tokio::test]
    async fn test_tick_fires_overdue_at_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("at-test", "", "hello", std::env::temp_dir());
        job.at_time = Some(now - chrono::Duration::minutes(1));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let records = JobStore::with_dir(dir)
            .unwrap()
            .load_run_records(job.id, 10)
            .unwrap();
        assert!(
            !records.is_empty(),
            "Expected run record for overdue at job"
        );
    }

    #[tokio::test]
    async fn test_tick_skips_not_overdue_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("skip-test", "0 * * * *", "hello", std::env::temp_dir());
        job.last_run = Some(now - chrono::Duration::seconds(30));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let records = JobStore::with_dir(dir)
            .unwrap()
            .load_run_records(job.id, 10)
            .unwrap();
        assert!(records.is_empty(), "Should not fire non-overdue job");
    }

    #[tokio::test]
    async fn test_tick_skips_disabled_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("disabled-test", "* * * * *", "hello", std::env::temp_dir());
        job.last_run = Some(now - chrono::Duration::minutes(2));
        job.enabled = false;
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let records = JobStore::with_dir(dir)
            .unwrap()
            .load_run_records(job.id, 10)
            .unwrap();
        assert!(records.is_empty(), "Should not fire disabled job");
    }

    #[tokio::test]
    async fn test_delete_after_run() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("delete-test", "", "hello", std::env::temp_dir());
        job.at_time = Some(now - chrono::Duration::minutes(1));
        job.delete_after_run = true;
        let job_id = job.id;
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job)
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let store = JobStore::with_dir(dir).unwrap();
        assert!(
            store.get_job(job_id).is_err(),
            "Job should be deleted after run"
        );
    }

    /// A3: a timed-out job must still leave a run record and advance
    /// last_run, otherwise it refires every heartbeat forever.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_timeout_records_run_and_advances_last_run() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("timeout-test", "* * * * *", "", std::env::temp_dir());
        job.command = Some("sleep 30".into());
        job.timeout_secs = Some(1);
        job.last_run = Some(now - chrono::Duration::minutes(2));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let store = JobStore::with_dir(dir).unwrap();
        let records = store.load_run_records(job.id, 10).unwrap();
        assert!(!records.is_empty(), "timeout must produce a run record");
        assert!(!records[0].success, "timeout record must be a failure");
        let reloaded = store.get_job(job.id).unwrap();
        assert!(
            reloaded.last_run.is_some() && reloaded.last_run.unwrap() >= now,
            "last_run must advance after a timeout so the job doesn't refire forever"
        );
    }

    /// A2: edits made while a run is in flight must survive run completion.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_concurrent_edit_survives_run_completion() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("edit-race-test", "* * * * *", "", std::env::temp_dir());
        job.command = Some("sleep 1".into());
        job.timeout_secs = Some(10);
        job.last_run = Some(now - chrono::Duration::minutes(2));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        // Edit while the job is running
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let store = JobStore::with_dir(dir.clone()).unwrap();
        let mut edited = store.get_job(job.id).unwrap();
        edited.enabled = false;
        edited.description = Some("edited mid-run".into());
        store.update_job(&edited).unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let reloaded = store.get_job(job.id).unwrap();
        assert!(!reloaded.enabled, "disable during run must not be reverted");
        assert_eq!(
            reloaded.description.as_deref(),
            Some("edited mid-run"),
            "edits during run must not be reverted"
        );
        assert!(reloaded.last_run.is_some(), "last_run must still be set");
    }

    /// A1: the active-run record must carry the child's PID, not the
    /// daemon's, so boo kill/wait signal the right process.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_active_run_records_child_pid() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("pid-test", "* * * * *", "", std::env::temp_dir());
        job.command = Some("sleep 2".into());
        job.timeout_secs = Some(10);
        job.last_run = Some(now - chrono::Duration::minutes(2));
        JobStore::with_dir(dir.clone())
            .unwrap()
            .add_job(job.clone())
            .unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;

        let store = JobStore::with_dir(dir).unwrap();
        let mut active = None;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if let Some(run) = store.get_active_run(job.id) {
                active = Some(run);
                break;
            }
        }
        let active = active.expect("active run should be recorded while job runs");
        assert_ne!(
            active.pid,
            std::process::id(),
            "active run must record the child PID, not this process"
        );
        assert!(active.pid != 0, "active run must record a real PID");
    }

    #[tokio::test]
    async fn test_shutdown() {
        let tmp = TempDir::new().unwrap();
        let scheduler = Arc::new(Scheduler::new(
            MockClock::new(Utc::now()),
            test_config(),
            Some(tmp.path().to_path_buf()),
        ));
        let s = scheduler.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            s.trigger_shutdown();
        });
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), scheduler.run()).await;
        assert!(
            result.is_ok(),
            "run() should return promptly after shutdown"
        );
    }
}
