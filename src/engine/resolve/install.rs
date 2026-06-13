use std::{
    collections::{BTreeMap, BTreeSet},
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
        pkg_ids = BulletList(&pkg_ids.iter().map(|(source, pkg_id)| format!("{pkg_id} (from {source})")).collect::<Vec<_>>()),
    ))]
    ConflictingRequirements {
        pkg_ids: Vec<(InstallTargetSource, PackageId)>,
    },
}

#[derive(Debug, Clone)]
enum ResolveState {
    Resolved(ResolvedInstallTargetWithSource),
    Unresolved { pkg_spec: PackageSpec },
}

#[derive(Debug, Clone, derive_more::Display)]
enum InstallTargetSource {
    #[display("package registry {_0}")]
    Registry(RegistryId),
    #[display("installed package")]
    Installed,
    #[display("manifest file: {}", _0.display())]
    File(PathBuf),
}

#[derive(Debug, Clone)]
struct ResolvedInstallTargetWithSource {
    source: InstallTargetSource,
    pkg: Arc<PackageDefinition>,
    should_activate: bool,
}

impl ResolvedInstallTargetWithSource {
    fn into_target(self) -> ResolvedInstallTarget {
        ResolvedInstallTarget {
            pkg: self.pkg,
            should_activate: self.should_activate,
        }
    }
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

    dedup_by_id(&mut targets);
    check_conflicting_targets(&cx, &targets)?;

    Ok(targets
        .into_iter()
        .map(ResolvedInstallTargetWithSource::into_target)
        .collect())
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

    let targets = pkgs
        .iter()
        .map(|(path, pkg)| ResolvedInstallTargetWithSource {
            source: InstallTargetSource::File(path.clone()),
            pkg: Arc::clone(pkg),
            should_activate,
        })
        .collect::<Vec<_>>();

    check_conflicting_targets(&cx, &targets)?;

