use crate::config;
use crate::error::{BooError, Result};
use crate::is_pid_alive;
use crate::job::{Job, RunRecord};
use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRun {
    pub job_id: Uuid,
    pub job_name: String,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    pub manual: bool,
}

pub struct JobStore {
    jobs_path: PathBuf,
    lock_path: PathBuf,
    runs_dir: PathBuf,
}

impl JobStore {
    pub fn new() -> Result<Self> {
        let boo_dir = config::boo_dir();
        Self::with_dir(boo_dir)
    }

    pub fn with_dir(dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        let runs_dir = dir.join("runs");
        std::fs::create_dir_all(&runs_dir)?;
        Ok(Self {
            jobs_path: dir.join("jobs.json"),
            lock_path: dir.join("jobs.lock"),
            runs_dir,
        })
    }

    /// All mutations go through this single lock scope to prevent TOCTOU races.
    fn with_lock<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce() -> Result<R>,
    {
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.lock_path)?;
        lock_file.lock_exclusive()?;
        let result = f();
        drop(lock_file);
        result
    }

    fn read_jobs_unlocked(&self) -> Result<Vec<Job>> {
        if !self.jobs_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.jobs_path)?;
        Ok(serde_json::from_str(&content)?)
    }

    fn write_jobs_unlocked(&self, jobs: &[Job]) -> Result<()> {
        let json = serde_json::to_string_pretty(jobs)?;
        let tmp = self.jobs_path.with_extension("tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, &self.jobs_path)?;
        Ok(())
    }

    pub fn load_jobs(&self) -> Result<Vec<Job>> {
        self.with_lock(|| self.read_jobs_unlocked())
    }

    /// Atomic add: single lock for read + write.
    pub fn add_job(&self, job: Job) -> Result<()> {
        self.with_lock(|| {
            let mut jobs = self.read_jobs_unlocked()?;
            jobs.push(job);
            self.write_jobs_unlocked(&jobs)
        })
    }

    /// Atomic remove: single lock for read + write.
    pub fn remove_job(&self, id: Uuid) -> Result<()> {
        self.with_lock(|| {
            let mut jobs = self.read_jobs_unlocked()?;
            let len = jobs.len();
            jobs.retain(|j| j.id != id);
            if jobs.len() == len {
                return Err(BooError::JobNotFound(id));
            }
            self.write_jobs_unlocked(&jobs)
        })
    }

    /// Atomic update: single lock for read + write.
    pub fn update_job(&self, job: &Job) -> Result<()> {
        self.with_lock(|| {
            let mut jobs = self.read_jobs_unlocked()?;
            let pos = jobs.iter().position(|j| j.id == job.id)
                .ok_or(BooError::JobNotFound(job.id))?;
            jobs[pos] = job.clone();
            self.write_jobs_unlocked(&jobs)
        })
    }

    pub fn get_job(&self, id: Uuid) -> Result<Job> {
        self.with_lock(|| {
            let jobs = self.read_jobs_unlocked()?;
            jobs.into_iter().find(|j| j.id == id)
                .ok_or(BooError::JobNotFound(id))
        })
    }

    pub fn append_run_record(&self, record: &RunRecord) -> Result<()> {
        let log_path = self.runs_dir.join(format!("{}.jsonl", record.job_id));
        let json = serde_json::to_string(record)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)?;
        writeln!(file, "{}", json)?;
        Ok(())
    }

    pub fn load_run_records(&self, job_id: Uuid, limit: usize) -> Result<Vec<RunRecord>> {
        let log_path = self.runs_dir.join(format!("{}.jsonl", job_id));
        if !log_path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(log_path)?;
        let lines: Vec<String> = BufReader::new(file).lines()
            .collect::<std::io::Result<Vec<_>>>()?;
        let mut records = Vec::new();
        for line in lines.iter().rev().take(limit) {
            if let Ok(record) = serde_json::from_str(line) {
                records.push(record);
            }
        }
        records.reverse();
        Ok(records)
    }

    pub fn write_active_run(&self, run: &ActiveRun) -> Result<()> {
        let path = self.runs_dir.join(format!("{}.active", run.job_id));
        let json = serde_json::to_string(run)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn remove_active_run(&self, job_id: Uuid) {
        let path = self.runs_dir.join(format!("{}.active", job_id));
        let _ = std::fs::remove_file(path);
    }

    pub fn list_active_runs(&self) -> Vec<ActiveRun> {
        let Ok(entries) = std::fs::read_dir(&self.runs_dir) else { return Vec::new() };
        entries.filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "active"))
            .filter_map(|e| {
                let content = std::fs::read_to_string(e.path()).ok()?;
                let run: ActiveRun = serde_json::from_str(&content).ok()?;
                if is_pid_alive(run.pid) { Some(run) } else {
                    // Stale .active file — process died without cleanup
                    let _ = std::fs::remove_file(e.path());
                    None
                }
            })
            .collect()
    }

    pub fn get_active_run(&self, job_id: Uuid) -> Option<ActiveRun> {
        let path = self.runs_dir.join(format!("{}.active", job_id));
        let content = std::fs::read_to_string(path).ok()?;
        let run: ActiveRun = serde_json::from_str(&content).ok()?;
        if is_pid_alive(run.pid) { Some(run) } else { None }
    }

    pub fn rotate_logs(&self, job_id: Uuid, max_runs: usize) -> Result<()> {
        let log_path = self.runs_dir.join(format!("{}.jsonl", job_id));
        if !log_path.exists() {
            return Ok(());
        }
        let file = File::open(&log_path)?;
        let lines: Vec<String> = BufReader::new(file).lines()
            .collect::<std::io::Result<Vec<_>>>()?;
        if lines.len() <= max_runs {
            return Ok(());
        }
        // Delete output files referenced by the records we're about to drop
        let drop_lines = &lines[..lines.len() - max_runs];
        for line in drop_lines {
            if let Ok(record) = serde_json::from_str::<crate::job::RunRecord>(line) {
                let _ = std::fs::remove_file(&record.output_path);
                let _ = std::fs::remove_file(record.output_path.with_extension("response"));
            }
        }
        let keep = &lines[lines.len() - max_runs..];
        let content = keep.join("\n") + "\n";
        // Atomic write: tmp + rename to prevent corruption on crash
        let tmp = log_path.with_extension("jsonl.tmp");
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &log_path)?;
        Ok(())
    }
}

