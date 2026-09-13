//! Running async work from the synchronous storage traits.
//!
//! The repository traits are synchronous (the local adapter is), and the AWS SDK is not. The
//! AWS adapter is therefore written the way it wants to be - `async fn` methods that await the
//! SDK - and this bridges the two: the sync trait methods hand the async work to a runtime on
//! its own thread and wait for the answer.
//!
//! It is deliberately small and temporary. When the traits themselves become async
//! (`doc/aws-plan.md`, option A) this module and the one-line wrappers that use it go away; the
//! async implementation they call does not change.
//!
//! Waiting inside an async runtime would panic (`block_on` from within a runtime), which is why
//! the work runs on a thread of its own rather than on the caller's.

use std::future::Future;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

/// A job for the runtime thread: a future, run against the runtime that thread owns.
type Job = Box<dyn FnOnce(&tokio::runtime::Runtime) + Send>;

/// A tokio runtime on a dedicated thread, with a blocking call into it.
#[derive(Clone)]
pub struct BlockingRuntime {
    sender: Sender<Job>,
}

impl BlockingRuntime {
    pub fn new() -> Self {
        let (sender, jobs) = channel::<Job>();
        std::thread::Builder::new()
            .name("aws-runtime".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build the runtime for the AWS adapter");
                // Runs until every sender is dropped, i.e. for the life of the adapter.
                while let Ok(job) = jobs.recv() {
                    job(&runtime);
                }
            })
            .expect("failed to start the thread for the AWS adapter");
        BlockingRuntime { sender }
    }

    /// Run `future` on that runtime and wait for its result.
    ///
    /// The future owns everything it touches (`Send + 'static`): it runs on another thread, so
    /// it cannot borrow from the caller.
    pub fn block_on<F, T>(&self, future: F) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (sender, answer) = channel::<T>();
        let sender = Mutex::new(sender);
        let job: Job = Box::new(move |runtime: &tokio::runtime::Runtime| {
            let result = runtime.block_on(future);
            // A closed receiver means the caller panicked; nothing to report to anyone.
            if let Ok(sender) = sender.into_inner() {
                let _ = sender.send(result);
            }
        });
        self.sender
            .send(job)
            .expect("the AWS adapter's runtime thread is gone");
        answer
            .recv()
            .expect("the AWS adapter's runtime thread did not answer")
    }
}

impl Default for BlockingRuntime {
    fn default() -> Self {
        BlockingRuntime::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bridge works from inside a runtime as well, which is where every caller lives: an
    /// axum handler runs on the application's runtime, and waiting there directly would panic.
    #[tokio::test]
    async fn waits_for_a_future_from_inside_a_runtime() {
        let runtime = BlockingRuntime::new();
        let answer = runtime.block_on(async { 1 + 1 });
        assert_eq!(answer, 2);
    }

    #[test]
    fn waits_for_a_future_outside_a_runtime() {
        let runtime = BlockingRuntime::new();
        assert_eq!(runtime.block_on(async { "done" }), "done");
    }

    /// An owned value moves to the other thread and its result comes back.
    #[test]
    fn moves_values_across_the_thread() {
        let runtime = BlockingRuntime::new();
        let text = String::from("payload");
        let length = runtime.block_on(async move { text.len() });
        assert_eq!(length, 7);
    }
}