    Ok(targets
        .into_iter()
        .map(ResolvedInstallTargetWithSource::into_target)
        .collect())
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
        ResolveState::Resolved(ResolvedInstallTargetWithSource {
            source: InstallTargetSource::Installed,
            pkg: Arc::new(entry.make_definition()),
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
) -> Result<ResolvedInstallTargetWithSource, S::Error>
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
        [(reg_id, pkg)] => Ok(ResolvedInstallTargetWithSource {
            source: InstallTargetSource::Registry(reg_id.clone()),
            pkg: Arc::clone(pkg),
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

fn dedup_by_id(targets: &mut Vec<ResolvedInstallTargetWithSource>) {
    let mut seen = BTreeSet::new();
    targets.retain(|target| seen.insert(target.pkg.id.clone()));
}

fn check_conflicting_targets<S>(
    cx: &ReportContext<InstallResolveScope<S>>,
    targets: &[ResolvedInstallTargetWithSource],
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut pkg_ids = BTreeMap::<PackageId, Vec<_>>::new();
    let mut versions_by_name = BTreeMap::<PackageName, Vec<_>>::new();
    for target in targets {
        let pkg_id = &target.pkg.id;
        pkg_ids
            .entry(pkg_id.clone())
            .or_default()
            .push((target.source.clone(), pkg_id.clone()));

        if !target.should_activate {
            continue;
        }

        versions_by_name
            .entry(pkg_id.name().clone())
            .or_default()
            .push((target.source.clone(), pkg_id.clone()));
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
    use crate::{engine::resolve, util::testing};

    static PKG: LazyLock<Arc<PackageDefinition>> =
        LazyLock::new(|| testing::make_package_definition("example-font@1.0.0"));

    fn installed_target(
        pkg: &Arc<PackageDefinition>,
        should_activate: bool,
    ) -> ResolvedInstallTargetWithSource {
        ResolvedInstallTargetWithSource {
            source: InstallTargetSource::Installed,
            pkg: Arc::clone(pkg),
            should_activate,
        }
    }

    #[test]
    fn resolve_spec_from_installed_packages_returns_resolved_when_matching_package_is_installed() {
        let spec = PackageSpec::from(PKG.id.name().clone());

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &PKG);

            let target = resolve_spec_from_installed_packages(db, &spec, true);
            assert_matches!(
                target,
                ResolveState::Resolved(ResolvedInstallTargetWithSource {
                    source: InstallTargetSource::Installed,
                    pkg: resolved_pkg,
                    should_activate: true,
                }) if resolved_pkg.id == PKG.id,
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
            resolve::testing::assert_install_target_ids(
                &targets,
                &[&installed_pkg.id.to_string(), "example-font@1.0.0"],
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
        let path = PathBuf::from("example-font.toml");

        testing::with_context(|cx| {
            let targets =
                resolve_install_targets_by_manifest(cx, &[(path.clone(), Arc::clone(&PKG))], true)
                    .unwrap();

            assert_eq!(targets.len(), 1);
            assert_matches!(
                &targets[0],
                ResolvedInstallTarget {
                    pkg: resolved_pkg,
                    should_activate: true,
                } if Arc::ptr_eq(resolved_pkg, &PKG)
            );
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_duplicated_package_ids() {
        let pkgs = vec![
            (
                PathBuf::from("manifest-a.toml"),
                testing::make_package_definition("example-font@0.1.0"),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                testing::make_package_definition("example-font@0.1.0"),
            ),
        ];

        testing::with_context(|cx| {
            let err = resolve_install_targets_by_manifest(cx, &pkgs, true).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_duplicated_package_ids_without_activation() {
        let pkgs = vec![
            (
                PathBuf::from("manifest-a.toml"),
                testing::make_package_definition("example-font@0.1.0"),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                testing::make_package_definition("example-font@0.1.0"),
            ),
        ];

        testing::with_context(|cx| {
            let err = resolve_install_targets_by_manifest(cx, &pkgs, false).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_install_targets_by_manifest_reports_multiple_versions_with_activation() {
        let pkgs = vec![
            (
                PathBuf::from("manifest-a.toml"),
                testing::make_package_definition("example-font@0.1.0"),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                testing::make_package_definition("example-font@0.2.0"),
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
                testing::make_package_definition("example-font@0.1.0"),
            ),
            (
                PathBuf::from("manifest-b.toml"),
                testing::make_package_definition("example-font@0.2.0"),
            ),
        ];

        testing::with_context(|cx| {
            let targets = resolve_install_targets_by_manifest(cx, &pkgs, false).unwrap();
            resolve::testing::assert_install_target_ids(
                &targets,
                &["example-font@0.1.0", "example-font@0.2.0"],
            );
            assert!(targets.iter().all(|target| !target.should_activate));
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
            assert_eq!(target.pkg.id, new_pkg_id);
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
            assert_eq!(stable_target.pkg.id, released_pkg_id);
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
            assert_eq!(target.pkg.id, pre_release_pkg_id);
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
            assert_eq!(target.pkg.id, pre_release_pkg_id);
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
            installed_target(
                &testing::make_package_definition("example-font@0.1.0"),
                true,
            ),
            installed_target(
                &testing::make_package_definition("example-font@0.2.0"),
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
            installed_target(
                &testing::make_package_definition("example-font@0.1.0"),
                false,
            ),
            installed_target(
                &testing::make_package_definition("example-font@0.2.0"),
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
            resolve::testing::assert_install_target_ids(
                &targets,
                &["example-font@0.1.0", "example-font@0.2.0"],
            );
            assert!(targets.iter().all(|target| !target.should_activate));
        });
    }

    #[test]
    fn dedup_by_id_collapses_duplicate_targets() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let expected_pkg_id = pkg.id.clone();
        let mut targets = vec![installed_target(&pkg, true), installed_target(&pkg, true)];

        dedup_by_id(&mut targets);

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].pkg.id, expected_pkg_id);
    }
}
