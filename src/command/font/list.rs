use std::{collections::BTreeMap, io};

use snafu::{IntoError as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        args::ListFontArgs,
        context::{ReportContext, RootContext},
        reporter::{
            ErrorReportExt as _, NeverReport, OperationError, ReportScope, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    platform::windows::services::font_enumerator::{
        FontEnumeratorError, SystemFont, SystemFontEnumerator, SystemFontSource,
    },
    util::font_family::{FontFamilyAccumulator, FontFamilyInfo},
};

#[derive(Debug, Default)]
struct ListFontsScope {}

impl ReportScope for ListFontsScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = ListFontsWarnReport;
    type ErrorReportValue = ListFontsErrorReport;
    type Error = ListFontsError;
}

impl RootReportScope for ListFontsScope {}

#[derive(Debug, Snafu)]
enum ListFontsWarnReport {
    #[snafu(display("failed to enumerate system font"))]
    EnumerateSystemFont { source: FontEnumeratorError },
}

#[derive(Debug, Snafu)]
enum ListFontsErrorReport {
    #[snafu(display("failed to create font enumerator"))]
    CreateFontEnumerator { source: FontEnumeratorError },
    #[snafu(display("failed to enumerate system fonts"))]
    EnumerateSystemFonts { source: FontEnumeratorError },
    #[snafu(display("failed to write list of fonts to stdout"))]
    WriteListFonts { source: io::Error },
}

#[derive(Debug, Snafu)]
pub(crate) enum ListFontsError {
    #[snafu(display("failed to list fonts; see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for ListFontsError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) fn list_fonts(cx: &RootContext, args: &ListFontArgs) -> Result<(), ListFontsError> {
    let ListFontArgs {
        show_system_fonts,
        show_user_fonts,
    } = args;
    let cx = ListFontsScope::start(cx);

    let font_enumerator = SystemFontEnumerator::new()
        .context(CreateFontEnumeratorSnafu)
        .report_error(&cx)?;

    let system_fonts = font_enumerator
        .fonts(cx.app_dirs())
        .context(EnumerateSystemFontsSnafu)
        .report_error(&cx)?;

    let system_fonts = system_fonts.filter(|font| match font {
        Ok(font) => match font.source {
            SystemFontSource::System => *show_system_fonts,
            SystemFontSource::User => *show_user_fonts,
            // Keep visible fonts whose storage cannot be attributed instead of silently dropping
            // them from the default view.
            SystemFontSource::FotonPackage { .. } | SystemFontSource::Unknown => true,
        },
        Err(_) => true,
    });

    let grouped_fonts = group_by_source(&cx, system_fonts)?;
    render_grouped_fonts(io::stdout().lock(), &grouped_fonts)
        .context(WriteListFontsSnafu)
        .report_error(&cx)?;

    Ok(())
}

fn group_by_source<I>(
    cx: &ReportContext<ListFontsScope>,
    fonts: I,
) -> Result<BTreeMap<SystemFontSource, Vec<FontFamilyInfo>>, ListFontsError>
where
    I: Iterator<Item = Result<SystemFont, FontEnumeratorError>>,
{
    let mut grouped_fonts: BTreeMap<SystemFontSource, FontFamilyAccumulator> = BTreeMap::new();

    for font in fonts {
        match font {
            Ok(font) => {
                let accumulator = grouped_fonts.entry(font.source.clone()).or_default();
                accumulator.add_face(font.family.clone(), font.face.clone());
            }
            Err(e) => EnumerateSystemFontSnafu.into_error(e).report_warn(cx)?,
        }
    }

    Ok(grouped_fonts
        .into_iter()
        .map(|(source, accumulator)| (source, accumulator.into_families()))
        .collect())
}

fn render_grouped_fonts<W>(
    mut writer: W,
    grouped_fonts: &BTreeMap<SystemFontSource, Vec<FontFamilyInfo>>,
) -> io::Result<()>
where
    W: io::Write,
{
    for (source, families) in grouped_fonts {
        match source {
            SystemFontSource::System => writeln!(writer, "System Fonts:")?,
            SystemFontSource::User => writeln!(writer, "User Fonts:")?,
            SystemFontSource::FotonPackage { pkg_id } => writeln!(writer, "Package {pkg_id}:")?,
            SystemFontSource::Unknown => writeln!(writer, "Unknown Source Fonts:")?,
        }
        for family in families {
            writeln!(writer, "  - {family}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, str::FromStr as _};

    use super::*;
    use crate::{package::PackageId, util::macros::concat_line};

    fn font_family_info(family: &str, faces: &[&str]) -> FontFamilyInfo {
        FontFamilyInfo {
            family: family.into(),
            faces: faces
                .iter()
                .map(|face| (*face).into())
                .collect::<BTreeSet<_>>(),
        }
    }

    #[test]
    fn render_grouped_fonts_prints_sections_and_grouped_faces() {
        let mut grouped_fonts = BTreeMap::new();
        grouped_fonts.insert(
            SystemFontSource::System,
            vec![font_family_info("Example Sans", &["Regular"])],
        );
        grouped_fonts.insert(
            SystemFontSource::User,
            vec![font_family_info("Example Serif", &["Italic"])],
        );
        grouped_fonts.insert(
            SystemFontSource::FotonPackage {
                pkg_id: PackageId::from_str("example-font@1.2.3").unwrap(),
            },
            vec![font_family_info("Example Font", &["Bold", "Regular"])],
        );
        grouped_fonts.insert(
            SystemFontSource::Unknown,
            vec![font_family_info("Example Mystery", &["Regular"])],
        );

        let mut output = vec![];
        render_grouped_fonts(&mut output, &grouped_fonts).unwrap();
        let output = String::from_utf8(output).unwrap();

        assert_eq!(
            output,
            concat_line!(
                "Package example-font@1.2.3:",
                "  - Example Font (Bold, Regular)",
                "System Fonts:",
                "  - Example Sans (Regular)",
                "User Fonts:",
                "  - Example Serif (Italic)",
                "Unknown Source Fonts:",
                "  - Example Mystery (Regular)",
                "",
            )
        );
    }
}
