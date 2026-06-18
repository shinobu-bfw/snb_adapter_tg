use std::future::Future;

/// Spawn a fire-and-forget async task. If a Tokio runtime is already running on
/// this thread (the adapter's dispatcher runtime), spawn onto it; otherwise run
/// the future to completion on a fresh OS thread via the adapter helper.
pub(crate) fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(future);
    } else {
        std::thread::spawn(move || snb_core::adapter::run_async(future));
    }
}
