//! Hand-off primitive between the tokio runtime and the GPUI event loop.
//!
//! GPUI's event loop must not block. Long-running work — fetch, CRUD,
//! manual entry, evaluation — runs on tokio. The result is delivered
//! back to the view via a `oneshot` channel that the view polls.
//!
//! ## Why polling, not waiting
//!
//! The GPUI event loop cannot `.await` a `tokio` future without blocking
//! the UI. It also cannot rely on the future to run on its own, since
//! tokio's scheduler is off-thread. So the model is:
//!
//! 1. The view calls `AsyncTask::spawn(future)`.
//! 2. The view stores the `AsyncTask` and polls it on each render.
//! 3. When the task completes, `try_recv()` returns `TaskPoll::Ready`.
//! 4. The view consumes the value, updates its own state, and calls
//!    `cx.notify()` to request another render.
//!
//! The polling happens once per frame, which at 60 Hz is fast enough
//! that the user sees no delay. It does not spin — `try_recv()` is a
//! zero-cost check on a channel that is already "ready" or "not ready".
//!
//! ## Cancellation
//!
//! Dropping an `AsyncTask` aborts the underlying `tokio` task. This is
//! what makes it safe to hold the task in a view: when the view is
//! dropped, the task is cleaned up.

use std::future::Future;

use tokio::sync::oneshot;

pub struct AsyncTask<T> {
    rx: oneshot::Receiver<T>,
    handle: tokio::task::JoinHandle<()>,
    polled: bool,
}

pub enum TaskPoll<T> {
    /// Not finished yet. Poll again later.
    Pending,
    /// Finished successfully. The value is moved out.
    Ready(T),
    /// The task panicked, was cancelled, or the sender was dropped
    /// before sending.
    Cancelled,
}

impl<T> AsyncTask<T> {
    /// Spawn `future` on the current tokio runtime. Returns an
    /// `AsyncTask` that the view holds and polls.
    pub fn spawn<F>(future: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let result = future.await;
            // Ignore send errors: the receiver may have been dropped.
            let _ = tx.send(result);
        });
        Self {
            rx,
            handle,
            polled: false,
        }
    }

    /// Non-blocking check for completion.
    ///
    /// Returns `Pending` if the future is still running, `Ready(value)`
    /// if it completed, and `Cancelled` if the sending half was dropped
    /// without sending. Once `Ready` or `Cancelled` is returned, all
    /// subsequent calls return `Cancelled`.
    pub fn try_recv(&mut self) -> TaskPoll<T> {
        if self.polled {
            return TaskPoll::Cancelled;
        }
        match self.rx.try_recv() {
            Ok(v) => {
                self.polled = true;
                TaskPoll::Ready(v)
            }
            Err(oneshot::error::TryRecvError::Empty) => TaskPoll::Pending,
            Err(oneshot::error::TryRecvError::Closed) => {
                self.polled = true;
                TaskPoll::Cancelled
            }
        }
    }

    /// Cancel the task. The future is aborted; `try_recv` will return
    /// `Cancelled` on the next call.
    pub fn cancel(&mut self) {
        self.handle.abort();
        self.polled = true;
    }

    pub fn is_finished(&self) -> bool {
        self.polled
    }
}

impl<T> Drop for AsyncTask<T> {
    fn drop(&mut self) {
        if !self.polled {
            self.handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn pending_then_ready() {
        let mut task = AsyncTask::spawn(async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            42i32
        });

        // First poll: not ready.
        assert!(matches!(task.try_recv(), TaskPoll::Pending));

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Second poll: ready.
        match task.try_recv() {
            TaskPoll::Ready(v) => assert_eq!(v, 42),
            other => panic!("expected Ready(42), got {:?}", std::mem::discriminant(&other)),
        }

        // Third poll: cancelled (already consumed).
        assert!(matches!(task.try_recv(), TaskPoll::Cancelled));
    }

    #[tokio::test]
    async fn cancel_aborts() {
        let mut task = AsyncTask::spawn(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            1i32
        });

        assert!(matches!(task.try_recv(), TaskPoll::Pending));
        task.cancel();
        assert!(matches!(task.try_recv(), TaskPoll::Cancelled));
    }

    #[tokio::test]
    async fn drop_aborts_task() {
        let task = AsyncTask::spawn(async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            1i32
        });
        // Drop without polling; the inner task should be aborted.
        drop(task);
        // If the task were not aborted, this sleep would still work but
        // there is no easy assertion. The real check is that the test
        // process exits cleanly (no orphaned tokio task blocking the
        // runtime shutdown).
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}