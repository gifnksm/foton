use std::io;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::InfoArgs,
        context::RootContext,
        reporter::{
            NeverReport, OperationError, ReportScope, RootReportScope, ScopeResultErrorExt as _,
        },
    },
    db::PackageDbEntry,
    engine,
    package::{PackageManifest, PackageSource, PackageSpec},
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
    let InfoArgs { pkg_specs } = args;

    let cx = InfoScope::start(cx);
    let reporter = cx.reporter();

    let mut db_lock_file = engine::open_db_lock_file(&cx)?;
    let db = engine::load_database(&cx, &mut db_lock_file)?;

    let mut res = Ok(());
    for pkg_spec in pkg_specs {
        let mut entries = db.entries_by_spec(pkg_spec).peekable();
        if entries.peek().is_none() {
            res = Err(reporter.report_error(NoMatchingPackageSnafu { pkg_spec }.build()));
            continue;
        }

        for entry in entries {
            render_package_info(io::stdout().lock(), &entry)
                .context(WriteInfoSnafu)
                .report_error(reporter)?;
        }
    }

    res
}

fn render_package_info<W>(mut writer: W, entry: &PackageDbEntry<'_>) -> io::Result<()>
where
    W: io::Write,
{
    let PackageManifest {
        name,
        display_name,
        version,
        description,
        aliases,
        faces,
        homepage,
        repository,
        license,
        sources,
    } = &*entry.manifest();
    writeln!(writer, "Name: {name}")?;
    if let Some(display_name) = display_name {
        writeln!(writer, "Display Name: {display_name}")?;
    }
    writeln!(writer, "Version: {version}")?;
    writeln!(writer, "Installation State: {}", entry.installation_state())?;
    writeln!(writer, "Activation State: {}", entry.activation_state())?;
    if let Some(description) = description {
        writeln!(writer, "Description: {description}")?;
    }
    if !aliases.is_empty() {
        writeln!(writer, "Aliases:")?;
        for alias in aliases {
            writeln!(writer, "  - {alias}")?;
        }
    }
    if !faces.is_empty() {
        writeln!(writer, "Faces:")?;
        for font_name in faces {
            writeln!(writer, "  - {font_name}")?;
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
    for (source_index, source) in sources.iter().enumerate() {
        writeln!(writer, "Sources[{source_index}]:")?;
        let PackageSource {
            url,
            hash,
            include,
            exclude,
        } = source;
        writeln!(writer, "  - URL: {url}")?;
        writeln!(writer, "  - Hash: {hash}")?;
        if !include.is_empty() {
            writeln!(writer, "  - Includes:")?;
            for pattern in include {
                writeln!(writer, "    - {pattern}")?;
            }
        }
        if !exclude.is_empty() {
            writeln!(writer, "  - Excludes:")?;
            for pattern in exclude {
                writeln!(writer, "    - {pattern}")?;
            }
        }
    }
    writeln!(writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::util::{macros::concat_line, testing};

    #[test]
    fn render_package_info_prints_all_present_fields() {
        let manifest = testing::make_manifest("example-font@0.1.0");

        let output = testing::with_db(|_cx, db| {
            testing::mark_as_active(db, &manifest);

            let mut output = Vec::new();
            let entry = db.entry_by_id(&manifest.id()).unwrap();
            render_package_info(&mut output, &entry).unwrap();
            String::from_utf8(output).unwrap()
        });

        assert_eq!(
            output,
            concat_line!(
                "Name: example-font",
                "Version: 0.1.0",
                "Installation State: installed",
                "Activation State: active",
                "Sources[0]:",
                "  - URL: https://example.com/example-font-0.1.0.zip",
                "  - Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "  - Includes:",
                "    - **/*.ttf",
                "    - **/*.otf",
                "    - **/*.ttc",
                "",
                "",
            )
        );
    }

    #[test]
    fn render_package_info_prints_optional_metadata_fields_when_present() {
        let manifest: PackageManifest = toml::from_str(
            r#"
name = "example-font"
display-name = "Example Font"
version = "0.1.0"
description = "Example font family for UI and coding"
aliases = ["Example Font UI"]
faces = ["Example Font Regular", "Example Font Bold", "Example Font UI Regular", "Example Font UI Bold"]
homepage = "https://example.com/example-font"
repository = "https://github.com/example/example-font"
license = "OFL-1.1"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
exclude = ["fonts/exclude.ttf"]
"#,
        )
        .unwrap();
        let manifest = Arc::new(manifest);

        let output = testing::with_db(|_cx, db| {
            testing::mark_as_incomplete_install(db, &manifest);

            let mut output = Vec::new();
            let entry = db.entry_by_id(&manifest.id()).unwrap();
            render_package_info(&mut output, &entry).unwrap();
            String::from_utf8(output).unwrap()
        });

        assert_eq!(
            output,
            concat_line!(
                "Name: example-font",
                "Display Name: Example Font",
                "Version: 0.1.0",
                "Installation State: incomplete-install",
                "Activation State: inactive",
                "Description: Example font family for UI and coding",
                "Aliases:",
                "  - Example Font UI",
                "Faces:",
                "  - Example Font Regular",
                "  - Example Font Bold",
                "  - Example Font UI Regular",
                "  - Example Font UI Bold",
                "Homepage: https://example.com/example-font",
                "Repository: https://github.com/example/example-font",
                "License: OFL-1.1",
                "Sources[0]:",
                "  - URL: https://example.com/example-font-0.1.0.zip",
                "  - Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                "  - Includes:",
                "    - fonts/*.ttf",
                "  - Excludes:",
                "    - fonts/exclude.ttf",
                "",
                "",
            )
        );
    }
}
