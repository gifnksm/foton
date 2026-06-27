use std::{marker::PhantomData, path::PathBuf};

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{
            ErrorReportExt as _, NeverReport, ReportScope, ScopeResultErrorExt as _,
            ScopeResultWarnExt as _, SubReportScope,
        },
    },
    engine::ExtractedFile,
    package::PackageFont,
    platform::windows::services::font::{FontInspector, FontInspectorError, InspectedFont},
    util::{
        fs as fs_util,
        macros::concat_line,
        path::{AbsolutePath, FileName},
    },
};

#[derive(Debug, Default)]
struct ValidationScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ValidationScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = ValidationWarnReport;
    type ErrorReportValue = ValidationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ValidationScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum ValidationWarnReport {
    #[snafu(display("unsupported font file found: {path}", path = path.display()))]
    UnsupportedFontFile { path: PathBuf },
    #[snafu(display(
        concat_line!(
            "failed to remove unsupported font file: {path}",
            "manual cleanup may be required",
        ),
        path = path.display(),
    ))]
    RemoveUnsupportedFontFile {
        path: PathBuf,
        source: fs_util::FsError,
    },
}

#[derive(Debug, Snafu)]
enum ValidationErrorReport {
    #[snafu(display("failed to create font inspector"))]
    CreateFontInspector {
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
    },
    #[snafu(display("failed to inspect font file: {file_name}", file_name = file_name.display()))]
    InspectFont {
        file_name: FileName,
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
    },
}

pub(super) fn validate_and_prune_fonts<S>(
    cx: &ReportContext<S>,
    fonts_dir: &AbsolutePath,
    extracted: &[ExtractedFile],
) -> Result<Vec<PackageFont>, S::Error>
where
    S: ReportScope,
{
    let cx = ValidationScope::start_with_report(cx, "Validating fonts...");

    let mut valid_pkg_fonts = vec![];
    let inspector = FontInspector::new()
        .context(CreateFontInspectorSnafu)
        .report_error(&cx)?;

    for file in extracted {
        let file_name = file.file_name();
        let path = &fonts_dir.join(file_name);
        let Some(InspectedFont {}) = inspector
            .inspect_font(path)
            .context(InspectFontSnafu { file_name })
            .report_error(&cx)?
        else {
            UnsupportedFontFileSnafu { path }.build().report_warn(&cx)?;
            fs_util::remove_file(path)
                .context(RemoveUnsupportedFontFileSnafu { path })
                .report_warn(&cx)?;
            continue;
        };

        let pkg_font = PackageFont::new(file_name);
        valid_pkg_fonts.push(pkg_font);
    }

    Ok(valid_pkg_fonts)
}
