use std::marker::PhantomData;

use async_trait::async_trait;
use snafu::IntoError as _;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, SubReportScope},
    },
    db::{PackageDatabase, PackageDatabaseError, PackageDatabaseTransaction},
    engine::{
        ActivateOp,
        execute::{ExecuteErrorReport, PreparedStepOp, support::CleanupTracker},
    },
    package::PackageId,
    platform::windows::steps::unregistration::{self, UnregistrationIntent},
};

mod registration;

#[derive(Debug, Default)]
struct ActivateExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ActivateExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ActivateExecutionScope<S> where S: ReportScope {}

#[derive(Debug)]
pub(in crate::engine) struct PreparedActivateStep<S>
where
    S: ReportScope,
{
    pkg_id: PackageId,
    cleanup_tracker: CleanupTracker,
    registration_guard: Option<registration::RegistrationGuard<ActivateExecutionScope<S>>>,
    activation_persisted: bool,
}

impl<S> PreparedActivateStep<S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn from_plan_step(
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: ActivateOp,
    ) -> Self {
        let ActivateOp { pkg_id, .. } = step;

        tx.begin_activate(&pkg_id).unwrap();
        Self {
            pkg_id,
            cleanup_tracker: CleanupTracker::default(),
            registration_guard: None,
            activation_persisted: false,
        }
    }
}

#[async_trait]
impl<S> PreparedStepOp<S> for PreparedActivateStep<S>
where
    S: ReportScope,
{
    async fn execute(
        &mut self,
        cx: &ReportContext<S>,
        db: &PackageDatabase<'_>,
    ) -> Result<(), S::Error> {
        let pkg_id = &self.pkg_id;
        let cx =
            ActivateExecutionScope::start_with_report(cx, format_args!("Activating {pkg_id}..."));
        let entry = db.entry_by_id(pkg_id).unwrap();

        self.cleanup_tracker.do_base_cleanup(|| {
            unregistration::unregister_package_fonts(
                &cx,
                pkg_id,
                UnregistrationIntent::CleanupBeforeActivation,
            )?;
            Ok(())
        })?;

        self.registration_guard = Some(registration::register_package_fonts(
            &cx,
            self.cleanup_tracker.clone(),
            pkg_id,
            entry.fonts(),
        )?);

        Ok(())
    }

    fn on_complete(
        &mut self,
        _cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        tx.complete_activate(&self.pkg_id).unwrap();
        Ok(())
    }

    fn after_commit(&mut self) {
        if let Some(guard) = self.registration_guard.take() {
            guard.disarm();
        }
        self.activation_persisted = true;
    }

    fn on_failure(&mut self, cx: &ReportContext<S>, tx: &mut PackageDatabaseTransaction<'_, '_>) {
        assert!(!self.activation_persisted);

        let cx = ActivateExecutionScope::start(cx);
        cx.reporter()
            .report_info(format_args!("rolling back activation changes..."));

        let _ = self.registration_guard.take();

        let cleanup_required = self.cleanup_tracker.cleanup_required();
        tx.cancel_activate(&self.pkg_id, cleanup_required).unwrap();
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        super::CommitActivateTransactionSnafu {
            pkg_id: &self.pkg_id,
        }
        .into_error(source)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::{
        engine::ActivateReason,
        package::{ActivationState, PackageDefinition},
        util::testing::{self, TestScope},
    };

    static PKG: LazyLock<PackageDefinition> =
        LazyLock::new(|| testing::make_package_definition("example-font@0.1.0"));

    fn activate_op() -> ActivateOp {
        ActivateOp {
            pkg_id: PKG.id.clone(),
            reason: ActivateReason::RequestedByUser,
        }
    }

    #[test]
    fn prepared_activate_step_from_plan_step_begins_incomplete_activation_in_transaction() {
        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &PKG);

            let mut tx = db.transaction();
            let step: PreparedActivateStep<TestScope> =
                PreparedActivateStep::from_plan_step(&mut tx, activate_op());
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::IncompleteActivation,
            );
            assert_eq!(step.pkg_id, PKG.id);
        });
    }

    #[test]
    fn prepared_activate_step_on_complete_marks_entry_active() {
        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let mut tx = db.transaction();
            let mut step = PreparedActivateStep::from_plan_step(&mut tx, activate_op());

            step.on_complete(cx, &mut tx).unwrap();
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::Active,
            );
        });
    }

    #[test]
    fn prepared_activate_step_on_failure_restores_inactive_when_cleanup_is_not_required() {
        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let mut tx = db.transaction();
            let mut step = PreparedActivateStep::from_plan_step(&mut tx, activate_op());

            step.on_failure(cx, &mut tx);
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::Inactive,
            );
        });
    }

    #[test]
    fn prepared_activate_step_on_failure_marks_incomplete_deactivation_when_cleanup_is_required() {
        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let mut tx = db.transaction();
            let mut step = PreparedActivateStep::from_plan_step(&mut tx, activate_op());
            step.cleanup_tracker.request_cleanup();

            step.on_failure(cx, &mut tx);
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::IncompleteDeactivation,
            );
        });
    }
}
