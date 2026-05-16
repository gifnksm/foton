use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::PathBuf,
};

use ::glob::Pattern;
use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        args::CheckManifestArgs,
        context::{ReportContext, RootContext},
        message::BulletList,
        reporter::{
            NeverReport, OperationError, ReportScope, ReportValue, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    engine,
    package::{self, Package, PackageDirs, PackageId, PackageManifest, PackageManifestError},
    util::{
        fs::FsError,
        glob::{self, GLOB_MATCH_OPTIONS},
        path::AbsolutePath,
        text::NormalizedString,
    },
};

#[derive(Debug)]
struct CheckManifestScope {}

impl ReportScope for CheckManifestScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = CheckManifestWarnReport;
    type ErrorReportValue = CheckManifestErrorReport;
    type Error = CheckManifestError;
}

impl RootReportScope for CheckManifestScope {
    fn new() -> Self {
        Self {}
    }
}

#[derive(Debug, Snafu)]
enum CheckManifestWarnReport {
    #[snafu(display("`package.display-name` is missing in manifest"))]
    MissingDisplayName,
    #[snafu(display("`package.license` is missing in manifest"))]
    MissingLicense,
    #[snafu(display("`package.description` is missing in manifest"))]
    MissingDescription,
    #[snafu(display(
        concat!(
            "following fields have the same normalized display name: {normalized}\n",
            "{values}",
            "consider removing one of the values to avoid duplication\n",
        ),
        normalized = normalized,
        values = BulletList(values),
    ))]
    DuplicatedDisplayName {
        normalized: String,
        values: Vec<ValueWithSource>,
    },
    #[snafu(display(
        concat!(
            "following fields have the same normalized font face: {normalized}\n",
            "{values}",
            "consider removing one of the values to avoid duplication\n",
        ),
        normalized = normalized,
        values = BulletList(values),
    ))]
    DuplicatedFace {
        normalized: String,
        values: Vec<ValueWithSource>,
    },
    #[snafu(display("`package.faces` does not contain the included font face: {face}"))]
    UnlistedFace { face: String },
    #[snafu(display(
        concat!(
            "`package.sources[{source_index}].include` contains wildcard pattern(s): {pattern}\n",
            "this pattern extracts to the following installable font paths:\n",
            "{extracted}\n",
            "consider replacing them with fixed strings to avoid unexpected matches\n",
        ),
        source_index = source_index,
        pattern = pattern,
        extracted = BulletList(&extracted.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    WildcardPattern {
        source_index: usize,
        pattern: Pattern,
        extracted: BTreeSet<PathBuf>,
    },
    #[snafu(display(
        concat!(
            "`package.sources[{source_index}].include` contains pattern `{pattern}` that matches no installable font paths\n",
            "consider removing it or replacing it with a pattern that matches the intended paths\n",
        ),
        source_index = source_index,
        pattern = pattern,
    ))]
    IncludePatternMatchesNothing {
        source_index: usize,
        pattern: Pattern,
    },
    #[snafu(display(
        concat!(
            "`package.sources[{source_index}].include` contains multiple patterns that match the same installable font path: {path}\n",
            "{patterns}\n",
            "consider removing redundant patterns",
        ),
        source_index = source_index,
        path = path.display(),
        patterns = BulletList(patterns),
    ))]
    MultiplePatternsMatchSamePath {
        source_index: usize,
        path: PathBuf,
        patterns: Vec<Pattern>,
    },
}

#[derive(Debug, derive_more::Display)]
#[display("`{value}` (from `{source_field}`)")]
struct ValueWithSource {
    value: String,
    source_field: &'static str,
}

