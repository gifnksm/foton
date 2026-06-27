use std::marker::PhantomData;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope},
    },
    engine::ExtractedFile,
    package::PackageFont,
    platform::windows::services::font::{FontInspector, FontInspectorError, InspectedFont},
    util::path::{AbsolutePath, FileName},
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
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ValidationErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ValidationScope<S> where S: ReportScope {}

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

pub(super) fn validate_fonts<S>(
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

        let InspectedFont { faces: _ } = inspector
            .inspect_font(path)
            .context(InspectFontSnafu { file_name })
            .report_error(&cx)?;

        let pkg_font = PackageFont::new(file_name);
        valid_pkg_fonts.push(pkg_font);
    }

    Ok(valid_pkg_fonts)
}
