use std::iter;

use snafu::Snafu;

use crate::{
    cli::{
        args::InstallArgs,
        context::{ReportContext, RootContext},
        reporter::{
            NeverReport, OperationError, ReportScope, ResultIteratorExt as _, RootReportScope,
        },
    },
    engine::{self, InstallResolveInput, InstallResolveOptions},
    registry::RegistrySpec,
};

#[derive(Debug, Default)]
struct InstallScope {}

impl ReportScope for InstallScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = NeverReport;
    type Error = InstallError;
}

impl RootReportScope for InstallScope {}

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
    let should_activate = !args.no_activate;

    let cx = InstallScope::start_with_report(
        cx,
        format_args!("Installing {} package(s)...", args.len()),
    );

    let targets = resolve_target_inputs(&cx, args)?;
    let registries = resolve_install_registries(&cx, args)?;
    let options = InstallResolveOptions {
        registries,
        include_pre_release: args.pre_release,
        should_activate,
    };

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let mut db = engine::load_database(&cx, &mut db_lock_file)?;

    let targets = engine::resolve_install_targets(&cx, &db, &targets, &options)?;
    let plan = engine::plan_install(&db, &targets);
    engine::report_plan(&cx, &plan);

    if !plan.has_side_effects() {
        cx.reporter()
            .report_info(format_args!("already installed, nothing to do"));
        return Ok(());
    }

    engine::confirm(&cx, "Do you want to continue?").await?;

    engine::execute_plan(&cx, &mut db, plan).await?;

    Ok(())
}

fn resolve_target_inputs<S>(
    cx: &ReportContext<S>,
    args: &InstallArgs,
) -> Result<Vec<InstallResolveInput>, S::Error>
where
    S: ReportScope,
{
    iter::chain(
        args.manifests.iter().map(|path| {
            let pkg = engine::resolve_manifest(cx, path)?;
            Ok(InstallResolveInput::Manifest {
                path: path.clone(),
                pkg,
            })
        }),
        args.pkg_specs
            .iter()
            .cloned()
            .map(|pkg_spec| Ok(InstallResolveInput::PackageSpec { pkg_spec })),
    )
    .collect_to_end()
}

fn resolve_install_registries<S>(
    cx: &ReportContext<S>,
    args: &InstallArgs,
) -> Result<Vec<RegistrySpec>, S::Error>
where
    S: ReportScope,
{
    engine::resolve_registries_by_id(cx, args.registries.as_deref())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, str::FromStr as _};

    use tempfile::TempDir;

    use super::*;
    use crate::{package::PackageSpec, registry::RegistryId, util::testing};

    #[test]
    fn resolve_target_inputs_resolves_manifests_and_package_specs() {
        let tempdir = TempDir::new().unwrap();
        let manifest_path = tempdir.path().join("manifest.toml");
        fs::write(
            &manifest_path,
            testing::make_manifest_str("manifest-font@0.1.0"),
        )
        .unwrap();
        let args = InstallArgs {
            manifests: vec![manifest_path.clone()],
            registries: None,
            pkg_specs: vec![PackageSpec::from_str("registry-font").unwrap()],
            pre_release: false,
            no_activate: false,
        };

        testing::with_context(|cx| {
            let targets = resolve_target_inputs(cx, &args).unwrap();
            assert_eq!(targets.len(), 2);

            let InstallResolveInput::Manifest { path, pkg } = &targets[0] else {
                panic!("expected manifest target, got {:?}", targets[0]);
            };
            assert_eq!(path, &manifest_path);
            assert_eq!(pkg.id.to_string(), "manifest-font@0.1.0");

            let InstallResolveInput::PackageSpec { pkg_spec } = &targets[1] else {
                panic!("expected package spec target, got {:?}", targets[1]);
            };
            assert_eq!(pkg_spec, &PackageSpec::from_str("registry-font").unwrap());
        });
    }

    #[test]
    fn resolve_install_registries_reports_unknown_explicit_registry_without_package_specs() {
        let args = InstallArgs {
            manifests: vec![PathBuf::from("manifest.toml")],
            registries: Some(vec![RegistryId::new("unknown").unwrap()]),
            pkg_specs: vec![],
            pre_release: false,
            no_activate: false,
        };

        testing::with_context(|cx| {
            let err = resolve_install_registries(cx, &args).unwrap_err();
            assert!(err.is_failed());
        });
    }
}
