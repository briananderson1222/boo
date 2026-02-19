use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};

/// Abstraction over system clock for deterministic testing.
pub trait Clock: Send + Sync + Clone {
    fn now(&self) -> DateTime<Utc>;
}

/// Production clock using real system time.
#[derive(Clone)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Test clock with manually controllable time.
#[derive(Clone)]
pub struct MockClock {
    now: Arc<Mutex<DateTime<Utc>>>,
}

impl MockClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self { now: Arc::new(Mutex::new(start)) }
    }

    pub fn advance(&self, duration: chrono::Duration) {
        *self.now.lock().unwrap() += duration;
    }

    pub fn set(&self, time: DateTime<Utc>) {
        *self.now.lock().unwrap() = time;
    }
}

impl Clock for MockClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().unwrap()
    }
}
