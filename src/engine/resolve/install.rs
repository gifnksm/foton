use std::{
    collections::{BTreeMap, HashSet},
    iter,
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
            ErrorReportExt as _, NeverReport, ReportScope, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::PackageDatabase,
    engine::{InstallResolutionDetail, InstallTargetKind},
    package::{PackageDefinition, PackageId, PackageName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
    util::macros::concat_line,
};

use super::ResolvedInstallTarget;

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
        pkg_ids = BulletList(&pkg_ids.iter().map(|(source, pkg_id)| format!("{pkg_id} (resolved from {source})")).collect::<Vec<_>>()),
    ))]
    ConflictingRequirements {
        pkg_ids: Vec<(InstallResolutionDetail, PackageId)>,
    },
}

#[derive(Debug, Clone)]
enum ResolveState {
    Resolved(ResolvedInstallTarget),
    Unresolved { pkg_spec: PackageSpec },
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

    let mut indexes = LazyRegistryIndexes::new(&cx, registries);
    let mut targets = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_spec_from_installed_packages(db, pkg_spec, should_activate))
        .map(|state| match state {
            ResolveState::Resolved(target) => Ok(target),
            ResolveState::Unresolved { pkg_spec } => resolve_spec_from_registry(
                &cx,
                &mut indexes,
                &pkg_spec,
                include_pre_release,
                should_activate,
            ),
        })
        .collect_to_end::<Vec<_>>()?;

    dedup_targets(&mut targets);
    check_conflicting_targets(&cx, &targets)?;

    Ok(targets)
}

pub(crate) fn resolve_install_targets_by_manifest<S>(
    cx: &ReportContext<S>,
    pkgs: &[(PathBuf, Arc<PackageDefinition>)],
    should_activate: bool,
) -> Result<Vec<ResolvedInstallTarget>, S::Error>
where
    S: ReportScope,
{
    let cx =
        InstallResolveScope::start_with_report(cx, format_args!("Resolving install target..."));

    let mut targets = pkgs
        .iter()
        .map(|(path, pkg)| ResolvedInstallTarget {
            kind: InstallTargetKind::Install {
                pkg: Arc::clone(pkg),
            },
            detail: InstallResolutionDetail::ManifestFile { path: path.clone() },
            should_activate,
        })
        .collect::<Vec<_>>();

    dedup_targets(&mut targets);
    check_conflicting_targets(&cx, &targets)?;

    Ok(targets)
}

fn resolve_spec_from_installed_packages(
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
    should_activate: bool,
) -> ResolveState {
    let installed_pkg = db
        .entries_by_spec(pkg_spec)
        .filter(|entry| entry.installation_state().is_installed())
        .max_by(|a, b| a.id().version().cmp(b.id().version()));
    if let Some(entry) = installed_pkg {
        ResolveState::Resolved(ResolvedInstallTarget {
            kind: InstallTargetKind::AlreadyInstalled {
                pkg_id: entry.id().clone(),
            },
            detail: InstallResolutionDetail::Installed {
                pkg_spec: pkg_spec.clone(),
            },
            should_activate,
        })
    } else {
        ResolveState::Unresolved {
            pkg_spec: pkg_spec.clone(),
        }
    }
}

#[derive(Debug)]
struct LazyRegistryIndexes<'cx, 'reg, S> {
    cx: &'cx ReportContext<InstallResolveScope<S>>,
    registries: &'reg [RegistrySpec],
    indexes: Option<Vec<RegistryIndex>>,
}

impl<'cx, 'reg, S> LazyRegistryIndexes<'cx, 'reg, S>
where
    S: ReportScope,
{
    fn new(
        cx: &'cx ReportContext<InstallResolveScope<S>>,
        registries: &'reg [RegistrySpec],
    ) -> Self {
        Self {
            cx,
            registries,
            indexes: None,
        }
    }

    #[cfg(test)]
    fn new_for_test(
        cx: &'cx ReportContext<InstallResolveScope<S>>,
        indexes: Vec<RegistryIndex>,
    ) -> Self {
        Self {
            cx,
            registries: &[],
            indexes: Some(indexes),
        }
    }

    fn get_or_fetch(&mut self) -> Result<&[RegistryIndex], S::Error> {
        if self.indexes.is_none() {
            let indexes = super::fetch_registries(self.cx, self.registries)?;
            self.indexes = Some(indexes);
        }
        Ok(self.indexes.as_ref().unwrap())
    }
}

