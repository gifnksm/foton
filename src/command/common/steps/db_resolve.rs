use std::sync::Arc;

use crate::{
    cli::context::StepContext,
    db::PackageDatabase,
    package::{PackageId, PackageManifest, PackageSpec, PackageState},
    util::reporter::{NeverReport, ReportValue, Step},
};

#[derive(Debug)]
struct DbResolveStep<S> {
    step: Arc<S>,
}

impl<S> Step for DbResolveStep<S>
where
    S: Step,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = DbResolveErrorReport;
    type Error = S::Error;

    fn make_failed(&self) -> Self::Error {
        self.step.make_failed()
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum DbResolveErrorReport {
    #[display(
        "multiple packages match the specified package `{pkg_spec}`:\n{pkg_ids}",
        pkg_ids = pkg_ids.iter().map(|id| format!("- {id}")).collect::<Vec<_>>().join("\n")
    )]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
}

impl From<DbResolveErrorReport> for ReportValue<'static> {
    fn from(report: DbResolveErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

pub(in crate::command) fn resolve_spec_in_db<'a, S>(
    cx: &StepContext<S>,
    db: &'a PackageDatabase<'_>,
    spec: &PackageSpec,
) -> Result<Option<(PackageState, &'a PackageManifest)>, S::Error>
where
    S: Step,
{
    let cx = cx.with_step(DbResolveStep {
        step: Arc::clone(cx.step()),
    });
    let candidates = match spec {
        PackageSpec::Id(id) => {
            return Ok(db.entry_by_id(id));
        }
        PackageSpec::QualifiedName(qualified_name) => db
            .entries_by_qualified_name(qualified_name)
            .collect::<Vec<_>>(),
        PackageSpec::Name(name) => db.entries_by_name(name).collect::<Vec<_>>(),
    };
    if candidates.len() > 1 {
        return Err(cx
            .reporter()
            .report_error(DbResolveErrorReport::MultipleMatchingPackages {
                pkg_spec: spec.clone(),
                pkg_ids: candidates
                    .into_iter()
                    .map(|(_state, manifest)| manifest.metadata.id())
                    .collect(),
            }));
    }
    Ok(candidates.into_iter().next())
}

#[cfg(test)]
mod tests {
    use crate::{
        command::common,
        db::BeginInstallResult,
        util::testing::{self, TempdirContext, TestError, TestStep},
    };

    use super::*;

    #[test]
    fn resolve_spec_in_db_returns_none_for_missing_specs() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(TestStep {});
        let mut lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let db = common::steps::load_database(&cx, &mut lock_file).unwrap();

        for spec in [
            "example-namespace/example-font@0.1.0"
                .parse::<PackageSpec>()
                .unwrap(),
            "example-namespace/example-font"
                .parse::<PackageSpec>()
                .unwrap(),
            "example-font".parse::<PackageSpec>().unwrap(),
        ] {
            let resolved = resolve_spec_in_db(&cx, &db, &spec).unwrap();
            assert_eq!(
                resolved.map(|(state, manifest)| (state, manifest.metadata.id())),
                None
            );
        }
    }

    #[test]
    fn resolve_spec_in_db_resolves_installed_entry_from_id_and_qualified_name() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(TestStep {});
        let mut lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut lock_file).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let expected = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&expected).unwrap();

        for spec in [
            "example-namespace/example-font@0.1.0"
                .parse::<PackageSpec>()
                .unwrap(),
            "example-namespace/example-font"
                .parse::<PackageSpec>()
                .unwrap(),
        ] {
            let resolved = resolve_spec_in_db(&cx, &db, &spec)
                .unwrap()
                .map(|(state, manifest)| (state, manifest.metadata.id()));
            assert_eq!(resolved, Some((PackageState::Installed, expected.clone())));
        }
    }

    #[test]
    fn resolve_spec_in_db_reports_multiple_matches_for_name() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(TestStep {});
        let mut lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut lock_file).unwrap();

        let manifest1 = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id1 = manifest1.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest1),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id1).unwrap();

        let manifest2 = testing::make_manifest("other-namespace", "example-font", "1.0.0");
        let pkg_id2 = manifest2.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest2),
            BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id2).unwrap();

        let spec = "example-font".parse::<PackageSpec>().unwrap();
        let err = resolve_spec_in_db(&cx, &db, &spec).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }

    #[test]
    fn resolve_spec_resolves_pending_entries() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(TestStep {});
        let mut lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut lock_file).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let expected = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest),
            BeginInstallResult::CanInstall
        ));

        let spec = "example-namespace/example-font"
            .parse::<PackageSpec>()
            .unwrap();
        let resolved = resolve_spec_in_db(&cx, &db, &spec)
            .unwrap()
            .map(|(state, manifest)| (state, manifest.metadata.id()));
        assert_eq!(
            resolved,
            Some((PackageState::PendingInstall, expected.clone()))
        );

        db.begin_uninstall(&expected);

        let resolved = resolve_spec_in_db(&cx, &db, &spec)
            .unwrap()
            .map(|(state, manifest)| (state, manifest.metadata.id()));
        assert_eq!(resolved, Some((PackageState::PendingUninstall, expected)));
    }

    #[test]
    fn resolve_spec_in_db_reports_multiple_matches_for_name_across_pending_states() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(TestStep {});
        let mut lock_file = common::steps::open_db_lock_file(&cx).unwrap();
        let mut db = common::steps::load_database(&cx, &mut lock_file).unwrap();

        let manifest1 = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        assert!(matches!(
            db.begin_install(&manifest1),
            BeginInstallResult::CanInstall
        ));

        let manifest2 = testing::make_manifest("other-namespace", "example-font", "1.0.0");
        let pkg_id2 = manifest2.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest2),
            BeginInstallResult::CanInstall
        ));
        db.begin_uninstall(&pkg_id2);

        let spec = "example-font".parse::<PackageSpec>().unwrap();
        let err = resolve_spec_in_db(&cx, &db, &spec).unwrap_err();

        assert!(matches!(err, TestError::Failed));
    }
}
