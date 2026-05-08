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
};

#[derive(Debug)]
struct UninstallResolveScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for UninstallResolveScope<S>
where
    S: ReportScope,
{
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
    concat!(
        "multiple packages match the specified package `{pkg_spec}`:\n",
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

#[derive(Debug, Clone)]
pub(crate) struct ResolvedUninstallTarget {
    pub(crate) pkg_id: PackageId,
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
        .filter_map(|pkg_spec| resolve_spec(&cx, db, pkg_spec).transpose())
        .collect_to_end::<Vec<_>>()?;
    let mut pkg_ids = BTreeSet::new();
    targets.retain(|target| pkg_ids.insert(target.pkg_id.clone()));
    Ok(targets)
}

fn resolve_spec<S>(
    cx: &ReportContext<UninstallResolveScope<S>>,
    db: &PackageDatabase<'_>,
    pkg_spec: &PackageSpec,
) -> Result<Option<ResolvedUninstallTarget>, S::Error>
where
    S: ReportScope,
{
    let candidates = db.entries_by_spec(pkg_spec).collect::<Vec<_>>();
    match &candidates[..] {
        [] => {
            cx.reporter().report_info(format_args!(
                "no package matches the specified package `{pkg_spec}`; nothing to do"
            ));
            Ok(None)
        }
        [(_state, manifest)] => Ok(Some(ResolvedUninstallTarget {
            pkg_id: manifest.metadata.id(),
        })),
        _ => Err(cx.reporter().report_error(
            MultipleMatchingPackagesSnafu {
                pkg_spec: pkg_spec.clone(),
                pkg_ids: candidates
                    .into_iter()
                    .map(|(_state, manifest)| manifest.metadata.id())
                    .collect::<Vec<_>>(),
            }
            .build(),
        )),
    }
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
    fn resolve_spec_returns_none_for_missing_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let scope_cx = UninstallResolveScope::start(&cx);

        testing::with_db(&cx, |db| {
            let pkg_specs = vec![
                PackageSpec::from_str("example-namespace/example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-namespace/example-font").unwrap(),
                PackageSpec::from_str("example-font").unwrap(),
            ];
            for spec in &pkg_specs {
                let resolved = resolve_spec(&scope_cx, &db, spec).unwrap();
                assert!(resolved.is_none());
            }

            let targets = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap();
            assert!(targets.is_empty());
        });
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_qualified_name() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        testing::with_db(&cx, |mut db| {
            let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
            let expected = manifest.metadata.id();
            testing::mark_as_installed(&mut db, &manifest);

            for spec in [
                PackageSpec::from_str("example-namespace/example-font@0.1.0").unwrap(),
                PackageSpec::from_str("example-namespace/example-font").unwrap(),
            ] {
                let resolved = resolve_spec(&cx, &db, &spec).unwrap().unwrap();
                assert_eq!(resolved.pkg_id, expected);
            }
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest1 = testing::make_manifest("example-namespace/example-font@0.1.0");
        let manifest2 = testing::make_manifest("other-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest1);
            testing::mark_as_installed(&mut db, &manifest2);

            let err = resolve_spec(&cx, &db, &spec).unwrap_err();
            assert!(matches!(err, TestError::Failed));
        });
    }

    #[test]
    fn resolve_uninstall_targets_reports_ambiguous_spec_even_with_exact_target() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest1 = testing::make_manifest("example-namespace/example-font@0.1.0");
        let manifest2 = testing::make_manifest("other-namespace/example-font@1.0.0");
        let pkg_specs = vec![
            PackageSpec::from_str("example-namespace/example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest1);
            testing::mark_as_installed(&mut db, &manifest2);

            let err = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap_err();
            assert!(matches!(err, TestError::Failed));
        });
    }

    #[test]
    fn resolve_uninstall_targets_collapses_duplicate_specs() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let expected_pkg_id = manifest.metadata.id();
        let pkg_specs = vec![
            PackageSpec::from_str("example-namespace/example-font@0.1.0").unwrap(),
            PackageSpec::from_str("example-namespace/example-font").unwrap(),
        ];

        testing::with_db(&cx, |mut db| {
            testing::mark_as_installed(&mut db, &manifest);
            let targets = resolve_uninstall_targets(&cx, &db, &pkg_specs).unwrap();

            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].pkg_id, expected_pkg_id);
        });
    }

    #[test]
    fn resolve_spec_resolves_pending_entries() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest = testing::make_manifest("example-namespace/example-font@0.1.0");
        let pkg_id = manifest.metadata.id();

        let spec = PackageSpec::from_str("example-namespace/example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &manifest);

            let resolved = resolve_spec(&cx, &db, &spec).unwrap().unwrap();
            assert_eq!(resolved.pkg_id, pkg_id);

            let plan = testing::make_uninstall_plan(&pkg_id);
            db.apply_plan_transaction(&plan).unwrap();

            let resolved = resolve_spec(&cx, &db, &spec).unwrap().unwrap();
            assert_eq!(resolved.pkg_id, pkg_id);
        });
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name_across_pending_states() {
        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);
        let cx = UninstallResolveScope::start(&cx);

        let manifest1 = testing::make_manifest("example-namespace/example-font@0.1.0");
        let manifest2 = testing::make_manifest("other-namespace/example-font@1.0.0");
        let spec = PackageSpec::from_str("example-font").unwrap();

        testing::with_db(&cx, |mut db| {
            testing::mark_as_pending_installed(&mut db, &manifest1);
            testing::mark_as_pending_uninstalled(&mut db, &manifest2);

            let err = resolve_spec(&cx, &db, &spec).unwrap_err();
            assert!(matches!(err, TestError::Failed));
        });
    }
}
