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
        DeactivateOp,
        execute::{ExecuteErrorReport, PreparedStepOp},
    },
    package::PackageId,
    platform::windows::steps::unregistration::{self, UnregistrationIntent},
};

#[derive(Debug, Default)]
struct DeactivateExecutionScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DeactivateExecutionScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DeactivateExecutionScope<S> where S: ReportScope {}

#[derive(Debug)]
pub(in crate::engine) struct PreparedDeactivateStep {
    pkg_id: PackageId,
}

impl PreparedDeactivateStep {
    pub(in crate::engine) fn from_plan_step(
        tx: &mut PackageDatabaseTransaction<'_, '_>,
        step: DeactivateOp,
    ) -> Self {
        let DeactivateOp { pkg_id, .. } = step;

        tx.begin_deactivate(&pkg_id).unwrap();
        Self { pkg_id }
    }
}

#[async_trait]
impl<S> PreparedStepOp<S> for PreparedDeactivateStep
where
    S: ReportScope,
{
    async fn execute(
        &mut self,
        cx: &ReportContext<S>,
        _db: &PackageDatabase<'_>,
    ) -> Result<(), S::Error> {
        let pkg_id = &self.pkg_id;

        let cx = DeactivateExecutionScope::start_with_report(
            cx,
            format_args!("Deactivating {pkg_id}..."),
        );

        unregistration::unregister_package_fonts(
            &cx,
            pkg_id,
            UnregistrationIntent::DeactivationStep,
        )?;

        Ok(())
    }

    fn on_complete(
        &mut self,
        _cx: &ReportContext<S>,
        tx: &mut PackageDatabaseTransaction<'_, '_>,
    ) -> Result<(), S::Error> {
        tx.complete_deactivate(&self.pkg_id).unwrap();
        Ok(())
    }

    fn after_commit(&mut self) {}

    fn on_failure(&mut self, _cx: &ReportContext<S>, _tx: &mut PackageDatabaseTransaction<'_, '_>) {
    }

    fn commit_error_report(&self, source: PackageDatabaseError) -> ExecuteErrorReport {
        super::CommitDeactivateTransactionSnafu {
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
        engine::DeactivateReason,
        package::{ActivationState, PackageDefinition},
        util::testing,
    };

    static PKG: LazyLock<PackageDefinition> =
        LazyLock::new(|| testing::make_package_definition("example-font@0.1.0"));

    fn deactivate_op() -> DeactivateOp {
        DeactivateOp {
            pkg_id: PKG.id.clone(),
            reason: DeactivateReason::RequestedByUser,
        }
    }

    #[test]
    fn prepared_deactivate_step_from_plan_step_begins_incomplete_deactivation_in_transaction() {
        testing::with_db(|_cx, db| {
            testing::mark_as_active(db, &PKG);

            let mut tx = db.transaction();
            let step = PreparedDeactivateStep::from_plan_step(&mut tx, deactivate_op());
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::IncompleteDeactivation,
            );
            assert_eq!(step.pkg_id, PKG.id);
        });
    }

    #[test]
    fn prepared_deactivate_step_on_complete_marks_entry_inactive() {
        testing::with_db(|cx, db| {
            testing::mark_as_active(db, &PKG);

            let mut tx = db.transaction();
            let mut step = PreparedDeactivateStep::from_plan_step(&mut tx, deactivate_op());

            step.on_complete(cx, &mut tx).unwrap();
            tx.commit().unwrap();

            assert_eq!(
                db.entry_by_id(&PKG.id).unwrap().activation_state(),
                ActivationState::Inactive,
            );
        });
    }
}
