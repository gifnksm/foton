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
    engine::{ResolvedInstallTarget, resolve::registry},
    package::{PackageId, PackageManifest, PackageQualifiedName, PackageSpec},
    registry::{RegistryId, RegistryIndex, RegistryIndexError, RegistrySpec},
};

#[derive(Debug)]
struct UpdateResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UpdateResolveScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UpdateResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UpdateResolveScope<S>
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
enum UpdateResolveErrorReport {
    #[snafu(display("no installed package matches the specified package `{pkg_spec}`"))]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[snafu(display(
    concat!(
        "multiple installed packages match the specified package `{pkg_spec}`:\n",
        "{pkg_ids}",
    ),
    pkg_spec = pkg_spec,
    pkg_ids = BulletList(pkg_ids),
))]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
    #[snafu(display(
        "failed to find the latest package for `{qualified_name}` in registry `{reg_id}`"
    ))]
    FindLatestPackage {
        reg_id: RegistryId,
        qualified_name: PackageQualifiedName,
        source: RegistryIndexError,
    },
    #[snafu(display(
        concat!(
            "multiple packages match the specified package `{qualified_name}`:\n",
            "{pkg_ids}\n",
            "specify one of the matching package IDs or registry IDs listed above explicitly to disambiguate",
        ),
        qualified_name = qualified_name,
        pkg_ids = BulletList(&pkg_ids.iter().map(|(registry, id)| format!("{id} (in {registry})")).collect::<Vec<_>>()),
    ))]
    MultipleMatchingPackagesInRegistries {
        qualified_name: PackageQualifiedName,
        pkg_ids: Vec<(RegistryId, PackageId)>,
    },
}

impl From<UpdateResolveErrorReport> for ReportValue<'static> {
    fn from(report: UpdateResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn resolve_update_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    registries: &[RegistrySpec],
    pkg_specs: &[PackageSpec],
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
        .filter_map(|pkg_id| find_update_target(&cx, &indexes, &pkg_id).transpose())
        .map(|res| res.map(|manifest| ResolvedInstallTarget { manifest }))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(targets)
}

fn collect_latest_packages<I>(pkg_ids: I) -> BTreeMap<PackageQualifiedName, PackageId>
where
    I: IntoIterator<Item = PackageId>,
{
    let mut latest_packages: BTreeMap<PackageQualifiedName, PackageId> = BTreeMap::new();
    for pkg_id in pkg_ids {
        latest_packages
            .entry(pkg_id.qualified_name().clone())
            .and_modify(|existing| {
                if existing.version() < pkg_id.version() {
                    *existing = pkg_id.clone();
                }
            })
            .or_insert(pkg_id);
    }
    latest_packages
}

fn all_installed_packages(db: &PackageDatabase<'_>) -> Vec<PackageId> {
    collect_latest_packages(
        db.entries()
            .filter_map(|(state, manifest)| state.is_installed().then(|| manifest.metadata.id())),
    )
    .into_values()
    .collect()
}

fn resolve_installed_package<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<PackageId, S::Error>
where
    S: ReportScope,
{
    let latest_packages = collect_latest_packages(
        db.entries_by_spec(pkg_spec)
            .filter_map(|(state, manifest)| state.is_installed().then(|| manifest.metadata.id())),
    );
    if latest_packages.len() > 1 {
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesSnafu {
                pkg_spec,
                pkg_ids: latest_packages.values().cloned().collect::<Vec<_>>(),
            }
            .build(),
        ));
    }
    let Some(pkg_id) = latest_packages.into_values().next() else {
        return Err(cx
            .reporter()
            .report_error(NoMatchingPackageSnafu { pkg_spec }.build()));
    };
    Ok(pkg_id)
}

fn dedup(pkg_ids: &mut Vec<PackageId>) {
    let latest_versions = collect_latest_packages(pkg_ids.iter().cloned());
    let mut seen = BTreeSet::new();
    pkg_ids.retain(|pkg_id| {
        latest_versions.get(pkg_id.qualified_name()) == Some(pkg_id) && seen.insert(pkg_id.clone())
    });
}

