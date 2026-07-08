use std::{
    collections::{BTreeMap, BTreeSet},
    marker::PhantomData,
};

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
    engine::ResolvedActivateTarget,
    package::{PackageId, PackageName, PackageSpec},
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct ActivateResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ActivateResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ActivateResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ActivateResolveScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum ActivateResolveErrorReport {
    #[snafu(display("no installed package matches `{pkg_spec}`"))]
    NoMatchingPackage { pkg_spec: PackageSpec },
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
    #[snafu(display(
        concat_line!(
            "multiple versions of the same package would be activated:",
            "{pkg_ids}",
            "activate only one version",
        ),
        pkg_ids = BulletList(pkg_ids),
    ))]
    ConflictingActivations { pkg_ids: Vec<PackageId> },
}

pub(crate) fn resolve_activate_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Result<Vec<ResolvedActivateTarget>, S::Error>
where
    S: ReportScope,
{
    let cx =
        ActivateResolveScope::start_with_report(cx, format_args!("Resolving activate target..."));
    let mut targets = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_spec(&cx, db, pkg_spec))
        .collect_to_end::<Vec<_>>()?;
    dedup_targets(&mut targets);
    check_conflicting_activations(&cx, &targets)?;
    Ok(targets)
}

fn resolve_spec<S>(
    cx: &ReportContext<ActivateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<ResolvedActivateTarget, S::Error>
where
    S: ReportScope,
{
    let candidates = db
        .entries_by_spec(pkg_spec)
        .filter(|entry| entry.installation_state().is_installed())
        .collect::<Vec<_>>();
    match &candidates[..] {
        [] => Err(NoMatchingPackageSnafu { pkg_spec }.build().report_error(cx)),
        [entry] => Ok(ResolvedActivateTarget {
            pkg_id: entry.id().clone(),
        }),
        _ => Err(MultipleMatchingPackagesSnafu {
            pkg_spec: pkg_spec.clone(),
            pkg_ids: candidates
                .into_iter()
                .map(|entry| entry.id().clone())
                .collect::<Vec<_>>(),
        }
        .build()
        .report_error(cx)),
    }
}

fn dedup_targets(targets: &mut Vec<ResolvedActivateTarget>) {
    let mut seen_pkg_id = BTreeSet::new();
    targets.retain(|target| seen_pkg_id.insert(target.pkg_id.clone()));
}

fn check_conflicting_activations<S>(
    cx: &ReportContext<ActivateResolveScope<S>>,
    targets: &[ResolvedActivateTarget],
) -> Result<(), S::Error>
where
    S: ReportScope,
{
    let mut versions_by_name = BTreeMap::<PackageName, Vec<_>>::new();
    for target in targets {
        versions_by_name
            .entry(target.pkg_id.name().clone())
            .or_default()
            .push(target.pkg_id.clone());
    }
    versions_by_name
        .into_values()
        .map(|pkg_ids| {
            if pkg_ids.len() > 1 {
                Err(ConflictingActivationsSnafu { pkg_ids }
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
    use std::{str::FromStr as _, sync::LazyLock};

    use super::*;
    use crate::{package::PackageDefinition, util::testing};

    static PKG: LazyLock<PackageDefinition> =
        LazyLock::new(|| testing::make_package_definition("example-font@0.1.0"));
    static EXACT_SPEC: LazyLock<PackageSpec> =
        LazyLock::new(|| PackageSpec::from_str("example-font@0.1.0").unwrap());
    static NAME_SPEC: LazyLock<PackageSpec> =
        LazyLock::new(|| PackageSpec::from_str("example-font").unwrap());

    #[test]
    fn resolve_spec_reports_missing_package() {
        testing::with_scoped_db(|cx, db| {
            let err = resolve_spec(cx, db, &NAME_SPEC).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_name() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            for spec in [EXACT_SPEC.clone(), NAME_SPEC.clone()] {
                let target = resolve_spec(cx, db, &spec).unwrap();
                assert_eq!(target.pkg_id, PKG.id);
            }
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let pkg1 = testing::make_package_definition("example-font@0.1.0");
        let pkg2 = testing::make_package_definition("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_scoped_db(|cx, db| {
            testing::mark_as_installed(db, &pkg1);
            testing::mark_as_installed(db, &pkg2);

            let err = resolve_spec(cx, db, &spec).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_activate_targets_collapses_duplicate_specs() {
        let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let targets = resolve_activate_targets(cx, db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].pkg_id, PKG.id);
        });
    }

    #[test]
    fn resolve_activate_targets_reports_conflicting_requested_versions() {
        let pkg_specs = vec![
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font@1.0.0").unwrap(),
        ];
        let pkg1 = testing::make_package_definition("example-font@0.1.0");
        let pkg2 = testing::make_package_definition("example-font@1.0.0");

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &pkg1);
            testing::mark_as_installed(db, &pkg2);

            let err = resolve_activate_targets(cx, db, &pkg_specs).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_spec_ignores_incomplete_install_entries() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_incomplete_install(db, &PKG);

            let err = resolve_spec(cx, db, &NAME_SPEC).unwrap_err();
            assert!(err.is_failed());
        });
    }
}
