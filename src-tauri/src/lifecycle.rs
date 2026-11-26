//! Running-job registry and the quit-guard decision (SHL-28).
//!
//! Scout, Publish and Analytics will register a job here while they run. On quit the app asks the user
//! to stop-and-quit or cancel when anything is active, then closes the X Chrome window (BRW-16).

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Inner {
    next: AtomicU64,
    jobs: Mutex<BTreeMap<u64, String>>,
    stop_requested: AtomicBool,
}

#[derive(Clone, Default)]
pub struct JobRegistry(Arc<Inner>);

/// Held for the lifetime of a job; dropping it unregisters the job.
pub struct JobGuard {
    registry: JobRegistry,
    id: u64,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, label: &str) -> JobGuard {
        let id = self.0.next.fetch_add(1, Ordering::SeqCst);
        self.0.jobs.lock().unwrap().insert(id, label.to_string());
        JobGuard { registry: self.clone(), id }
    }

    pub fn active(&self) -> Vec<String> {
        self.0.jobs.lock().unwrap().values().cloned().collect()
    }

    /// Jobs poll this between steps and finish as failed-safe (never mid-click) when it turns true.
    pub fn stop_requested(&self) -> bool {
        self.0.stop_requested.load(Ordering::SeqCst)
    }

    pub fn request_stop_all(&self) {
        self.0.stop_requested.store(true, Ordering::SeqCst);
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.registry.0.jobs.lock().unwrap().remove(&self.id);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuitDecision {
    Proceed,
    NeedsConfirmation { jobs: Vec<String> },
}

pub fn decide_quit(registry: &JobRegistry) -> QuitDecision {
    let jobs = registry.active();
    if jobs.is_empty() {
        QuitDecision::Proceed
    } else {
        QuitDecision::NeedsConfirmation { jobs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_app_quits_without_asking() {
        assert_eq!(decide_quit(&JobRegistry::new()), QuitDecision::Proceed);
    }

    #[test]
    fn running_job_requires_confirmation_until_it_ends() {
        let reg = JobRegistry::new();
        let guard = reg.register("Publish: reply to @naval");
        let other = reg.register("Scout session");
        match decide_quit(&reg) {
            QuitDecision::NeedsConfirmation { jobs } => assert_eq!(jobs.len(), 2),
            other => panic!("expected confirmation, got {other:?}"),
        }
        drop(guard);
        drop(other);
        assert_eq!(decide_quit(&reg), QuitDecision::Proceed);
    }

    #[test]
    fn stop_all_is_visible_to_jobs() {
        let reg = JobRegistry::new();
        let _g = reg.register("Scout");
        assert!(!reg.stop_requested());
        reg.request_stop_all();
        assert!(reg.stop_requested());
    }
}
