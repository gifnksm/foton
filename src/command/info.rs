use std::io;

use crate::{
    cli::context::{RootContext, StepContext},
    db::{DbLockFile, DbLockFileError, PackageDatabase, PackageDatabaseError},
    package::{
        PackageId, PackageManifest, PackageMetadata, PackageSource, PackageSpec, PackageState,
    },
    util::reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
};

#[derive(Debug)]
struct InfoStep {}

impl Step for InfoStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InfoErrorReport;
    type Error = InfoError;

    fn make_failed(&self) -> Self::Error {
        InfoError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InfoErrorReport {
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
    #[display("no package matches the specified package `{pkg_spec}`")]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[display("failed to write package info to stdout")]
    WriteInfo {
        #[error(source)]
        source: io::Error,
    },
}

impl From<InfoErrorReport> for ReportValue<'static> {
    fn from(report: InfoErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum InfoError {
    #[display("failed to print package information")]
    Failed,
}

pub(crate) fn info_package(cx: &RootContext, pkg_spec: &PackageSpec) -> Result<(), InfoError> {
    let cx = cx.with_step(InfoStep {});
    let reporter = cx.reporter();

    let mut db_lock = DbLockFile::open(cx.app_dirs())
        .map_err(|source| InfoErrorReport::OpenDbLockFile { source })
        .report_error(reporter)?;
    let db_lock_guard = db_lock
        .try_acquire()
        .map_err(|source| match source {
            DbLockFileError::AlreadyLocked { .. } => InfoErrorReport::DbAlreadyLocked { source },
            _ => InfoErrorReport::AcquireDbLock { source },
        })
        .report_error(reporter)?;

    let db = PackageDatabase::load(cx.app_dirs(), &db_lock_guard)
        .map_err(|source| InfoErrorReport::LoadDatabase { source })
        .report_error(reporter)?;

    let Some((state, manifest)) = resolve_spec(&cx, &db, pkg_spec)? else {
        return Err(reporter.report_error(InfoErrorReport::NoMatchingPackage {
            pkg_spec: pkg_spec.clone(),
        }));
    };

    render_package_info(io::stdout().lock(), state, manifest)
        .map_err(|source| InfoErrorReport::WriteInfo { source })
        .report_error(reporter)?;

    Ok(())
}

fn resolve_spec<'a>(
    cx: &StepContext<InfoStep>,
    db: &'a PackageDatabase<'_>,
    spec: &PackageSpec,
) -> Result<Option<(PackageState, &'a PackageManifest)>, InfoError> {
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
            .report_error(InfoErrorReport::MultipleMatchingPackages {
                pkg_spec: spec.clone(),
                pkg_ids: candidates
                    .into_iter()
                    .map(|(_state, manifest)| manifest.metadata.id())
                    .collect(),
            }));
    }
    Ok(candidates.into_iter().next())
}

