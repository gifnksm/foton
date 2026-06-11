use std::{
    collections::{BTreeMap, BTreeSet, btree_map},
    marker::PhantomData,
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
    db::{PackageDatabase, PackageDbEntry},
    package::{PackageId, PackageManifest, PackageName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
    util::macros::concat_line,
};

use super::{InstallTargetSource, ResolvedInstallTarget, lookup};

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

#[derive(Debug, Clone)]
struct UpdateCandidate {
    pkg_id: PackageId,
    should_activate: bool,
}

impl UpdateCandidate {
    fn new(pkg_id: PackageId, should_activate: bool) -> Self {
        Self {
            pkg_id,
            should_activate,
        }
    }

    fn from_db_entry(entry: &PackageDbEntry<'_>) -> Self {
        Self::new(entry.manifest().id(), entry.activation_state().is_active())
    }

    fn resolved(self, reg_id: RegistryId, manifest: Arc<PackageManifest>) -> ResolvedInstallTarget {
        ResolvedInstallTarget {
            source: InstallTargetSource::Registry(reg_id),
            manifest,
            should_activate: self.should_activate,
        }
    }
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
    let mut candidates = collect_requested_packages(&cx, db, pkg_specs)?;

    if candidates.is_empty() {
        return Ok(vec![]);
    }

    dedup_candidates(&mut candidates);

    let indexes = super::fetch_registries(&cx, registries)?;
    let targets = candidates
        .into_iter()
        .filter_map(|candidate| {
            let res = find_update_target(&cx, &indexes, &candidate.pkg_id, include_pre_release)
                .transpose()?;
            Some(res.map(|(reg_id, manifest)| candidate.resolved(reg_id, manifest)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(targets)
}

fn collect_requested_packages<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Result<Vec<UpdateCandidate>, S::Error>
where
    S: ReportScope,
{
    if pkg_specs.is_empty() {
        Ok(all_installed_packages(db))
    } else {
        pkg_specs
            .iter()
            .map(|pkg_spec| resolve_spec_from_installed_packages(cx, db, pkg_spec))
            .collect_to_end()
    }
}

fn collect_latest_packages<I>(candidates: I) -> BTreeMap<PackageName, UpdateCandidate>
where
    I: IntoIterator<Item = UpdateCandidate>,
{
    let mut latest_packages = BTreeMap::new();
    for candidate in candidates {
        match latest_packages.entry(candidate.pkg_id.name().clone()) {
            btree_map::Entry::Vacant(e) => {
                e.insert(candidate);
            }
            btree_map::Entry::Occupied(mut e) => {
                if e.get().pkg_id.version() < candidate.pkg_id.version() {
                    e.insert(candidate);
                }
            }
        }
    }
    latest_packages
}

fn all_installed_packages(db: &PackageDatabase<'_>) -> Vec<UpdateCandidate> {
    collect_latest_packages(db.entries().filter_map(|entry| {
        entry
            .installation_state()
            .is_installed()
            .then(|| UpdateCandidate::from_db_entry(&entry))
    }))
    .into_values()
    .collect()
}

fn resolve_spec_from_installed_packages<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<UpdateCandidate, S::Error>
where
    S: ReportScope,
{
    let latest_packages =
        collect_latest_packages(db.entries_by_spec(pkg_spec).filter_map(|entry| {
            entry
                .installation_state()
                .is_installed()
                .then(|| UpdateCandidate::from_db_entry(&entry))
        }));
    if latest_packages.len() > 1 {
        return Err(MultipleMatchingPackagesSnafu {
            pkg_spec,
            pkg_ids: latest_packages
                .values()
                .map(|candidate| candidate.pkg_id.clone())
                .collect::<Vec<_>>(),
        }
        .build()
        .report_error(&cx));
    }
    let Some(candidate) = latest_packages.into_values().next() else {
        return Err(NoMatchingPackageSnafu { pkg_spec }
            .build()
            .report_error(&cx));
    };
    Ok(candidate)
}

fn dedup_candidates(candidates: &mut Vec<UpdateCandidate>) {
    let latest_versions = collect_latest_packages(candidates.iter().cloned());
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| {
        latest_versions
            .get(candidate.pkg_id.name())
            .is_some_and(|latest| latest.pkg_id == candidate.pkg_id)
            && seen.insert(candidate.pkg_id.clone())
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
    let matches = lookup::collect_registry_matches(indexes, |index| {
        index
            .find_latest_package_by_name(pkg_id.name(), include_pre_release)
            .context(FindLatestPackageSnafu {
                reg_id: index.id(),
                name: pkg_id.name(),
            })
            .report_error(cx)
            .map(|manifest| manifest.filter(|manifest| &manifest.version > pkg_id.version()))
    })?;

    lookup::into_unique_match(matches).map_err(|matches| {
        let pkg_ids = matches
            .into_iter()
            .map(|(reg_id, manifest)| (reg_id, manifest.id()))
            .collect::<Vec<_>>();
        MultipleMatchingPackagesInRegistriesSnafu {
            name: pkg_id.name(),
            pkg_ids,
        }
        .build()
        .report_error(cx)
    })
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use crate::{engine::resolve, util::testing};

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
            resolve::testing::assert_install_target_ids(&targets, &["update-font@2.0.0"]);
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

            resolve::testing::assert_single_install_target_id(&targets, "example-font@2.0.0");
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

            resolve::testing::assert_single_install_target(&targets, "example-font@4.0.0", false);
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

            resolve::testing::assert_single_install_target_id(&targets, "example-font@1.0.1");
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

            resolve::testing::assert_single_install_target_id(&targets, "example-font@2.0.0-rc-1");
        });
    }
}
