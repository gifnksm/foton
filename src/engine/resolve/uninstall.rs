use std::{collections::BTreeSet, marker::PhantomData};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        message::BulletList,
        reporter::{NeverReport, ReportScope, ReportValue, ResultIteratorExt as _, SubReportScope},
    },
    db::PackageDatabase,
    package::{PackageId, PackageSpec},
    util::macros::concat_line,
};

#[derive(Debug)]
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

impl<S> SubReportScope<S> for UninstallResolveScope<S>
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

impl From<UninstallResolveErrorReport> for ReportValue<'static> {
    fn from(report: UninstallResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, Clone, derive_more::IsVariant)]
pub(crate) enum ResolvedUninstallTarget {
    Uninstall { pkg_id: PackageId },
    AlreadyUninstalled { pkg_spec: PackageSpec },
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
            pkg_id: entry.manifest.id(),
        }),
        _ => Err(cx.reporter().report_error(
            MultipleMatchingPackagesSnafu {
                pkg_spec: pkg_spec.clone(),
                pkg_ids: candidates
                    .into_iter()
                    .map(|entry| entry.manifest.id())
                    .collect::<Vec<_>>(),
            }
            .build(),
        )),
    }
}

fn dedup_targets(targets: &mut Vec<ResolvedUninstallTarget>) {
    let mut seen_pkg_id = BTreeSet::new();
    let mut seen_pkg_spec = BTreeSet::new();
    targets.retain(|target| match target {
        ResolvedUninstallTarget::Uninstall { pkg_id } => seen_pkg_id.insert(pkg_id.clone()),
        ResolvedUninstallTarget::AlreadyUninstalled { pkg_spec } => {
            seen_pkg_spec.insert(pkg_spec.clone())
        }
    });
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, str::FromStr as _};

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

    #[test]
    fn resolve_spec_returns_unresolved_for_missing_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let scope_cx = UninstallResolveScope::start(&cx);

        testing::with_db(&cx, |db| {
            let pkg_specs = vec![
                PackageSpec::from_str("example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ];
            for spec in &pkg_specs {
                let target = resolve_spec(&scope_cx, &db, spec).unwrap();
                assert_matches!(target, ResolvedUninstallTarget::AlreadyUninstalled { pkg_spec } if pkg_spec == *spec);
            }

            let targets = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap();
            assert_eq!(targets.len(), 2);
        });
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_name() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        testing::with_db(&cx, |mut db| {
            let manifest = testing::make_manifest("example-font@0.1.0");
            let expected = manifest.id();
            testing::mark_as_installed(&mut db, &manifest);

            for spec in [
                PackageSpec::from_str("example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ] {
                let target = resolve_spec(&cx, &db, &spec).unwrap();
                assert_matches!(target, ResolvedUninstallTarget::Uninstall  { pkg_id } if pkg_id == expected);
            }
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest1);
            testing::mark_as_installed(&mut db, &manifest2);

            let err = resolve_spec(&cx, &db, &spec).unwrap_err();
            assert_matches!(err, TestError::Failed);
        });
    }

    #[test]
    fn resolve_uninstall_targets_reports_ambiguous_spec_even_with_exact_target() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let pkg_specs = vec![
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest1);
            testing::mark_as_installed(&mut db, &manifest2);

            let err = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap_err();
            assert_matches!(err, TestError::Failed);
        });
    }

    #[test]
    fn resolve_uninstall_targets_collapses_duplicate_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let expected_pkg_id = manifest.id();
        let pkg_specs = vec![
            PackageSpec::from_str("example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let targets = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap();

            assert_eq!(targets.len(), 1);
            assert_matches!(&targets[0], ResolvedUninstallTarget::Uninstall { pkg_id } if pkg_id == &expected_pkg_id);
        });
    }

    #[test]
    fn resolve_spec_resolves_incomplete_entries() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("example-font@0.1.0");
        let pkg_id = manifest.id();

        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &manifest);

            let target = resolve_spec(&cx, &db, &spec).unwrap();
            assert_matches!(target, ResolvedUninstallTarget::Uninstall { pkg_id: target_id } if target_id == pkg_id);

            testing::mark_as_incomplete_uninstall(&mut db, &manifest);

            let target = resolve_spec(&cx, &db, &spec).unwrap();
            assert_matches!(target, ResolvedUninstallTarget::Uninstall { pkg_id: target_id } if target_id == pkg_id);
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name_across_incomplete_states() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest1 = testing::make_manifest("example-font@0.1.0");
        let manifest2 = testing::make_manifest("example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_incomplete_install(&mut db, &manifest1);
            testing::mark_as_incomplete_uninstall(&mut db, &manifest2);

            let err = resolve_spec(&cx, &db, &spec).unwrap_err();
            assert_matches!(err, TestError::Failed);
        });
    }
}
