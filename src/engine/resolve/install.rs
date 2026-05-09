use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    sync::Arc,
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

use super::registry;

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
            "specify one of the matching package IDs or registry IDs listed above explicitly to disambiguate",
        ),
        pkg_spec = pkg_spec,
        pkg_ids = BulletList(&pkg_ids.iter().map(|(registry, id)| format!("{id} (in {registry})")).collect::<Vec<_>>()),
    ))]
    MultipleMatchingPackagesInRegistries {
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
    pub(crate) manifest: Arc<PackageManifest>,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
enum ResolveState {
    Resolved { manifest: Arc<PackageManifest> },
    Unresolved { pkg_spec: PackageSpec },
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

    let targets: Vec<_> = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_installed_package(db, pkg_spec))
        .collect();

    let mut targets = if targets.iter().all(ResolveState::is_resolved) {
        targets
            .into_iter()
            .map(|target| match target {
                ResolveState::Resolved { manifest } => ResolvedInstallTarget { manifest },
                ResolveState::Unresolved { .. } => unreachable!(),
            })
            .collect()
    } else {
        let indexes = registry::fetch_registries(&cx, registries)?;
        targets
            .into_iter()
            .map(|target| resolve_from_registry(&cx, &indexes, target))
            .collect_to_end()?
    };

    dedup_and_check_conflicts(&cx, &mut targets)?;

    Ok(targets)
}

fn resolve_installed_package(db: &PackageDatabase<'_>, pkg_spec: &PackageSpec) -> ResolveState {
    let installed_pkg = db
        .entries_by_spec(pkg_spec)
        .filter_map(|(state, manifest)| (state == PackageState::Installed).then_some(manifest))
        .max_by(|a, b| a.metadata.version.cmp(&b.metadata.version));
    if let Some(manifest) = installed_pkg {
        ResolveState::Resolved { manifest }
    } else {
        ResolveState::Unresolved {
            pkg_spec: pkg_spec.clone(),
        }
    }
}

fn resolve_from_registry<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    indexes: &[RegistryIndex],
    target: ResolveState,
) -> Result<ResolvedInstallTarget, S::Error>
where
    S: ReportScope,
{
    match target {
        ResolveState::Resolved { manifest } => Ok(ResolvedInstallTarget { manifest }),
        ResolveState::Unresolved { pkg_spec } => resolve_spec_from_registry(cx, indexes, &pkg_spec),
    }
}

fn resolve_spec_from_registry<S>(
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
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesInRegistriesSnafu { pkg_spec, pkg_ids }.build(),
        ));
    }

    let Some((_reg_id, manifest)) = manifests.into_iter().next() else {
        return Err(cx
            .reporter()
            .report_error(PackageNotFoundForSpecSnafu { pkg_spec }.build()));
    };

    Ok(ResolvedInstallTarget {
        manifest: Arc::new(manifest),
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
        let metadata = &target.manifest.metadata;
        versions_by_name
            .entry(metadata.qualified_name.clone())
            .or_default()
            .insert(metadata.version.clone())
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
    use std::{fs, path::Path, str::FromStr as _};

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
        resolve_spec_from_registry(&cx, &[index], pkg_spec)
    }

    fn resolve_for_test_with_indexes(
        indexes: &[RegistryIndex],
        pkg_spec: &PackageSpec,
    ) -> Result<ResolvedInstallTarget, TestError> {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);
        resolve_spec_from_registry(&cx, indexes, pkg_spec)
    }

    #[test]
    fn resolve_installed_package_returns_resolved_when_matching_package_is_installed() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("other-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);

            let target = resolve_installed_package(&db, &spec);
            assert!(
                matches!(target, ResolveState::Resolved { manifest: resolved_manifest } if Arc::ptr_eq(&resolved_manifest, &manifest))
            );
        });
    }

    #[test]
    fn resolve_installed_package_returns_unresolved_when_matching_package_is_pending_install() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &manifest);

            let target = resolve_installed_package(&db, &spec);
            assert!(matches!(
                target,
                ResolveState::Unresolved { pkg_spec } if pkg_spec == spec
            ));
        });
    }

    #[test]
    fn resolve_installed_package_returns_unresolved_when_no_packages_match() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = InstallResolveScope::start(&cx);

        let installed_manifest = testing::make_manifest("example-namespace/installed-font@1.0.0");
        let missing_spec = PackageSpec::from_str("missing-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &installed_manifest);

            let target = resolve_installed_package(&db, &missing_spec);
            assert!(matches!(
                target,
                ResolveState::Unresolved { pkg_spec } if pkg_spec == missing_spec));
        });
    }

    #[test]
    fn resolve_package_resolves_name_to_latest_manifest() {
        let tempdir = TempDir::new().unwrap();
        write_manifest(tempdir.path(), "example-namespace/example-font@0.1.0");
        write_manifest(tempdir.path(), "example-namespace/example-font@0.2.0");

        let spec = PackageSpec::from_str("example-font").unwrap();
        let target = resolve_for_test(tempdir.path(), &spec).unwrap();
        assert_eq!(
            target.manifest.metadata.id().to_string(),
            "example-namespace/example-font@0.2.0"
        );
    }

    #[test]
    fn resolve_install_targets_resolves_installed_specs_and_collapses_duplicates() {
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
            assert_eq!(targets.len(), 2);
            assert_eq!(
                targets[0].manifest.metadata.id(),
                installed_manifest.metadata.id()
            );
            assert_eq!(
                targets[1].manifest.metadata.id().to_string(),
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

        let mut targets = vec![
            ResolvedInstallTarget {
                manifest: testing::make_manifest("example-namespace/example-font@0.1.0"),
            },
            ResolvedInstallTarget {
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

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let expected_pkg_id = manifest.metadata.id();
        let mut targets = vec![
            ResolvedInstallTarget {
                manifest: Arc::clone(&manifest),
            },
            ResolvedInstallTarget { manifest },
        ];

        dedup_and_check_conflicts(&cx, &mut targets).unwrap();

        assert_eq!(targets.len(), 1);
        assert!(
            matches!(&targets[0], ResolvedInstallTarget { manifest } if manifest.metadata.id() == expected_pkg_id)
        );
    }
}
