use std::io;

use snafu::{ResultExt as _, Snafu};

use crate::{
    cli::{
        args::InfoArgs,
        context::RootContext,
        reporter::{
            NeverReport, OperationError, ReportScope, ReportValue, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    engine,
    package::{PackageManifest, PackageMetadata, PackageSource, PackageSpec, PackageState},
};

#[derive(Debug)]
struct InfoScope {}

impl ReportScope for InfoScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InfoErrorReport;
    type Error = InfoError;
}

impl RootReportScope for InfoScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
enum InfoErrorReport {
    #[snafu(display("no package matches the specified package `{pkg_spec}`"))]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[snafu(display("failed to write package info to stdout"))]
    WriteInfo { source: io::Error },
}

impl From<InfoErrorReport> for ReportValue<'static> {
    fn from(report: InfoErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
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
        let mut manifests = db.entries_by_spec(pkg_spec).peekable();
        if manifests.peek().is_none() {
            res = Err(reporter.report_error(NoMatchingPackageSnafu { pkg_spec }.build()));
            continue;
        }

        for (state, manifest) in manifests {
            render_package_info(io::stdout().lock(), state, &manifest)
                .context(WriteInfoSnafu)
                .report_error(reporter)?;
        }
    }

    res
}

fn render_package_info<W>(
    mut writer: W,
    state: PackageState,
    manifest: &PackageManifest,
) -> io::Result<()>
where
    W: io::Write,
{
    let PackageManifest { metadata, sources } = manifest;
    let PackageMetadata {
        name,
        display_name,
        version,
        description,
        aliases,
        faces,
        homepage,
        repository,
        license,
    } = metadata;
    writeln!(writer, "Name: {name}")?;
    if let Some(display_name) = display_name {
        writeln!(writer, "Display Name: {display_name}")?;
    }
    writeln!(writer, "Version: {version}")?;
    writeln!(writer, "State: {state}")?;
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
    use super::*;
    use crate::util::testing;

    #[test]
    fn render_package_info_prints_all_present_fields() {
        let manifest = testing::make_manifest("example-font@0.1.0");
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::Installed, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-font\n",
                "Version: 0.1.0\n",
                "State: installed\n",
                "Sources[0]:\n",
                "  - URL: https://example.com/example-font-0.1.0.zip\n",
                "  - Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  - Includes:\n",
                "    - **/*.ttf\n",
                "    - **/*.otf\n",
                "    - **/*.ttc\n",
                "\n",
            )
        );
    }

    #[test]
    fn render_package_info_prints_optional_metadata_fields_when_present() {
        let manifest: PackageManifest = toml::from_str(
            r#"
[package]
name = "example-font"
display-name = "Example Font"
version = "0.1.0"
description = "Example font"
aliases = ["Example Font UI"]
faces = ["Example Font Regular", "Example Font Bold", "Example Font UI Regular", "Example Font UI Bold"]
homepage = "https://example.com/home"
repository = "https://example.com/repo"
license = "MIT"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
exclude = ["fonts/exclude.ttf"]
"#,
        )
        .unwrap();
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::PendingInstall, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-font\n",
                "Display Name: Example Font\n",
                "Version: 0.1.0\n",
                "State: pending-install\n",
                "Description: Example font\n",
                "Aliases:\n",
                "  - Example Font UI\n",
                "Faces:\n",
                "  - Example Font Regular\n",
                "  - Example Font Bold\n",
                "  - Example Font UI Regular\n",
                "  - Example Font UI Bold\n",
                "Homepage: https://example.com/home\n",
                "Repository: https://example.com/repo\n",
                "License: MIT\n",
                "Sources[0]:\n",
                "  - URL: https://example.com/example-font-0.1.0.zip\n",
                "  - Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  - Includes:\n",
                "    - fonts/*.ttf\n",
                "  - Excludes:\n",
                "    - fonts/exclude.ttf\n",
                "\n",
            )
        );
    }
}
