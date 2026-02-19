use crate::job::Job;
use chrono::{DateTime, Utc};
use croner::Cron;

/// Parse and return next occurrence after `from`. Returns None if invalid cron.
pub fn next_occurrence(cron_expr: &str, from: DateTime<Utc>) -> crate::error::Result<DateTime<Utc>> {
    let cron = Cron::new(cron_expr).parse()
        .map_err(|e| crate::error::BooError::CronParse(e.to_string()))?;
    
    cron.find_next_occurrence(&from, false)
        .map_err(|e| crate::error::BooError::CronParse(e.to_string()))
}

/// Check if a job is overdue given current time.
/// A job is overdue if next_occurrence(cron, last_run) <= now.
/// If last_run is None, use created_at as the reference.
pub fn is_overdue(job: &Job, now: DateTime<Utc>) -> bool {
    let reference_time = job.last_run.unwrap_or(job.created_at);
    
    match next_occurrence(&job.cron_expr, reference_time) {
        Ok(next) => next <= now,
        Err(_) => false,
    }
}

/// Count how many occurrences were missed between last_run and now.
/// Capped at 1000 to prevent unbounded iteration for very old last_run times.
pub fn missed_count(cron_expr: &str, from: DateTime<Utc>, to: DateTime<Utc>) -> u32 {
    let mut count = 0u32;
    let mut current = from;
    
    while let Ok(next) = next_occurrence(cron_expr, current) {
        if next > to || count >= 1000 {
            break;
        }
        count += 1;
        current = next;
    }
    
    count
}

/// Return the next N occurrences from `from` for preview (boo next command).
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
    use proptest::prelude::*;
    use chrono::{Duration, Utc};

    prop_compose! {
        fn arb_cron()(min in 0..60u32, hour in 0..24u32, dom in 1..29u32, month in 1..13u32, dow in 0..7u32) -> String {
            format!("{} {} {} {} {}", min, hour, dom, month, dow)
        }
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
            let past = now - Duration::hours(25); // Ensure we're past any daily cron
            
            if let Ok(next) = next_occurrence(&cron_expr, past) {
                if next <= now {
                    let job = crate::job::Job::new(
                        "test".to_string(),
                        cron_expr,
                        "test".to_string(),
                        std::path::PathBuf::from("/tmp")
                    );
                    let mut job_with_last_run = job;
                    job_with_last_run.last_run = Some(past);
                    prop_assert!(is_overdue(&job_with_last_run, now));
                }
            }
        }

        #[test]
        fn is_overdue_false_when_before_next(cron_expr in arb_cron()) {
            let now = Utc::now();
            let future = now + Duration::hours(25); // Ensure we're before next daily cron
            
            if let Ok(next) = next_occurrence(&cron_expr, now) {
                if next > future {
                    let job = crate::job::Job::new(
                        "test".to_string(),
                        cron_expr,
                        "test".to_string(),
                        std::path::PathBuf::from("/tmp")
                    );
                    let mut job_with_last_run = job;
                    job_with_last_run.last_run = Some(now);
                    prop_assert!(!is_overdue(&job_with_last_run, future));
                }
            }
        }

        #[test]
        fn missed_count_non_negative(cron_expr in arb_cron(), from in prop::num::i64::ANY, to in prop::num::i64::ANY) {
            let from_dt = DateTime::from_timestamp(from.abs() % 1_000_000_000, 0).unwrap_or_else(Utc::now);
            let to_dt = DateTime::from_timestamp(to.abs() % 1_000_000_000, 0).unwrap_or_else(Utc::now);
            let count = missed_count(&cron_expr, from_dt, to_dt);
            // u32 is always >= 0, just verify it returns without panic
            let _ = count;
        }

        #[test]
        fn next_n_occurrences_correct_count_and_order(cron_expr in arb_cron(), n in 1..10usize) {
            let from = Utc::now();
            if let Ok(occurrences) = next_n_occurrences(&cron_expr, from, n) {
                prop_assert_eq!(occurrences.len(), n);
                
                // Check ascending order
                for i in 1..occurrences.len() {
                    prop_assert!(occurrences[i] > occurrences[i-1]);
                }
            }
        }
    }
}