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
    db::{InstallTransactionResult, PackageDatabase, PackageDatabaseError},
    engine::{Execution, InstallExecution, ResolvedInstallTarget, UninstallExecution},
    package::PackageId,
};

#[derive(Debug)]
struct InstallPrepareScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallPrepareScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallPrepareErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallPrepareScope<S>
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
enum InstallPrepareErrorReport {
    #[snafu(display("failed to prepare install state for package {pkg_id}"))]
    PrepareInstall {
        pkg_id: PackageId,
        source: PackageDatabaseError,
    },
}

impl From<InstallPrepareErrorReport> for ReportValue<'static> {
    fn from(report: InstallPrepareErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn prepare_install<'db, S>(
    cx: &ReportContext<S>,
    db: &Arc<Mutex<PackageDatabase<'db>>>,
    targets: &[ResolvedInstallTarget],
) -> Result<Vec<Execution<'db, S>>, S::Error>
where
    S: ReportScope,
{
    let outer_cx = cx;
    let cx = InstallPrepareScope::start(cx);
    let mut executions = vec![];

    for target in targets {
        let InstallTransactionResult { pending_uninstalls } = db
            .lock()
            .unwrap()
            .apply_install_transaction(&target.manifest)
            .with_context(|_| PrepareInstallSnafu {
                pkg_id: target.manifest.metadata.id(),
            })
            .report_error(cx.reporter())?;
        for pkg_id in pending_uninstalls {
            executions.push(UninstallExecution::new(Arc::clone(db), pkg_id).into());
        }
        executions.push(
            InstallExecution::new(outer_cx, Arc::clone(db), Arc::clone(&target.manifest)).into(),
        );
    }

    Ok(executions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        package::PackageState,
        registry::RegistryId,
        util::testing::{self, TempdirContext, TestScope},
    };

    #[test]
    fn prepare_install_schedules_cleanup_before_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let old_manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let old_pkg_id = old_manifest.metadata.id();
        let new_manifest = testing::make_manifest("example-namespace/example-font@0.2.0");
        let new_pkg_id = new_manifest.metadata.id();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &old_manifest);
            let db = Arc::new(Mutex::new(db));
            let targets = vec![ResolvedInstallTarget {
                reg_id: RegistryId::new("test-registry").unwrap(),
                manifest: Arc::clone(&new_manifest),
            }];

            let executions = prepare_install(&cx, &db, &targets).unwrap();

            assert_eq!(executions.len(), 2);
            assert!(matches!(&executions[0], Execution::Uninstall(_)));
            assert!(matches!(&executions[1], Execution::Install(_)));
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&old_pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::PendingUninstall),
            );
            assert_eq!(
                db.lock()
                    .unwrap()
                    .entry_by_id(&new_pkg_id)
                    .map(|(state, _)| state),
                Some(PackageState::PendingInstall),
            );
        });
    }
}
