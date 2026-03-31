//! Minimal scheduler primitives with lease-like de-dup guard.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;

#[derive(Clone, Default)]
pub struct Scheduler {
    locks: Arc<Mutex<HashSet<String>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spawn_interval<F>(&self, job_id: &str, period: Duration, mut task: F) -> Option<JoinHandle<()>>
    where
        F: FnMut() + Send + 'static,
    {
        {
            let mut locks = self.locks.lock().expect("scheduler lock poisoned");
            if !locks.insert(job_id.to_string()) {
                return None;
            }
        }
        let id = job_id.to_string();
        let locks = Arc::clone(&self.locks);
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            loop {
                ticker.tick().await;
                task();
                if !locks.lock().expect("scheduler lock poisoned").contains(&id) {
                    break;
                }
            }
        }))
    }

    pub fn cancel(&self, job_id: &str) {
        self.locks
            .lock()
            .expect("scheduler lock poisoned")
            .remove(job_id);
    }
}
