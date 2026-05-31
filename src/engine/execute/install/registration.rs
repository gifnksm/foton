use crate::{
    cli::{context::ReportContext, reporter::ReportScope},
    engine::execute::install::CleanupTracker,
    package::{Package, PackageId},
    platform::windows::steps::{registration, unregistration},
};

pub(super) fn register_package_fonts<S>(
    cx: &ReportContext<S>,
    cleanup_tracker: CleanupTracker,
    package: &Package,
) -> Result<RegistrationGuard<S>, S::Error>
where
    S: ReportScope,
{
    let mut guard = RegistrationGuard {
        armed: true,
        registered: false,
        cx: cx.clone(),
        cleanup_tracker,
        pkg_id: package.id().clone(),
    };
    registration::register_package_fonts(cx, package)?;
    guard.registered = true;
    Ok(guard)
}

#[must_use]
#[derive(Debug)]
pub(super) struct RegistrationGuard<S>
where
    S: ReportScope,
{
    armed: bool,
    registered: bool,
    cx: ReportContext<S>,
    cleanup_tracker: CleanupTracker,
    pkg_id: PackageId,
}

impl<S> Drop for RegistrationGuard<S>
where
    S: ReportScope,
{
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if !self.registered {
            // Registration may fail after creating some persistent registry state.
            // If full registration did not complete, keep the package marked for
            // cleanup instead of assuming rollback is unnecessary.
            self.cleanup_tracker.request_cleanup();
            return;
        }
        self.cx.reporter().report_info(format_args!(
            "rolling back registration of package fonts..."
        ));
        let _ = self.cleanup_tracker.do_rollback_cleanup(|| {
            unregistration::unregister_package_fonts(&self.cx, &self.pkg_id)
        });
    }
}

impl<S> RegistrationGuard<S>
where
    S: ReportScope,
{
    pub(super) fn disarm(mut self) {
        self.armed = false;
    }
}

#[cfg(test)]
mod tests {
    use std::{
        process,
        sync::{
            LazyLock,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{TempdirContext, TestScope},
    };

    fn test_app_id() -> String {
        static TEST_ID: AtomicUsize = AtomicUsize::new(0);
        format!(
            "io.github.gifnksm.foton.test.install-registration.{}.{}",
            process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        )
    }

    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| "example-font@0.1.0".parse().unwrap());

    #[test]
    fn dropping_unregistered_guard_requests_cleanup() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cleanup_tracker = CleanupTracker::default();

        let guard = RegistrationGuard {
            armed: true,
            registered: false,
            cx: cx.clone(),
            cleanup_tracker: cleanup_tracker.clone(),
            pkg_id: PKG_ID.clone(),
        };
        drop(guard);

        assert!(cleanup_tracker.cleanup_required());
    }

    #[test]
    fn disarming_unregistered_guard_does_not_request_cleanup() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cleanup_tracker = CleanupTracker::default();

        let guard = RegistrationGuard {
            armed: true,
            registered: false,
            cx: cx.clone(),
            cleanup_tracker: cleanup_tracker.clone(),
            pkg_id: PKG_ID.clone(),
        };
        guard.disarm();

        assert!(!cleanup_tracker.cleanup_required());
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry and session should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn dropping_registered_guard_keeps_cleanup_clear_when_unregister_succeeds() {
        let cx = TempdirContext::with_app_id(test_app_id());
        let cx = TestScope::start(&cx);
        let cleanup_tracker = CleanupTracker::default();

        let guard = RegistrationGuard {
            armed: true,
            registered: true,
            cx: cx.clone(),
            cleanup_tracker: cleanup_tracker.clone(),
            pkg_id: PKG_ID.clone(),
        };
        drop(guard);

        assert!(!cleanup_tracker.cleanup_required());
    }
}
