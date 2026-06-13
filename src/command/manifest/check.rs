use std::{
    collections::{BTreeMap, BTreeSet},
    io, iter,
    path::PathBuf,
};

use snafu::{OptionExt as _, ResultExt as _, Snafu};

use crate::{
    cli::{
        args::CheckManifestArgs,
        context::{ReportContext, RootContext},
        message::BulletList,
        reporter::{
            ErrorReportExt as _, NeverReport, OperationError, ReportScope, RootReportScope,
            ScopeResultErrorExt as _,
        },
    },
    engine::{self, ExtractDetail},
    package::{
        self, FontEntry, PackageDefinition, PackageDirs, PackageId, PackageManifest,
        PackageManifestError,
    },
    util::{
        fs::FsError, glob::PathPattern, macros::concat_line, path::AbsolutePath,
        text::NormalizedString,
    },
};

#[derive(Debug, Default)]
struct CheckManifestScope {}

impl ReportScope for CheckManifestScope {
    type NoticeReportValue = NeverReport;
    type WarnReportValue = CheckManifestWarnReport;
    type ErrorReportValue = CheckManifestErrorReport;
    type Error = CheckManifestError;
}

impl RootReportScope for CheckManifestScope {}

#[derive(Debug, Snafu)]
enum CheckManifestWarnReport {
    #[snafu(display("`display-name` field is missing in manifest"))]
    MissingDisplayName,
    #[snafu(display("`license` field is missing in manifest"))]
    MissingLicense,
    #[snafu(display("`description` field is missing in manifest"))]
    MissingDescription,
    #[snafu(display(
        concat_line!(
            "`homepage` and `repository` fields have the same URL: {url}",
            "if there is no distinct upstream homepage, omit `homepage` rather than repeating `repository`",
        ),
        url = url,
    ))]
    SameHomepageAndRepository { url: String },
    #[snafu(display(
        concat_line!(
            "following fields have the same normalized display name: {normalized}",
            "{values}",
            "consider removing one of the values to avoid duplication",
        ),
        normalized = normalized,
        values = BulletList(values),
    ))]
    DuplicatedDisplayName {
        normalized: String,
        values: Vec<ValueWithSource>,
    },
    #[snafu(display(
        concat_line!(
            "following fields have the same normalized font face: {normalized}",
            "{values}",
            "consider removing one of the values to avoid duplication",
        ),
        normalized = normalized,
        values = BulletList(values),
    ))]
    DuplicatedFace {
        normalized: String,
        values: Vec<ValueWithSource>,
    },
    #[snafu(display("`faces` does not list the included font face: {face}"))]
    UnlistedFace { face: String },
    #[snafu(display(
        concat_line!(
            "`sources[{source_index}].include` contains wildcard pattern: {pattern}",
            "this pattern matches the following paths:",
            "{extracted}",
            "consider replacing them with fixed strings to avoid unexpected matches",
        ),
        source_index = source_index,
        pattern = pattern,
        extracted = BulletList(&extracted.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    WildcardPattern {
        source_index: usize,
        pattern: PathPattern,
        extracted: BTreeSet<PathBuf>,
    },
    #[snafu(display(
        concat_line!(
            "`{pattern}` in `sources[{source_index}].include` does not match any paths",
            "consider removing it or replacing it with a pattern that matches the intended paths",
        ),
        source_index = source_index,
        pattern = pattern,
    ))]
    IncludePatternMatchesNothing {
        source_index: usize,
        pattern: PathPattern,
    },
    #[snafu(display(
        concat_line!(
            "`sources[{source_index}].include` contains multiple patterns that match the same path: {path}",
            "{patterns}",
            "consider removing redundant patterns",
        ),
        source_index = source_index,
        path = path.display(),
        patterns = BulletList(patterns),
    ))]
    MultipleIncludePatternsMatchSamePath {
        source_index: usize,
        path: PathBuf,
        patterns: Vec<PathPattern>,
    },
    #[snafu(display(
        concat_line!(
            "`{pattern}` in `sources[{source_index}].exclude` does not match any paths",
            "consider removing it or replacing it with a pattern that matches the intended paths",
        ),
        source_index = source_index,
        pattern = pattern,
    ))]
    ExcludePatternMatchesNothing {
        source_index: usize,
        pattern: PathPattern,
    },
    #[snafu(display(
        concat_line!(
            "`sources[{source_index}]` contains font-like paths that match neither `include` nor `exclude`",
            "{skipped}",
            "consider adding them to `include` or `exclude` explicitly, depending on the intended behavior",
        ),
        source_index = source_index,
        skipped = BulletList(&skipped.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    SuspiciousSkip {
        source_index: usize,
        skipped: BTreeSet<PathBuf>,
    },
}

