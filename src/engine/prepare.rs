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
        Execution, ExecutionPlan, ExecutionPlanOp, ExecutionResult, InstallExecution, InstallOp,
        UninstallExecution, UninstallOp,
    },
    package::PackageId,
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
    pub(in crate::engine) fn pop_executable(&mut self) -> Option<Execution<'db, S>> {
        if let Some(idx) = self.executions.iter().position(Execution::can_execute) {
            Some(self.executions.remove(idx))
        } else {
            None
        }
    }

    pub(in crate::engine) fn notify_result(&mut self, pkg_id: &PackageId, result: ExecutionResult) {
        for exec in &mut self.executions {
            exec.notify_result(pkg_id, result);
        }
    }

    pub(in crate::engine) fn len(&self) -> usize {
        self.executions.len()
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
            ExecutionPlanOp::Install(InstallOp {
                manifest,
                replacing_pkg_ids,
                ..
            }) => InstallExecution::new(
                outer_cx,
                Arc::clone(db),
                Arc::clone(manifest),
                replacing_pkg_ids.clone(),
            )
            .into(),
            ExecutionPlanOp::Uninstall(UninstallOp {
                pkg_id, conditions, ..
            }) => {
                UninstallExecution::new(Arc::clone(db), pkg_id.clone(), conditions.clone()).into()
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
        engine::{
            ExecutionCondition, ExecutionResult, InstallReason, SkipOp, SkipReason,
            UninstallExecution, UninstallReason,
        },
        package::{PackageId, PackageState},
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn prepare_applies_plan_transaction_to_unconditional_ops_and_skips_skip_ops() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let install_manifest = testing::make_manifest("install-font@0.1.0");
        let install_pkg_id = install_manifest.id();
        let uninstall_manifest = testing::make_manifest("uninstall-font@0.1.0");
        let uninstall_pkg_id = uninstall_manifest.id();
        let conditional_uninstall_manifest =
            testing::make_manifest("conditional-uninstall-font@0.1.0");
        let conditional_uninstall_pkg_id = conditional_uninstall_manifest.id();
        let skipped_pkg_id = "skipped-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &uninstall_manifest);
            testing::mark_as_installed(&mut db, &conditional_uninstall_manifest);
            let db = Arc::new(Mutex::new(db));
            let plan = ExecutionPlan::new_for_test([
                InstallOp {
                    manifest: Arc::clone(&install_manifest),
                    reason: InstallReason::RequestedByUser,
                    replacing_pkg_ids: vec![conditional_uninstall_pkg_id.clone()],
                }
                .into(),
                UninstallOp {
                    pkg_id: uninstall_pkg_id.clone(),
                    reason: UninstallReason::RequestedByUser,
                    conditions: vec![],
                }
                .into(),
                UninstallOp {
                    pkg_id: conditional_uninstall_pkg_id.clone(),
                    reason: UninstallReason::ConflictWithInstall {
                        pkg_id: install_pkg_id.clone(),
                    },
                    conditions: vec![ExecutionCondition::AfterSuccess(install_pkg_id.clone())],
                }
                .into(),
                SkipOp {
                    pkg_spec: skipped_pkg_id.clone().into(),
                    reason: SkipReason::AlreadyInstalled,
                }
                .into(),
            ]);

            let prepared_plan = prepare(&cx, &db, &plan).unwrap();

            assert_eq!(prepared_plan.executions.len(), 3);
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
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&conditional_uninstall_pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::Installed),
            );
            assert!(db.lock().unwrap().entry_by_id(&skipped_pkg_id).is_none());
        });
    }

    #[test]
    fn prepared_executions_unblocks_dependent_execution_after_success() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_pkg_id = "dependency-font@0.1.0".parse::<PackageId>().unwrap();
        let dependent_pkg_id = "dependent-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let blocked_execution: Execution<'_, TestScope> = UninstallExecution::new(
                Arc::clone(&db),
                dependent_pkg_id.clone(),
                vec![ExecutionCondition::AfterSuccess(dependency_pkg_id.clone())],
            )
            .into();
            let ready_execution: Execution<'_, TestScope> =
                UninstallExecution::new(Arc::clone(&db), dependency_pkg_id.clone(), vec![]).into();
            let mut prepared = PreparedExecutions {
                executions: vec![blocked_execution, ready_execution],
            };

            let first = prepared.pop_executable().unwrap();
            assert!(
                matches!(first, Execution::Uninstall(exec) if exec.target_id() == dependency_pkg_id)
            );

            prepared.notify_result(&dependency_pkg_id, ExecutionResult::Success);

            let second = prepared.pop_executable().unwrap();
            assert!(
                matches!(second, Execution::Uninstall(exec) if exec.target_id() == dependent_pkg_id)
            );
            assert!(prepared.pop_executable().is_none());
        });
    }

    #[test]
    fn prepared_executions_keep_dependent_execution_blocked_after_failure() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let dependency_pkg_id = "dependency-font@0.1.0".parse::<PackageId>().unwrap();
        let dependent_pkg_id = "dependent-font@0.1.0".parse::<PackageId>().unwrap();

        testing::with_db(&cx, |db| {
            let db = Arc::new(Mutex::new(db));
            let blocked_execution: Execution<'_, TestScope> = UninstallExecution::new(
                Arc::clone(&db),
                dependent_pkg_id,
                vec![ExecutionCondition::AfterSuccess(dependency_pkg_id.clone())],
            )
            .into();
            let ready_execution: Execution<'_, TestScope> =
                UninstallExecution::new(Arc::clone(&db), dependency_pkg_id.clone(), vec![]).into();
            let mut prepared = PreparedExecutions {
                executions: vec![blocked_execution, ready_execution],
            };

            let first = prepared.pop_executable().unwrap();
            assert!(
                matches!(first, Execution::Uninstall(exec) if exec.target_id() == dependency_pkg_id)
            );

            prepared.notify_result(&dependency_pkg_id, ExecutionResult::Failure);

            assert!(prepared.pop_executable().is_none());
            assert_eq!(prepared.len(), 1);
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
