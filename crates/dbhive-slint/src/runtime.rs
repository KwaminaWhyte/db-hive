//! The bridge between async database work and Slint's UI thread.
//!
//! the app polled every in-flight job once per frame. Slint is retained mode,
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

#[cfg(test)]
use std::sync::atomic::AtomicBool;

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
///
/// `work` runs as its own task and is awaited through its join handle, so a
/// panic inside it is caught here instead of silently killing the task: the
/// panic hook has logged it (sanitized), and the UI is told with a toast
/// (`app::shell::report_worker_panic`). `done` does not run then; there is no
/// value to give it.
///
/// `ui` is the id of the window that asked for the work. Completions and
/// panic reports are bound to it while they run, so `with_ctx` inside
/// reaches that window when several are open.
pub fn spawn<T, F, D>(ui: u64, work: F, done: D)
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
    D: FnOnce(T) + Send + 'static,
{
    let task = handle().spawn(work);
    handle().spawn(async move {
        match task.await {
            Ok(out) => {
                let _ = slint::invoke_from_event_loop(move || {
                    crate::app::with_ctx_bound(ui, move || done(out))
                });
            }
            Err(e) if e.is_panic() => {
                let _ = slint::invoke_from_event_loop(move || {
                    crate::app::with_ctx_bound(ui, crate::app::report_worker_panic)
                });
            }
            // Cancelled: the runtime is shutting down with the app.
            Err(_) => {}
        }
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
        assert!(
            !first.is_current(),
            "a stale result would overwrite a newer one"
        );
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

    #[test]
    fn a_panicking_task_does_not_run_its_completion() {
        // The panic is caught inside `spawn`; without a Slint event loop the
        // toast cannot be delivered here, but the point is that the process
        // survives and `done` is not called with a value that never existed.
        static DONE: AtomicBool = AtomicBool::new(false);
        DONE.store(false, Ordering::SeqCst);
        spawn(
            1,
            async {
                panic!("intentional test panic");
            },
            |_: ()| DONE.store(true, Ordering::SeqCst),
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!DONE.load(Ordering::SeqCst));
    }
}
