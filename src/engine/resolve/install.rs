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
            ErrorReportExt as _, NeverReport, ReportScope, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    db::PackageDatabase,
    package::{InstallationState, PackageDefinition, PackageId, PackageName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
    util::macros::concat_line,
};

use super::{InstallTargetSource, ResolvedInstallTarget, lookup};

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

    dedup_by_id(&mut targets);
    check_conflicts(&cx, &targets)?;

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

    let targets = pkgs
        .iter()
        .map(|(path, pkg)| ResolvedInstallTarget {
            source: InstallTargetSource::File(path.clone()),
            pkg: Arc::clone(pkg),
            should_activate,
        })
        .collect::<Vec<_>>();

    check_conflicts(&cx, &targets)?;

    Ok(targets)
}

fn resolve_spec_from_installed_packages(
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
    should_activate: bool,
) -> ResolveState {
    let installed_pkg = db
        .entries_by_spec(pkg_spec)
        .filter_map(|entry| {
            (entry.installation_state() == InstallationState::Installed)
                .then_some(entry.definition())
        })
        .max_by(|a, b| a.id.version().cmp(b.id.version()));
    if let Some(pkg) = installed_pkg {
        ResolveState::Resolved(ResolvedInstallTarget {
            source: InstallTargetSource::Installed,
            pkg,
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
    let matches = lookup::collect_registry_matches(indexes, |index| {
        index
            .find_latest_package_by_spec(pkg_spec, include_pre_release)
            .context(FindLatestPackageBySpecSnafu { pkg_spec })
            .report_error(cx)
    })?;

    let Some((reg_id, pkg)) = lookup::into_unique_match(matches).map_err(|matches| {
        let pkg_ids = matches
            .into_iter()
            .map(|(registry, pkg)| (registry, pkg.id.clone()))
            .collect::<Vec<_>>();
        MultipleMatchingPackagesInRegistriesSnafu { pkg_spec, pkg_ids }
            .build()
            .report_error(cx)
    })?
    else {
        return Err(PackageNotFoundForSpecSnafu { pkg_spec }
            .build()
            .report_error(cx));
    };

    Ok(ResolvedInstallTarget {
        source: InstallTargetSource::Registry(reg_id),
        pkg,
        should_activate,
    })
}

fn dedup_by_id(targets: &mut Vec<ResolvedInstallTarget>) {
    let mut seen = BTreeSet::new();
    targets.retain(|target| seen.insert(target.pkg.id.clone()));
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
        let pkg = &target.pkg;
        versions_by_name
            .entry(pkg.id.name().clone())
            .or_default()
            .push((target.source.clone(), pkg.id.clone()));
    }
    versions_by_name
        .into_values()
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

    #[test]
    fn resolve_spec_from_installed_packages_returns_resolved_when_matching_package_is_installed() {
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(|_cx, db| {
            testing::mark_as_installed(db, &PKG);

            let target = resolve_spec_from_installed_packages(db, &spec, true);
            assert_matches!(
                target,
                ResolveState::Resolved(ResolvedInstallTarget {
                    source: InstallTargetSource::Installed,
                    pkg: resolved_pkg,
                    should_activate: true,
                }) if Arc::ptr_eq(&resolved_pkg, &PKG)
            );
        });
    }

    #[test]
    fn resolve_spec_from_installed_packages_returns_unresolved_when_matching_package_is_incomplete_install()
     {
        let spec = PackageSpec::from_str("example-font").unwrap();

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
    fn resolve_install_targets_by_manifest_sets_file_source() {
        let path = PathBuf::from("example-font.toml");

        testing::with_context(|cx| {
            let targets =
                resolve_install_targets_by_manifest(cx, &[(path.clone(), Arc::clone(&PKG))], true)
                    .unwrap();

            assert_eq!(targets.len(), 1);
            assert_matches!(
                &targets[0],
                ResolvedInstallTarget {
                    source: InstallTargetSource::File(source_path),
                    pkg: resolved_pkg,
                    should_activate: true,
                } if source_path == &path && Arc::ptr_eq(resolved_pkg, &PKG)
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
    fn resolve_spec_from_registry_resolves_name_to_latest_package() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@0.1.0");
        testing::write_manifest(registry_dir.path(), "example-font@0.2.0");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            resolve::testing::assert_install_target_id(&target, "example-font@0.2.0");
        });
    }

    #[test]
    fn resolve_spec_from_registry_skips_pre_release_by_default() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let stable_target =
                resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            resolve::testing::assert_install_target_id(&stable_target, "example-font@1.0.0");
        });
    }

    #[test]
    fn resolve_spec_from_registry_includes_pre_release_when_requested() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font").unwrap();
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, true, true).unwrap();
            resolve::testing::assert_install_target_id(&target, "example-font@2.0.0-rc-1");
        });
    }

    #[test]
    fn resolve_spec_from_registry_allows_exact_pre_release_id_without_flag() {
        let (registry_dir, index) = testing::make_registry_index("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        testing::with_scoped_context(|cx| {
            let spec = PackageSpec::from_str("example-font@2.0.0-rc-1").unwrap();
            let mut indexes = LazyRegistryIndexes::new_for_test(cx, vec![index]);
            let target = resolve_spec_from_registry(cx, &mut indexes, &spec, false, true).unwrap();
            resolve::testing::assert_install_target_id(&target, "example-font@2.0.0-rc-1");
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
    fn check_conflicts_reports_multiple_versions_of_same_package() {
        let targets = vec![
            ResolvedInstallTarget::installed(
                &testing::make_package_definition("example-font@0.1.0"),
                true,
            ),
            ResolvedInstallTarget::installed(
                &testing::make_package_definition("example-font@0.2.0"),
                true,
            ),
        ];

        testing::with_scoped_context(|cx| {
            let err = check_conflicts(cx, &targets).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn dedup_by_id_collapses_duplicate_targets() {
        let pkg = testing::make_package_definition("example-font@0.1.0");
        let expected_pkg_id = pkg.id.clone();
        let mut targets = vec![
            ResolvedInstallTarget::installed(&pkg, true),
            ResolvedInstallTarget::installed(&pkg, true),
        ];

        dedup_by_id(&mut targets);

        assert_eq!(targets.len(), 1);
        assert_matches!(&targets[0], ResolvedInstallTarget { pkg, .. } if pkg.id == expected_pkg_id);
    }
}
