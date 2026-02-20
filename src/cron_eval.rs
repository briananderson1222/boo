use crate::job::Job;
use chrono::{DateTime, Duration, Utc};
use croner::Cron;

/// Parse and return next cron occurrence after `from`.
pub fn next_occurrence(cron_expr: &str, from: DateTime<Utc>) -> crate::error::Result<DateTime<Utc>> {
    let cron = Cron::new(cron_expr).parse()
        .map_err(|e| crate::error::BooError::CronParse(e.to_string()))?;
    cron.find_next_occurrence(&from, false)
        .map_err(|e| crate::error::BooError::CronParse(e.to_string()))
}

/// Check if a job is overdue given current time. Handles all schedule types.
pub fn is_overdue(job: &Job, now: DateTime<Utc>) -> bool {
    // At: overdue if time has passed and never run
    if let Some(at_time) = job.at_time {
        return job.last_run.is_none() && at_time <= now;
    }
    // Every: overdue if reference + interval <= now
    if let Some(every_secs) = job.every_secs {
        let reference = job.last_run.unwrap_or(job.created_at);
        return reference + Duration::seconds(every_secs as i64) <= now;
    }
    // Cron: next occurrence from last_run (or created_at) <= now
    let reference = job.last_run.unwrap_or(job.created_at);
    match next_occurrence(&job.cron_expr, reference) {
        Ok(next) => next <= now,
        Err(_) => false,
    }
}

/// Compute the next fire time for display purposes.
pub fn next_fire_time(job: &Job, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Some(at_time) = job.at_time {
        return if job.last_run.is_none() { Some(at_time) } else { None };
    }
    if let Some(every_secs) = job.every_secs {
        let reference = job.last_run.unwrap_or(job.created_at);
        return Some(reference + Duration::seconds(every_secs as i64));
    }
    next_occurrence(&job.cron_expr, now).ok()
}

/// Count missed occurrences between from and to. Capped at 1000.
pub fn missed_count(cron_expr: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> u32 {
    let mut count = 0u32;
    let mut current = from;
    while let Ok(next) = next_occurrence(cron_expr, current) {
        if next > to || count >= 1000 { break; }
        count += 1;
        current = next;
    }
    count
}

/// Return the next N cron occurrences from `from` for preview.
pub fn next_n_occurrences(cron_expr: &str, from: DateTime<Utc>, n: usize) -> crate::error::Result<Vec<DateTime<Utc>>> {
    let mut occurrences = Vec::with_capacity(n);
    let mut current = from;
    for _ in 0..n {
        let next = next_occurrence(cron_expr, current)?;
        occurrences.push(next);
        current = next;
    }
    Ok(occurrences)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Job;
    use proptest::prelude::*;
    use std::path::PathBuf;

    prop_compose! {
        fn arb_cron()(min in 0..60u32, hour in 0..24u32, dom in 1..29u32, month in 1..13u32, dow in 0..7u32) -> String {
            format!("{} {} {} {} {}", min, hour, dom, month, dow)
        }
    }

    fn make_job(cron: &str) -> Job {
        Job::new("test", cron, "test", PathBuf::from("/tmp"))
    }

    #[test]
    fn test_at_overdue_when_time_passed() {
        let now = Utc::now();
        let mut job = make_job("");
        job.at_time = Some(now - Duration::minutes(5));
        assert!(is_overdue(&job, now));
    }

    #[test]
    fn test_at_not_overdue_before_time() {
        let now = Utc::now();
        let mut job = make_job("");
        job.at_time = Some(now + Duration::minutes(5));
        assert!(!is_overdue(&job, now));
    }

    #[test]
    fn test_at_not_overdue_after_run() {
        let now = Utc::now();
        let mut job = make_job("");
        job.at_time = Some(now - Duration::minutes(5));
        job.last_run = Some(now - Duration::minutes(1));
        assert!(!is_overdue(&job, now));
    }

    #[test]
    fn test_every_overdue_after_interval() {
        let now = Utc::now();
        let mut job = make_job("");
        job.every_secs = Some(300); // 5 minutes
        job.last_run = Some(now - Duration::minutes(10));
        assert!(is_overdue(&job, now));
    }

    #[test]
    fn test_every_not_overdue_within_interval() {
        let now = Utc::now();
        let mut job = make_job("");
        job.every_secs = Some(300);
        job.last_run = Some(now - Duration::minutes(2));
        assert!(!is_overdue(&job, now));
    }

    #[test]
    fn test_next_fire_time_at() {
        let now = Utc::now();
        let future = now + Duration::hours(1);
        let mut job = make_job("");
        job.at_time = Some(future);
        assert_eq!(next_fire_time(&job, now), Some(future));
    }

    #[test]
    fn test_next_fire_time_at_already_run() {
        let now = Utc::now();
        let mut job = make_job("");
        job.at_time = Some(now - Duration::hours(1));
        job.last_run = Some(now - Duration::minutes(30));
        assert_eq!(next_fire_time(&job, now), None); // one-shot, already fired
    }

    #[test]
    fn test_next_fire_time_every() {
        let now = Utc::now();
        let mut job = make_job("");
        job.every_secs = Some(3600);
        job.last_run = Some(now - Duration::minutes(30));
        let expected = job.last_run.unwrap() + Duration::seconds(3600);
        assert_eq!(next_fire_time(&job, now), Some(expected));
    }

    proptest! {
        #[test]
        fn next_occurrence_always_after_from(cron_expr in arb_cron(), from in prop::num::i64::ANY) {
            let from_dt = DateTime::from_timestamp(from.abs() % 2_000_000_000, 0).unwrap_or_else(Utc::now);
            if let Ok(next) = next_occurrence(&cron_expr, from_dt) {
                prop_assert!(next > from_dt);
            }
        }

        #[test]
        fn is_overdue_true_when_past_next(cron_expr in arb_cron()) {
            let now = Utc::now();
            let past = now - Duration::hours(25);
            if let Ok(next) = next_occurrence(&cron_expr, past) {
                if next <= now {
                    let mut job = make_job(&cron_expr);
                    job.last_run = Some(past);
                    prop_assert!(is_overdue(&job, now));
                }
            }
        }

        #[test]
        fn missed_count_non_negative(cron_expr in arb_cron(), from in prop::num::i64::ANY, to in prop::num::i64::ANY) {
            let from_dt = DateTime::from_timestamp(from.abs() % 1_000_000_000, 0).unwrap_or_else(Utc::now);
            let to_dt = DateTime::from_timestamp(to.abs() % 1_000_000_000, 0).unwrap_or_else(Utc::now);
            let _ = missed_count(&cron_expr, from_dt, to_dt);
        }

        #[test]
        fn next_n_occurrences_correct_count_and_order(cron_expr in arb_cron(), n in 1..10usize) {
            let from = Utc::now();
            if let Ok(occurrences) = next_n_occurrences(&cron_expr, from, n) {
                prop_assert_eq!(occurrences.len(), n);
                for i in 1..occurrences.len() {
                    prop_assert!(occurrences[i] > occurrences[i-1]);
                }
            }
        }
    }
}
