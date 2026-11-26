//! Serial publish queue: approved jobs run one at a time in click order. Approvals are consumed by the
//! queue, so nothing can enqueue work without having passed the gate.

use super::executor::JobResult;
use super::gate::Approval;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tokio::sync::{mpsc, oneshot};

pub struct Job {
    pub approval: Approval,
    pub reply: Option<oneshot::Sender<JobResult>>,
}

#[async_trait::async_trait]
pub trait JobRunner: Send + Sync {
    async fn run(&self, approval: &Approval) -> JobResult;
}

pub struct PublishQueue {
    tx: mpsc::UnboundedSender<Job>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Job>>>,
    pending: AtomicUsize,
}

impl Default for PublishQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PublishQueue {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self { tx, rx: Mutex::new(Some(rx)), pending: AtomicUsize::new(0) }
    }

    pub fn submit(&self, approval: Approval) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(Job { approval, reply: None });
    }

    pub fn submit_and_wait(&self, approval: Approval) -> oneshot::Receiver<JobResult> {
        let (reply, rx) = oneshot::channel();
        self.pending.fetch_add(1, Ordering::SeqCst);
        let _ = self.tx.send(Job { approval, reply: Some(reply) });
        rx
    }

    pub fn pending(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }

    /// The worker takes the receiver once at startup.
    pub fn take_receiver(&self) -> Option<mpsc::UnboundedReceiver<Job>> {
        self.rx.lock().unwrap().take()
    }

    pub async fn run_worker(&self, mut rx: mpsc::UnboundedReceiver<Job>, runner: &dyn JobRunner) {
        while let Some(job) = rx.recv().await {
            let result = runner.run(&job.approval).await;
            self.pending.fetch_sub(1, Ordering::SeqCst);
            if let Some(reply) = job.reply {
                let _ = reply.send(result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::bundle::ActionKind;
    use crate::drafts::repo::testutil::*;
    use crate::publish::gate::approve_action;
    use std::sync::Arc;

    struct Recorder(Mutex<Vec<String>>);

    #[async_trait::async_trait]
    impl JobRunner for Recorder {
        async fn run(&self, a: &Approval) -> JobResult {
            self.0.lock().unwrap().push(a.platform_post_id().to_string());
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            JobResult { ok: true, message: None, published_post_id: None }
        }
    }

    #[tokio::test]
    async fn jobs_run_one_at_a_time_in_submission_order() {
        let (conn, profile) = db();
        acknowledge(&conn);
        let q = Arc::new(PublishQueue::new());
        let mut waits = vec![];
        for pid in ["1", "2", "3"] {
            let o = post(&conn, &profile, pid, "a", "t");
            waits.push(q.submit_and_wait(approve_action(&conn, &o, ActionKind::Like, false, false).unwrap()));
        }
        assert_eq!(q.pending(), 3);
        let rec = Recorder(Mutex::default());
        let rx = q.take_receiver().unwrap();
        assert!(q.take_receiver().is_none());
        let q2 = q.clone();
        let worker = tokio::spawn(async move {
            q2.run_worker(rx, &rec).await;
            rec.0.into_inner().unwrap()
        });
        for w in waits {
            assert!(w.await.unwrap().ok);
        }
        assert_eq!(q.pending(), 0);
        drop(q);
        let _ = tokio::time::timeout(std::time::Duration::from_millis(50), worker).await;
    }
}
