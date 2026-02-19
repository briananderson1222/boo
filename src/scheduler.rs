use crate::clock::Clock;
use crate::config::{Config, runs_dir};
use crate::cron_eval;
use crate::executor;
use crate::job::{Job, RunRecord};
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
}

impl<C: Clock + 'static> Scheduler<C> {
    pub fn new(clock: C, config: Config, store_dir: Option<PathBuf>) -> Self {
        Self {
            clock,
            config,
            store_dir,
            running_jobs: Arc::new(Mutex::new(HashSet::new())),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        }
    }

    pub async fn run(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.heartbeat_secs));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    self.tick().await;
                }
                _ = self.shutdown.notified() => {
                    // Wait for running jobs to finish with 30s timeout
                    let timeout = tokio::time::timeout(Duration::from_secs(30), async {
                        loop {
                            let running = self.running_jobs.lock().await;
                            if running.is_empty() {
                                break;
                            }
                            drop(running);
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    });
                    
                    if timeout.await.is_err() {
                        eprintln!("Timeout waiting for jobs to finish, shutting down anyway");
                    }
                    return;
                }
            }
        }
    }

    async fn tick(&self) {
        let store = match self.create_store() {
            Ok(store) => store,
            Err(e) => {
                eprintln!("Failed to create store: {}", e);
                return;
            }
        };

        let jobs = match store.load_jobs() {
            Ok(jobs) => jobs,
            Err(e) => {
                eprintln!("Failed to load jobs: {}", e);
                return;
            }
        };

        let now = self.clock.now();
        
        for job in jobs {
            if !job.enabled {
                continue;
            }

            if !cron_eval::is_overdue(&job, now) {
                continue;
            }

            let running = self.running_jobs.lock().await;
            if running.contains(&job.id) && !job.allow_overlap {
                continue;
            }
            drop(running);

            self.spawn_job(job);
        }
    }

    fn spawn_job(&self, job: Job) {
        let config = self.config.clone();
        let store_dir = self.store_dir.clone();
        let running_jobs = self.running_jobs.clone();
        let clock = self.clock.clone();
        
        tokio::spawn(async move {
            // Add to running jobs
            {
                let mut running = running_jobs.lock().await;
                running.insert(job.id);
            }

            let result = Self::execute_job_impl(job.clone(), config, store_dir, clock).await;
            
            if let Err(e) = result {
                eprintln!("Job execution failed for {}: {}", job.name, e);
            }

            // Remove from running jobs
            {
                let mut running = running_jobs.lock().await;
                running.remove(&job.id);
            }
        });
    }

    async fn execute_job_impl(
        job: Job,
        config: Config,
        store_dir: Option<PathBuf>,
        clock: C,
    ) -> crate::error::Result<()> {
        let store = if let Some(ref dir) = store_dir {
            JobStore::with_dir(dir.clone())?
        } else {
            JobStore::new()?
        };

        let now = clock.now();
        let from_time = job.last_run.unwrap_or(job.created_at);
        
        let scheduled_for = cron_eval::next_occurrence(&job.cron_expr, from_time)?;
        let missed_count = cron_eval::missed_count(&job.cron_expr, from_time, now);

        // Create log directory (use store_dir if set, otherwise global runs_dir)
        let base_runs = store_dir.as_ref()
            .map(|d| d.join("runs"))
            .unwrap_or_else(runs_dir);
        let log_dir = base_runs.join(job.id.to_string());
        std::fs::create_dir_all(&log_dir)?;

        // Create log file path (include millis to avoid collision with allow_overlap)
        let timestamp = now.format("%Y%m%d_%H%M%S");
        let millis = now.timestamp_subsec_millis();
        let log_path = log_dir.join(format!("{}_{:03}.log", timestamp, millis));

        // Execute the job
        let result = executor::execute_job(&job, &config, &log_path).await?;

        // Create run record
        let record = RunRecord {
            job_id: job.id,
            job_name: job.name.clone(),
            fired_at: now,
            scheduled_for,
            missed_count,
            duration_secs: result.duration_secs,
            exit_code: result.exit_code,
            success: result.success,
            output_path: result.output_path.clone(),
            manual: false,
        };

        // Save run record
        store.append_run_record(&record)?;

        // Rotate logs
        store.rotate_logs(job.id, config.max_log_runs)?;

        // Update job last_run time
        let mut updated_job = job.clone();
        updated_job.last_run = Some(now);
        store.update_job(&updated_job)?;

        // Send notification
        notifier::notify(&job, &result);

        Ok(())
    }

    pub fn trigger_shutdown(&self) {
        self.shutdown.notify_waiters();
    }

    fn create_store(&self) -> crate::error::Result<JobStore> {
        if let Some(dir) = &self.store_dir {
            JobStore::with_dir(dir.clone())
        } else {
            JobStore::new()
        }
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
        Config {
            kiro_cli_path: "echo".to_string(),
            default_timeout_secs: 5,
            max_log_runs: 10,
            heartbeat_secs: 60,
        }
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
    async fn test_tick_fires_overdue_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("fire-test".to_string(), "* * * * *".to_string(), "hello".to_string(), PathBuf::from("/tmp"));
        job.last_run = Some(now - chrono::Duration::minutes(2));

        let store = JobStore::with_dir(dir.clone()).unwrap();
        store.add_job(job.clone()).unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let store = JobStore::with_dir(dir).unwrap();
        let records = store.load_run_records(job.id, 10).unwrap();
        assert!(!records.is_empty(), "Expected run record for overdue job");
    }

    #[tokio::test]
    async fn test_tick_skips_not_overdue_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("skip-test".to_string(), "0 * * * *".to_string(), "hello".to_string(), PathBuf::from("/tmp"));
        job.last_run = Some(now - chrono::Duration::seconds(30));

        let store = JobStore::with_dir(dir.clone()).unwrap();
        store.add_job(job.clone()).unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let store = JobStore::with_dir(dir).unwrap();
        let records = store.load_run_records(job.id, 10).unwrap();
        assert!(records.is_empty(), "Should not fire non-overdue job");
    }

    #[tokio::test]
    async fn test_tick_skips_disabled_job() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().to_path_buf();
        let now = Utc::now();

        let mut job = Job::new("disabled-test".to_string(), "* * * * *".to_string(), "hello".to_string(), PathBuf::from("/tmp"));
        job.last_run = Some(now - chrono::Duration::minutes(2));
        job.enabled = false;

        let store = JobStore::with_dir(dir.clone()).unwrap();
        store.add_job(job.clone()).unwrap();

        let scheduler = Scheduler::new(MockClock::new(now), test_config(), Some(dir.clone()));
        scheduler.tick().await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        let store = JobStore::with_dir(dir).unwrap();
        let records = store.load_run_records(job.id, 10).unwrap();
        assert!(records.is_empty(), "Should not fire disabled job");
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

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            scheduler.run(),
        ).await;
        assert!(result.is_ok(), "run() should return promptly after shutdown");
    }
}
