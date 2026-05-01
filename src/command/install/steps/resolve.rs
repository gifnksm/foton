use std::sync::Arc;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    package::{PackageId, PackageManifest, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError},
};

#[derive(Debug)]
struct ResolveScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for ResolveScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ResolveErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }
}

impl<S> SubReportScope<S> for ResolveScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum ResolveErrorReport {
    #[snafu(display("failed to find package by {pkg_spec}"))]
    FindLatestPackagesBySpec {
        pkg_spec: PackageSpec,
        source: RegistryIndexError,
    },
    #[snafu(display(
        "multiple packages match the specified package `{pkg_spec}`:\n{pkg_ids}\nspecify one of the matching package IDs listed above explicitly to disambiguate",
        pkg_ids = pkg_ids.iter().map(|(registry, id)| format!("- {id} (in {registry})")).collect::<Vec<_>>().join("\n")
    ))]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<(RegistryId, PackageId)>,
    },
    #[snafu(display("no package found matching the specified package `{pkg_spec}`"))]
    PackageNotFoundForSpec { pkg_spec: PackageSpec },
}

impl From<ResolveErrorReport> for ReportValue<'static> {
    fn from(report: ResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn resolve_package<S>(
    cx: &ReportContext<S>,
    indexes: &[RegistryIndex],
    pkg_spec: &PackageSpec,
) -> Result<PackageManifest, S::Error>
where
    S: ReportScope,
{
    let cx = ResolveScope::start_with_report(cx, format_args!("Resolving {pkg_spec}..."));
    let reporter = cx.reporter();

    let mut manifests = vec![];
    for index in indexes {
        let pkgs = index
            .find_latest_packages_by_spec(pkg_spec)
            .context(FindLatestPackagesBySpecSnafu { pkg_spec })
            .report_error(reporter)?;
        manifests.extend(pkgs.into_values().map(|manifest| (index.id(), manifest)));
    }

    if manifests.len() > 1 {
        let pkg_ids = manifests
            .into_iter()
            .map(|(registry, pkg)| (registry.clone(), pkg.metadata.id()))
            .collect::<Vec<_>>();
        return Err(
            reporter.report_error(MultipleMatchingPackagesSnafu { pkg_spec, pkg_ids }.build())
        );
    }
    let Some((registry, manifest)) = manifests.into_iter().next() else {
        return Err(reporter.report_error(PackageNotFoundForSpecSnafu { pkg_spec }.build()));
    };

    reporter.report_info(format_args!(
        "found package {} in registry {registry}",
        manifest.metadata.id()
    ));

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

    use super::*;

    fn write_manifest(root: &Path, namespace: &str, name: &str, version: &str) {
        let dir = root.join(namespace).join(name).join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.toml"),
            testing::make_manifest_str(namespace, name, version),
        )
        .unwrap();
    }

    fn resolve_for_test(
        registry_path: &Path,
        pkg_spec: &PackageSpec,
    ) -> Result<PackageManifest, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let index = RegistryIndex::open(
            "test-registry".parse().unwrap(),
            registry_path.to_path_buf(),
        )
        .unwrap();
        resolve_package(&cx, &[index], pkg_spec)
    }

    fn resolve_for_test_with_indexes(
        indexes: &[RegistryIndex],
        pkg_spec: &PackageSpec,
    ) -> Result<PackageManifest, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        resolve_package(&cx, indexes, pkg_spec)
    }

    #[test]
    fn resolve_package_resolves_name_to_latest_manifest() {
        let tempdir = TempDir::new().unwrap();
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.1.0");
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.2.0");

        let spec: PackageSpec = "example-font".parse().unwrap();
        let manifest = resolve_for_test(tempdir.path(), &spec).unwrap();

        assert_eq!(
            manifest.metadata.id().to_string(),
            "example-namespace/example-font@0.2.0"
        );
    }

    #[test]
    fn resolve_package_reports_multiple_matching_packages_for_name() {
        let tempdir = TempDir::new().unwrap();
        write_manifest(tempdir.path(), "example-namespace", "example-font", "0.2.0");
        write_manifest(tempdir.path(), "other-namespace", "example-font", "1.0.0");

        let spec: PackageSpec = "example-font".parse().unwrap();
        let err = resolve_for_test(tempdir.path(), &spec).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }

    #[test]
    fn resolve_package_reports_not_found_for_missing_spec() {
        let tempdir = TempDir::new().unwrap();

        let spec: PackageSpec = "example-namespace/example-font".parse().unwrap();
        let err = resolve_for_test(tempdir.path(), &spec).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }

    #[test]
    fn resolve_package_reports_multiple_matching_packages_across_registries() {
        let tempdir1 = TempDir::new().unwrap();
        let tempdir2 = TempDir::new().unwrap();
        write_manifest(
            tempdir1.path(),
            "example-namespace",
            "example-font",
            "0.2.0",
        );
        write_manifest(tempdir2.path(), "other-namespace", "example-font", "1.0.0");

        let indexes = vec![
            RegistryIndex::open("registry-a".parse().unwrap(), tempdir1.path().to_path_buf())
                .unwrap(),
            RegistryIndex::open("registry-b".parse().unwrap(), tempdir2.path().to_path_buf())
                .unwrap(),
        ];

        let spec: PackageSpec = "example-font".parse().unwrap();
        let err = resolve_for_test_with_indexes(&indexes, &spec).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }
}
