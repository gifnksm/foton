use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use snafu::Snafu;

use crate::{
    cli::{
        args::{InstallArgs, InstallTargets},
        context::{ReportContext, RootContext},
        reporter::{NeverReport, OperationError, ReportScope, RootReportScope},
    },
    db::PackageDatabase,
    engine::{self, ResolvedInstallTarget},
    package::{PackageManifest, PackageSpec},
    registry::RegistrySpec,
};

#[derive(Debug)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = InstallError;
}

impl RootReportScope for InstallScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum InstallError {
    #[snafu(display("failed to install package(s); see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for InstallError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn install_package(
    cx: &RootContext,
    args: &InstallArgs,
) -> Result<(), InstallError> {
    let InstallArgs { target } = args;
    let targets = target.to_targets();

    let cx = InstallScope::start_with_report(
        cx,
        format_args!("Installing {} package(s)...", targets.len()),
    );

    let targets = CheckedInstallTargets::try_from_args(&cx, targets)?;

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = targets.resolve_target(&cx, &db)?;
    let plan = engine::plan_install(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already installed, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    let db = Arc::new(Mutex::new(db));
    let prepared_plan = engine::prepare(&cx, &db, &plan)?;
    engine::execute_plan(&cx, prepared_plan).await?;

    Ok(())
}

enum CheckedInstallTargets {
    Manifest {
        manifests: Vec<(PathBuf, Arc<PackageManifest>)>,
    },
    PackageSpec {
        registries: Vec<RegistrySpec>,
        pkg_specs: Vec<PackageSpec>,
    },
}

impl CheckedInstallTargets {
    fn try_from_args(
        cx: &ReportContext<InstallScope>,
        target: InstallTargets,
    ) -> Result<Self, InstallError> {
        match target {
            InstallTargets::Manifest { manifests } => {
                let manifests = engine::resolve_manifests(cx, &manifests)?;
                Ok(Self::Manifest { manifests })
            }
            InstallTargets::PackageSpec {
                registries,
                pkg_specs,
            } => {
                let registries = engine::resolve_registries_by_id(cx, registries.as_deref())?;
                Ok(Self::PackageSpec {
                    registries,
                    pkg_specs,
                })
            }
        }
    }

    fn resolve_target(
        &self,
        cx: &ReportContext<InstallScope>,
        db: &PackageDatabase<'_>,
    ) -> Result<Vec<ResolvedInstallTarget>, InstallError> {
        match self {
            CheckedInstallTargets::Manifest { manifests } => {
                engine::resolve_install_targets_by_manifest(cx, manifests)
            }
            CheckedInstallTargets::PackageSpec {
                registries,
                pkg_specs,
            } => engine::resolve_install_targets_by_spec(cx, db, registries, pkg_specs),
        }
    }
}
