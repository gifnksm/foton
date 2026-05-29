use std::{marker::PhantomData, path::PathBuf, sync::Arc};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ResultIteratorExt as _,
            ScopeResultErrorExt as _, SubReportScope,
        },
    },
    package::{PackageManifest, PackageManifestError},
};

#[derive(Debug)]
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

impl<S> SubReportScope<S> for ManifestScope<S>
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
enum ManifestErrorReport {
    #[snafu(transparent)]
    ReadManifest { source: PackageManifestError },
}

impl From<ManifestErrorReport> for ReportValue<'static> {
    fn from(report: ManifestErrorReport) -> ReportValue<'static> {
        ReportValue::BoxedError(report.into())
    }
}

pub(crate) fn resolve_manifests<S>(
    cx: &ReportContext<S>,
    manifests: &[PathBuf],
) -> Result<Vec<(PathBuf, Arc<PackageManifest>)>, S::Error>
where
    S: ReportScope,
{
    let cx = ManifestScope::start(cx);
    manifests
        .iter()
        .map(|path| {
            let manifest = PackageManifest::read(path)
                .map_err(ManifestErrorReport::from)
                .report_error(cx.reporter())?;
            Ok((path.clone(), manifest))
        })
        .collect_to_end()
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, fs};

    use tempfile::TempDir;

    use super::*;
    use crate::{
        cli::reporter::RootReportScope as _,
        util::testing::{self, TempdirContext, TestError, TestScope},
    };

    #[test]
    fn resolve_manifests_reads_manifest_files() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, testing::make_manifest_str("example-font@0.1.0")).unwrap();

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let manifests = resolve_manifests(&cx, std::slice::from_ref(&path)).unwrap();

        assert_eq!(manifests.len(), 1);
        assert_eq!(manifests[0].0, path);
        assert_eq!(manifests[0].1.id().to_string(), "example-font@0.1.0");
    }

    #[test]
    fn resolve_manifests_reports_invalid_manifest_files() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, "not valid toml").unwrap();

        let cx = TempdirContext::new();
        let cx = TestScope::start(&cx);

        let err = resolve_manifests(&cx, &[path]).unwrap_err();

        assert_matches!(err, TestError::Failed);
    }
}
