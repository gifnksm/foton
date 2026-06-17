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
    engine::ResolvedDeactivateTarget,
    package::{PackageId, PackageSpec},
    util::macros::concat_line,
};

#[derive(Debug, Default)]
struct DeactivateResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for DeactivateResolveScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DeactivateResolveErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for DeactivateResolveScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum DeactivateResolveErrorReport {
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

pub(crate) fn resolve_deactivate_targets<S>(
    cx: &ReportContext<S>,
    db: &PackageDatabase<'_>,
    pkg_specs: &[PackageSpec],
) -> Result<Vec<ResolvedDeactivateTarget>, S::Error>
where
    S: ReportScope,
{
    let cx = DeactivateResolveScope::start_with_report(
        cx,
        format_args!("Resolving deactivate target..."),
    );
    let mut targets = pkg_specs
        .iter()
        .map(|pkg_spec| resolve_spec(&cx, db, pkg_spec))
        .collect_to_end::<Vec<_>>()?;
    dedup_targets(&mut targets);
    Ok(targets)
}

fn resolve_spec<S>(
    cx: &ReportContext<DeactivateResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<ResolvedDeactivateTarget, S::Error>
where
    S: ReportScope,
{
    let candidates = db
        .entries_by_spec(pkg_spec)
        .filter(|entry| entry.installation_state().is_installed())
        .collect::<Vec<_>>();
    match &candidates[..] {
        [] => Ok(ResolvedDeactivateTarget::AlreadyInactive {
            pkg_spec: pkg_spec.clone(),
        }),
        [entry] => {
            if entry.activation_state().is_inactive() {
                Ok(ResolvedDeactivateTarget::AlreadyInactive {
                    pkg_spec: entry.id().clone().into(),
                })
            } else {
                Ok(ResolvedDeactivateTarget::Deactivate {
                    pkg_id: entry.id().clone(),
                })
            }
        }
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

fn dedup_targets(targets: &mut Vec<ResolvedDeactivateTarget>) {
    let mut seen_pkg_id = BTreeSet::new();
    let mut seen_pkg_spec = BTreeSet::new();
    targets.retain(|target| match target {
        ResolvedDeactivateTarget::Deactivate { pkg_id } => seen_pkg_id.insert(pkg_id.clone()),
        ResolvedDeactivateTarget::AlreadyInactive { pkg_spec } => {
            seen_pkg_spec.insert(pkg_spec.clone())
        }
    });
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
    fn resolve_spec_returns_already_inactive_for_missing_package() {
        testing::with_scoped_db(|cx, db| {
            let target = resolve_spec(cx, db, &NAME_SPEC).unwrap();
            assert!(matches!(
                target,
                ResolvedDeactivateTarget::AlreadyInactive { pkg_spec } if pkg_spec == *NAME_SPEC
            ));
        });
    }

    #[test]
    fn resolve_spec_resolves_active_entries_from_id_and_name() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_active(db, &PKG);

            for spec in [EXACT_SPEC.clone(), NAME_SPEC.clone()] {
                let target = resolve_spec(cx, db, &spec).unwrap();
                assert!(matches!(
                    target,
                    ResolvedDeactivateTarget::Deactivate { pkg_id } if pkg_id == PKG.id
                ));
            }
        });
    }

    #[test]
    fn resolve_spec_returns_already_inactive_for_inactive_entries() {
        testing::with_scoped_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let target = resolve_spec(cx, db, &NAME_SPEC).unwrap();
            assert!(matches!(
                target,
                ResolvedDeactivateTarget::AlreadyInactive { pkg_spec }
                    if pkg_spec == PackageSpec::from(PKG.id.clone())
            ));
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let pkg1 = testing::make_package_definition("example-font@0.1.0");
        let pkg2 = testing::make_package_definition("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_scoped_db(|cx, db| {
            testing::mark_as_active(db, &pkg1);
            testing::mark_as_installed(db, &pkg2);

            let err = resolve_spec(cx, db, &spec).unwrap_err();
            assert!(err.is_failed());
        });
    }

    #[test]
    fn resolve_deactivate_targets_collapses_duplicate_specs_for_same_active_package() {
        let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];

        testing::with_db(|cx, db| {
            testing::mark_as_active(db, &PKG);

            let targets = resolve_deactivate_targets(cx, db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 1);
            assert!(matches!(
                &targets[0],
                ResolvedDeactivateTarget::Deactivate { pkg_id } if *pkg_id == PKG.id
            ));
        });
    }

    #[test]
    fn resolve_deactivate_targets_collapses_duplicate_specs_for_same_inactive_package() {
        let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];

        testing::with_db(|cx, db| {
            testing::mark_as_installed(db, &PKG);

            let targets = resolve_deactivate_targets(cx, db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 1);
            assert!(matches!(
                &targets[0],
                ResolvedDeactivateTarget::AlreadyInactive { pkg_spec }
                    if *pkg_spec == PackageSpec::from(PKG.id.clone())
            ));
        });
    }

    #[test]
    fn resolve_spec_resolves_incomplete_activation_states_for_cleanup() {
        testing::with_scoped_db(|cx, db| {
            for mark_as_state in [
                testing::mark_as_incomplete_activation,
                testing::mark_as_incomplete_deactivation,
            ] {
                mark_as_state(db, &PKG);

                let target = resolve_spec(cx, db, &NAME_SPEC).unwrap();
                assert!(matches!(
                    target,
                    ResolvedDeactivateTarget::Deactivate { pkg_id } if pkg_id == PKG.id
                ));
            }
        });
    }

    #[test]
    fn resolve_deactivate_targets_keeps_missing_specs_as_noop_targets() {
        let pkg_specs = vec![EXACT_SPEC.clone(), NAME_SPEC.clone()];

        testing::with_db(|cx, db| {
            let targets = resolve_deactivate_targets(cx, db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 2);
            assert!(matches!(
                &targets[0],
                ResolvedDeactivateTarget::AlreadyInactive { pkg_spec } if *pkg_spec == *EXACT_SPEC
            ));
            assert!(matches!(
                &targets[1],
                ResolvedDeactivateTarget::AlreadyInactive { pkg_spec } if *pkg_spec == *NAME_SPEC
            ));
        });
    }
}
