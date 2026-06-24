use std::io;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::InfoArgs,
        context::{ReportContext, RootContext},
        reporter::{
            ErrorReportExt as _, NeverReport, OperationError, ReportScope, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    db::PackageDbEntry,
    engine,
    package::{PackageDefinition, PackageDirs, PackageSpec},
};

#[derive(Debug, Default)]
struct InfoScope {}

impl ReportScope for InfoScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InfoErrorReport;
    type Error = InfoError;
}

impl RootReportScope for InfoScope {}

#[derive(Debug, Snafu)]
enum InfoErrorReport {
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
    } = args;

    let cx = InfoScope::start(cx);

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file, *exit_on_lock)?;

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
            render_package_info(&cx, io::stdout().lock(), &entry)
                .context(WriteInfoSnafu)
                .report_error(&cx)?;
        }
    }

    res
}

fn render_package_info<S, W>(
    cx: &ReportContext<S>,
    mut writer: W,
    entry: &PackageDbEntry<'_>,
) -> io::Result<()>
where
    S: ReportScope,
    W: io::Write,
{
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
    let pkg_dirs = PackageDirs::new(cx.app_dirs(), &id);

    writeln!(writer, "Package ID: {id}")?;
    if let Some(display_name) = display_name {
        writeln!(writer, "Display Name: {display_name}")?;
    }
    writeln!(writer, "Installation State: {}", entry.installation_state())?;
    writeln!(writer, "Activation State: {}", entry.activation_state())?;
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
    if entry.installation_state().is_installed() {
        let font_entries = entry.font_entries().collect::<Vec<_>>();
        writeln!(
            writer,
            "Fonts Directory: {}",
            pkg_dirs.fonts_dir().display(),
        )?;
        writeln!(writer, "Installed Fonts ({}):", font_entries.len())?;
        for font in &font_entries {
            writeln!(
                writer,
                "  - {} ({})",
                font.file_name().display(),
                font.title(),
            )?;
        }
    }
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        db::PackageDatabase,
        package::FontEntry,
        util::{macros::concat_line, path::FileName, testing},
    };

    fn make_font_entries(entries: &[(&str, &str)]) -> Vec<FontEntry> {
        entries
            .iter()
            .copied()
            .map(|(title, file_name)| FontEntry::new(title, FileName::new(file_name).unwrap()))
            .collect()
    }

    fn mark_as_active_with_fonts(
        db: &mut PackageDatabase<'_>,
        pkg: &PackageDefinition,
        font_entries: &[FontEntry],
    ) {
        let mut tx = db.transaction();
        tx.begin_install(pkg);
        tx.complete_install(&pkg.id, font_entries).unwrap();
        tx.begin_activate(&pkg.id).unwrap();
        tx.complete_activate(&pkg.id).unwrap();
        tx.commit().unwrap();
    }

    #[test]
    fn render_package_info_prints_installed_package_details_and_recorded_fonts() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
display-name = "Example Font"
version = "0.1.0"
description = "Example font family for UI and coding"
aliases = ["Example Font UI"]
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "fonts/*.ttf" }]
ignore = ["fonts/exclude.ttf"]
"#,
        );
        let font_entries = make_font_entries(&[
            ("Example Font Regular", "example-font-regular.ttf"),
            ("Example Font Bold", "example-font-bold.ttf"),
        ]);

        let (output, expected) = testing::with_db(|cx, db| {
            mark_as_active_with_fonts(db, &pkg, &font_entries);

            let mut output = Vec::new();
            let entry = db.entry_by_id(&pkg.id).unwrap();
            render_package_info(cx, &mut output, &entry).unwrap();
            let output = String::from_utf8(output).unwrap();

            let pkg_dirs = PackageDirs::new(cx.app_dirs(), &pkg.id);
            let expected = format!(
                concat_line!(
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
                    "Fonts Directory: {fonts_dir}",
                    "Installed Fonts (2):",
                    "  - example-font-regular.ttf (Example Font Regular)",
                    "  - example-font-bold.ttf (Example Font Bold)",
                    "",
                    "",
                ),
                fonts_dir = pkg_dirs.fonts_dir().display(),
            );

            (output, expected)
        });

        assert_eq!(output, expected);
    }

    #[test]
    fn render_package_info_omits_installed_font_details_for_incomplete_install() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
display-name = "Example Font"
version = "0.1.0"
description = "Example font family for UI and coding"
aliases = ["Example Font UI"]
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "fonts/*.ttf" }]
ignore = ["fonts/exclude.ttf"]
"#,
        );

        let output = testing::with_db(|cx, db| {
            testing::mark_as_incomplete_install(db, &pkg);

            let mut output = Vec::new();
            let entry = db.entry_by_id(&pkg.id).unwrap();
            render_package_info(cx, &mut output, &entry).unwrap();
            String::from_utf8(output).unwrap()
        });

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

        assert_eq!(output, expected);
    }
}
