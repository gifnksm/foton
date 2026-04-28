use crate::{
    command::common,
    db::{DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError},
    package::{PackageId, PackageSpec},
    util::{
        app_dirs::AppDirs,
        reporter::{
            NeverReport, ReportValue, Reporter, Step, StepReporter, StepResultErrorExt as _,
        },
    },
};

#[derive(Debug)]
struct UninstallStep<'a> {
    pkg_spec: &'a PackageSpec,
}

impl Step for UninstallStep<'_> {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = UninstallErrorReport;
    type Error = UninstallError;

    fn report_prelude(&self, reporter: &Reporter) {
        reporter.report_step(format_args!("Uninstalling {}...", self.pkg_spec));
    }

    fn make_failed(&self) -> Self::Error {
        UninstallError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallErrorReport {
    #[display("failed to open database lock file")]
    OpenDbLockFile {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("another install or uninstall operation is already in progress")]
    DbAlreadyLocked {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("failed to acquire database lock")]
    AcquireDbLock {
        #[error(source)]
        source: DbLockFileError,
    },
    #[display("failed to load package database")]
    LoadDatabase {
        #[error(source)]
        source: PackageDatabaseError,
    },
    #[display(
        "multiple packages match the specified package `{pkg_spec}`:\n{pkg_ids}",
        pkg_ids = pkg_ids.iter().map(|id| format!("- {id}")).collect::<Vec<_>>().join("\n")
    )]
    MultipleMatchingPackages {
        pkg_spec: PackageSpec,
        pkg_ids: Vec<PackageId>,
    },
}

impl From<UninstallErrorReport> for ReportValue<'static> {
    fn from(report: UninstallErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum UninstallError {
    #[display("failed to uninstall package")]
    Failed,
}

pub(crate) fn uninstall_package(
    reporter: &Reporter,
    app_id: &str,
    app_dirs: &AppDirs,
    pkg_spec: &PackageSpec,
) -> Result<(), UninstallError> {
    let reporter = reporter.with_step(UninstallStep { pkg_spec });

    let mut db_lock = DbLockFile::open(app_dirs)
        .map_err(|source| UninstallErrorReport::OpenDbLockFile { source })
        .report_error(&reporter)?;
    let db_lock_guard = db_lock
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => {
                UninstallErrorReport::DbAlreadyLocked { source }
            }
            _ => UninstallErrorReport::AcquireDbLock { source },
        })
        .report_error(&reporter)?;

    let mut db = PackageDatabase::load(app_dirs, &db_lock_guard)
        .map_err(|source| UninstallErrorReport::LoadDatabase { source })
        .report_error(&reporter)?;

    let Some(pkg_id) = resolve_spec(&reporter, &db, pkg_spec)? else {
        reporter.report_info(format_args!(
            "no package matches the specified package `{pkg_spec}`; nothing to do"
        ));
        return Ok(());
    };

    common::steps::uninstall_transaction(&reporter, app_id, &mut db, app_dirs, &pkg_id)?;

    Ok(())
}

