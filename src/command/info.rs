use std::io;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::InfoArgs,
        context::{ReportContext, RootContext},
        reporter::{
            ErrorReportExt as _, NeverReport, OperationError, ReportScope, RootReportScope,
            ScopeResultErrorExt as _, ScopeResultWarnExt as _,
        },
    },
    db::PackageDbEntry,
    engine,
    package::{
        ActivationState, InstallationState, PackageDefinition, PackageDirs, PackageFont, PackageId,
        PackageSpec,
    },
    platform::windows::services::font_inspector::{
        FontInspector, FontInspectorError, InspectedFont, InspectedFontFace,
    },
    util::{
        font_family::{FontFamilyAccumulator, FontFamilyInfo},
        path::{AbsolutePath, FileName},
    },
};

#[derive(Debug, Default)]
struct InfoScope {}

impl ReportScope for InfoScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = InfoWarnReport;
    type ErrorReportValue = InfoErrorReport;
    type Error = InfoError;
}

impl RootReportScope for InfoScope {}

#[derive(Debug, Snafu)]
enum InfoWarnReport {
    #[snafu(display("failed to inspect font file: {path}", path = path.display()))]
    InspectFontFile {
        path: AbsolutePath,
        #[snafu(source(from(FontInspectorError, Box::new)))]
        source: Box<FontInspectorError>,
    },
}

#[derive(Debug, Snafu)]
enum InfoErrorReport {
    #[snafu(display("failed to create font inspector"))]
    CreateFontInspector { source: FontInspectorError },
    #[snafu(display("no package matches the specified package `{pkg_spec}`"))]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[snafu(display("failed to write package info to stdout"))]
    WriteInfo { source: io::Error },
}

#[derive(Debug, Snafu)]
pub(crate) enum InfoError {
    #[snafu(display("failed to print package information; see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for InfoError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) fn info_package(cx: &RootContext, args: &InfoArgs) -> Result<(), InfoError> {
    let InfoArgs {
        pkg_specs,
        exit_on_lock,
        show_files,
    } = args;

    let cx = InfoScope::start(cx);

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file, *exit_on_lock)?;

    let inspector = FontInspector::new()
        .context(CreateFontInspectorSnafu)
        .report_error(&cx)?;

    let mut res = Ok(());
    for pkg_spec in pkg_specs {
        let mut entries = db.entries_by_spec(pkg_spec).peekable();
        if entries.peek().is_none() {
            res = Err(NoMatchingPackageSnafu { pkg_spec }
                .build()
                .report_error(&cx));
            continue;
        }

        for entry in entries {
            let info = load_package_info(&cx, &entry, &inspector)?;
            render_package_info(io::stdout().lock(), &info, *show_files)
                .context(WriteInfoSnafu)
                .report_error(&cx)?;
        }
    }

    res
}

#[derive(Debug)]
struct PackageInfo {
    id: PackageId,
    installation_state: InstallationState,
    activation_state: ActivationState,
    display_name: Option<String>,
    description: Option<String>,
    aliases: Vec<String>,
    homepage: Option<String>,
    repository: Option<String>,
    license: Option<String>,
    installed_fonts: Option<InstalledFontsInfo>,
}

#[derive(Debug)]
struct InstalledFontsInfo {
    fonts_dir: AbsolutePath,
    families: Vec<FontFamilyInfo>,
    has_unavailable_fonts: bool,
    font_files: Vec<InstalledFontFile>,
}

#[derive(Debug)]
struct InstalledFontFile {
    file_name: FileName,
    inspection: InstalledFontInspection,
}

#[derive(Debug, derive_more::IsVariant)]
enum InstalledFontInspection {
    Available(Vec<FontFamilyInfo>),
    Unavailable,
}

fn load_package_info(
    cx: &ReportContext<InfoScope>,
    entry: &PackageDbEntry<'_>,
    inspector: &FontInspector,
) -> Result<PackageInfo, InfoError> {
    let installation_state = entry.installation_state();
    let activation_state = entry.activation_state();
    let PackageDefinition {
        id,
        display_name,
        description,
        aliases,
        homepage,
        repository,
        license,
        sources: _,
    } = entry.make_definition();
    let installed_fonts = installation_state
        .is_installed()
        .then(|| {
            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &id);
            let fonts_dir = pkg_dirs.fonts_dir().clone();
            let fonts = entry
                .fonts()
                .map(|font| load_font_info(cx, &pkg_dirs, &font, inspector))
                .collect::<Result<Vec<_>, _>>()?;
            let families = collect_families_from_files(&fonts);
            Ok(InstalledFontsInfo {
                fonts_dir,
                families,
                has_unavailable_fonts: fonts.iter().any(|font| !font.inspection.is_available()),
                font_files: fonts,
            })
        })
        .transpose()?;
    Ok(PackageInfo {
        id,
        installation_state,
        activation_state,
        display_name,
        description,
        aliases,
        homepage,
        repository,
        license,
        installed_fonts,
    })
}

