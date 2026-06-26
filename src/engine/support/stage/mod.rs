use std::{marker::PhantomData, path::PathBuf};

use snafu::Snafu;

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ErrorReportExt as _, NeverReport, OperationError as _, ReportScope, SubReportScope,
        },
    },
    engine::support::stage::extract::ExtractResult,
    package::{ArchiveOptions, FontEntry, PackageDefinition, PackageDirs, PackageId},
    util::path::FileName,
};

mod download;
mod extract;
mod validate;

#[derive(Debug, Default)]
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

impl<S> SubReportScope<S> for StageScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum StageErrorReport {
    #[snafu(display("no valid font files found in package {pkg_id}"))]
    NoValidFonts { pkg_id: PackageId },
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedFile {
    file_name: FileName,
}

impl ExtractedFile {
    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractEntry {
    pub(crate) path_in_archive: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) enum ExtractDetail<'s> {
    FontFile {},
    Archive(ArchiveExtractDetail<'s>),
}

impl<'s> ExtractDetail<'s> {
    pub(crate) fn as_archive(&self) -> Option<&ArchiveExtractDetail<'s>> {
        match self {
            Self::FontFile {} => None,
            Self::Archive(detail) => Some(detail),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ArchiveExtractDetail<'s> {
    pub(crate) options: &'s ArchiveOptions,
    pub(crate) included: Vec<ExtractEntry>,
    pub(crate) excluded: Vec<ExtractEntry>,
    pub(crate) suspicious_skips: Vec<ExtractEntry>,
}

pub(crate) async fn stage_package<'s, S>(
    cx: &ReportContext<S>,
    pkg_dirs: &PackageDirs,
    pkg: &'s PackageDefinition,
) -> Result<(Vec<FontEntry>, Vec<ExtractDetail<'s>>), S::Error>
where
    S: ReportScope,
{
    let cx = StageScope::start(cx);

    let reporter = cx.reporter();
    let package_fonts_dir = pkg_dirs.fonts_dir();

    let mut extracted_files = vec![];
    let mut extract_details = vec![];

    for source in &pkg.sources {
        let (file, file_size) = cx
            .cancel_token()
            .run_until_cancelled(download::download_source_file(&cx, &pkg.id, source))
            .await
            .unwrap_or(Err(S::Error::cancelled()))?;
        let ExtractResult { files, detail } =
            extract::extract_contents(&cx, file, file_size, source, package_fonts_dir)?;
        extracted_files.extend(files);
        extract_details.push(detail);
    }

    let valid_entries =
        validate::validate_and_prune_fonts(&cx, package_fonts_dir, &extracted_files)?;
    if cx.cancel_token().is_cancelled() {
        return Err(S::Error::cancelled());
    }

    if valid_entries.is_empty() {
        return Err(NoValidFontsSnafu { pkg_id: &pkg.id }
            .build()
            .report_error(&cx));
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    Ok((valid_entries, extract_details))
}
