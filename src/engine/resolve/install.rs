use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{
            NeverReport, ReportScope, ReportValue, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::PackageDatabase,
    package::{
        PackageId, PackageManifest, PackageQualifiedName, PackageSpec, PackageState, PackageVersion,
    },
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySource},
};

#[derive(Debug)]
struct InstallResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallResolveScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallResolveScope<S>
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
enum InstallResolveErrorReport {
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
    #[snafu(display(
        concat!(
            "multiple versions of package `{pkg_name}` are being installed:\n",
            "{versions}\n",
            "this may cause unexpected behavior; consider installing only one version of the package\n",
        ),
        pkg_name = pkg_name,
        versions = BulletList(versions),
    ))]
    ConflictingRequirements {
        pkg_name: PackageQualifiedName,
        versions: BTreeSet<PackageVersion>,
    },
}

impl From<InstallResolveErrorReport> for ReportValue<'static> {
    fn from(report: InstallResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInstallTarget {
    pub(crate) reg_id: RegistryId,
    pub(crate) manifest: PackageManifest,
}

pub(crate) fn resolve_install_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    registries: &[(RegistryId, &RegistrySource)],
    pkg_specs: &[PackageSpec],
) -> Result<Vec<ResolvedInstallTarget>, S::Error>
where
    S: ReportScope,
{
    let cx =
        InstallResolveScope::start_with_report(cx, format_args!("Resolving install target..."));

    let pkg_specs = filter_installed_packages(&cx, db, pkg_specs);
    if pkg_specs.is_empty() {
        return Ok(vec![]);
    }

    let indexes = super::registry::fetch_registries(&cx, registries)?;

    let mut targets = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_spec(&cx, &indexes, pkg_spec))
        .collect_to_end()?;

    dedup_and_check_conflicts(&cx, &mut targets)?;

    Ok(targets)
}

fn filter_installed_packages<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Vec<PackageSpec>
where
    S: ReportScope,
{
    pkg_specs
        .iter()
        .filter_map(|pkg_spec| {
            let mut installed_pkgs = db
                .entries_by_spec(pkg_spec)
                .filter(|(state, _)| *state == PackageState::Installed)
                .peekable();
            if installed_pkgs.peek().is_none() {
                Some(pkg_spec.clone())
            } else {
                let installed_pkgs = installed_pkgs
                    .map(|(_, manifest)| manifest.metadata.id())
                    .collect::<Vec<_>>();
                cx.reporter().report_info(format_args!(
                    concat!(
                        "package {pkg_spec} matches already installed package(s), skipping\n",
                        "{bl}"
                    ),
                    pkg_spec = pkg_spec,
                    bl = BulletList(&installed_pkgs),
                ));
                None
            }
        })
        .collect()
}

fn resolve_spec<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    indexes: &[RegistryIndex],
    pkg_spec: &PackageSpec,
) -> Result<ResolvedInstallTarget, S::Error>
where
    S: ReportScope,
{
    let mut manifests = vec![];
    for index in indexes {
        let pkgs = index
            .find_latest_packages_by_spec(pkg_spec)
            .context(FindLatestPackagesBySpecSnafu { pkg_spec })
            .report_error(cx.reporter())?;
        manifests.extend(pkgs.into_values().map(|manifest| (index.id(), manifest)));
    }

    if manifests.len() > 1 {
        let pkg_ids = manifests
            .into_iter()
            .map(|(registry, pkg)| (registry.clone(), pkg.metadata.id()))
            .collect::<Vec<_>>();
        return Err(cx
            .reporter()
            .report_error(MultipleMatchingPackagesSnafu { pkg_spec, pkg_ids }.build()));
    }
    let Some((reg_id, manifest)) = manifests.into_iter().next() else {
        return Err(cx
            .reporter()
            .report_error(PackageNotFoundForSpecSnafu { pkg_spec }.build()));
    };

    Ok(ResolvedInstallTarget {
        reg_id: reg_id.clone(),
        manifest,
    })
}