#[derive(Debug, derive_more::Display)]
#[display("`{value}` (from `{source_field}`)")]
struct ValueWithSource {
    value: String,
    source_field: &'static str,
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
        .report_error(&cx)?;
    let pkg_id = manifest.id();

    let tempdir = tempfile::tempdir()
        .context(CreateTempDirSnafu)
        .report_error(&cx)?;

    let tempdir_path = AbsolutePath::new(tempdir.path())
        .with_context(|| NonAbsoluteTempDirSnafu {
            path: tempdir.path(),
        })
        .report_error(&cx)?;

    let pkg_dirs = PackageDirs::new_in(&tempdir_path, &pkg_id);
    let pkg = manifest.into();

    package::create_new_package_dirs(&pkg_dirs)
        .context(CreatePackageDirsSnafu { pkg_id })
        .report_error(&cx)?;

    let (font_entries, extract_details) = engine::stage_package(&cx, &pkg_dirs, &pkg).await?;
    check_fields(&cx, &pkg, &font_entries, &extract_details)?;

    Ok(())
}

fn check_fields(
    cx: &ReportContext<CheckManifestScope>,
    pkg: &PackageDefinition,
    font_entries: &[FontEntry],
    extract_details: &[ExtractDetail],
) -> Result<(), CheckManifestError> {
    let mut reports = vec![];

    check_missing_fields(pkg, &mut reports);
    check_homepage_and_repository_url(pkg, &mut reports);
    check_display_name_duplication(pkg, &mut reports);
    check_face_duplication(pkg, &mut reports);
    check_face_completeness(pkg, font_entries, &mut reports);
    check_include_extraction(pkg, extract_details, &mut reports);
    check_exclude_extraction(pkg, extract_details, &mut reports);
    check_suspicious_skips(extract_details, &mut reports);

    let mut err = None;
    for report in reports {
        if let Err(e) = report.report_warn(&cx) {
            err = Some(e);
        }
    }
    if let Some(err) = err {
        return Err(err);
    }
    Ok(())
}

fn check_missing_fields(pkg: &PackageDefinition, reports: &mut Vec<CheckManifestWarnReport>) {
    if pkg.display_name.is_none() {
        reports.push(MissingDisplayNameSnafu.build());
    }
    if pkg.license.is_none() {
        reports.push(MissingLicenseSnafu.build());
    }
    if pkg.description.is_none() {
        reports.push(MissingDescriptionSnafu.build());
    }
}

fn check_homepage_and_repository_url(
    pkg: &PackageDefinition,
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    if let Some(url) = &pkg.homepage
        && pkg.repository == pkg.homepage
    {
        reports.push(SameHomepageAndRepositorySnafu { url: url.clone() }.build());
    }
}

fn check_display_name_duplication(
    pkg: &PackageDefinition,
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    let mut display_names: BTreeMap<String, Vec<_>> = BTreeMap::new();
    if let Some(display_name) = &pkg.display_name {
        let normalized = NormalizedString::new(display_name).into_compact();
        display_names
            .entry(normalized)
            .or_default()
            .push(ValueWithSource {
                value: display_name.clone(),
                source_field: "display-name",
            });
    }
    for alias in &pkg.aliases {
        let normalized = NormalizedString::new(alias).into_compact();
        display_names
            .entry(normalized)
            .or_default()
            .push(ValueWithSource {
                value: alias.clone(),
                source_field: "aliases",
            });
    }
    for (normalized, values) in display_names {
        if values.len() > 1 {
            reports.push(DuplicatedDisplayNameSnafu { normalized, values }.build());
        }
    }
}

fn check_face_duplication(pkg: &PackageDefinition, reports: &mut Vec<CheckManifestWarnReport>) {
    let mut faces: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for face in &pkg.faces {
        let normalized = NormalizedString::new(face).into_compact();
        faces.entry(normalized).or_default().push(ValueWithSource {
            value: face.clone(),
            source_field: "faces",
        });
    }
    for (normalized, values) in faces {
        if values.len() > 1 {
            reports.push(DuplicatedFaceSnafu { normalized, values }.build());
        }
    }
}

fn check_face_completeness(
    pkg: &PackageDefinition,
    font_entries: &[FontEntry],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    for entry in font_entries {
        if !pkg.faces.iter().any(|face| entry.title() == face) {
            reports.push(
                UnlistedFaceSnafu {
                    face: entry.title(),
                }
                .build(),
            );
        }
    }
}