impl From<CheckManifestWarnReport> for ReportValue<'static> {
    fn from(report: CheckManifestWarnReport) -> Self {
        Self::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
enum CheckManifestErrorReport {
    #[snafu(transparent)]
    ReadManifest { source: PackageManifestError },
    #[snafu(display("failed to create temporary directory for manifest checking"))]
    CreateTempDir { source: io::Error },
    #[snafu(display("path of temporary directory for manifest checking is not an absolute path: {path}", path = path.display()))]
    NonAbsoluteTempDir { path: PathBuf },
    #[snafu(display("failed to create package directories for package {pkg_id}"))]
    CreatePackageDirs { pkg_id: PackageId, source: FsError },
}

impl From<CheckManifestErrorReport> for ReportValue<'static> {
    fn from(report: CheckManifestErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

#[derive(Debug, Snafu)]
pub(crate) enum CheckManifestError {
    #[snafu(display("failed to check manifest; see previous messages for details"))]
    Failed,
    #[snafu(display("operation cancelled"))]
    Cancelled,
}

impl OperationError for CheckManifestError {
    fn failed() -> Self {
        Self::Failed
    }

    fn cancelled() -> Self {
        Self::Cancelled
    }
}

pub(crate) async fn check_manifest(
    cx: &RootContext,
    args: &CheckManifestArgs,
) -> Result<(), CheckManifestError> {
    let CheckManifestArgs { manifest_path } = args;
    let cx = CheckManifestScope::start_with_report(cx, format_args!("Checking manifest..."));

    let manifest = PackageManifest::read(manifest_path)
        .map_err(CheckManifestErrorReport::from)
        .report_error(cx.reporter())?;
    let pkg_id = manifest.metadata.id();

    let tempdir = tempfile::tempdir()
        .context(CreateTempDirSnafu)
        .report_error(cx.reporter())?;

    let tempdir_path = AbsolutePath::new(tempdir.path())
        .with_context(|| NonAbsoluteTempDirSnafu {
            path: tempdir.path(),
        })
        .report_error(cx.reporter())?;

    let pkg_dirs = PackageDirs::new_in(&tempdir_path, &pkg_id);

    package::create_new_package_dirs(&pkg_dirs)
        .context(CreatePackageDirsSnafu { pkg_id })
        .report_error(cx.reporter())?;

    let package = engine::stage_package(&cx, &pkg_dirs, &manifest).await?;
    check_manifest_fields(&cx, &manifest, &package)?;

    Ok(())
}

fn check_manifest_fields(
    cx: &ReportContext<CheckManifestScope>,
    manifest: &PackageManifest,
    package: &Package,
) -> Result<(), CheckManifestError> {
    let mut reports = vec![];
    if manifest.metadata.display_name.is_none() {
        reports.push(MissingDisplayNameSnafu.build());
    }
    if manifest.metadata.license.is_none() {
        reports.push(MissingLicenseSnafu.build());
    }
    if manifest.metadata.description.is_none() {
        reports.push(MissingDescriptionSnafu.build());
    }

    check_display_name_duplication(manifest, &mut reports);
    check_face_duplication(manifest, &mut reports);
    check_face_completeness(manifest, package, &mut reports);
    check_include_extraction(manifest, package, &mut reports);

    let mut err = None;
    for report in reports {
        if let Err(e) = cx.reporter().report_warn(report) {
            err = Some(e);
        }
    }
    if let Some(err) = err {
        return Err(err);
    }
    Ok(())
}

fn check_display_name_duplication(
    manifest: &PackageManifest,
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    let mut display_names: BTreeMap<String, Vec<_>> = BTreeMap::new();
    if let Some(display_name) = &manifest.metadata.display_name {
        let normalized = NormalizedString::new(display_name).into_compact();
        display_names
            .entry(normalized)
            .or_default()
            .push(ValueWithSource {
                value: display_name.clone(),
                source_field: "package.display-name",
            });
    }
    for alias in &manifest.metadata.aliases {
        let normalized = NormalizedString::new(alias).into_compact();
        display_names
            .entry(normalized)
            .or_default()
            .push(ValueWithSource {
                value: alias.clone(),
                source_field: "package.aliases",
            });
    }
    for (normalized, values) in display_names {
        if values.len() > 1 {
            reports.push(DuplicatedDisplayNameSnafu { normalized, values }.build());
        }
    }
}

fn check_face_duplication(manifest: &PackageManifest, reports: &mut Vec<CheckManifestWarnReport>) {
    let mut faces: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for face in &manifest.metadata.faces {
        let normalized = NormalizedString::new(face).into_compact();
        faces.entry(normalized).or_default().push(ValueWithSource {
            value: face.clone(),
            source_field: "package.faces",
        });
    }
    for (normalized, values) in faces {
        if values.len() > 1 {
            reports.push(DuplicatedFaceSnafu { normalized, values }.build());
        }
    }
}

fn check_face_completeness(
    manifest: &PackageManifest,
    package: &Package,
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    for entry in package.entries() {
        if !manifest
            .metadata
            .faces
            .iter()
            .any(|face| entry.title() == face)
        {
            reports.push(
                UnlistedFaceSnafu {
                    face: entry.title(),
                }
                .build(),
            );
        }
    }
}

fn find_match_paths(
    package: &Package,
    source_index: usize,
    pattern: &Pattern,
) -> BTreeSet<PathBuf> {
    package
        .entries()
        .iter()
        .filter_map(|entry| {
            let source = entry.source();
            (source.source_index() == source_index
                && pattern.matches_path_with(source.path_in_source(), GLOB_MATCH_OPTIONS))
            .then(|| source.path_in_source().clone())
        })
        .collect()
}

fn check_include_extraction(
    manifest: &PackageManifest,
    package: &Package,
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    for (source_index, source) in manifest.sources.iter().enumerate() {
        let mut source_patterns: BTreeMap<PathBuf, Vec<_>> = BTreeMap::new();
        for pattern in &source.include {
            let extracted = find_match_paths(package, source_index, pattern);
            for path in &extracted {
                source_patterns
                    .entry(path.clone())
                    .or_default()
                    .push(pattern.clone());
            }
            if extracted.is_empty() {
                reports.push(
                    IncludePatternMatchesNothingSnafu {
                        source_index,
                        pattern: pattern.clone(),
                    }
                    .build(),
                );
            } else if glob::is_wildcard_pattern(pattern) {
                reports.push(
                    WildcardPatternSnafu {
                        source_index,
                        pattern: pattern.clone(),
                        extracted,
                    }
                    .build(),
                );
            }
        }
        for (path, patterns) in source_patterns {
            if patterns.len() > 1 {
                reports.push(
                    MultiplePatternsMatchSamePathSnafu {
                        source_index,
                        path,
                        patterns,
                    }
                    .build(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::{
        package::{FontEntry, FontSource},
        util::{path::FileName, testing},
    };

    static PKG_ID: LazyLock<PackageId> = LazyLock::new(|| "example-font@0.1.0".parse().unwrap());

    fn parse_manifest(input: &str) -> PackageManifest {
        toml::from_str(input).unwrap()
    }

    fn make_package(entries: &[(&str, &str, usize, &str)]) -> Package {
        let (_tempdir, _app_dirs, pkg_dirs) = testing::make_package_dirs(&PKG_ID);
        let entries = entries
            .iter()
            .map(|(title, file_name, source_index, path_in_source)| {
                FontEntry::new(
                    *title,
                    FileName::new(*file_name).unwrap(),
                    FontSource::new(*source_index, *path_in_source),
                )
            })
            .collect();
        Package::new(PKG_ID.clone(), pkg_dirs, entries)
    }

    #[test]
    fn check_display_name_duplication_reports_compact_duplicates() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
display-name = "UDPGothic"
version = "0.1.0"
aliases = ["UDP Gothic"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let mut reports = vec![];

        check_display_name_duplication(&manifest, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::DuplicatedDisplayName { normalized, values }]
                if normalized == "udpgothic" && values.len() == 2
        ));
    }

    #[test]
    fn check_face_duplication_reports_compact_duplicates() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"
faces = ["HackGen Regular", "HackGenRegular"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let mut reports = vec![];

        check_face_duplication(&manifest, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::DuplicatedFace { normalized, values }]
                if normalized == "hackgenregular" && values.len() == 2
        ));
    }

    #[test]
    fn check_face_completeness_reports_unlisted_faces() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"
faces = ["Listed Face"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let package = make_package(&[("Actual Face", "actual.ttf", 0, "fonts/actual.ttf")]);
        let mut reports = vec![];

        check_face_completeness(&manifest, &package, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::UnlistedFace { face }] if face == "Actual Face"
        ));
    }

    #[test]
    fn check_include_extraction_reports_patterns_matching_no_installable_font_paths() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/missing.ttf"]
"#,
        );
        let package = make_package(&[("Actual Face", "actual.ttf", 0, "fonts/actual.ttf")]);
        let mut reports = vec![];

        check_include_extraction(&manifest, &package, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::IncludePatternMatchesNothing { source_index, pattern }]
                if *source_index == 0 && pattern.as_str() == "fonts/missing.ttf"
        ));
    }

    #[test]
    fn check_include_extraction_reports_wildcard_patterns_with_matching_installable_font_paths() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]