fn resolve_spec_from_registry<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    indexes: &mut LazyRegistryIndexes<'_, '_, S>,
    pkg_spec: &PackageSpec,
    include_pre_release: bool,
    should_activate: bool,
) -> Result<ResolvedInstallTarget, S::Error>
where
    S: ReportScope,
{
    let indexes = indexes.get_or_fetch()?;
    let matches = indexes
        .iter()
        .filter_map(|index| {
            find_index_target_from_index(cx, index, pkg_spec, include_pre_release).transpose()
        })
        .collect::<Result<Vec<_>, _>>()?;

    match &matches[..] {
        [] => Err(PackageNotFoundForSpecSnafu { pkg_spec }
            .build()
            .report_error(cx)),
        [(reg_id, pkg)] => Ok(ResolvedInstallTarget {
            kind: InstallTargetKind::Install {
                pkg: Arc::clone(pkg),
            },
            detail: InstallResolutionDetail::Registry {
                reg_id: reg_id.clone(),
                pkg_spec: pkg_spec.clone(),
            },
            should_activate,
        }),
        _ => {
            let pkg_ids = matches
                .into_iter()
                .map(|(reg_id, pkg)| (reg_id.clone(), pkg.id.clone()))
                .collect::<Vec<_>>();
            Err(
                MultipleMatchingPackagesInRegistriesSnafu { pkg_spec, pkg_ids }
                    .build()
                    .report_error(cx),
            )
        }
    }
}

fn find_index_target_from_index<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    index: &RegistryIndex,
    pkg_spec: &PackageSpec,
    include_pre_release: bool,
) -> Result<Option<(RegistryId, Arc<PackageDefinition>)>, S::Error>
where
    S: ReportScope,
{
    let pkg = index
        .find_latest_package_by_spec(pkg_spec, include_pre_release)
        .context(FindLatestPackageBySpecSnafu { pkg_spec })
        .report_error(cx)?;

    if let Some(pkg) = pkg {
        Ok(Some((index.id().clone(), pkg)))
    } else {
        Ok(None)
    }
}

fn dedup_targets(targets: &mut Vec<ResolvedInstallTarget>) {
    let mut seen = HashSet::new();
    targets.retain(|target| seen.insert(target.kind.clone()));
}

