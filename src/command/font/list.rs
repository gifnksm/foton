use std::{collections::BTreeMap, ffi::OsString, io, path::PathBuf};

use serde::Serialize;
use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::{ListFontArgs, ListFontFormat},
        context::{ReportContext, RootContext},
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, ScopeResultErrorExt as _,
            ScopeResultWarnExt as _,
        },
    },
    package::PackageId,
    platform::windows::services::font_enumerator::{
        FontEnumeratorError, FontLocation, SystemFont, SystemFontEnumerator,
    },
    util::{
        font_family::{FontFamilyAccumulator, FontFamilyInfo},
        ser_de,
    },
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
    #[snafu(display("failed to fetch system font information"))]
    FetchSystemFontInformation { source: FontEnumeratorError },
}

#[derive(Debug, Snafu)]
enum ListFontsErrorReport {
    #[snafu(display("failed to create font enumerator"))]
    CreateFontEnumerator { source: FontEnumeratorError },
    #[snafu(display("failed to enumerate system fonts"))]
    EnumerateSystemFonts { source: FontEnumeratorError },
    #[snafu(display("failed to serialize font information"))]
    SerializeToJsonl { source: serde_json::Error },
    #[snafu(display("failed to render font information to stdout"))]
    RenderToStdout { source: io::Error },
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
        include_system_fonts,
        include_user_fonts,
        format,
    } = args;
    let cx = ListFontsScope::start(cx);

    let font_enumerator = SystemFontEnumerator::new()
        .context(CreateFontEnumeratorSnafu)
        .report_error(&cx)?;

    let system_fonts = font_enumerator
        .fonts(cx.app_dirs())
        .context(EnumerateSystemFontsSnafu)
        .report_error(&cx)?;

    let system_fonts = system_fonts
        .filter(|font| match font {
            Ok(font) => match font.location {
                FontLocation::SystemDir { .. } => *include_system_fonts,
                FontLocation::UserDir { .. } => *include_user_fonts,
                // Keep visible fonts whose storage cannot be attributed instead of silently dropping
                // them from the default view.
                FontLocation::PackageDir { .. } | FontLocation::Unknown => true,
            },
            Err(_) => true,
        })
        .filter_map(|res| {
            res.context(FetchSystemFontInformationSnafu)
                .report_warn(&cx)
                .transpose()
        });

    match format {
        ListFontFormat::Text => {
            let grouped_fonts = group_by_location(system_fonts)?;
            render_grouped_fonts(&cx, io::stdout().lock(), &grouped_fonts)?;
        }
        ListFontFormat::Jsonl => {
            let lines = serialize_to_jsonl(&cx, system_fonts);
            render_lines(&cx, io::stdout().lock(), lines)?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum FontLocationGroup {
    PackageDir { pkg_id: PackageId },
    SystemDir,
    UserDir,
    Unknown,
}

impl FontLocationGroup {
    fn new(loc: FontLocation) -> Self {
        match loc {
            FontLocation::PackageDir { path: _, pkg_id } => Self::PackageDir { pkg_id },
            FontLocation::SystemDir { path: _ } => Self::SystemDir,
            FontLocation::UserDir { path: _ } => Self::UserDir,
            FontLocation::Unknown => Self::Unknown,
        }
    }
}

fn group_by_location<I>(
    fonts: I,
) -> Result<BTreeMap<FontLocationGroup, Vec<FontFamilyInfo>>, ListFontsError>
where
    I: IntoIterator<Item = Result<SystemFont, ListFontsError>>,
{
    let mut grouped_fonts: BTreeMap<FontLocationGroup, FontFamilyAccumulator> = BTreeMap::new();

    for font in fonts {
        let font = font?;
        let group = FontLocationGroup::new(font.location);
        let accumulator = grouped_fonts.entry(group).or_default();
        accumulator.add_face(font.family, font.face);
    }

    Ok(grouped_fonts
        .into_iter()
        .map(|(group, accumulator)| (group, accumulator.into_families()))
        .collect())
}

fn render_grouped_fonts<W>(
    cx: &ReportContext<ListFontsScope>,
    mut writer: W,
    grouped_fonts: &BTreeMap<FontLocationGroup, Vec<FontFamilyInfo>>,
) -> Result<(), ListFontsError>
where
    W: io::Write,
{
    (|| {
        for (loc, families) in grouped_fonts {
            match loc {
                FontLocationGroup::SystemDir => {
                    writeln!(writer, "Fonts from System Font Directory:")?;
                }
                FontLocationGroup::UserDir => {
                    writeln!(writer, "Fonts from User Font Directories:")?;
                }
                FontLocationGroup::PackageDir { pkg_id } => {
                    writeln!(writer, "Fonts from Package {pkg_id}:")?;
                }
                FontLocationGroup::Unknown => writeln!(writer, "Fonts from Unknown Locations:")?,
            }
            for family in families {
                writeln!(writer, "  - {family}")?;
            }
        }
        Ok(())
    })()
    .context(RenderToStdoutSnafu)
    .report_error(cx)
}

fn serialize_to_jsonl<I>(
    cx: &ReportContext<ListFontsScope>,
    fonts: I,
) -> impl Iterator<Item = Result<String, ListFontsError>>
where
    I: IntoIterator<Item = Result<SystemFont, ListFontsError>>,
{
    fonts.into_iter().map(move |res| {
        res.and_then(|font| {
            let entry = JsonlEntry::from(font);
            serde_json::to_string(&entry)
                .context(SerializeToJsonlSnafu)
                .report_error(cx)
        })
    })
}

fn render_lines<W, I>(
    cx: &ReportContext<ListFontsScope>,
    mut writer: W,
    lines: I,
) -> Result<(), ListFontsError>
where
    W: io::Write,
    I: IntoIterator<Item = Result<String, ListFontsError>>,
{
    for line in lines {
        let line = line?;
        writeln!(writer, "{line}")
            .context(RenderToStdoutSnafu)
            .report_error(cx)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct JsonlEntry {
    #[serde(with = "ser_de::readable_os_string")]
    family: OsString,
    #[serde(with = "ser_de::readable_os_string")]
    face: OsString,
    location: JsonlEntryLocation,
}

impl From<SystemFont> for JsonlEntry {
    fn from(value: SystemFont) -> Self {
        let SystemFont {
            family,
            face,
            location,
        } = value;
        Self {
            family,
            face,
            location: location.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "kebab-case"
)]
enum JsonlEntryLocation {
    PackageDir {
        #[serde(with = "ser_de::readable_path_buf")]
        path: PathBuf,
        #[serde(rename = "package-id")]
        pkg_id: PackageId,
    },
    SystemDir {
        #[serde(with = "ser_de::readable_path_buf")]
        path: PathBuf,
    },
    UserDir {
        #[serde(with = "ser_de::readable_path_buf")]
        path: PathBuf,
    },
    Unknown,
}

impl From<FontLocation> for JsonlEntryLocation {
    fn from(value: FontLocation) -> Self {
        match value {
            FontLocation::PackageDir { path, pkg_id } => Self::PackageDir { path, pkg_id },
            FontLocation::SystemDir { path } => Self::SystemDir { path },
            FontLocation::UserDir { path } => Self::UserDir { path },
            FontLocation::Unknown => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, str::FromStr as _};

    use super::*;
    use crate::{package::PackageId, util::testing};

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
            FontLocationGroup::SystemDir,
            vec![font_family_info("Example Sans", &["Regular"])],
        );
        grouped_fonts.insert(
            FontLocationGroup::UserDir,
            vec![font_family_info("Example Serif", &["Italic"])],
        );
        grouped_fonts.insert(
            FontLocationGroup::PackageDir {
                pkg_id: PackageId::from_str("example-font@1.2.3").unwrap(),
            },
            vec![font_family_info("Example Font", &["Bold", "Regular"])],
        );
        grouped_fonts.insert(
            FontLocationGroup::Unknown,
            vec![font_family_info("Example Mystery", &["Regular"])],
        );

        testing::with_root_context(|cx| {
            let cx = ListFontsScope::start(cx);

            let mut output = vec![];
            render_grouped_fonts(&cx, &mut output, &grouped_fonts).unwrap();
            let output = String::from_utf8(output).unwrap();

            let expected = indoc::indoc! {r"
                Fonts from Package example-font@1.2.3:
                  - Example Font (Bold, Regular)
                Fonts from System Font Directory:
                  - Example Sans (Regular)
                Fonts from User Font Directories:
                  - Example Serif (Italic)
                Fonts from Unknown Locations:
                  - Example Mystery (Regular)
            "};

            assert_eq!(output, expected);
        });
    }
}