"#,
        );
        let package = make_package(&[
            ("Font A", "a.ttf", 0, "fonts/a.ttf"),
            ("Font B", "b.ttf", 0, "fonts/b.ttf"),
            ("Font C", "c.ttf", 1, "fonts/c.ttf"),
        ]);
        let mut reports = vec![];

        check_include_extraction(&manifest, &package, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::WildcardPattern { source_index, pattern, extracted }]
                if *source_index == 0
                    && pattern.as_str() == "fonts/*.ttf"
                    && extracted
                        == &BTreeSet::from([PathBuf::from("fonts/a.ttf"), PathBuf::from("fonts/b.ttf")])
        ));
    }

    #[test]
    fn check_include_extraction_reports_multiple_patterns_matching_same_path() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/a.ttf", "fonts/a.ttf"]
"#,
        );
        let package = make_package(&[("Font A", "a.ttf", 0, "fonts/a.ttf")]);
        let mut reports = vec![];

        check_include_extraction(&manifest, &package, &mut reports);

        assert!(matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::MultiplePatternsMatchSamePath {
                source_index,
                path,
                patterns,
            }] if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && patterns.iter().map(Pattern::as_str).collect::<Vec<_>>()
                    == vec!["fonts/a.ttf", "fonts/a.ttf"]
        ));
    }

    #[test]
    fn check_include_extraction_reports_wildcard_and_duplicate_match_independently() {
        let manifest = parse_manifest(
            r#"
[package]
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf", "fonts/a.ttf"]
"#,
        );
        let package = make_package(&[("Font A", "a.ttf", 0, "fonts/a.ttf")]);
        let mut reports = vec![];

        check_include_extraction(&manifest, &package, &mut reports);

        assert!(reports.iter().any(|report| matches!(
            report,
            CheckManifestWarnReport::WildcardPattern {
                source_index,
                pattern,
                extracted,
            } if *source_index == 0
                && pattern.as_str() == "fonts/*.ttf"
                && extracted == &BTreeSet::from([PathBuf::from("fonts/a.ttf")])
        )));
        assert!(reports.iter().any(|report| matches!(
            report,
            CheckManifestWarnReport::MultiplePatternsMatchSamePath {
                source_index,
                path,
                patterns,
            } if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && patterns.iter().map(Pattern::as_str).collect::<Vec<_>>()
                    == vec!["fonts/*.ttf", "fonts/a.ttf"]
        )));
    }
}
