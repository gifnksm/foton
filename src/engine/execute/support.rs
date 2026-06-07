use std::sync::{Arc, Mutex};

// Tracks whether a failed install/activate can be treated as cleanly rolled back.
// If any step may have left registry or filesystem state behind, the DB entry
// should stay recoverable as `IncompleteUninstall`/`IncompleteDeactivation` instead
// of being removed.
#[derive(Debug, Default, Clone)]
pub(in crate::engine::execute) struct CleanupTracker {
    inner: Arc<Mutex<CleanupTrackerInner>>,
}

#[derive(Debug, Default)]
struct CleanupTrackerInner {
    base_cleanup_failed: bool,
    rollback_cleanup_failed: bool,
    cleanup_requested: bool,
}

impl CleanupTracker {
    pub(in crate::engine::execute) fn cleanup_required(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.base_cleanup_failed || inner.rollback_cleanup_failed || inner.cleanup_requested
    }

    pub(in crate::engine::execute) fn request_cleanup(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.cleanup_requested = true;
    }

    pub(in crate::engine::execute) fn do_base_cleanup<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let res = f();
        if res.is_err() {
            self.inner.lock().unwrap().base_cleanup_failed = true;
        }
        res
    }

    pub(in crate::engine::execute) fn do_rollback_cleanup<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        let res = f();
        if res.is_err() {
            self.inner.lock().unwrap().rollback_cleanup_failed = true;
        }
        res
    }
}
