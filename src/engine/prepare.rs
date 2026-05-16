use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::{PackageDatabase, PackageDatabaseError},
    engine::{
        Execution, ExecutionPlan, ExecutionPlanOp, InstallExecution, InstallOp, UninstallExecution,
        UninstallOp,
    },
};

#[derive(Debug)]
struct PrepareScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for PrepareScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = PrepareErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for PrepareScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

#[derive(Debug, Snafu)]
enum PrepareErrorReport {
    #[snafu(display("failed to apply execution plan transaction"))]
    ApplyTransaction { source: PackageDatabaseError },
}

impl From<PrepareErrorReport> for ReportValue<'static> {
    fn from(report: PrepareErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug)]
pub(crate) struct PreparedExecutions<'db, S>
where
    S: ReportScope,
{
    executions: Vec<Execution<'db, S>>,
}

impl<'db, S> PreparedExecutions<'db, S>
where
    S: ReportScope,
{
    pub(in crate::engine) fn into_executions(self) -> Vec<Execution<'db, S>> {
        self.executions
    }
}

pub(crate) fn prepare<'db, S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    plan: &ExecutionPlan,
) -> Result<PreparedExecutions<'db, S>, S::Error>
where
    S: ReportScope,
{
    let outer_cx = cx;
    let cx = PrepareScope::start(cx);
    let mut executions = vec![];

    db.lock()
        .unwrap()
        .apply_plan_transaction(plan)
        .context(ApplyTransactionSnafu)
        .report_error(cx.reporter())?;

    for op in plan.ops() {
        let execution = match op {
            ExecutionPlanOp::Install(InstallOp { manifest, .. }) => {
                InstallExecution::new(outer_cx, Arc::clone(db), Arc::clone(manifest)).into()
            }
            ExecutionPlanOp::Uninstall(UninstallOp { pkg_id, .. }) => {
                UninstallExecution::new(Arc::clone(db), pkg_id.clone()).into()
            }
            ExecutionPlanOp::Skip(_) => continue,
        };
        executions.push(execution);
    }

    Ok(PreparedExecutions { executions })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        engine::{SkipOp, SkipReason, UninstallReason},
        package::{PackageId, PackageState},
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn prepare_applies_plan_transaction_and_skips_skip_ops() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let install_manifest = testing::make_manifest("install-font@0.1.0");
        let install_pkg_id = install_manifest.id();
        let uninstall_manifest = testing::make_manifest("uninstall-font@0.1.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &uninstall_manifest);
            let db = Arc::new(Mutex::new(db));
            let plan = ExecutionPlan::new_for_test([
                InstallOp {
                    manifest: Arc::clone(&install_manifest),
                    reason: crate::engine::InstallReason::RequestedByUser,
                }
                .into(),
                UninstallOp {
                    pkg_id: uninstall_pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                }
                .into(),
                SkipOp {
                    pkg_spec: skipped_pkg_id.clone().into(),
                    reason: SkipReason::AlreadyInstalled,
                }
                .into(),
            ]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert_eq!(prepared_plan.executions.len(), 2);
            assert!(
                prepared_plan
                    .executions
                    .iter()
                    .any(|execution| matches!(execution, Execution::Install(_)))
            );
            assert!(
                prepared_plan
                    .executions
                    .iter()
                    .any(|execution| matches!(execution, Execution::Uninstall(_)))
            );
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&install_pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::PendingInstall),
            );
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&uninstall_pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::PendingUninstall),
            );
            assert!(db.lock().unwrap().entry_by_id(&skipped_pkg_id).is_none());
        });
    }

    #[test]
    fn prepare_returns_no_executions_for_skip_only_plan() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let plan = ExecutionPlan::new_for_test([SkipOp {
                pkg_spec: skipped_pkg_id.clone().into(),
                reason: SkipReason::AlreadyInstalled,
            }
            .into()]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert!(prepared_plan.executions.is_empty());
            assert!(db.lock().unwrap().entry_by_id(&skipped_pkg_id).is_none());
        });
    }
}