fn load_font_info(
    cx: &ReportContext<InfoScope>,
    pkg_dirs: &PackageDirs,
    font: &PackageFont,
    inspector: &FontInspector,
) -> Result<InstalledFontFile, InfoError> {
    let path = &font.path(pkg_dirs);
    let res = inspector
        .inspect_font(path)
        .context(InspectFontFileSnafu { path })
        .report_warn(cx)?;
    let inspection = match res {
        Some(InspectedFont { faces }) => {
            let mut accum = FontFamilyAccumulator::default();
            for face in faces {
                let InspectedFontFace { family, face } = face;
                accum.add_face(family, face);
            }
            InstalledFontInspection::Available(accum.into_families())
        }
        None => InstalledFontInspection::Unavailable,
    };
    Ok(InstalledFontFile {
        file_name: font.file_name().clone(),
        inspection,
    })
}

fn collect_families_from_files(fonts: &[InstalledFontFile]) -> Vec<FontFamilyInfo> {
    let mut accum = FontFamilyAccumulator::default();
    for font in fonts {
        if let InstalledFontInspection::Available(inspection) = &font.inspection {
            for family in inspection {
                accum.append_family(family);
            }
        }
    }
    accum.into_families()
}

fn render_package_info<W>(mut writer: W, info: &PackageInfo, show_files: bool) -> io::Result<()>
where
    W: io::Write,
{
    let PackageInfo {
        id,
        installation_state,
        activation_state,
        display_name,
        description,
        aliases,
        homepage,
        repository,
        license,
        installed_fonts,
    } = info;

    writeln!(writer, "Package ID: {id}")?;
    if let Some(display_name) = display_name {
        writeln!(writer, "Display Name: {display_name}")?;
    }
    writeln!(writer, "Installation State: {installation_state}")?;
    writeln!(writer, "Activation State: {activation_state}")?;
    if let Some(description) = description {
        writeln!(writer, "Description: {description}")?;
    }
    if !aliases.is_empty() {
        writeln!(writer, "Aliases ({}):", aliases.len())?;
        for alias in aliases {
            writeln!(writer, "  - {alias}")?;
        }
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
    if let Some(installed_fonts) = installed_fonts {
        let InstalledFontsInfo {
            fonts_dir,
            font_files: fonts,
            families,
            has_unavailable_fonts,
        } = installed_fonts;

        writeln!(writer, "Installed Font Families ({}):", families.len())?;
        for family in families {
            writeln!(writer, "  - {family}")?;
        }
        if *has_unavailable_fonts {
            writeln!(
                writer,
                "  (some font files could not be inspected; see warnings)"
            )?;
        }

        if show_files {
            writeln!(writer, "Fonts Directory: {}", fonts_dir.display())?;
            writeln!(writer, "Installed Font Files ({}):", fonts.len())?;
            for font in fonts {
                let InstalledFontFile {
                    file_name,
                    inspection,
                } = font;
                writeln!(writer, "  - {}", file_name.display())?;
                match inspection {
                    InstalledFontInspection::Available(families) => {
                        for family in families {
                            writeln!(writer, "    - {family}")?;
                        }
                    }
                    InstalledFontInspection::Unavailable => {
                        writeln!(writer, "    (failed to inspect font file; see warnings)")?;
                    }
                }
            }
        }
    }
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::{macros::concat_line, path::FileName};

    #[track_caller]
    fn check_rendered_output(info: &PackageInfo, expected: &str, show_files: bool) {
        let mut output = vec![];
        render_package_info(&mut output, info, show_files).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output, expected);
    }

    fn font_family_info(family: &str, faces: &[&str]) -> FontFamilyInfo {
        FontFamilyInfo {
            family: family.into(),
            faces: faces.iter().map(Into::into).collect(),
        }
    }

    #[test]
    fn collect_families_from_files_groups_faces_by_family_across_files() {
        let fonts = [
            InstalledFontFile {
                file_name: FileName::new("example-font-collection.ttc").unwrap(),
                inspection: InstalledFontInspection::Available(vec![
                    font_family_info("Example Font", &["Regular", "Bold"]),
                    font_family_info("Example Font UI", &["Regular"]),
                ]),
            },
            InstalledFontFile {
                file_name: FileName::new("example-font-italic.ttf").unwrap(),
                inspection: InstalledFontInspection::Available(vec![font_family_info(
                    "Example Font",
                    &["Italic"],
                )]),
            },
            InstalledFontFile {
                file_name: FileName::new("example-font-bold.ttf").unwrap(),
                inspection: InstalledFontInspection::Unavailable,
            },
        ];
        assert_eq!(
            collect_families_from_files(&fonts),
            vec![
                font_family_info("Example Font", &["Bold", "Italic", "Regular"]),
                font_family_info("Example Font UI", &["Regular"]),
            ]
        );
    }

    #[test]
    fn render_package_info_prints_installed_package_details_and_recorded_fonts() {
        let info = PackageInfo {
            id: "example-font@0.1.0".parse().unwrap(),
            installation_state: InstallationState::Installed,
            activation_state: ActivationState::Active,
            display_name: Some("Example Font".to_owned()),
            description: Some("Example font family for UI and coding".to_owned()),
            aliases: vec!["Example Font UI".to_owned()],
            homepage: Some("https://example.com/example-font".to_owned()),
            repository: Some("https://github.com/example/example-font".to_owned()),
            license: Some("OFL-1.1".to_owned()),
            installed_fonts: Some(InstalledFontsInfo {
                fonts_dir: AbsolutePath::new(r"C:\path\to\fonts").unwrap(),
                families: vec![
                    font_family_info("Example Font", &["Regular", "Bold"]),
                    font_family_info("Example Font UI", &["Regular"]),
                ],
                has_unavailable_fonts: true,
                font_files: vec![
                    InstalledFontFile {
                        file_name: FileName::new("example-font-collection.ttc").unwrap(),
                        inspection: InstalledFontInspection::Available(vec![font_family_info(
                            "Example Font",
                            &["Regular", "Bold"],
                        )]),
                    },
                    InstalledFontFile {
                        file_name: FileName::new("example-font-ui-regular.ttf").unwrap(),
                        inspection: InstalledFontInspection::Available(vec![font_family_info(
                            "Example Font UI",
                            &["Regular"],
                        )]),
                    },
                    InstalledFontFile {
                        file_name: FileName::new("example-font-bold.ttf").unwrap(),
                        inspection: InstalledFontInspection::Unavailable,
                    },
                ],
            }),
        };

        let expected = concat_line!(
            "Package ID: example-font@0.1.0",
            "Display Name: Example Font",
            "Installation State: installed",
            "Activation State: active",
            "Description: Example font family for UI and coding",
            "Aliases (1):",
            "  - Example Font UI",
            "Homepage: https://example.com/example-font",
            "Repository: https://github.com/example/example-font",
            "License: OFL-1.1",
            "Installed Font Families (2):",
            "  - Example Font (Bold, Regular)",
            "  - Example Font UI (Regular)",
            "  (some font files could not be inspected; see warnings)",
            r"Fonts Directory: C:\path\to\fonts",
            "Installed Font Files (3):",
            "  - example-font-collection.ttc",
            "    - Example Font (Bold, Regular)",
            "  - example-font-ui-regular.ttf",
            "    - Example Font UI (Regular)",
            "  - example-font-bold.ttf",
            "    (failed to inspect font file; see warnings)",
            "",
            "",
        );
        check_rendered_output(&info, expected, true);

        let expected = concat_line!(
            "Package ID: example-font@0.1.0",
            "Display Name: Example Font",
            "Installation State: installed",
            "Activation State: active",
            "Description: Example font family for UI and coding",
            "Aliases (1):",
            "  - Example Font UI",
            "Homepage: https://example.com/example-font",
            "Repository: https://github.com/example/example-font",
            "License: OFL-1.1",
            "Installed Font Families (2):",
            "  - Example Font (Bold, Regular)",
            "  - Example Font UI (Regular)",
            "  (some font files could not be inspected; see warnings)",
            "",
            "",
        );
        check_rendered_output(&info, expected, false);
    }

    #[test]
    fn render_package_info_omits_installed_font_details_for_incomplete_install() {
        let info = PackageInfo {
            id: "example-font@0.1.0".parse().unwrap(),
            installation_state: InstallationState::IncompleteInstall,
            activation_state: ActivationState::Inactive,
            display_name: Some("Example Font".to_owned()),
            description: Some("Example font family for UI and coding".to_owned()),
            aliases: vec!["Example Font UI".to_owned()],
            homepage: Some("https://example.com/example-font".to_owned()),
            repository: Some("https://github.com/example/example-font".to_owned()),
            license: Some("OFL-1.1".to_owned()),
            installed_fonts: None,
        };

        let expected = concat_line!(
            "Package ID: example-font@0.1.0",
            "Display Name: Example Font",
            "Installation State: incomplete-install",
            "Activation State: inactive",
            "Description: Example font family for UI and coding",
            "Aliases (1):",
            "  - Example Font UI",
            "Homepage: https://example.com/example-font",
            "Repository: https://github.com/example/example-font",
            "License: OFL-1.1",
            "",
            "",
        );
        check_rendered_output(&info, expected, true);
        check_rendered_output(&info, expected, false);
    }
}
