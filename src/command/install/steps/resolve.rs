use std::{path::Path, sync::Arc};

use crate::{
    package::{PackageId, PackageManifest, PackageName, PackageQualifiedName, PackageSpec},
    registry::{PackageRegistry, PackageRegistryError},
    util::reporter::{
        NeverReport, ReportValue, RootReporter, Step, StepReporter, StepResultErrorExt as _,
    },
};

#[derive(Debug)]
struct ResolveStep<S> {
    step: Arc<S>,
    spec: PackageSpec,
}

impl<S> Step for ResolveStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ResolveErrorReport;
    type Error = S::Error;

    fn report_prelude(&self, reporter: &RootReporter) {
        reporter.report_step(format_args!("Resolving {}...", self.spec));
    }

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum ResolveErrorReport {
    #[display("failed to find package by {name}")]
    FindLatestPackagesByName {
        name: PackageName,
        #[error(source)]
        source: PackageRegistryError,
    },
    #[display("failed to find package by {qualified_name}")]
    FindLatestPackageByQualifiedName {
        qualified_name: PackageQualifiedName,
        #[error(source)]
        source: PackageRegistryError,
    },
    #[display("failed to find package by {pkg_id}")]
    FindPackageById {
        pkg_id: PackageId,
        #[error(source)]
        source: PackageRegistryError,
    },
    #[display(
        "multiple packages match the specified package `{pkg_spec}`:\n{pkg_ids}\nspecify one of the matching package IDs listed above explicitly to disambiguate",
        pkg_ids = pkg_ids.iter().map(|id| format!("- {id}")).collect::<Vec<_>>().join("\n")
    )]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
    #[display("no package found matching the specified package `{pkg_spec}`")]
    PackageNotFoundForSpec { pkg_spec: PackageSpec },
}

impl From<ResolveErrorReport> for ReportValue<'static> {
    fn from(report: ResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn resolve_package<S>(
    reporter: &StepReporter<S>,
    registry_path: &Path,
    pkg_spec: &PackageSpec,
) -> Result<PackageManifest, S::Error>
where
    S: Step,
{
    let reporter = reporter.with_step(ResolveStep {
        step: Arc::clone(reporter.step()),
        spec: pkg_spec.clone(),
    });

    let registry = PackageRegistry::new(registry_path.to_path_buf());

    let manifests = match pkg_spec {
        PackageSpec::Name(name) => registry
            .find_latest_packages_by_name(name)
            .map_err(|source| {
                let name = name.clone();
                ResolveErrorReport::FindLatestPackagesByName { name, source }
            })
            .report_error(&reporter)?
            .into_values()
            .collect::<Vec<_>>(),
        PackageSpec::QualifiedName(qualified_name) => registry
            .find_latest_package_by_qualified_name(qualified_name)
            .map_err(|source| {
                let qualified_name = qualified_name.clone();
                ResolveErrorReport::FindLatestPackageByQualifiedName {
                    qualified_name,
                    source,
                }
            })
            .report_error(&reporter)?
            .into_iter()
            .collect(),
        PackageSpec::Id(pkg_id) => registry
            .find_package_by_id(pkg_id)
            .map_err(|source| {
                let pkg_id = pkg_id.clone();
                ResolveErrorReport::FindPackageById { pkg_id, source }
            })
            .report_error(&reporter)?
            .into_iter()
            .collect(),
    };

    if manifests.len() > 1 {
        let pkg_spec = pkg_spec.clone();
        let pkg_ids = manifests.into_iter().map(|pkg| pkg.metadata.id()).collect();
        return Err(reporter
            .report_error(ResolveErrorReport::MultipleMatchingPackages { pkg_spec, pkg_ids }));
    }
    let Some(manifest) = manifests.into_iter().next() else {
        let pkg_spec = pkg_spec.clone();
        return Err(reporter.report_error(ResolveErrorReport::PackageNotFoundForSpec { pkg_spec }));
    };

    reporter.report_info(format_args!("found package {}", manifest.metadata.id()));

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use crate::command::install::{InstallError, InstallStep};

    use super::*;

    fn make_registry_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    fn write_manifest(
        root: &Path,
        namespace: &str,
        name: &str,
        version: &str,
        manifest_name: &str,
        manifest_version: &str,
    ) {
        let dir = root.join(namespace).join(name).join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.toml"),
            format!(
                r#"
[package]
name = "{manifest_name}"
version = "{manifest_version}"

[[sources]]
url = "https://example.com/{name}-{version}.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#
            ),
        )
        .unwrap();
    }

    fn resolve_for_test(
        registry_path: &Path,
        pkg_spec: &PackageSpec,
    ) -> Result<PackageManifest, InstallError> {
        let reporter = RootReporter::message_reporter();
        let reporter = reporter.with_step(InstallStep {
            pkg_spec: pkg_spec.clone(),
        });
        resolve_package(&reporter, registry_path, pkg_spec)
    }

    #[test]
    fn resolve_package_resolves_name_to_latest_manifest() {
        let tempdir = make_registry_dir();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.9.0",
            "yuru7/hackgen",
            "2.9.0",
        );
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.10.0",
        );

        let spec: PackageSpec = "hackgen".parse().unwrap();
        let manifest = resolve_for_test(tempdir.path(), &spec).unwrap();

        assert_eq!(manifest.metadata.id().to_string(), "yuru7/hackgen@2.10.0");
    }

    #[test]
    fn resolve_package_reports_multiple_matching_packages_for_name() {
        let tempdir = make_registry_dir();
        write_manifest(
            tempdir.path(),
            "yuru7",
            "hackgen",
            "2.10.0",
            "yuru7/hackgen",
            "2.10.0",
        );
        write_manifest(
            tempdir.path(),
            "someone",
            "hackgen",
            "1.0.0",
            "someone/hackgen",
            "1.0.0",
        );

        let spec: PackageSpec = "hackgen".parse().unwrap();
        let err = resolve_for_test(tempdir.path(), &spec).unwrap_err();

        assert!(matches!(err, InstallError::Failed));
    }

    #[test]
    fn resolve_package_reports_not_found_for_missing_spec() {
        let tempdir = make_registry_dir();

        let spec: PackageSpec = "yuru7/hackgen".parse().unwrap();
        let err = resolve_for_test(tempdir.path(), &spec).unwrap_err();

        assert!(matches!(err, InstallError::Failed));
    }
}