fn render_package_info<W>(
    mut writer: W,
    state: PackageState,
    manifest: &PackageManifest,
) -> io::Result<()>
where
    W: io::Write,
{
    let PackageManifest { metadata, sources } = manifest;
    let PackageMetadata {
        qualified_name,
        version,
        description,
        homepage,
        repository,
        license,
    } = metadata;
    writeln!(writer, "Name: {qualified_name}")?;
    writeln!(writer, "Version: {version}")?;
    writeln!(writer, "State: {state}")?;
    if let Some(description) = description {
        writeln!(writer, "Description: {description}")?;
    }
    if let Some(homepage) = homepage {
        writeln!(writer, "Homepage: {homepage}")?;
    }
    if let Some(repository) = repository {
        writeln!(writer, "Repository: {repository}")?;
    }
    if let Some(license) = license {
        writeln!(writer, "License: {license}")?;
    }
    writeln!(writer, "Sources:")?;
    for source in sources {
        let PackageSource { url, hash, include } = source;
        writeln!(writer, "- URL: {url}")?;
        writeln!(writer, "  Hash: {hash}")?;
        writeln!(
            writer,
            "  Includes: {}",
            include
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        db::{DbLockFile, PackageDatabase},
        util::testing::{self, TempdirContext},
    };

    use super::*;

    #[test]
    fn resolve_spec_returns_none_for_missing_specs() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InfoStep {});
        let mut lock_file = DbLockFile::open(cx.app_dirs()).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let db = PackageDatabase::load(cx.app_dirs(), &lock_file_guard).unwrap();

        for spec in [
            "example-namespace/example-font@0.1.0"
                .parse::<PackageSpec>()
                .unwrap(),
            "example-namespace/example-font"
                .parse::<PackageSpec>()
                .unwrap(),
            "example-font".parse::<PackageSpec>().unwrap(),
        ] {
            let resolved = resolve_spec(&cx, &db, &spec).unwrap();
            assert_eq!(
                resolved.map(|(state, manifest)| (state, manifest.metadata.id())),
                None
            );
        }
    }

    #[test]
    fn resolve_spec_resolves_installed_entry_from_id_and_qualified_name() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InfoStep {});
        let mut lock_file = DbLockFile::open(cx.app_dirs()).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = PackageDatabase::load(cx.app_dirs(), &lock_file_guard).unwrap();
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let expected = manifest.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest),
            crate::db::BeginInstallResult::CanInstall
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
            let resolved = resolve_spec(&cx, &db, &spec)
                .unwrap()
                .map(|(state, manifest)| (state, manifest.metadata.id()));
            assert_eq!(resolved, Some((PackageState::Installed, expected.clone())));
        }
    }

    #[test]
    fn resolve_spec_reports_multiple_matches_for_name() {
        let cx = TempdirContext::new();
        let cx = cx.with_step(InfoStep {});
        let mut lock_file = DbLockFile::open(cx.app_dirs()).unwrap();
        let lock_file_guard = lock_file.try_acquire().unwrap();
        let mut db = PackageDatabase::load(cx.app_dirs(), &lock_file_guard).unwrap();

        let manifest1 = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let pkg_id1 = manifest1.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest1),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id1).unwrap();

        let manifest2 = testing::make_manifest("other-namespace", "example-font", "1.0.0");
        let pkg_id2 = manifest2.metadata.id();
        assert!(matches!(
            db.begin_install(&manifest2),
            crate::db::BeginInstallResult::CanInstall
        ));
        db.complete_install(&pkg_id2).unwrap();

        let spec = "example-font".parse::<PackageSpec>().unwrap();
        let err = resolve_spec(&cx, &db, &spec).unwrap_err();

        assert!(matches!(err, InfoError::Failed));
    }

    #[test]
    fn render_package_info_prints_all_present_fields() {
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::Installed, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-namespace/example-font\n",
                "Version: 0.1.0\n",
                "State: installed\n",
                "Sources:\n",
                "- URL: https://example.com/example-font-0.1.0.zip\n",
                "  Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  Includes: **/*.ttf, **/*.otf, **/*.ttc\n",
            )
        );
    }

    #[test]
    fn render_package_info_prints_optional_metadata_fields_when_present() {
        let manifest: PackageManifest = toml::from_str(
            r#"
[package]
name = "example-namespace/example-font"
version = "0.1.0"
description = "Example font"
homepage = "https://example.com/home"
repository = "https://example.com/repo"
license = "MIT"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
"#,
        )
        .unwrap();
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::PendingInstall, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-namespace/example-font\n",
                "Version: 0.1.0\n",
                "State: pending-install\n",
                "Description: Example font\n",
                "Homepage: https://example.com/home\n",
                "Repository: https://example.com/repo\n",
                "License: MIT\n",
                "Sources:\n",
                "- URL: https://example.com/example-font-0.1.0.zip\n",
                "  Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  Includes: fonts/*.ttf\n",
            )
        );
    }
}