fn resolve_spec(
    reporter: &StepReporter<'_, UninstallStep<'_>>,
    db: &PackageDatabase<'_>,
    spec: &PackageSpec,
) -> Result<Option<PackageId>, UninstallError> {
    let candidates = match spec {
        PackageSpec::Id(id) => {
            return Ok(db.entry_by_id(id).map(|_| id.clone()));
        }
        PackageSpec::QualifiedName(qualified_name) => db
            .entries_by_qualified_name(qualified_name)
            .map(|(_, manifest)| manifest.metadata.id())
            .collect::<Vec<_>>(),
        PackageSpec::Name(name) => db
            .entries_by_name(name)
            .map(|(_, manifest)| manifest.metadata.id())
            .collect::<Vec<_>>(),
    };
    if candidates.len() > 1 {
        return Err(
            reporter.report_error(UninstallErrorReport::MultipleMatchingPackages {
                pkg_spec: spec.clone(),
                pkg_ids: candidates,
            }),
        );
    }
    Ok(candidates.into_iter().next())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::{
        db::{DbLockFile, PackageDatabase},
        package::PackageManifest,
        util::{app_dirs::AppDirs, path::AbsolutePath, reporter::Reporter},
    };

    use super::*;

    fn make_app_dirs() -> (TempDir, AppDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let app_data_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let app_dirs = AppDirs::new_for_test(app_data_dir);
        (tempdir, app_dirs)
    }

    fn load_db<'a>(
        app_dirs: &AppDirs,
        lock_file_guard: &'a crate::db::DbLockFileGuard<'_>,
    ) -> PackageDatabase<'a> {
        PackageDatabase::load(app_dirs, lock_file_guard).unwrap()
    }

    fn test_manifest(namespace: &str, name: &str, version: &str) -> PackageManifest {
        toml::from_str(&format!(
            r#"
[package]
name = "{namespace}/{name}"
version = "{version}"
description = "HackGen"

[[sources]]
url = "https://example.com/hackgen.zip"
hash = "sha256:ed182e2a4b95792d94dea7932f6b45280b5ae353651be249d5f6b7867b788db7"
"#
        ))
        .unwrap()
    }

    fn resolve_for_test(
        db: &PackageDatabase<'_>,
        spec: &PackageSpec,
    ) -> Result<Option<PackageId>, UninstallError> {
        let reporter = Reporter::message_reporter();
        let reporter = reporter.with_step(UninstallStep { pkg_spec: spec });
        resolve_spec(&reporter, db, spec)
    }

    #[test]
    fn resolve_spec_returns_none_for_missing_specs() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let db = load_db(&app_dirs, &lock_file_guard);

        for spec in [
            "yuru7/hackgen@2.10.0".parse::<PackageSpec>().unwrap(),
            "yuru7/hackgen".parse::<PackageSpec>().unwrap(),
            "hackgen".parse::<PackageSpec>().unwrap(),
        ] {
            let resolved = resolve_for_test(&db, &spec).unwrap();
            assert_eq!(resolved, None);
        }
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_qualified_name() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = load_db(&app_dirs, &lock_file_guard);
        let manifest = test_manifest("yuru7", "hackgen", "2.10.0");
        let expected = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.complete_install(&expected).unwrap();

        for spec in [
            "yuru7/hackgen@2.10.0".parse::<PackageSpec>().unwrap(),
            "yuru7/hackgen".parse::<PackageSpec>().unwrap(),
        ] {
            let resolved = resolve_for_test(&db, &spec).unwrap();
            assert_eq!(resolved, Some(expected.clone()));
        }
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = load_db(&app_dirs, &lock_file_guard);

        let manifest1 = test_manifest("yuru7", "hackgen", "2.10.0");
        let pkg_id1 = manifest1.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest1),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id1).unwrap();

        let manifest2 = test_manifest("someone", "hackgen", "1.0.0");
        let pkg_id2 = manifest2.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest2),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id2).unwrap();

        let spec = "hackgen".parse::<PackageSpec>().unwrap();
        let err = resolve_for_test(&db, &spec).unwrap_err();

        assert!(matches!(err, UninstallError::Failed));
    }

    #[test]
    fn resolve_spec_resolves_pending_entries() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = load_db(&app_dirs, &lock_file_guard);
        let manifest = test_manifest("yuru7", "hackgen", "2.10.0");
        let expected = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest),
            crate::db::BeginInstallResult::CanInstall
        ));

        let spec = "yuru7/hackgen".parse::<PackageSpec>().unwrap();
        let resolved = resolve_for_test(&db, &spec).unwrap();
        assert_eq!(resolved, Some(expected.clone()));

        db.begin_uninstall(&expected);

        let resolved = resolve_for_test(&db, &spec).unwrap();
        assert_eq!(resolved, Some(expected));
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name_across_pending_states() {
        let (_tempdir, app_dirs) = make_app_dirs();
        let mut lock_file = DbLockFile::open(&app_dirs).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = load_db(&app_dirs, &lock_file_guard);

        let manifest1 = test_manifest("yuru7", "hackgen", "2.10.0");
        assert!(matches!(
            db.begin_install(&manifest1),
            crate::db::BeginInstallResult::CanInstall
        ));

        let manifest2 = test_manifest("someone", "hackgen", "1.0.0");
        let pkg_id2 = manifest2.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest2),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.begin_uninstall(&pkg_id2);

        let spec = "hackgen".parse::<PackageSpec>().unwrap();
        let err = resolve_for_test(&db, &spec).unwrap_err();

        assert!(matches!(err, UninstallError::Failed));
    }
}
