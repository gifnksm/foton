use std::{marker::PhantomData, path::PathBuf, sync::Arc};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ResultIteratorExt as _, ScopeResultErrorExt as _,
            SubReportScope,
        },
    },
    package::{PackageManifest, PackageManifestError},
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
                .report_error(&cx)?;
            Ok((path.clone(), manifest))
        })
        .collect_to_end()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;
    use crate::util::testing;

    #[test]
    fn resolve_manifests_reads_manifest_files() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, testing::make_manifest_str("example-font@0.1.0")).unwrap();

        testing::with_context(|cx| {
            let manifests = resolve_manifests(cx, std::slice::from_ref(&path)).unwrap();
            assert_eq!(manifests.len(), 1);
            assert_eq!(manifests[0].0, path);
            assert_eq!(manifests[0].1.id().to_string(), "example-font@0.1.0");
        });
    }

    #[test]
    fn resolve_manifests_reports_invalid_manifest_files() {
        let tempdir = TempDir::new().unwrap();
        let path = tempdir.path().join("manifest.toml");
        fs::write(&path, "not valid toml").unwrap();

        testing::with_context(|cx| {
            let err = resolve_manifests(cx, &[path]).unwrap_err();
            assert!(err.is_failed());
        });
    }
}
