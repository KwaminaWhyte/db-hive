//! The bridge between async database work and Slint's UI thread.
//!
//! egui polled every in-flight job once per frame. Slint is retained mode,
//! so there is no frame loop to poll from, and nothing should be: work runs
//! on Tokio and pushes its result back with `slint::invoke_from_event_loop`,
//! which runs the completion on the UI thread. The UI does nothing while it
//! waits.
//!
//! Two rules keep this safe:
//!
//! 1. Only `Send` data crosses. Slint models are `Rc`-based and live on the
//!    UI thread, so futures return plain Rust values and the completion turns
//!    them into models there.
//! 2. A late result must not overwrite a newer one. Each kind of request
//!    carries a [`Latest`] token; switching database twice quickly means the
//!    first switch's table list is discarded when it finally arrives, instead
//!    of replacing the second's.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

/// The process-wide Tokio runtime.
///
/// Shared by every window, like `AppState`, so two windows on one connection
/// reuse one pool.
pub fn handle() -> &'static tokio::runtime::Handle {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("dbhive-worker")
            .build()
            .expect("failed to start the Tokio runtime")
    })
    .handle()
}

/// Run `work` on the runtime, then `done` with its output on the UI thread.
///
/// If the event loop has already exited (the app is closing), the result is
/// dropped: there is no UI left to show it in.
pub fn spawn<T, F, D>(work: F, done: D)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    D: FnOnce(T) + Send + 'static,
{
    handle().spawn(async move {
        let out = work.await;
        let _ = slint::invoke_from_event_loop(move || done(out));
    });
}

/// Identifies the most recent request of one kind.
///
/// Clone it into the completion; call [`Latest::begin`] when starting a
/// request and [`Ticket::is_current`] when its result lands.
#[derive(Clone, Default)]
pub struct Latest(Arc<AtomicU64>);

/// Proof of which request a result belongs to.
#[derive(Clone)]
pub struct Ticket {
    latest: Latest,
    id: u64,
}

impl Latest {
    /// Start a new request, superseding any still in flight.
    pub fn begin(&self) -> Ticket {
        let id = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        Ticket {
            latest: self.clone(),
            id,
        }
    }
}

impl Ticket {
    /// Whether this is still the newest request of its kind.
    pub fn is_current(&self) -> bool {
        self.latest.0.load(Ordering::SeqCst) == self.id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_request_supersedes_an_older_one() {
        let latest = Latest::default();
        let first = latest.begin();
        assert!(first.is_current());
        let second = latest.begin();
        assert!(!first.is_current(), "a stale result would overwrite a newer one");
        assert!(second.is_current());
    }

    #[test]
    fn separate_kinds_do_not_interfere() {
        let tables = Latest::default();
        let query = Latest::default();
        let t = tables.begin();
        let _q = query.begin();
        assert!(t.is_current());
    }

    #[test]
    fn tickets_work_across_threads() {
        let latest = Latest::default();
        let ticket = latest.begin();
        let handle = std::thread::spawn(move || ticket.is_current());
        assert!(handle.join().unwrap());
    }
}
