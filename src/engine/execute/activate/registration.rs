use std::marker::PhantomData;

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{ErrorReportExt as _, ReportScope, SubReportScope},
    },
    engine::execute::support::CleanupTracker,
    package::{PackageFont, PackageId},
    platform::windows::steps::{
        registration,
        unregistration::{self, UnregistrationIntent},
    },
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct RegistrationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for RegistrationScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = RegistrationNoticeReport;
    type WarnReportValue = S::WarnReportValue;
    type ErrorReportValue = S::ErrorReportValue;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for RegistrationScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum RegistrationNoticeReport {
    #[snafu(display(
        concat_line!(
            "failed to roll back registration of package fonts after activation failure",
            "run `foton repair {pkg_id}` to retry cleanup",
            "if repair does not resolve the problem, manual cleanup may be required",
        ),
        pkg_id = pkg_id,
    ))]
    RollbackRegistrationAfterActivationFailure { pkg_id: PackageId },
}

pub(super) fn register_package_fonts<S, I>(
    cx: &ReportContext<S>,
    cleanup_tracker: CleanupTracker,
    pkg_id: &PackageId,
    pkg_fonts: I,
) -> Result<RegistrationGuard<S>, S::Error>
where
    S: ReportScope,
    I: IntoIterator<Item = PackageFont>,
{
    let cx = RegistrationScope::start(cx);
    let mut guard = RegistrationGuard {
        armed: true,
        registered: false,
        cx: cx.clone(),
        cleanup_tracker,
        pkg_id: pkg_id.clone(),
    };
    registration::register_package_fonts(&cx, pkg_id, pkg_fonts)?;
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
    cx: ReportContext<RegistrationScope<S>>,
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
        if self
            .cleanup_tracker
            .do_rollback_cleanup(|| {
                unregistration::unregister_package_fonts(
                    &self.cx,
                    &self.pkg_id,
                    UnregistrationIntent::RollbackAfterActivationFailure,
                )
            })
            .is_err()
        {
            RollbackRegistrationAfterActivationFailureSnafu {
                pkg_id: self.pkg_id.clone(),
            }
            .build()
            .report_notice(&self.cx);
        }
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
    use std::sync::LazyLock;

    use super::*;
    use crate::util::testing;

    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| "example-font@0.1.0".parse().unwrap());

    #[test]
    fn dropping_unregistered_guard_requests_cleanup() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();

            let guard = RegistrationGuard {
                armed: true,
                registered: false,
                cx: RegistrationScope::start(cx),
                cleanup_tracker: cleanup_tracker.clone(),
                pkg_id: PKG_ID.clone(),
            };
            drop(guard);

            assert!(cleanup_tracker.cleanup_required());
        });
    }

    #[test]
    fn disarming_unregistered_guard_does_not_request_cleanup() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();

            let guard = RegistrationGuard {
                armed: true,
                registered: false,
                cx: RegistrationScope::start(cx),
                cleanup_tracker: cleanup_tracker.clone(),
                pkg_id: PKG_ID.clone(),
            };
            guard.disarm();

            assert!(!cleanup_tracker.cleanup_required());
        });
    }

    #[test]
    #[cfg_attr(
        not(build_for_sandbox),
        ignore = "registry and session should be isolated in sandbox tests. use `cargo xtask sandbox run --test` instead."
    )]
    fn dropping_registered_guard_keeps_cleanup_clear_when_unregister_succeeds() {
        testing::with_context(|cx| {
            let cleanup_tracker = CleanupTracker::default();

            let guard = RegistrationGuard {
                armed: true,
                registered: true,
                cx: RegistrationScope::start(cx),
                cleanup_tracker: cleanup_tracker.clone(),
                pkg_id: PKG_ID.clone(),
            };
            drop(guard);

            assert!(!cleanup_tracker.cleanup_required());
        });
    }
}
