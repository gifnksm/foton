use std::{collections::BTreeSet, sync::Arc};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{
            NeverReport, ReportScope, ReportValue, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    package::{PackageId, PackageManifest, PackageSpec},
    registry::{
        self, FetchRegistryError, RegistryId, RegistryIndex, RegistryIndexError, RegistrySource,
    },
};

#[derive(Debug)]
struct RegistryScope<S> {
    base_scope: Arc<S>,
}

impl<S> ReportScope for RegistryScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = RegistryErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.base_scope.make_failed()
    }

    fn make_cancelled(&self) -> Self::Error {
        self.base_scope.make_cancelled()
    }
}

impl<S> SubReportScope<S> for RegistryScope<S>
where
    S: ReportScope,
{
    fn new(base_scope: Arc<S>) -> Self {
        Self { base_scope }
    }
}

#[derive(Debug, Snafu)]
enum RegistryErrorReport {
    #[snafu(display(
        concat!(
            "specified registry `{reg_id}` not found in configuration\n",
            "available registries:\n",
            "{registry_ids}",
        ),
        reg_id = reg_id,
        registry_ids = BulletList(available_registry_ids),
    ))]
    RegistryNotFound {
        reg_id: RegistryId,
        available_registry_ids: BTreeSet<RegistryId>,
    },
    #[snafu(display("no enabled registries found in configuration"))]
    NoEnabledRegistries,
    #[snafu(display("failed to fetch registry `{id}`"))]
    FetchRegistry {
        id: RegistryId,
        #[snafu(source(from(FetchRegistryError, Box::new)))]
        source: Box<FetchRegistryError>,
    },
    #[snafu(display("failed to find package by {pkg_spec}"))]
    FindLatestPackagesBySpec {
        pkg_spec: PackageSpec,
        #[snafu(source(from(RegistryIndexError, Box::new)))]
        source: Box<RegistryIndexError>,
    },
    #[snafu(display(
        concat!(
            "multiple packages match the specified package `{pkg_spec}`:\n",
            "{pkg_ids}\n",
            "specify one of the matching package IDs listed above explicitly to disambiguate",
        ),
        pkg_spec = pkg_spec,
        pkg_ids = BulletList(&pkg_ids.iter().map(|(registry, id)| format!("{id} (in {registry})")).collect::<Vec<_>>()),
    ))]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<(RegistryId, PackageId)>,
    },
    #[snafu(display("no package found matching the specified package `{pkg_spec}`"))]
    PackageNotFoundForSpec { pkg_spec: PackageSpec },
}

impl From<RegistryErrorReport> for ReportValue<'static> {
    fn from(report: RegistryErrorReport) -> ReportValue<'static> {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn resolve_registries_by_id<'a, S>(
    cx: &'a ReportContext<S>,
    registry_ids: Option<&[RegistryId]>,
) -> Result<Vec<(RegistryId, &'a RegistrySource)>, S::Error>
where
    S: ReportScope,
{
    let config_registries = &cx.config().registries;
    let cx = RegistryScope::start_with_report(cx, format_args!("Fetching package registries..."));

    let registries: Vec<_> = match registry_ids {
        Some(registry_ids) => {
            let registry_ids = registry_ids.iter().cloned().collect::<BTreeSet<_>>();
            registry_ids
                .iter()
                .map(|reg_id| {
                    config_registries
                        .get(reg_id)
                        .map(|registry| (reg_id.clone(), &registry.source))
                        .with_context(|| {
                            let available_registry_ids =
                                config_registries.keys().cloned().collect::<BTreeSet<_>>();
                            RegistryNotFoundSnafu {
                                reg_id,
                                available_registry_ids,
                            }
                        })
                        .report_error(cx.reporter())
                })
                .collect_to_end()?
        }
        None => config_registries
            .iter()
            .filter(|(_id, registry)| registry.enabled)
            .map(|(id, registry)| (id.clone(), &registry.source))
            .collect(),
    };
    if registries.is_empty() {
        return Err(cx.reporter().report_error(NoEnabledRegistriesSnafu.build()));
    }
    Ok(registries)
}

pub(in crate::command) fn fetch_registries<S>(
    cx: &ReportContext<S>,
    registries: &[(RegistryId, &RegistrySource)],
) -> Result<Vec<RegistryIndex>, S::Error>
where
    S: ReportScope,
{
    let cx = RegistryScope::start(cx);
    registries
        .iter()
        .map(|(id, source)| {
            registry::fetch_registry(cx.app_dirs(), id, source).context(FetchRegistrySnafu { id })
        })
        .collect::<Result<Vec<_>, _>>()
        .report_error(cx.reporter())
}

pub(crate) fn resolve_package_specs<S>(
    cx: &ReportContext<S>,
    indexes: &[RegistryIndex],
    pkg_specs: &[PackageSpec],
) -> Result<Vec<Arc<PackageManifest>>, S::Error>
where
    S: ReportScope,
{
    pkg_specs
        .iter()
        .map(|pkg_spec| resolve_package_spec(cx, indexes, pkg_spec))
        .collect_to_end()
}

fn resolve_package_spec<S>(
    cx: &ReportContext<S>,
    indexes: &[RegistryIndex],
    pkg_spec: &PackageSpec,
) -> Result<Arc<PackageManifest>, S::Error>
where
    S: ReportScope,
{
    let cx = RegistryScope::start_with_report(cx, format_args!("Resolving {pkg_spec}..."));
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

    Ok(Arc::new(manifest))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

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
    ) -> Result<Arc<PackageManifest>, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let index = RegistryIndex::open(
            "test-registry".parse().unwrap(),
            registry_path.to_path_buf(),
        )
        .unwrap();
        resolve_package_spec(&cx, &[index], pkg_spec)
    }

    fn resolve_for_test_with_indexes(
        indexes: &[RegistryIndex],
        pkg_spec: &PackageSpec,
    ) -> Result<Arc<PackageManifest>, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        resolve_package_spec(&cx, indexes, pkg_spec)
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
