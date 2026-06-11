use std::{collections::BTreeSet, marker::PhantomData};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{
            ErrorReportExt as _, NeverReport, ReportScope, ResultIteratorExt as _, SubReportScope,
        },
    },
    db::PackageDatabase,
    package::{PackageId, PackageSpec},
    util::macros::concat_line,
};

use super::ResolvedUninstallTarget;

#[derive(Debug, Default)]
struct UninstallResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for UninstallResolveScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum UninstallResolveErrorReport {
    #[snafu(display(
    concat_line!(
        "multiple packages match the specified package `{pkg_spec}`:",
        "{pkg_ids}",
    ),
    pkg_spec = pkg_spec,
    pkg_ids = BulletList(pkg_ids),
))]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
}

pub(crate) fn resolve_uninstall_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Result<Vec<ResolvedUninstallTarget>, S::Error>
where
    S: ReportScope,
{
    let cx =
        UninstallResolveScope::start_with_report(cx, format_args!("Resolving uninstall target..."));
    let mut targets = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_spec(&cx, db, pkg_spec))
        .collect_to_end::<Vec<_>>()?;
    dedup_targets(&mut targets);
    Ok(targets)
}

fn resolve_spec<S>(
    cx: &ReportContext<UninstallResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<ResolvedUninstallTarget, S::Error>
where
    S: ReportScope,
{
    let candidates = db.entries_by_spec(pkg_spec).collect::<Vec<_>>();
    match &candidates[..] {
        [] => Ok(ResolvedUninstallTarget::AlreadyUninstalled {
            pkg_spec: pkg_spec.clone(),
        }),
        [entry] => Ok(ResolvedUninstallTarget::Uninstall {
            pkg_id: entry.manifest().id(),
            should_deactivate: !entry.activation_state().is_inactive(),
        }),
        _ => Err(MultipleMatchingPackagesSnafu {
            pkg_spec: pkg_spec.clone(),
            pkg_ids: candidates
                .into_iter()
                .map(|entry| entry.manifest().id())
                .collect::<Vec<_>>(),
        }
        .build()
        .report_error(&cx)),
    }
}

fn dedup_targets(targets: &mut Vec<ResolvedUninstallTarget>) {
    let mut seen_pkg_id = BTreeSet::new();
    let mut seen_pkg_spec = BTreeSet::new();
    targets.retain(|target| match target {
        ResolvedUninstallTarget::Uninstall {
            pkg_id,
            should_deactivate: _,
        } => seen_pkg_id.insert(pkg_id.clone()),
        ResolvedUninstallTarget::AlreadyUninstalled { pkg_spec } => {
            seen_pkg_spec.insert(pkg_spec.clone())
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr as _,
        sync::{Arc, LazyLock},
    };

    use super::*;
    use crate::{engine::resolve, package::PackageManifest, util::testing};

    static MANIFEST: LazyLock<Arc<PackageManifest>> =
        LazyLock::new(|| testing::make_manifest("example-font@0.1.0"));
    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| MANIFEST.id());
    static EXACT_SPEC: LazyLock<PackageSpec> =
        LazyLock::new(|| PackageSpec::from_str("example-font@0.1.0").unwrap());
    static NAME_SPEC: LazyLock<PackageSpec> =
        LazyLock::new(|| PackageSpec::from_str("example-font").unwrap());

    #[test]
    fn resolve_spec_returns_already_uninstalled_for_missing_specs() {
        testing::with_db(|cx, db| {
            let scope_cx = UninstallResolveScope::start(cx);
            let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];
            for spec in &pkg_specs {
                let target = resolve_spec(&scope_cx, db, spec).unwrap();
                resolve::testing::assert_already_uninstalled(&target, spec);
            }

            let targets = resolve_uninstall_targets(cx, db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 2);
        });
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_name_without_deactivation() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_installed(db, &MANIFEST);

            for spec in [EXACT_SPEC.clone(), NAME_SPEC.clone()] {
                let target = resolve_spec(cx, db, &spec).unwrap();
                resolve::testing::assert_uninstall_target(&target, &PKG_ID, false);
            }
        });
    }

    #[test]
    fn resolve_spec_requests_deactivation_for_active_entries() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_active(db, &MANIFEST);

            let target = resolve_spec(cx, db, &NAME_SPEC).unwrap();
            resolve::testing::assert_uninstall_target(&target, &PKG_ID, true);
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_scoped_db(|cx, db| {
            testing::mark_as_installed(db, &manifest1);
            testing::mark_as_installed(db, &manifest2);

            let err = resolve_spec(cx, db, &spec).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_uninstall_targets_reports_ambiguous_spec_even_with_exact_target() {
        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let pkg_specs = vec![
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &manifest1);
            testing::mark_as_installed(db, &manifest2);

            let err = resolve_uninstall_targets(cx, db, &pkg_specs).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_uninstall_targets_collapses_duplicate_specs() {
        let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &MANIFEST);
            let targets = resolve_uninstall_targets(cx, db, &pkg_specs).unwrap();

            assert_eq!(targets.len(), 1);
            resolve::testing::assert_uninstall_target(&targets[0], &PKG_ID, false);
        });
    }

    #[test]
    fn resolve_spec_resolves_incomplete_entries_without_deactivation() {
        testing::with_scoped_db(|cx, db| {
            for mark_as_state in [
                testing::mark_as_incomplete_install,
                testing::mark_as_incomplete_uninstall,
            ] {
                mark_as_state(db, &MANIFEST);

                let target = resolve_spec(cx, db, &NAME_SPEC).unwrap();
                resolve::testing::assert_uninstall_target(&target, &PKG_ID, false);
            }
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name_across_incomplete_states() {
        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_scoped_db(|cx, db| {
            testing::mark_as_incomplete_install(db, &manifest1);
            testing::mark_as_incomplete_uninstall(db, &manifest2);

            let err = resolve_spec(cx, db, &spec).unwrap_err();
            assert!(err.is_failed());
        });
    }
}
