use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
    path::PathBuf,
    sync::Arc,
};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{
            NeverReport, ReportScope, ResultIteratorExt as _, ScopeResultErrorExt as _,
            SubReportScope,
        },
    },
    db::PackageDatabase,
    package::{InstallationState, PackageId, PackageManifest, PackageName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
    util::macros::concat_line,
};

use super::registry;

#[derive(Debug, Default)]
struct InstallResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for InstallResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InstallResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for InstallResolveScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum InstallResolveErrorReport {
    #[snafu(display("failed to find package for `{pkg_spec}`"))]
    FindLatestPackageBySpec {
        pkg_spec: PackageSpec,
        #[snafu(source(from(RegistryIndexError, Box::new)))]
        source: Box<RegistryIndexError>,
    },
    #[snafu(display(
        concat_line!(
            "multiple packages match the specified package `{pkg_spec}`:",
            "{pkg_ids}",
            "specify one of the matching package IDs or registry IDs listed above explicitly to disambiguate",
        ),
        pkg_spec = pkg_spec,
        pkg_ids = BulletList(&pkg_ids.iter().map(|(registry, id)| format!("{id} (in {registry})")).collect::<Vec<_>>()),
    ))]
    MultipleMatchingPackagesInRegistries {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<(RegistryId, PackageId)>,
    },
    #[snafu(display("no package found for `{pkg_spec}`"))]
    PackageNotFoundForSpec { pkg_spec: PackageSpec },
    #[snafu(display(
        concat_line!(
            "conflicting packages are being installed:",
            "{pkg_ids}",
            "this may cause unexpected behavior; consider installing only one version of the package",
        ),
        pkg_ids = BulletList(&pkg_ids.iter().map(|(source, pkg_id)| format!("{pkg_id} (from {source})")).collect::<Vec<_>>()),
    ))]
    ConflictingRequirements {
        pkg_ids: Vec<(InstallTargetSource, PackageId)>,
    },
}

#[derive(Debug, Clone, derive_more::Display)]
pub(crate) enum InstallTargetSource {
    #[display("package registry {_0}")]
    Registry(RegistryId),
    #[display("installed package")]
    Installed,
    #[display("manifest file: {}", _0.display())]
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedInstallTarget {
    pub(crate) source: InstallTargetSource,
    pub(crate) manifest: Arc<PackageManifest>,
    pub(crate) should_activate: bool,
}

#[derive(Debug, Clone, derive_more::IsVariant)]
enum ResolveState {
    Resolved {
        source: InstallTargetSource,
        manifest: Arc<PackageManifest>,
    },
    Unresolved {
        pkg_spec: PackageSpec,
    },
}

pub(crate) fn resolve_install_targets_by_spec<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    registries: &[RegistrySpec],
    pkg_specs: &[PackageSpec],
    include_pre_release: bool,
    should_activate: bool,
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
                ResolveState::Resolved { source, manifest } => ResolvedInstallTarget {
                    source,
                    manifest,
                    should_activate,
                },
                ResolveState::Unresolved { .. } => unreachable!(),
            })
            .collect()
    } else {
        let indexes = registry::fetch_registries(&cx, registries)?;
        targets
            .into_iter()
            .map(|target| {
                resolve_from_registry(&cx, &indexes, target, include_pre_release, should_activate)
            })
            .collect_to_end()?
    };

    dedup_by_id(&mut targets);
    check_conflicts(&cx, &targets)?;

    Ok(targets)
}

pub(crate) fn resolve_install_targets_by_manifest<S>(
    cx: &ReportContext<S>,
    manifests: &[(PathBuf, Arc<PackageManifest>)],
    should_activate: bool,
) -> Result<Vec<ResolvedInstallTarget>, S::Error>
where
    S: ReportScope,
{
    let cx =
        InstallResolveScope::start_with_report(cx, format_args!("Resolving install target..."));

    let targets = manifests
        .iter()
        .map(|(path, manifest)| ResolvedInstallTarget {
            source: InstallTargetSource::File(path.clone()),
            manifest: Arc::clone(manifest),
            should_activate,
        })
        .collect::<Vec<_>>();

    check_conflicts(&cx, &targets)?;

    Ok(targets)
}