fn find_update_target<S>(
    cx: &ReportContext<UpdateResolveScope<S>>,
    indexes: &[RegistryIndex],
    pkg_id: &PackageId,
) -> Result<Option<Arc<PackageManifest>>, S::Error>
where
    S: ReportScope,
{
    let mut manifests = vec![];
    for index in indexes {
        let manifest = index
            .find_latest_package_by_qualified_name(pkg_id.qualified_name())
            .context(FindLatestPackageSnafu {
                reg_id: index.id(),
                qualified_name: pkg_id.qualified_name(),
            })
            .report_error(cx.reporter())?
            .filter(|manifest| &manifest.metadata.version > pkg_id.version());
        if let Some(manifest) = manifest {
            manifests.push((index.id().clone(), Arc::new(manifest)));
        }
    }
    if manifests.len() > 1 {
        let pkg_ids = manifests
            .into_iter()
            .map(|(reg_id, manifest)| (reg_id, manifest.metadata.id()))
            .collect::<Vec<_>>();
        return Err(cx.reporter().report_error(
            MultipleMatchingPackagesInRegistriesSnafu {
                qualified_name: pkg_id.qualified_name().clone(),
                pkg_ids,
            }
            .build(),
        ));
    }
    Ok(manifests
        .into_iter()
        .next()
        .map(|(_registry, manifest)| manifest))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

    #[test]
    fn resolve_update_targets_updates_installed_packages_with_newer_versions_only() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-namespace/update-font@2.0.0");
        testing::write_manifest(registry_dir.path(), "example-namespace/current-font@1.0.0");
        testing::write_manifest(registry_dir.path(), "example-namespace/pending-font@2.0.0");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        testing::with_db(&cx, |mut db| {
            let update_manifest = testing::make_manifest("example-namespace/update-font@1.0.0");
            let current_manifest = testing::make_manifest("example-namespace/current-font@1.0.0");
            let pending_manifest = testing::make_manifest("example-namespace/pending-font@1.0.0");

            testing::mark_as_installed(&mut db, &update_manifest);
            testing::mark_as_installed(&mut db, &current_manifest);
            testing::mark_as_pending_installed(&mut db, &pending_manifest);

            let targets = resolve_update_targets(&cx, &db, &[registry], &[]).unwrap();
            let target_ids = targets
                .iter()
                .map(|target| target.manifest.metadata.id().to_string())
                .collect::<Vec<_>>();

            assert_eq!(target_ids, vec!["example-namespace/update-font@2.0.0"]);
        });
    }

    #[test]
    fn resolve_update_targets_collapses_duplicate_specs_for_same_installed_package() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-namespace/example-font@2.0.0");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let pkg_specs = vec![
            PackageSpec::from_str("example-namespace/example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-namespace/example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            let manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
            testing::mark_as_installed(&mut db, &manifest);

            let targets = resolve_update_targets(&cx, &db, &[registry], &pkg_specs).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(
                targets[0].manifest.metadata.id().to_string(),
                "example-namespace/example-font@2.0.0"
            );
        });
    }

    #[test]
    fn resolve_update_targets_prefers_latest_installed_version_for_duplicate_package_names() {
        let (registry_dir, registry) = testing::make_registry_spec("test-registry");
        testing::write_manifest(registry_dir.path(), "example-namespace/example-font@2.0.0");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let pkg_specs = vec![
            PackageSpec::from_str("example-namespace/example-font@1.0.0").unwrap(),
            PackageSpec::from_str("example-namespace/example-font@3.0.0").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            let older_manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
            let newer_manifest = testing::make_manifest("example-namespace/example-font@3.0.0");
            testing::mark_as_installed(&mut db, &older_manifest);
            testing::mark_as_installed(&mut db, &newer_manifest);

            let targets = resolve_update_targets(&cx, &db, &[registry], &pkg_specs).unwrap();

            assert!(targets.is_empty());
        });
    }

    #[test]
    fn resolve_installed_package_reports_multiple_matches_for_name() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UpdateResolveScope::start(&cx);

        let manifest1 = testing::make_manifest("example-namespace/example-font@0.1.0");
        let manifest2 = testing::make_manifest("other-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest1);
            testing::mark_as_installed(&mut db, &manifest2);

            let err = resolve_installed_package(&cx, &db, &spec).unwrap_err();
            assert!(matches!(err, TestError::Failed));
        });
    }

    #[test]
    fn resolve_update_targets_reports_multiple_matching_packages_across_registries() {
        let (registry_dir1, registry1) = testing::make_registry_spec("registry-a");
        let (registry_dir2, registry2) = testing::make_registry_spec("registry-b");
        testing::write_manifest(registry_dir1.path(), "example-namespace/example-font@2.0.0");
        testing::write_manifest(registry_dir2.path(), "example-namespace/example-font@3.0.0");

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let registries = [registry1, registry2];
        let pkg_specs = vec![PackageSpec::from_str("example-namespace/example-font").unwrap()];

        testing::with_db(&cx, |mut db| {
            let manifest = testing::make_manifest("example-namespace/example-font@1.0.0");
            testing::mark_as_installed(&mut db, &manifest);

            let err = resolve_update_targets(&cx, &db, &registries, &pkg_specs).unwrap_err();

            assert!(matches!(err, TestError::Failed));
        });
    }
}
