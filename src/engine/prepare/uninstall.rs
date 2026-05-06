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
    engine::{Execution, ResolvedUninstallTarget, UninstallExecution},
    package::PackageId,
};

#[derive(Debug)]
struct UninstallPrepareScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallPrepareScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallPrepareErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UninstallPrepareScope<S>
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
enum UninstallPrepareErrorReport {
    #[snafu(display("failed to prepare uninstall state for package {pkg_id}"))]
    PrepareUninstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<UninstallPrepareErrorReport> for ReportValue<'static> {
    fn from(report: UninstallPrepareErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn prepare_uninstall<'db, S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    targets: &[ResolvedUninstallTarget],
) -> Result<Vec<Execution<'db, S>>, S::Error>
where
    S: ReportScope,
{
    let cx = UninstallPrepareScope::start(cx);
    let mut executions = vec![];

    for target in targets {
        let pkg_id = target.pkg_id.clone();
        db.lock()
            .unwrap()
            .apply_uninstall_transaction(&target.pkg_id)
            .with_context(|_| PrepareUninstallSnafu {
                pkg_id: &target.pkg_id,
            })
            .report_error(cx.reporter())?;
        executions.push(UninstallExecution::new(Arc::clone(db), pkg_id).into());
    }

    Ok(executions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        package::PackageState,
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn prepare_uninstall_marks_targets_pending_uninstall() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let db = Arc::new(Mutex::new(db));
            let targets = vec![ResolvedUninstallTarget {
                pkg_id: pkg_id.clone(),
                current_state: PackageState::Installed,
            }];

            let executions = prepare_uninstall(&cx, &db, &targets).unwrap();

            assert_eq!(executions.len(), 1);
            assert!(matches!(&executions[0], Execution::Uninstall(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::PendingUninstall),
            );
        });
    }
}