fn resolve_installed_package(db: &PackageDatabase<'_>, pkg_spec: &PackageSpec) -> ResolveState {
    let installed_pkg = db
        .entries_by_spec(pkg_spec)
        .filter_map(|entry| {
            (entry.installation_state() == InstallationState::Installed).then_some(entry.manifest())
        })
        .max_by(|a, b| a.version.cmp(&b.version));
    if let Some(manifest) = installed_pkg {
        ResolveState::Resolved {
            source: InstallTargetSource::Installed,
            manifest,
        }
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
    include_pre_release: bool,
    should_activate: bool,
) -> Result<ResolvedInstallTarget, S::Error>
where
    S: ReportScope,
{
    match target {
        ResolveState::Resolved { source, manifest } => Ok(ResolvedInstallTarget {
            source,
            manifest,
            should_activate,
        }),
        ResolveState::Unresolved { pkg_spec } => {
            resolve_spec_from_registry(cx, indexes, &pkg_spec, include_pre_release, should_activate)
        }
    }
}

fn resolve_spec_from_registry<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    indexes: &[RegistryIndex],
    pkg_spec: &PackageSpec,
    include_pre_release: bool,
    should_activate: bool,
) -> Result<ResolvedInstallTarget, S::Error>
where
    S: ReportScope,
{
    let mut manifests = vec![];
    for index in indexes {
        let manifest = index
            .find_latest_package_by_spec(pkg_spec, include_pre_release)
            .context(FindLatestPackageBySpecSnafu { pkg_spec })
            .report_error(cx.reporter())?;
        if let Some(manifest) = manifest {
            manifests.push((index.id(), manifest));
        }
    }

    if manifests.len() > 1 {
        let pkg_ids = manifests
            .into_iter()
            .map(|(registry, pkg)| (registry.clone(), pkg.id()))
            .collect::<Vec<_>>();
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesInRegistriesSnafu { pkg_spec, pkg_ids }.build(),
        ));
    }

    let Some((reg_id, manifest)) = manifests.into_iter().next() else {
        return Err(cx
            .reporter()
            .report_error(PackageNotFoundForSpecSnafu { pkg_spec }.build()));
    };

    Ok(ResolvedInstallTarget {
        source: InstallTargetSource::Registry(reg_id.clone()),
        manifest,
        should_activate,
    })
}

fn dedup_by_id(targets: &mut Vec<ResolvedInstallTarget>) {
    let mut seen = BTreeSet::new();
    targets.retain(|target| seen.insert(target.manifest.id()));
}