fn collect_included_paths(
    extract_detail: &ExtractDetail,
    pattern: &PathPattern,
) -> BTreeSet<PathBuf> {
    extract_detail
        .included
        .iter()
        .filter(|entry| pattern.matches(&entry.path_in_archive))
        .map(|entry| entry.path_in_archive.clone())
        .collect()
}

fn collect_excluded_paths(
    extract_detail: &ExtractDetail,
    pattern: &PathPattern,
) -> BTreeSet<PathBuf> {
    extract_detail
        .excluded
        .iter()
        .filter(|entry| pattern.matches(&entry.path_in_archive))
        .map(|entry| entry.path_in_archive.clone())
        .collect()
}

fn check_include_extraction(
    pkg: &PackageDefinition,
    extract_details: &[ExtractDetail],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    assert_eq!(pkg.sources.len(), extract_details.len());
    for (source_index, (source, extract_detail)) in
        iter::zip(&pkg.sources, extract_details).enumerate()
    {
        let mut source_patterns: BTreeMap<PathBuf, Vec<_>> = BTreeMap::new();
        for pattern in &source.include {
            let extracted = collect_included_paths(extract_detail, pattern);
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
            } else if pattern.contains_wildcard() {
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
                    MultipleIncludePatternsMatchSamePathSnafu {
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

fn check_exclude_extraction(
    pkg: &PackageDefinition,
    extract_details: &[ExtractDetail],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    assert_eq!(pkg.sources.len(), extract_details.len());
    for (source_index, (source, extract_detail)) in
        iter::zip(&pkg.sources, extract_details).enumerate()
    {
        for pattern in &source.exclude {
            let extracted = collect_excluded_paths(extract_detail, pattern);
            if extracted.is_empty() {
                reports.push(
                    ExcludePatternMatchesNothingSnafu {
                        source_index,
                        pattern: pattern.clone(),
                    }
                    .build(),
                );
            }
        }
    }
}

fn check_suspicious_skips(
    extract_details: &[ExtractDetail],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    for (source_index, extract_detail) in extract_details.iter().enumerate() {
        if !extract_detail.suspicious_skips.is_empty() {
            let skipped: BTreeSet<_> = extract_detail
                .suspicious_skips
                .iter()
                .map(|entry| entry.path_in_archive.clone())
                .collect();
            reports.push(
                SuspiciousSkipSnafu {
                    source_index,
                    skipped,
                }
                .build(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{
        engine::ExtractEntry,
        package::FontEntry,
        util::{path::FileName, testing},
    };

    fn make_font_entries(entries: &[(&str, &str)]) -> Vec<FontEntry> {
        entries
            .iter()
            .copied()
            .map(|(title, file_name)| FontEntry::new(title, FileName::new(file_name).unwrap()))
            .collect()
    }

    fn to_extract_entries(paths: &[&str]) -> Vec<ExtractEntry> {
        paths
            .iter()
            .copied()
            .map(|path| ExtractEntry {
                path_in_archive: path.into(),
            })
            .collect()
    }

    fn make_extract_detail(
        included: &[&str],
        excluded: &[&str],
        suspicious_skips: &[&str],
    ) -> ExtractDetail {
        ExtractDetail {
            included: to_extract_entries(included),
            excluded: to_extract_entries(excluded),
            suspicious_skips: to_extract_entries(suspicious_skips),
        }
    }

    #[test]
    fn check_missing_fields_reports_all_missing_fields() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let mut reports = vec![];

        check_missing_fields(&pkg, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [
                CheckManifestWarnReport::MissingDisplayName,
                CheckManifestWarnReport::MissingLicense,
                CheckManifestWarnReport::MissingDescription,
            ]
        );
    }

    #[test]
    fn check_homepage_and_repository_url_reports_same_url() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"
homepage = "https://example.com/repo"
repository = "https://example.com/repo"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let mut reports = vec![];

        check_homepage_and_repository_url(&pkg, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::SameHomepageAndRepository { url }] if url.as_str() == "https://example.com/repo"
        );
    }

    #[test]
    fn check_display_name_duplication_reports_compact_duplicates() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
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

        check_display_name_duplication(&pkg, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::DuplicatedDisplayName { normalized, values }]
                if normalized == "udpgothic" && values.len() == 2
        );
    }

    #[test]
    fn check_face_duplication_reports_compact_duplicates() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"
faces = ["HackGen Regular", "HackGenRegular"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let mut reports = vec![];

        check_face_duplication(&pkg, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::DuplicatedFace { normalized, values }]
                if normalized == "hackgenregular" && values.len() == 2
        );
    }

    #[test]
    fn check_face_completeness_reports_unlisted_faces() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"
faces = ["Listed Face"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
"#,
        );
        let font_entries = make_font_entries(&[("Actual Face", "actual.ttf")]);
        let mut reports = vec![];

        check_face_completeness(&pkg, &font_entries, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::UnlistedFace { face }] if face == "Actual Face"
        );
    }

    #[test]
    fn check_include_extraction_reports_patterns_matching_no_font_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/missing.ttf"]
"#,
        );
        let extract_details = [make_extract_detail(&["fonts/actual.ttf"], &[], &[])];
        let mut reports = vec![];

        check_include_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::IncludePatternMatchesNothing { source_index, pattern }]
                if *source_index == 0 && pattern.as_str() == "fonts/missing.ttf"
        );
    }

    #[test]
    fn check_include_extraction_reports_wildcard_patterns_with_matching_font_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf"]

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/c.ttf"]
"#,
        );
        let extract_details = [
            make_extract_detail(&["fonts/a.ttf", "fonts/b.ttf"], &[], &[]),
            make_extract_detail(&["fonts/c.ttf"], &[], &[]),
        ];
        let mut reports = vec![];

        check_include_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::WildcardPattern { source_index, pattern, extracted }]
                if *source_index == 0
                    && pattern.as_str() == "fonts/*.ttf"
                    && extracted
                        == &BTreeSet::from([PathBuf::from("fonts/a.ttf"), PathBuf::from("fonts/b.ttf")])
        );
    }

    #[test]
    fn check_include_extraction_reports_multiple_patterns_matching_same_path() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/a.ttf", "fonts/a.ttf"]
