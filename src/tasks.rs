//! The background tasks that must outlive every request.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// A task spawned and never looked at again ends without a word: the sweeps stop, the
/// subscriptions never expire, and the only symptom is an answer that never changes.
#[derive(Clone, Default)]
pub struct Watched {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    gone: Mutex<Vec<&'static str>>,
    leaving: AtomicBool,
}

impl Watched {
    /// Runs a task that is meant to last as long as the server does.
    pub fn spawn(
        &self,
        name: &'static str,
        task: impl Future<Output = ()> + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        let watched = self.clone();
        tokio::spawn(async move {
            let ended = tokio::spawn(task).await;
            watched.ended(name, ended.err());
        })
    }

    /// Said before the tasks are asked to stop, so a shutdown is not read as a fault.
    pub fn leaving(&self) {
        self.inner.leaving.store(true, Ordering::Relaxed);
    }

    /// The tasks that stopped while the server was still meant to be running them.
    pub fn gone(&self) -> Vec<&'static str> {
        crate::held(&self.inner.gone).clone()
    }

    fn ended(&self, name: &'static str, error: Option<tokio::task::JoinError>) {
        if self.inner.leaving.load(Ordering::Relaxed)
            || error
                .as_ref()
                .is_some_and(tokio::task::JoinError::is_cancelled)
        {
            return;
        }
        match error {
            Some(_) => {
                tracing::error!(task = name, "a background task panicked and is not running")
            }
            None => tracing::error!(task = name, "a background task ended and is not running"),
        }
        crate::held(&self.inner.gone).push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough for the watcher to have joined a task that ended at once.
    async fn settle() {
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn a_task_that_panics_is_named() {
        let watched = Watched::default();
        watched.spawn("sweeps", async { panic!("the pass loop fell over") });
        settle().await;
        assert_eq!(watched.gone(), vec!["sweeps"]);
    }

    #[tokio::test]
    async fn a_task_that_returns_is_named_too() {
        let watched = Watched::default();
        watched.spawn("sweeps", async {});
        settle().await;
        assert_eq!(
            watched.gone(),
            vec!["sweeps"],
            "a loop that must never end returning is the same fault as one that panics"
        );
    }

    #[tokio::test]
    async fn a_task_ending_on_the_way_out_is_not_a_fault() {
        let watched = Watched::default();
        watched.leaving();
        watched.spawn("ssdp", async {});
        settle().await;
        assert!(
            watched.gone().is_empty(),
            "every task ends at shutdown, and none of them is news"
        );
    }
}
