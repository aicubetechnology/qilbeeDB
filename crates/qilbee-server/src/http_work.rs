//! Per-server request/work lifetime tracking for graceful shutdown.
//!
//! A cancelled request can leave a blocking database operation alive. Its ticket
//! belongs to the worker closure, so shutdown still waits for its completion.
use axum::{extract::State, middleware::Next, response::Response};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{sync::Notify, task::JoinHandle};

tokio::task_local! {
    static REQUEST_WORK: Arc<HttpWork>;
}

#[derive(Default)]
pub(crate) struct HttpWork {
    active: AtomicUsize,
    panicked: AtomicBool,
    idle: Notify,
}

struct Ticket(Arc<HttpWork>);
impl Drop for Ticket {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.panicked.store(true, Ordering::Release);
        }
        if self.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.idle.notify_waiters();
        }
    }
}

impl HttpWork {
    pub(crate) fn panicked(&self) -> bool {
        self.panicked.load(Ordering::Acquire)
    }

    fn begin(self: &Arc<Self>) -> Ticket {
        self.active.fetch_add(1, Ordering::AcqRel);
        Ticket(self.clone())
    }

    // Only call after HTTP admission and request dispatch have finished. Zero
    // during ordinary serving is not a guarantee against a future request.
    pub(crate) async fn wait_idle(&self) {
        loop {
            let idle = self.idle.notified();
            tokio::pin!(idle);
            idle.as_mut().enable();
            if self.active.load(Ordering::Acquire) == 0 {
                return;
            }
            idle.await;
        }
    }
}

pub(crate) async fn track(
    State(work): State<Arc<HttpWork>>,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let _request = work.begin();
    REQUEST_WORK.scope(work, next.run(request)).await
}

/// All blocking work launched by HTTP handlers must use this wrapper. Calls
/// outside a tracked HTTP request retain Tokio's normal spawn semantics.
pub(crate) fn spawn_blocking<F, R>(operation: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let ticket = REQUEST_WORK.try_with(HttpWork::begin).ok();
    tokio::task::spawn_blocking(move || {
        let _ticket = ticket;
        operation()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelled_request_retains_worker_ticket_until_real_completion() {
        let work = Arc::new(HttpWork::default());
        let (release, wait) = mpsc::channel();
        let started = Arc::new(Notify::new());
        let task = {
            let work = work.clone();
            let started = started.clone();
            tokio::spawn(async move {
                REQUEST_WORK
                    .scope(work, async move {
                        spawn_blocking(move || {
                            started.notify_one();
                            wait.recv().unwrap();
                        })
                        .await
                        .unwrap();
                    })
                    .await;
            })
        };
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert_eq!(work.active.load(Ordering::Acquire), 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), work.wait_idle())
                .await
                .is_err()
        );
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), work.wait_idle())
            .await
            .unwrap();
        assert_eq!(work.active.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn panicking_worker_releases_ticket_and_reports_failure() {
        let work = Arc::new(HttpWork::default());
        let result = REQUEST_WORK
            .scope(work.clone(), async {
                spawn_blocking(|| panic!("injected worker panic")).await
            })
            .await;
        assert!(result.unwrap_err().is_panic());
        assert!(work.panicked());
        work.wait_idle().await;
        assert_eq!(work.active.load(Ordering::Acquire), 0);
    }
}