"#,
        );
        let extract_details = [make_extract_detail(&["fonts/a.ttf"], &[], &[])];
        let mut reports = vec![];

        check_include_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::MultipleIncludePatternsMatchSamePath {
                source_index,
                path,
                patterns,
            }] if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && patterns.iter().map(PathPattern::as_str).collect::<Vec<_>>()
                    == vec!["fonts/a.ttf", "fonts/a.ttf"]
        );
    }

    #[test]
    fn check_include_extraction_reports_wildcard_and_duplicate_match_independently() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
include = ["fonts/*.ttf", "fonts/a.ttf"]
"#,
        );
        let extract_details = [make_extract_detail(&["fonts/a.ttf"], &[], &[])];
        let mut reports = vec![];

        check_include_extraction(&pkg, &extract_details, &mut reports);

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
            CheckManifestWarnReport::MultipleIncludePatternsMatchSamePath {
                source_index,
                path,
                patterns,
            } if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && patterns.iter().map(PathPattern::as_str).collect::<Vec<_>>()
                    == vec!["fonts/*.ttf", "fonts/a.ttf"]
        )));
    }

    #[test]
    fn check_exclude_extraction_reports_patterns_matching_no_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
exclude = ["fonts/missing.ttf"]
"#,
        );
        let extract_details = [make_extract_detail(&["fonts/actual.ttf"], &[], &[])];
        let mut reports = vec![];

        check_exclude_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ExcludePatternMatchesNothing { source_index, pattern }]
                if *source_index == 0 && pattern.as_str() == "fonts/missing.ttf"
        );
    }

    #[test]
    fn check_exclude_extraction_does_not_report_when_pattern_matches_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
exclude = ["fonts/skip.ttf"]
"#,
        );
        let extract_details = [make_extract_detail(
            &["fonts/actual.ttf"],
            &["fonts/skip.ttf"],
            &[],
        )];
        let mut reports = vec![];

        check_exclude_extraction(&pkg, &extract_details, &mut reports);

        assert!(reports.is_empty());
    }

    #[test]
    fn check_suspicious_skips_reports_font_like_paths_not_covered_by_include_or_exclude() {
        let extract_details = [make_extract_detail(
            &["fonts/actual.ttf"],
            &[],
            &["fonts/SKIP.TTF", "fonts/skip.otf"],
        )];
        let mut reports = vec![];

        check_suspicious_skips(&extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::SuspiciousSkip { source_index, skipped }]
                if *source_index == 0
                    && skipped == &BTreeSet::from([
                        PathBuf::from("fonts/SKIP.TTF"),
                        PathBuf::from("fonts/skip.otf"),
                    ])
        );
    }
}
