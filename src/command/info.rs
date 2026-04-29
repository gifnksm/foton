use std::io;

use crate::{
    cli::context::RootContext,
    command::common,
    package::{PackageManifest, PackageMetadata, PackageSource, PackageSpec, PackageState},
    util::reporter::{NeverReport, ReportValue, Step, StepResultErrorExt as _},
};

#[derive(Debug)]
struct InfoStep {}

impl Step for InfoStep {
    type WarnReportValue = NeverReport;
    type ErrorReportValue = InfoErrorReport;
    type Error = InfoError;

    fn make_failed(&self) -> Self::Error {
        InfoError::Failed
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
enum InfoErrorReport {
    #[display("no package matches the specified package `{pkg_spec}`")]
    NoMatchingPackage { pkg_spec: PackageSpec },
    #[display("failed to write package info to stdout")]
    WriteInfo {
        #[error(source)]
        source: io::Error,
    },
}

impl From<InfoErrorReport> for ReportValue<'static> {
    fn from(report: InfoErrorReport) -> Self {
        ReportValue::BoxedError(report.into())
    }
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum InfoError {
    #[display("failed to print package information")]
    Failed,
}

pub(crate) fn info_package(cx: &RootContext, pkg_spec: &PackageSpec) -> Result<(), InfoError> {
    let cx = cx.with_step(InfoStep {});
    let reporter = cx.reporter();

    let mut db_lock_file = common::steps::open_db_lock_file(&cx)?;
    let db = common::steps::load_database(&cx, &mut db_lock_file)?;

    let Some((state, manifest)) = common::steps::resolve_spec_in_db(&cx, &db, pkg_spec)? else {
        return Err(reporter.report_error(InfoErrorReport::NoMatchingPackage {
            pkg_spec: pkg_spec.clone(),
        }));
    };

    render_package_info(io::stdout().lock(), state, manifest)
        .map_err(|source| InfoErrorReport::WriteInfo { source })
        .report_error(reporter)?;

    Ok(())
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
        qualified_name,
        version,
        description,
        homepage,
        repository,
        license,
    } = metadata;
    writeln!(writer, "Name: {qualified_name}")?;
    writeln!(writer, "Version: {version}")?;
    writeln!(writer, "State: {state}")?;
    if let Some(description) = description {
        writeln!(writer, "Description: {description}")?;
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
    writeln!(writer, "Sources:")?;
    for source in sources {
        let PackageSource { url, hash, include } = source;
        writeln!(writer, "- URL: {url}")?;
        writeln!(writer, "  Hash: {hash}")?;
        writeln!(
            writer,
            "  Includes: {}",
            include
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::util::testing;

    use super::*;

    #[test]
    fn render_package_info_prints_all_present_fields() {
        let manifest = testing::make_manifest("example-namespace", "example-font", "0.1.0");
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::Installed, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-namespace/example-font\n",
                "Version: 0.1.0\n",
                "State: installed\n",
                "Sources:\n",
                "- URL: https://example.com/example-font-0.1.0.zip\n",
                "  Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  Includes: **/*.ttf, **/*.otf, **/*.ttc\n",
            )
        );
    }

    #[test]
    fn render_package_info_prints_optional_metadata_fields_when_present() {
        let manifest: PackageManifest = toml::from_str(
            r#"
[package]
name = "example-namespace/example-font"
version = "0.1.0"
description = "Example font"
homepage = "https://example.com/home"
repository = "https://example.com/repo"
license = "MIT"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
"#,
        )
        .unwrap();
        let mut output = Vec::new();

        render_package_info(&mut output, PackageState::PendingInstall, &manifest).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert_eq!(
            output,
            concat!(
                "Name: example-namespace/example-font\n",
                "Version: 0.1.0\n",
                "State: pending-install\n",
                "Description: Example font\n",
                "Homepage: https://example.com/home\n",
                "Repository: https://example.com/repo\n",
                "License: MIT\n",
                "Sources:\n",
                "- URL: https://example.com/example-font-0.1.0.zip\n",
                "  Hash: sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n",
                "  Includes: fonts/*.ttf\n",
            )
        );
    }
}