fn check_conflicting_targets<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    targets: &[ResolvedInstallTarget],
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut pkg_ids = BTreeMap::<PackageId, Vec<_>>::new();
    let mut versions_by_name = BTreeMap::<PackageName, Vec<_>>::new();
    for target in targets {
        let pkg_id = target.kind.pkg_id();
        pkg_ids
            .entry(pkg_id.clone())
            .or_default()
            .push((target.detail.clone(), pkg_id.clone()));

        if !target.should_activate {
            continue;
        }

        versions_by_name
            .entry(pkg_id.name().clone())
            .or_default()
            .push((target.detail.clone(), pkg_id.clone()));
    }
    iter::chain(pkg_ids.into_values(), versions_by_name.into_values())
        .map(|pkg_ids| {
            if pkg_ids.len() > 1 {
                Err(ConflictingRequirementsSnafu { pkg_ids }
                    .build()
                    .report_error(&cx))
            } else {
                Ok(())
            }
        })
        .collect_to_end::<()>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, str::FromStr as _, sync::LazyLock};

    use tempfile::TempDir;

    use super::*;
    use crate::util::testing;

    static PKG: LazyLock<PackageDefinition> =
        LazyLock::new(|| testing::make_package_definition("example-font@1.0.0"));

    #[test]
    fn resolve_spec_from_installed_packages_returns_resolved_when_matching_package_is_installed() {
        let spec = PackageSpec::from(PKG.id.name().clone());

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &PKG);

            let target = resolve_spec_from_installed_packages(db, &spec, true);
            let ResolveState::Resolved(target) = target else {
                panic!("expected resolved target");
            };
            assert_eq!(
                target,
                ResolvedInstallTarget::already_installed(
                    "example-font@1.0.0",
                    "example-font",
                    true
                ),
            );
        });
    }

    #[test]
    fn resolve_spec_from_installed_packages_returns_unresolved_when_matching_package_is_incomplete_install()
     {
        let spec = PackageSpec::from(PKG.id.name().clone());

        testing::with_db(|_cx, db| {
            testing::mark_as_incomplete_install(db, &PKG);

            let target = resolve_spec_from_installed_packages(db, &spec, true);
            assert_matches!(
                target,
                ResolveState::Unresolved { pkg_spec } if pkg_spec == spec
            );
        });
    }

    #[test]
    fn resolve_spec_from_installed_packages_returns_unresolved_when_no_packages_match() {
        let installed_pkg = testing::make_package_definition("installed-font@1.0.0");
        let missing_spec = PackageSpec::from_str("missing-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &installed_pkg);

            let target = resolve_spec_from_installed_packages(db, &missing_spec, true);
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

        let installed_pkg = testing::make_package_definition("installed-font@1.0.0");

        let pkg_specs = vec![
            PackageSpec::from_str("installed-font").unwrap(),
            PackageSpec::from_str("example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &installed_pkg);

            let targets =
                resolve_install_targets_by_spec(cx, db, &[registry], &pkg_specs, false, true)
                    .unwrap();
            assert_eq!(
                targets,
                vec![
                    ResolvedInstallTarget::already_installed(
                        "installed-font@1.0.0",
                        "installed-font",
                        true,
                    ),
                    ResolvedInstallTarget::registry(
                        "example-font@1.0.0",
                        "test-registry",
                        "example-font@1.0.0",
                        true,
                    ),
                ],
            );
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
    fn resolve_install_targets_by_manifest_returns_resolved_target() {
        testing::with_context(|cx| {
            let targets = resolve_install_targets_by_manifest(
                cx,
                &[(PathBuf::from("example-font.toml"), Arc::new(PKG.clone()))],
                true,
            )
            .unwrap();

            assert_eq!(
                targets,
                vec![ResolvedInstallTarget::manifest(
                    "example-font@1.0.0",
                    "example-font.toml",
                    true,
                )],
            );
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_duplicated_package_ids() {
        let manifest1 = testing::make_package_definition("example-font@0.1.0");
        let mut manifest2 = manifest1.clone();
        manifest2.description = Some("some description".to_owned());
        let pkgs = vec![
            (PathBuf::from("manifest-a.toml"), Arc::new(manifest1)),
            (PathBuf::from("manifest-b.toml"), Arc::new(manifest2)),
        ];

        testing::with_context(|cx| {
            let err = resolve_install_targets_by_manifest(cx, &pkgs, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_dedups_identical_definitions() {
        let manifest = testing::make_package_definition("example-font@0.1.0");
        let pkgs = vec![
            (PathBuf::from("manifest-a.toml"), Arc::new(manifest.clone())),
            (PathBuf::from("manifest-b.toml"), Arc::new(manifest)),
        ];

        testing::with_context(|cx| {
            let targets = resolve_install_targets_by_manifest(cx, &pkgs, false).unwrap();
            assert_eq!(
                targets,
                vec![ResolvedInstallTarget::manifest(
                    "example-font@0.1.0",
                    "manifest-a.toml",
                    false,
                )],
            );
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_multiple_versions_with_activation() {
        let pkgs = vec![
            (
                PathBuf::from("manifest-a.toml"),
                Arc::new(testing::make_package_definition("example-font@0.1.0")),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                Arc::new(testing::make_package_definition("example-font@0.2.0")),
            ),
        ];

        testing::with_context(|cx| {
            let err = resolve_install_targets_by_manifest(cx, &pkgs, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_allows_multiple_versions_without_activation() {
        let pkgs = vec![
            (
                PathBuf::from("manifest-a.toml"),
                Arc::new(testing::make_package_definition("example-font@0.1.0")),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                Arc::new(testing::make_package_definition("example-font@0.2.0")),
            ),
        ];

        testing::with_context(|cx| {
            let targets = resolve_install_targets_by_manifest(cx, &pkgs, false).unwrap();
            assert_eq!(
                targets,
                vec![
                    ResolvedInstallTarget::manifest("example-font@0.1.0", "manifest-a.toml", false,),
                    ResolvedInstallTarget::manifest("example-font@0.2.0", "manifest-b.toml", false,),
                ],
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_resolves_name_to_latest_package() {
        let old_pkg_id = PackageId::from_str("example-font@0.1.0").unwrap();
        let new_pkg_id = PackageId::from_str("example-font@0.2.0").unwrap();

        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), &old_pkg_id);
        testing::write_manifest(registry_dir.path(), &new_pkg_id);

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from(old_pkg_id.name().clone());
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            assert_eq!(
                target,
                ResolvedInstallTarget::registry(
                    "example-font@0.2.0",
                    "test-registry",
                    "example-font",
                    true,
                ),
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_skips_pre_release_by_default() {
        let released_pkg_id = PackageId::from_str("example-font@1.0.0").unwrap();
        let pre_release_pkg_id = PackageId::from_str("example-font@2.0.0-rc-1").unwrap();

        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), &released_pkg_id);
        testing::write_manifest(registry_dir.path(), &pre_release_pkg_id);

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from(released_pkg_id.name().clone());
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let stable_target =
                resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            assert_eq!(
                stable_target,
                ResolvedInstallTarget::registry(
                    "example-font@1.0.0",
                    "test-registry",
                    "example-font",
                    true,
                ),
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_includes_pre_release_when_requested() {
        let released_pkg_id = PackageId::from_str("example-font@1.0.0").unwrap();
        let pre_release_pkg_id = PackageId::from_str("example-font@2.0.0-rc-1").unwrap();

        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), &released_pkg_id);
        testing::write_manifest(registry_dir.path(), &pre_release_pkg_id);

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from(released_pkg_id.name().clone());
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, true, true).unwrap();
            assert_eq!(
                target,
                ResolvedInstallTarget::registry(
                    "example-font@2.0.0-rc-1",
                    "test-registry",
                    "example-font",
                    true,
                ),
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_allows_exact_pre_release_id_without_flag() {
        let pre_release_pkg_id = PackageId::from_str("example-font@2.0.0-rc-1").unwrap();
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), &pre_release_pkg_id);

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from(pre_release_pkg_id.clone());
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            assert_eq!(
                target,
                ResolvedInstallTarget::registry(
                    "example-font@2.0.0-rc-1",
                    "test-registry",
                    "example-font@2.0.0-rc-1",
                    true,
                ),
            );
        });
    }

    #[test]
    fn resolve_spec_from_registry_reports_multiple_matching_packages_across_registries() {
        let (registry_dir1, index1) = testing::make_registry_index("registry-a");
        let (registry_dir2, index2) = testing::make_registry_index("registry-b");
        testing::write_manifest(registry_dir1.path(), "example-font@0.2.0");
        testing::write_manifest(registry_dir2.path(), "example-font@1.0.0");

        testing::with_scoped_context(|cx| {
            let spec: PackageSpec = "example-font".parse().unwrap();
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index1, index2]);
            let err = resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn check_conflicting_targets_reports_multiple_versions_of_same_package() {
        let targets = vec![
            ResolvedInstallTarget::already_installed(
                "example-font@0.1.0",
                "example-font@0.1.0",
                true,
            ),
            ResolvedInstallTarget::already_installed(
                "example-font@0.2.0",
                "example-font@0.2.0",
                true,
            ),
        ];

        testing::with_scoped_context(|cx| {
            let err = check_conflicting_targets(cx, &targets).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn check_conflicting_targets_allows_multiple_versions_without_activation() {
        let targets = vec![
            ResolvedInstallTarget::already_installed(
                "example-font@0.1.0",
                "example-font@0.1.0",
                false,
            ),
            ResolvedInstallTarget::already_installed(
                "example-font@0.2.0",
                "example-font@0.2.0",
                false,
            ),
        ];

        testing::with_scoped_context(|cx| {
            check_conflicting_targets(cx, &targets).unwrap();
        });
    }

    #[test]
    fn resolve_install_targets_by_spec_allows_multiple_versions_without_activation() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");

        let pkg_specs = vec![
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font@0.2.0").unwrap(),
        ];

        testing::with_db(|cx, db| {
            let targets =
                resolve_install_targets_by_spec(cx, db, &[registry], &pkg_specs, false, false)
                    .unwrap();
            assert_eq!(
                targets,
                vec![
                    ResolvedInstallTarget::registry(
                        "example-font@0.1.0",
                        "test-registry",
                        "example-font@0.1.0",
                        false,
                    ),
                    ResolvedInstallTarget::registry(
                        "example-font@0.2.0",
                        "test-registry",
                        "example-font@0.2.0",
                        false,
                    ),
                ],
            );
        });
    }

    #[test]
    fn dedup_by_id_collapses_duplicate_targets() {
        let mut targets = vec![
            ResolvedInstallTarget::already_installed(
                "example-font@0.1.0",
                "example-font@0.1.0",
                true,
            ),
            ResolvedInstallTarget::already_installed(
                "example-font@0.1.0",
                "example-font@0.1.0",
                true,
            ),
        ];

        dedup_targets(&mut targets);

        assert_eq!(
            targets,
            vec![ResolvedInstallTarget::already_installed(
                "example-font@0.1.0",
                "example-font@0.1.0",
                true,
            )],
        );
    }
}
