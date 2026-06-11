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
            NeverReport, ReportScope, ResultIteratorExt as _, ScopeResultErrorExt as _,
            SubReportScope,
        },
    },
    db::PackageDatabase,
    engine::{ResolvedInstallTarget, resolve::registry},
    package::{PackageId, PackageManifest, PackageName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
    util::macros::concat_line,
};

use super::InstallTargetSource;

#[derive(Debug, Default)]
struct UpdateResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UpdateResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UpdateResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UpdateResolveScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum UpdateResolveErrorReport {
    #[snafu(display("no installed package matches the specified package `{pkg_spec}`"))]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[snafu(display(
    concat_line!(
        "multiple installed packages match the specified package `{pkg_spec}`:",
        "{pkg_ids}",
    ),
    pkg_spec = pkg_spec,
    pkg_ids = BulletList(pkg_ids),
))]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
    #[snafu(display("failed to find the latest package for `{name}` in registry `{reg_id}`"))]
    FindLatestPackage {
        reg_id: RegistryId,
        name: PackageName,
        source: RegistryIndexError,
    },
    #[snafu(display(
        concat_line!(
            "multiple packages match the specified package `{name}`:",
            "{pkg_ids}",
            "specify one of the matching package IDs or registry IDs listed above explicitly to disambiguate",
        ),
        name = name,
        pkg_ids = BulletList(&pkg_ids.iter().map(|(registry, id)| format!("{id} (in {registry})")).collect::<Vec<_>>()),
    ))]
    MultipleMatchingPackagesInRegistries {
        name: PackageName,
        pkg_ids: Vec<(RegistryId, PackageId)>,
    },
}

