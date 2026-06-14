use std::{marker::PhantomData, path::Path, sync::Arc};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope},
    },
    package::{PackageDefinition, PackageManifest, PackageManifestError},
};

#[derive(Debug, Default)]
struct ManifestScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ManifestScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ManifestErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ManifestScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum ManifestErrorReport {
    #[snafu(transparent)]
    ReadManifest { source: PackageManifestError },
}

pub(crate) fn resolve_manifest<S, P>(
    cx: &ReportContext<S>,
    path: P,
) -> Result<Arc<PackageDefinition>, S::Error>
where
    S: ReportScope,
    P: AsRef<Path>,
{
    let cx = ManifestScope::start(cx);
    let manifest = PackageManifest::read(path.as_ref())
        .map_err(ManifestErrorReport::from)
        .report_error(&cx)?;
    Ok(Arc::new(manifest.into()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::util::testing;

    #[test]
    fn resolve_manifest_reads_manifest_file() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, testing::make_manifest_str("example-font@0.1.0")).unwrap();

        testing::with_context(|cx| {
            let manifest = resolve_manifest(cx, &path).unwrap();
            assert_eq!(manifest.id.to_string(), "example-font@0.1.0");
        });
    }

    #[test]
    fn resolve_manifest_reports_invalid_manifest_file() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, "not valid toml").unwrap();

        testing::with_context(|cx| {
            let err = resolve_manifest(cx, &path).unwrap_err();
            assert!(err.is_failed());
        });
    }
}
