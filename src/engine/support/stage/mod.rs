use std::marker::PhantomData;

use snafu::Snafu;

pub(crate) use self::extract::*;
use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, OperationError as _, ReportScope, ReportValue, SubReportScope},
    },
    package::{FontEntry, PackageDirs, PackageId, PackageManifest},
};

mod download;
mod extract;
mod validate;

#[derive(Debug)]
struct StageScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for StageScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = StageErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for StageScope<S>
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
enum StageErrorReport {
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
}

impl From<StageErrorReport> for ReportValue<'static> {
    fn from(report: StageErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

pub(crate) async fn stage_package<S>(
    cx: &ReportContext<S>,
    pkg_dirs: &PackageDirs,
    manifest: &PackageManifest,
) -> Result<(Vec<FontEntry>, Vec<ExtractDetail>), S::Error>
where
    S: ReportScope,
{
    let cx = StageScope::start(cx);

    let pkg_id = manifest.id();
    let reporter = cx.reporter();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut file_names = vec![];
    let mut extract_details = vec![];

    for source in &manifest.sources {
        let file = cx
            .cancel_token()
            .run_until_cancelled(download::download_archive(&cx, &pkg_id, source))
            .await
            .unwrap_or(Err(S::Error::cancelled()))?;
        let (source_file_names, extract_detail) = extract_archive(
            &cx,
            file,
            &source.include,
            &source.exclude,
            package_fonts_dir,
        )?;
        file_names.extend(source_file_names);
        extract_details.push(extract_detail);
    }

    let valid_entries = validate::validate_and_prune_fonts(&cx, package_fonts_dir, &file_names)?;
    if cx.cancel_token().is_cancelled() {
        return Err(S::Error::cancelled());
    }

    if valid_entries.is_empty() {
        return Err(reporter.report_error(NoValidFontsSnafu { pkg_id }.build()));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    Ok((valid_entries, extract_details))
}
