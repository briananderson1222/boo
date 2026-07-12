use crate::job::Job;
use chrono::{DateTime, Duration, Utc};
use croner::Cron;

/// Parse a timezone name into a chrono-tz zone. Returns an error for unknown
/// names so bad `--timezone` values are rejected at add/edit time.
pub fn parse_timezone(name: &str) -> crate::error::Result<chrono_tz::Tz> {
    name.parse::<chrono_tz::Tz>()
        .map_err(|_| crate::error::BooError::Other(format!("Unknown timezone: {name}")))
}

/// Next cron occurrence after `from`, evaluated in UTC.
pub fn next_occurrence(
    cron_expr: &str,
    from: DateTime<Utc>,
) -> crate::error::Result<DateTime<Utc>> {
    next_occurrence_tz(cron_expr, from, None)
}

/// Next cron occurrence after `from`, evaluated in the given timezone (so
/// "0 9 * * *" means 9am local wall-clock, DST included) and returned in UTC.
/// A `None` timezone means UTC.
pub fn next_occurrence_tz(
    cron_expr: &str,
    from: DateTime<Utc>,
    tz: Option<&str>,
) -> crate::error::Result<DateTime<Utc>> {
    let cron: Cron = cron_expr
        .parse()
        .map_err(|e: croner::errors::CronError| crate::error::BooError::CronParse(e.to_string()))?;
    match tz {
        Some(name) => {
            let zone = parse_timezone(name)?;
            let local = from.with_timezone(&zone);
            let next = cron
                .find_next_occurrence(&local, false)
                .map_err(|e| crate::error::BooError::CronParse(e.to_string()))?;
            Ok(next.with_timezone(&Utc))
        }
        None => cron
            .find_next_occurrence(&from, false)
            .map_err(|e| crate::error::BooError::CronParse(e.to_string())),
    }
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
    match next_occurrence_tz(&job.cron_expr, reference, job.timezone.as_deref()) {
        Ok(next) => next <= now,
        Err(e) => {
            // A job whose cron/timezone can't be evaluated would otherwise
            // just stop firing forever with no signal — surface it so an
            // operator watching the daemon log can see why.
            eprintln!(
                "boo: job '{}' has an unevaluable schedule and will not fire: {e}",
                job.name
            );
            false
        }
    }
}

/// Compute the next fire time for display purposes.
pub fn next_fire_time(job: &Job, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Some(at_time) = job.at_time {
        return if job.last_run.is_none() {
            Some(at_time)
        } else {
            None
        };
    }
    if let Some(every_secs) = job.every_secs {
        let reference = job.last_run.unwrap_or(job.created_at);
        return Some(reference + Duration::seconds(every_secs as i64));
    }
    next_occurrence_tz(&job.cron_expr, now, job.timezone.as_deref()).ok()
}

/// Count occurrences missed in (from, to], excluding the occurrence being
/// fired right now — an on-time run reports 0 missed. Capped at 1000.
pub fn missed_count(
    cron_expr: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    tz: Option<&str>,
) -> u32 {
    let mut count = 0u32;
    let mut current = from;
    while let Ok(next) = next_occurrence_tz(cron_expr, current, tz) {
        if next > to || count > 1000 {
            break;
        }
        count += 1;
        current = next;
    }
    count.saturating_sub(1)
}

/// Missed-interval count for `every` jobs, excluding the interval being
/// fired right now — an on-time run reports 0 missed.
pub fn missed_count_every(every_secs: u64, from: DateTime<Utc>, to: DateTime<Utc>) -> u32 {
    if every_secs == 0 || to <= from {
        return 0;
    }
    let elapsed = (to - from).num_seconds().max(0) as u64;
    (elapsed / every_secs).saturating_sub(1).min(1000) as u32
}

/// Return the next N cron occurrences from `from` for preview.
pub fn next_n_occurrences(
    cron_expr: &str,
    from: DateTime<Utc>,
    n: usize,
) -> crate::error::Result<Vec<DateTime<Utc>>> {
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

    prop_compose! {
        fn arb_cron()(min in 0..60u32, hour in 0..24u32, dom in 1..29u32, month in 1..13u32, dow in 0..7u32) -> String {
            format!("{} {} {} {} {}", min, hour, dom, month, dow)
        }
    }

    fn make_job(cron: &str) -> Job {
        Job::new("test", cron, "test", std::env::temp_dir())
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

    #[test]
    fn test_missed_count_on_time_is_zero() {
        // Fired exactly one minute after the previous run of a * * * * *
        // job: nothing was missed.
        let from = Utc::now();
        let to = from + Duration::seconds(60);
        assert_eq!(missed_count("* * * * *", from, to, None), 0);
    }

    #[test]
    fn test_missed_count_late_run() {
        // 10 minutes late on a every-minute cron: 9 occurrences were missed
        // before the one firing now.
        let from = "2026-01-01T00:00:30Z".parse().unwrap();
        let to = "2026-01-01T00:10:30Z".parse().unwrap();
        assert_eq!(missed_count("* * * * *", from, to, None), 9);
    }

    #[test]
    fn test_timezone_cron_evaluation() {
        // "0 9 * * *" in America/New_York should fire at 14:00 UTC in winter
        // (EST, UTC-5).
        let from: DateTime<Utc> = "2026-01-15T00:00:00Z".parse().unwrap();
        let next = next_occurrence_tz("0 9 * * *", from, Some("America/New_York")).unwrap();
        assert_eq!(next.to_rfc3339(), "2026-01-15T14:00:00+00:00");
        // Same expression in summer (EDT, UTC-4) → 13:00 UTC, proving DST is
        // honored rather than a fixed offset.
        let summer: DateTime<Utc> = "2026-07-15T00:00:00Z".parse().unwrap();
        let next = next_occurrence_tz("0 9 * * *", summer, Some("America/New_York")).unwrap();
        assert_eq!(next.to_rfc3339(), "2026-07-15T13:00:00+00:00");
    }

    #[test]
    fn test_unknown_timezone_rejected() {
        assert!(parse_timezone("Not/AZone").is_err());
        assert!(parse_timezone("America/New_York").is_ok());
    }

    #[test]
    fn test_missed_count_every_on_time_is_zero() {
        let from = Utc::now();
        assert_eq!(
            missed_count_every(300, from, from + Duration::seconds(300)),
            0
        );
        assert_eq!(
            missed_count_every(300, from, from + Duration::seconds(599)),
            0
        );
    }

    #[test]
    fn test_missed_count_every_late() {
        let from = Utc::now();
        // 3 intervals elapsed; one is the current fire, two were missed
        assert_eq!(
            missed_count_every(300, from, from + Duration::seconds(900)),
            2
        );
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
            let _ = missed_count(&cron_expr, from_dt, to_dt, None);
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