fn dedup_and_check_conflicts<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    targets: &mut Vec<ResolvedInstallTarget>,
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut versions_by_name: BTreeMap<PackageQualifiedName, BTreeSet<_>> = BTreeMap::new();
    targets.retain(|target| {
        versions_by_name
            .entry(target.manifest.metadata.qualified_name.clone())
            .or_default()
            .insert(target.manifest.metadata.version.clone())
    });
    versions_by_name
        .iter()
        .map(|(pkg_name, versions)| {
            if versions.len() > 1 {
                Err(cx.reporter().report_error(
                    ConflictingRequirementsSnafu {
                        pkg_name,
                        versions: versions.clone(),
                    }
                    .build(),
                ))
            } else {
                Ok(())
            }
        })
        .collect_to_end::<()>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, slice, str::FromStr as _};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        registry::{RegistryId, RegistrySource},
        util::{
            path::AbsolutePath,
            testing::{self, TempdirContext, TestError, TestScope},
        },
    };

    fn write_manifest(root: &Path, pkg_id: &str) {
        let pkg_id = PackageId::from_str(pkg_id).unwrap();
        let dir = root
            .join(pkg_id.namespace())
            .join(pkg_id.name())
            .join(pkg_id.version().to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.toml"),
            testing::make_manifest_str(pkg_id),
        )
        .unwrap();
    }

    fn resolve_for_test(
        registry_path: &Path,
        pkg_spec: &PackageSpec,
    ) -> Result<ResolvedInstallTarget, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);
        let index = RegistryIndex::open(
            "test-registry".parse().unwrap(),
            registry_path.to_path_buf(),
        )
        .unwrap();
        resolve_spec(&cx, &[index], pkg_spec)
    }

    fn resolve_for_test_with_indexes(
        indexes: &[RegistryIndex],
        pkg_spec: &PackageSpec,
    ) -> Result<ResolvedInstallTarget, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);
        resolve_spec(&cx, indexes, pkg_spec)
    }

    #[test]
    fn filter_installed_packages_skips_name_when_matching_package_is_installed() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("other-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);

            let filtered = filter_installed_packages(&cx, &db, &[spec]);
            assert!(filtered.is_empty());
        });
    }

    #[test]
    fn filter_installed_packages_keeps_pending_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &manifest);

            let filtered = filter_installed_packages(&cx, &db, slice::from_ref(&spec));
            assert_eq!(filtered, [spec]);
        });
    }

    #[test]
    fn filter_installed_packages_skips_only_installed_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let installed_manifest = testing::make_manifest("example-namespace/installed-font@1.0.0");
        let installed_spec = PackageSpec::from_str("installed-font").unwrap();
        let missing_spec = PackageSpec::from_str("missing-font").unwrap();
        let specs = vec![installed_spec, missing_spec.clone()];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &installed_manifest);

            let filtered = filter_installed_packages(&cx, &db, &specs);
            assert_eq!(filtered, [missing_spec]);
        });
    }

    #[test]
    fn resolve_package_resolves_name_to_latest_manifest() {
        let tempdir = TempDir::new().unwrap();
        write_manifest(tempdir.path(), "example-namespace/example-font@0.1.0");
        write_manifest(tempdir.path(), "example-namespace/example-font@0.2.0");

        let spec = PackageSpec::from_str("example-font").unwrap();
        let resolved = resolve_for_test(tempdir.path(), &spec).unwrap();

        assert_eq!(
            resolved.manifest.metadata.id().to_string(),
            "example-namespace/example-font@0.2.0"
        );
    }

    #[test]
    fn resolve_install_targets_filters_installed_specs_and_collapses_duplicates() {
        let registry_dir = TempDir::new().unwrap();
        write_manifest(
            registry_dir.path(),
            "example-namespace/installed-font@1.0.0",
        );
        write_manifest(registry_dir.path(), "example-namespace/example-font@1.0.0");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let installed_manifest = testing::make_manifest("example-namespace/installed-font@1.0.0");

        let reg_id = RegistryId::new("test-registry").unwrap();
        let reg_source = RegistrySource::Local(AbsolutePath::new(registry_dir.path()).unwrap());
        let pkg_specs = vec![
            PackageSpec::from_str("installed-font").unwrap(),
            PackageSpec::from_str("example-namespace/example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-namespace/example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &installed_manifest);

            let targets =
                resolve_install_targets(&cx, &db, &[(reg_id.clone(), &reg_source)], &pkg_specs)
                    .unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].reg_id, reg_id);
            assert_eq!(
                targets[0].manifest.metadata.id().to_string(),
                "example-namespace/example-font@1.0.0"
            );
        });
    }

    #[test]
    fn resolve_install_targets_reports_fetch_failure() {
        let registry_dir = TempDir::new().unwrap();
        let missing_registry_dir = registry_dir.path().join("missing");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let reg_id = RegistryId::new("test-registry").unwrap();
        let reg_source = RegistrySource::Local(AbsolutePath::new(&missing_registry_dir).unwrap());
        let pkg_specs = vec![PackageSpec::from_str("example-font").unwrap()];

        testing::with_db(&cx, |db| {
            let err = resolve_install_targets(&cx, &db, &[(reg_id, &reg_source)], &pkg_specs)
                .unwrap_err();
            assert!(matches!(err, TestError::Failed));
        });
    }

    #[test]
    fn resolve_package_reports_multiple_matching_packages_for_name() {
        let tempdir = TempDir::new().unwrap();
        write_manifest(tempdir.path(), "example-namespace/example-font@0.2.0");
        write_manifest(tempdir.path(), "other-namespace/example-font@1.0.0");

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
        write_manifest(tempdir1.path(), "example-namespace/example-font@0.2.0");
        write_manifest(tempdir2.path(), "other-namespace/example-font@1.0.0");

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

    #[test]
    fn dedup_and_check_conflicts_reports_multiple_versions_of_same_package() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let reg_id = RegistryId::new("example-registry").unwrap();
        let mut targets = vec![
            ResolvedInstallTarget {
                reg_id: reg_id.clone(),
                manifest: testing::make_manifest("example-namespace/example-font@0.1.0"),
            },
            ResolvedInstallTarget {
                reg_id,
                manifest: testing::make_manifest("example-namespace/example-font@0.2.0"),
            },
        ];

        let err = dedup_and_check_conflicts(&cx, &mut targets).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }

    #[test]
    fn dedup_and_check_conflicts_collapses_duplicate_targets() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let reg_id = RegistryId::new("example-registry").unwrap();
        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let expected_pkg_id = manifest.metadata.id();
        let mut targets = vec![
            ResolvedInstallTarget {
                reg_id: reg_id.clone(),
                manifest: manifest.clone(),
            },
            ResolvedInstallTarget { reg_id, manifest },
        ];

        dedup_and_check_conflicts(&cx, &mut targets).unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].manifest.metadata.id(), expected_pkg_id);
    }
}