fn check_conflicts<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    targets: &[ResolvedInstallTarget],
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut versions_by_name: BTreeMap<PackageName, Vec<_>> = BTreeMap::new();
    for target in targets {
        let manifest = &target.manifest;
        versions_by_name
            .entry(manifest.name.clone())
            .or_default()
            .push((target.source.clone(), manifest.id()));
    }
    versions_by_name
        .into_values()
        .map(|pkg_ids| {
            if pkg_ids.len() > 1 {
                Err(cx
                    .reporter()
                    .report_error(ConflictingRequirementsSnafu { pkg_ids }.build()))
            } else {
                Ok(())
            }
        })
        .collect_to_end::<()>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, str::FromStr as _};

    use tempfile::TempDir;

    use super::*;
    use crate::util::testing;

    #[test]
    fn resolve_installed_package_returns_resolved_when_matching_package_is_installed() {
        let manifest = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &manifest);

            let target = resolve_installed_package(db, &spec);
            assert_matches!(target, ResolveState::Resolved { manifest: resolved_manifest, .. } if Arc::ptr_eq(&resolved_manifest, &manifest));
        });
    }

    #[test]
    fn resolve_installed_package_returns_unresolved_when_matching_package_is_incomplete_install() {
        let manifest = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_incomplete_install(db, &manifest);

            let target = resolve_installed_package(db, &spec);
            assert_matches!(
                target,
                ResolveState::Unresolved { pkg_spec } if pkg_spec == spec
            );
        });
    }

    #[test]
    fn resolve_installed_package_returns_unresolved_when_no_packages_match() {
        let installed_manifest = testing::make_manifest("installed-font@1.0.0");
        let missing_spec = PackageSpec::from_str("missing-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &installed_manifest);

            let target = resolve_installed_package(db, &missing_spec);
            assert_matches!(
                target,
                ResolveState::Unresolved { pkg_spec } if pkg_spec == missing_spec
            );
        });
    }

    #[test]
    fn resolve_install_targets_by_spec_resolves_installed_specs_and_collapses_duplicates() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "installed-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");

        let installed_manifest = testing::make_manifest("installed-font@1.0.0");

        let pkg_specs = vec![
            PackageSpec::from_str("installed-font").unwrap(),
            PackageSpec::from_str("example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &installed_manifest);

            let targets =
                resolve_install_targets_by_spec(cx, db, &[registry], &pkg_specs, false, true)
                    .unwrap();
            assert_eq!(targets.len(), 2);
            assert_eq!(targets[0].manifest.id(), installed_manifest.id());
            assert_eq!(targets[1].manifest.id().to_string(), "example-font@1.0.0");
        });
    }

    #[test]
    fn resolve_install_targets_by_spec_reports_fetch_failure() {
        let registry_dir = TempDir::new().unwrap();
        let missing_registry_dir = registry_dir.path().join("missing");

        let registry =
            testing::make_registry_spec_at("test-registry", missing_registry_dir.as_path());
        let pkg_specs = vec![PackageSpec::from_str("example-font").unwrap()];

        testing::with_db(|cx, db| {
            let err = resolve_install_targets_by_spec(cx, db, &[registry], &pkg_specs, false, true)
                .unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_sets_file_source() {
        let path = PathBuf::from("example-font.toml");
        let manifest = testing::make_manifest("example-font@0.1.0");

        testing::with_context(|cx| {
            let targets = resolve_install_targets_by_manifest(
                cx,
                &[(path.clone(), Arc::clone(&manifest))],
                true,
            )
            .unwrap();

            assert_eq!(targets.len(), 1);
            assert_matches!(
                &targets[0],
                ResolvedInstallTarget {
                    source: InstallTargetSource::File(source_path),
                    manifest: resolved_manifest,
                    should_activate: true,
                } if source_path == &path && Arc::ptr_eq(resolved_manifest, &manifest)
            );
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_duplicated_package_ids() {
        let manifests = vec![
            (
                PathBuf::from("manifest-a.toml"),
                testing::make_manifest("example-font@0.1.0"),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                testing::make_manifest("example-font@0.1.0"),
            ),
        ];

        testing::with_context(|cx| {
            let err = resolve_install_targets_by_manifest(cx, &manifests, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_spec_from_registry_resolves_name_to_latest_manifest() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let target = resolve_spec_from_registry(cx, &[index], &spec, false, true).unwrap();
            assert_eq!(target.manifest.id().to_string(), "example-font@0.2.0");
        });
    }

    #[test]
    fn resolve_spec_from_registry_skips_pre_release_by_default() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let stable_target =
                resolve_spec_from_registry(cx, &[index], &spec, false, true).unwrap();
            assert_eq!(
                stable_target.manifest.id().to_string(),
                "example-font@1.0.0"
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_includes_pre_release_when_requested() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let target = resolve_spec_from_registry(cx, &[index], &spec, true, true).unwrap();
            assert_eq!(target.manifest.id().to_string(), "example-font@2.0.0-rc-1");
        });
    }

    #[test]
    fn resolve_spec_from_registry_allows_exact_pre_release_id_without_flag() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font@2.0.0-rc-1").unwrap();
            let target = resolve_spec_from_registry(cx, &[index], &spec, false, true).unwrap();
            assert_eq!(target.manifest.id().to_string(), "example-font@2.0.0-rc-1");
        });
    }

    #[test]
    fn resolve_spec_from_registry_reports_multiple_matching_packages_across_registries() {
        let (registry_dir1, index1) = testing::make_registry_index("registry-a");
        let (registry_dir2, index2) = testing::make_registry_index("registry-b");
        testing::write_manifest(registry_dir1.path(), "example-font@0.2.0");
        testing::write_manifest(registry_dir2.path(), "example-font@1.0.0");

        let indexes = [index1, index2];

        testing::with_scoped_context(|cx| {
            let spec: PackageSpec = "example-font".parse().unwrap();
            let err = resolve_spec_from_registry(cx, &indexes, &spec, false, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn check_conflicts_reports_multiple_versions_of_same_package() {
        let targets = vec![
            ResolvedInstallTarget {
                source: InstallTargetSource::Installed,
                manifest: testing::make_manifest("example-font@0.1.0"),
                should_activate: true,
            },
            ResolvedInstallTarget {
                source: InstallTargetSource::Installed,
                manifest: testing::make_manifest("example-font@0.2.0"),
                should_activate: true,
            },
        ];

        testing::with_scoped_context(|cx| {
            let err = check_conflicts(cx, &targets).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn dedup_by_id_collapses_duplicate_targets() {
        let manifest = testing::make_manifest("example-font@0.1.0");
        let expected_pkg_id = manifest.id();
        let mut targets = vec![
            ResolvedInstallTarget {
                source: InstallTargetSource::Installed,
                manifest: Arc::clone(&manifest),
                should_activate: true,
            },
            ResolvedInstallTarget {
                source: InstallTargetSource::Installed,
                manifest,
                should_activate: true,
            },
        ];

        dedup_by_id(&mut targets);

        assert_eq!(targets.len(), 1);
        assert_matches!(&targets[0], ResolvedInstallTarget { manifest, .. } if manifest.id() == expected_pkg_id);
    }
}