impl Default for JobStore {
    fn default() -> Self {
        Self::new().expect("Failed to create default JobStore")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Job;
    use chrono::Utc;
    use proptest::prelude::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn test_job() -> Job {
        Job::new("test", "0 0 * * *", "echo hello", std::env::temp_dir())
    }

    #[test]
    fn test_add_then_get_returns_same_job() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let job = test_job();
        let id = job.id;
        store.add_job(job.clone()).unwrap();
        assert_eq!(job, store.get_job(id).unwrap());
    }

    #[test]
    fn test_remove_then_get_returns_error() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let job = test_job();
        let id = job.id;
        store.add_job(job).unwrap();
        store.remove_job(id).unwrap();
        assert!(matches!(store.get_job(id), Err(BooError::JobNotFound(_))));
    }

    #[test]
    fn test_persistence_survives_reload() {
        let dir = tempdir().unwrap();
        let job = test_job();
        let id = job.id;
        {
            let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
            store.add_job(job.clone()).unwrap();
        }
        let store2 = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        assert_eq!(job, store2.get_job(id).unwrap());
    }

    #[test]
    fn test_add_multiple_jobs_list_returns_all() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let j1 = test_job();
        let mut j2 = test_job(); j2.name = "test2".into();
        let mut j3 = test_job(); j3.name = "test3".into();
        store.add_job(j1.clone()).unwrap();
        store.add_job(j2.clone()).unwrap();
        store.add_job(j3.clone()).unwrap();
        let jobs = store.load_jobs().unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[test]
    fn test_update_preserves_other_jobs() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let j1 = test_job();
        let mut j2 = test_job(); j2.name = "test2".into();
        store.add_job(j1.clone()).unwrap();
        store.add_job(j2.clone()).unwrap();
        let mut u1 = j1.clone(); u1.name = "updated".into();
        store.update_job(&u1).unwrap();
        let jobs = store.load_jobs().unwrap();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.contains(&u1));
        assert!(jobs.contains(&j2));
    }

    #[test]
    fn test_append_and_load_run_records() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let job_id = uuid::Uuid::new_v4();
        let record = RunRecord {
            job_id, job_name: "test".into(), fired_at: Utc::now(),
            scheduled_for: Utc::now(), missed_count: 0, duration_secs: 1.5,
            exit_code: Some(0), success: true, output_path: PathBuf::from("/tmp/test.log"), manual: false,
        };
        store.append_run_record(&record).unwrap();
        let records = store.load_run_records(job_id, 10).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].job_id, job_id);
    }

    #[test]
    fn test_rotate_logs_keeps_max() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        let job_id = uuid::Uuid::new_v4();
        for i in 0..5 {
            let record = RunRecord {
                job_id, job_name: format!("test-{i}"), fired_at: Utc::now(),
                scheduled_for: Utc::now(), missed_count: 0, duration_secs: 1.0,
                exit_code: Some(0), success: true, output_path: PathBuf::from("/tmp/test.log"), manual: false,
            };
            store.append_run_record(&record).unwrap();
        }
        store.rotate_logs(job_id, 3).unwrap();
        let records = store.load_run_records(job_id, 10).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].job_name, "test-2");
    }

    #[test]
    fn test_load_run_records_empty() {
        let dir = tempdir().unwrap();
        let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
        assert!(store.load_run_records(uuid::Uuid::new_v4(), 10).unwrap().is_empty());
    }

    proptest! {
        #[test]
        fn prop_add_then_get(name in "[a-zA-Z0-9_-]{1,20}") {
            let dir = tempdir().unwrap();
            let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
            let job = Job::new(name, "0 0 * * *", "test", std::env::temp_dir());
            let id = job.id;
            store.add_job(job.clone()).unwrap();
            prop_assert_eq!(job, store.get_job(id).unwrap());
        }

        #[test]
        fn prop_remove_then_get(name in "[a-zA-Z0-9_-]{1,20}") {
            let dir = tempdir().unwrap();
            let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
            let job = Job::new(name, "0 0 * * *", "test", std::env::temp_dir());
            let id = job.id;
            store.add_job(job).unwrap();
            store.remove_job(id).unwrap();
            prop_assert!(matches!(store.get_job(id), Err(BooError::JobNotFound(_))));
        }

        #[test]
        fn prop_persistence(name in "[a-zA-Z0-9_-]{1,20}") {
            let dir = tempdir().unwrap();
            let job = Job::new(name, "0 0 * * *", "test", std::env::temp_dir());
            let id = job.id;
            { JobStore::with_dir(dir.path().to_path_buf()).unwrap().add_job(job.clone()).unwrap(); }
            prop_assert_eq!(job, JobStore::with_dir(dir.path().to_path_buf()).unwrap().get_job(id).unwrap());
        }

        #[test]
        fn prop_add_multiple(names in prop::collection::vec("[a-zA-Z0-9_-]{1,20}", 1..5)) {
            let dir = tempdir().unwrap();
            let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
            for name in &names {
                store.add_job(Job::new(name.clone(), "0 0 * * *", "test", std::env::temp_dir())).unwrap();
            }
            prop_assert_eq!(store.load_jobs().unwrap().len(), names.len());
        }

        #[test]
        fn prop_update_preserves(n1 in "[a-zA-Z0-9_-]{1,20}", n2 in "[a-zA-Z0-9_-]{1,20}") {
            let dir = tempdir().unwrap();
            let store = JobStore::with_dir(dir.path().to_path_buf()).unwrap();
            let j1 = Job::new(n1, "0 0 * * *", "t1", std::env::temp_dir());
            let j2 = Job::new(n2, "0 0 * * *", "t2", std::env::temp_dir());
            store.add_job(j1.clone()).unwrap();
            store.add_job(j2.clone()).unwrap();
            let mut u1 = j1; u1.name = "updated".into();
            store.update_job(&u1).unwrap();
            let jobs = store.load_jobs().unwrap();
            prop_assert!(jobs.contains(&u1));
            prop_assert!(jobs.contains(&j2));
        }
    }
}
