//! Bridging async database work into egui's synchronous frame loop.
//!
//! egui is immediate mode: `update` runs every frame and must never block,
//! or the window stops responding. Every driver call in `dbhive-core` is
//! async and can take seconds (a remote Postgres over an SSH tunnel, say).
//!
//! The bridge is deliberately boring. Work is spawned onto a Tokio runtime
//! and its result comes back through a oneshot channel that the UI polls
//! once per frame. Nothing in the UI ever awaits, and nothing in the UI ever
//! holds a lock across a call.
//!
//! `ctx.request_repaint()` on completion matters: egui only redraws when
//! something happens, so without it a finished query would sit there
//! invisible until the user happened to move the mouse.

use std::future::Future;

use tokio::sync::oneshot;

/// A unit of in-flight async work whose result the UI polls for.
pub struct Task<T> {
    rx: oneshot::Receiver<T>,
    finished: bool,
}

impl<T: Send + 'static> Task<T> {
    /// Spawn `fut` on the runtime, repainting when it lands.
    pub fn spawn<F>(rt: &tokio::runtime::Handle, ctx: &egui::Context, fut: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let ctx = ctx.clone();
        rt.spawn(async move {
            let out = fut.await;
            // Send can only fail if the UI dropped the Task, e.g. the user
            // closed the tab while a query was running. That is expected, so
            // the result is discarded rather than treated as an error.
            let _ = tx.send(out);
            ctx.request_repaint();
        });
        Self {
            rx,
            finished: false,
        }
    }

    /// Take the result if it has arrived. Returns `None` while still running.
    ///
    /// After this yields `Some`, it yields `None` forever; callers move the
    /// value into their own state.
    pub fn poll(&mut self) -> Option<T> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.finished = true;
                Some(v)
            }
            Err(oneshot::error::TryRecvError::Empty) => None,
            Err(oneshot::error::TryRecvError::Closed) => {
                // The worker panicked or was cancelled. Nothing to deliver.
                self.finished = true;
                None
            }
        }
    }

    pub fn is_running(&self) -> bool {
        !self.finished
    }
}

/// A slot holding at most one in-flight task, plus its last outcome.
///
/// This is the shape almost every screen needs: a button that kicks off work,
/// a spinner while it runs, and either a result or an error afterwards.
/// Starting a new run replaces whatever was there.
pub struct Job<T> {
    task: Option<Task<T>>,
}

impl<T> Default for Job<T> {
    fn default() -> Self {
        Self { task: None }
    }
}

impl<T: Send + 'static> Job<T> {
    pub fn start<F>(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context, fut: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        self.task = Some(Task::spawn(rt, ctx, fut));
    }

    /// Poll for completion, clearing the slot when the result arrives.
    pub fn take_result(&mut self) -> Option<T> {
        let done = self.task.as_mut().and_then(|t| t.poll());
        if done.is_some() {
            self.task = None;
        }
        done
    }

    pub fn is_running(&self) -> bool {
        self.task.as_ref().is_some_and(|t| t.is_running())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn job_reports_running_then_delivers_once() {
        let rt = rt();
        let ctx = egui::Context::default();
        let mut job: Job<u32> = Job::default();

        let (tx, rx) = oneshot::channel::<()>();
        job.start(rt.handle(), &ctx, async move {
            let _ = rx.await;
            7
        });
        assert!(job.is_running());
        assert!(job.take_result().is_none());

        tx.send(()).unwrap();
        let mut got = None;
        for _ in 0..2000 {
            if let Some(v) = job.take_result() {
                got = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(got, Some(7));
        // The slot is cleared, so it neither reports running nor redelivers.
        assert!(!job.is_running());
        assert!(job.take_result().is_none());
    }

    #[test]
    fn idle_job_is_not_running() {
        let job: Job<u32> = Job::default();
        assert!(!job.is_running());
    }
}