pub(crate) fn resolve_update_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    registries: &[RegistrySpec],
    pkg_specs: &[PackageSpec],
    include_pre_release: bool,
) -> Result<Vec<ResolvedInstallTarget>, S::Error>
where
    S: ReportScope,
{
    let cx = UpdateResolveScope::start_with_report(cx, format_args!("Resolving update target..."));
    let mut pkg_ids = if pkg_specs.is_empty() {
        all_installed_packages(db)
    } else {
        pkg_specs
            .iter()
            .map(|pkg_spec| resolve_installed_package(&cx, db, pkg_spec))
            .collect_to_end()?
    };

    if pkg_ids.is_empty() {
        return Ok(vec![]);
    }

    dedup(&mut pkg_ids);

    let indexes = registry::fetch_registries(&cx, registries)?;
    let targets = pkg_ids
        .into_iter()
        .filter_map(|(pkg_id, should_activate)| {
            let res =
                find_update_target(&cx, &indexes, &pkg_id, include_pre_release).transpose()?;
            Some(res.map(|(reg_id, manifest)| (reg_id, manifest, should_activate)))
        })
        .map(|res| {
            res.map(
                |(reg_id, manifest, should_activate)| ResolvedInstallTarget {
                    source: InstallTargetSource::Registry(reg_id),
                    manifest,
                    should_activate,
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(targets)
}

fn collect_latest_packages<I>(pkg_ids: I) -> BTreeMap<PackageName, (PackageId, bool)>
where
    I: IntoIterator<Item = (PackageId, bool)>,
{
    let mut latest_packages: BTreeMap<PackageName, (PackageId, bool)> = BTreeMap::new();
    for (pkg_id, should_activate) in pkg_ids {
        latest_packages
            .entry(pkg_id.name().clone())
            .and_modify(|existing| {
                if existing.0.version() < pkg_id.version() {
                    *existing = (pkg_id.clone(), should_activate);
                }
            })
            .or_insert((pkg_id, should_activate));
    }
    latest_packages
}

fn all_installed_packages(db: &PackageDatabase<'_>) -> Vec<(PackageId, bool)> {
    collect_latest_packages(db.entries().filter_map(|entry| {
        entry
            .installation_state()
            .is_installed()
            .then(|| (entry.manifest().id(), entry.activation_state().is_active()))
    }))
    .into_values()
    .collect()
}

fn resolve_installed_package<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<(PackageId, bool), S::Error>
where
    S: ReportScope,
{
    let latest_packages =
        collect_latest_packages(db.entries_by_spec(pkg_spec).filter_map(|entry| {
            entry
                .installation_state()
                .is_installed()
                .then(|| (entry.manifest().id(), entry.activation_state().is_active()))
        }));
    if latest_packages.len() > 1 {
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesSnafu {
                pkg_spec,
                pkg_ids: latest_packages
                    .values()
                    .map(|(pkg_id, _activate)| pkg_id)
                    .cloned()
                    .collect::<Vec<_>>(),
            }
            .build(),
        ));
    }
    let Some((pkg_id, should_activate)) = latest_packages.into_values().next() else {
        return Err(cx
            .reporter()
            .report_error(NoMatchingPackageSnafu { pkg_spec }.build()));
    };
    Ok((pkg_id, should_activate))
}

fn dedup(pkg_ids: &mut Vec<(PackageId, bool)>) {
    let latest_versions = collect_latest_packages(pkg_ids.iter().cloned());
    let mut seen = BTreeSet::new();
    pkg_ids.retain(|(pkg_id, _activate)| {
        latest_versions
            .get(pkg_id.name())
            .is_some_and(|(latest_pkg_id, _activate)| latest_pkg_id == pkg_id)
            && seen.insert(pkg_id.clone())
    });
}

fn find_update_target<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    indexes: &[RegistryIndex],
    pkg_id: &PackageId,
    include_pre_release: bool,
) -> Result<Option<(RegistryId, Arc<PackageManifest>)>, S::Error>
where
    S: ReportScope,
{
    let mut manifests = vec![];
    for index in indexes {
        let manifest = index
            .find_latest_package_by_name(pkg_id.name(), include_pre_release)
            .context(FindLatestPackageSnafu {
                reg_id: index.id(),
                name: pkg_id.name(),
            })
            .report_error(cx.reporter())?
            .filter(|manifest| &manifest.version > pkg_id.version());
        if let Some(manifest) = manifest {
            manifests.push((index.id().clone(), manifest));
        }
    }
    if manifests.len() > 1 {
        let pkg_ids = manifests
            .into_iter()
            .map(|(reg_id, manifest)| (reg_id, manifest.id()))
            .collect::<Vec<_>>();
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesInRegistriesSnafu {
                name: pkg_id.name(),
                pkg_ids,
            }
            .build(),
        ));
    }
    Ok(manifests.into_iter().next())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use crate::util::testing;

    #[test]
    fn resolve_update_targets_updates_installed_packages_with_newer_versions_only() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "update-font@2.0.0");
        testing::write_manifest(registry_dir.path(), "current-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "incomplete-font@2.0.0");

        testing::with_db(|cx, db| {
            let update_manifest = testing::make_manifest("update-font@1.0.0");
            let current_manifest = testing::make_manifest("current-font@1.0.0");
            let incomplete_manifest = testing::make_manifest("incomplete-font@1.0.0");

            testing::mark_as_installed(db, &update_manifest);
            testing::mark_as_installed(db, &current_manifest);
            testing::mark_as_incomplete_install(db, &incomplete_manifest);

            let targets = resolve_update_targets(cx, db, &[registry], &[], false).unwrap();
            let target_ids = targets
                .iter()
                .map(|target| target.manifest.id().to_string())
                .collect::<Vec<_>>();

            assert_eq!(target_ids, vec!["update-font@2.0.0"]);
        });
    }

    #[test]
    fn resolve_update_targets_collapses_duplicate_specs_for_same_installed_package() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0");

        let pkg_specs = vec![
            PackageSpec::from_str("example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            let manifest = testing::make_manifest("example-font@1.0.0");
            testing::mark_as_installed(db, &manifest);

            let targets = resolve_update_targets(cx, db, &[registry], &pkg_specs, false).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].manifest.id().to_string(), "example-font@2.0.0");
        });
    }

    #[test]
    fn resolve_update_targets_prefers_latest_installed_version_when_multiple_versions_are_installed()
     {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0");

        let pkg_specs = vec![
            PackageSpec::from_str("example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-font@3.0.0").unwrap(),
        ];

        testing::with_db(|cx, db| {
            let older_manifest = testing::make_manifest("example-font@1.0.0");
            let newer_manifest = testing::make_manifest("example-font@3.0.0");
            testing::mark_as_installed(db, &older_manifest);
            testing::mark_as_installed(db, &newer_manifest);

            let targets = resolve_update_targets(cx, db, &[registry], &pkg_specs, false).unwrap();

            assert!(targets.is_empty());
        });
    }

    #[test]
    fn resolve_update_targets_inherits_activation_intent_from_selected_installed_version() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@4.0.0");

        let pkg_specs = vec![PackageSpec::from_str("example-font").unwrap()];

        testing::with_db(|cx, db| {
            let older_manifest = testing::make_manifest("example-font@1.0.0");
            let newer_manifest = testing::make_manifest("example-font@3.0.0");
            testing::mark_as_active(db, &older_manifest);
            testing::mark_as_installed(db, &newer_manifest);

            let targets = resolve_update_targets(cx, db, &[registry], &pkg_specs, false).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].manifest.id().to_string(), "example-font@4.0.0");
            assert!(!targets[0].should_activate);
        });
    }

    #[test]
    fn resolve_update_targets_reports_multiple_matching_packages_across_registries() {
        let (registry_dir1, registry1) = testing::make_registry_spec("registry-a");
        let (registry_dir2, registry2) = testing::make_registry_spec("registry-b");
        testing::write_manifest(registry_dir1.path(), "example-font@2.0.0");
        testing::write_manifest(registry_dir2.path(), "example-font@3.0.0");

        let registries = [registry1, registry2];
        let pkg_specs = vec![PackageSpec::from_str("example-font").unwrap()];

        testing::with_db(|cx, db| {
            let manifest = testing::make_manifest("example-font@1.0.0");
            testing::mark_as_installed(db, &manifest);

            let err = resolve_update_targets(cx, db, &registries, &pkg_specs, false).unwrap_err();

            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_update_targets_skips_pre_release_by_default() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.1");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        let registries = [registry];

        testing::with_db(|cx, db| {
            let manifest = testing::make_manifest("example-font@1.0.0");
            testing::mark_as_installed(db, &manifest);

            let targets = resolve_update_targets(cx, db, &registries, &[], false).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].manifest.id().to_string(), "example-font@1.0.1");
        });
    }

    #[test]
    fn resolve_update_targets_includes_pre_release_when_requested() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-font@1.0.1");
        testing::write_manifest(registry_dir.path(), "example-font@2.0.0-rc-1");

        let registries = [registry];

        testing::with_db(|cx, db| {
            let manifest = testing::make_manifest("example-font@1.0.0");
            testing::mark_as_installed(db, &manifest);

            let targets = resolve_update_targets(cx, db, &registries, &[], true).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(
                targets[0].manifest.id().to_string(),
                "example-font@2.0.0-rc-1"
            );
        });
    }
}
