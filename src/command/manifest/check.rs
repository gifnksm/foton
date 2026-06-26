use std::{
    collections::{BTreeMap, BTreeSet},
    io,
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
    engine::{self, ArchiveExtractDetail, ExtractDetail},
    package::{
        self, FontRule, IgnoreRule, PackageDefinition, PackageDirs, PackageId, PackageManifest,
        PackageManifestError,
    },
    util::{
        fs::FsError, glob::PathGlob, macros::concat_line, path::AbsolutePath,
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
            "`sources[{source_index}].contents.fonts` contains `glob` rule: {glob}",
            "this glob matches the following paths:",
            "{extracted}",
            "prefer explicit `path` entries when the intended files are known",
        ),
        source_index = source_index,
        glob = glob,
        extracted = BulletList(&extracted.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    ArchiveFontsGlob {
        source_index: usize,
        glob: PathGlob,
        extracted: BTreeSet<PathBuf>,
    },
    #[snafu(display(
        concat_line!(
            "`{rule}` in `sources[{source_index}].contents.fonts` does not match any paths",
            "consider removing it or replacing it with a `path` or `glob` that matches the intended files",
        ),
        source_index = source_index,
        rule = rule,
    ))]
    ArchiveFontsRuleMatchesNothing { source_index: usize, rule: FontRule },
    #[snafu(display(
        concat_line!(
            "`{rule}` in `sources[{source_index}].contents.fonts` matches only paths that are also ignored",
            "{ignored}",
            "consider removing the redundant `fonts` rule or adjusting `fonts` / `ignore` so the intended files are selected",
        ),
        source_index = source_index,
        rule = rule,
        ignored = BulletList(&ignored.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    ArchiveFontsRuleMatchesOnlyIgnoredPaths {
        source_index: usize,
        rule: FontRule,
        ignored: BTreeSet<PathBuf>,
    },
    #[snafu(display(
        concat_line!(
            "`sources[{source_index}].contents.fonts` contains multiple rules that match the same path: {path}",
            "{rules}",
            "consider removing redundant rules",
        ),
        source_index = source_index,
        path = path.display(),
        rules = BulletList(rules),
    ))]
    MultipleArchiveFontsRulesMatchSamePath {
        source_index: usize,
        path: PathBuf,
        rules: Vec<FontRule>,
    },
    #[snafu(display(
        concat_line!(
            "`{rule}` in `sources[{source_index}].contents.ignore` does not match any paths",
            "consider removing it or replacing it with a `path` or `glob` that matches the intended files",
        ),
        source_index = source_index,
        rule = rule,
    ))]
    ArchiveIgnoreRuleMatchesNothing {
        source_index: usize,
        rule: IgnoreRule,
    },
    #[snafu(display(
        concat_line!(
            "`sources[{source_index}]` contains font-like paths that match neither `fonts` nor `ignore`",
            "{skipped}",
            "consider adding them to `fonts` or `ignore` explicitly, depending on the intended behavior",
        ),
        source_index = source_index,
        skipped = BulletList(&skipped.iter().map(|path| path.display()).collect::<Vec<_>>()),
    ))]
    ArchiveSuspiciousSkip {
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

    let (_font_entries, extract_details) = engine::stage_package(&cx, &pkg_dirs, &pkg).await?;
    check_fields(&cx, &pkg, &extract_details)?;

    Ok(())
}

fn check_fields(
    cx: &ReportContext<CheckManifestScope>,
    pkg: &PackageDefinition,
    extract_details: &[ExtractDetail<'_>],
) -> Result<(), CheckManifestError> {
    let mut reports = vec![];

    check_missing_fields(pkg, &mut reports);
    check_homepage_and_repository_url(pkg, &mut reports);
    check_display_name_duplication(pkg, &mut reports);
    check_archive_fonts_extraction(pkg, extract_details, &mut reports);
    check_archive_ignore_extraction(pkg, extract_details, &mut reports);
    check_archive_suspicious_skips(extract_details, &mut reports);

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

fn collect_file_rule_matches(
    extract_detail: &ArchiveExtractDetail<'_>,
    rule: &FontRule,
) -> BTreeSet<PathBuf> {
    extract_detail
        .included
        .iter()
        .filter(|entry| rule.matches(&entry.path_in_archive))
        .map(|entry| entry.path_in_archive.clone())
        .collect()
}

fn collect_ignored_file_rule_matches(
    extract_detail: &ArchiveExtractDetail<'_>,
    rule: &FontRule,
) -> BTreeSet<PathBuf> {
    extract_detail
        .excluded
        .iter()
        .filter(|entry| rule.matches(&entry.path_in_archive))
        .map(|entry| entry.path_in_archive.clone())
        .collect()
}

fn collect_ignore_rule_matches(
    extract_detail: &ArchiveExtractDetail<'_>,
    rule: &IgnoreRule,
) -> BTreeSet<PathBuf> {
    extract_detail
        .excluded
        .iter()
        .filter(|entry| rule.matches(&entry.path_in_archive))
        .map(|entry| entry.path_in_archive.clone())
        .collect()
}

fn check_archive_fonts_extraction(
    pkg: &PackageDefinition,
    extract_details: &[ExtractDetail<'_>],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    assert_eq!(pkg.sources.len(), extract_details.len());
    for (source_index, extract_detail) in extract_details.iter().enumerate() {
        let Some(extract_detail) = extract_detail.as_archive() else {
            continue;
        };
        let mut source_rules: BTreeMap<PathBuf, Vec<_>> = BTreeMap::new();
        for rule in &extract_detail.options.fonts {
            let extracted = collect_file_rule_matches(extract_detail, rule);
            let ignored = collect_ignored_file_rule_matches(extract_detail, rule);
            for path in &extracted {
                source_rules
                    .entry(path.clone())
                    .or_default()
                    .push(rule.clone());
            }
            if extracted.is_empty() {
                if ignored.is_empty() {
                    reports.push(
                        ArchiveFontsRuleMatchesNothingSnafu {
                            source_index,
                            rule: rule.clone(),
                        }
                        .build(),
                    );
                } else {
                    reports.push(
                        ArchiveFontsRuleMatchesOnlyIgnoredPathsSnafu {
                            source_index,
                            rule: rule.clone(),
                            ignored,
                        }
                        .build(),
                    );
                }
            } else if let Some(glob) = rule.matcher().as_glob() {
                reports.push(
                    ArchiveFontsGlobSnafu {
                        source_index,
                        glob: glob.clone(),
                        extracted,
                    }
                    .build(),
                );
            }
        }
        for (path, rules) in source_rules {
            if rules.len() > 1 {
                reports.push(
                    MultipleArchiveFontsRulesMatchSamePathSnafu {
                        source_index,
                        path,
                        rules,
                    }
                    .build(),
                );
            }
        }
    }
}

fn check_archive_ignore_extraction(
    pkg: &PackageDefinition,
    extract_details: &[ExtractDetail<'_>],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    assert_eq!(pkg.sources.len(), extract_details.len());
    for (source_index, extract_detail) in extract_details.iter().enumerate() {
        let Some(extract_detail) = extract_detail.as_archive() else {
            continue;
        };
        for rule in &extract_detail.options.ignore {
            let extracted = collect_ignore_rule_matches(extract_detail, rule);
            if extracted.is_empty() {
                reports.push(
                    ArchiveIgnoreRuleMatchesNothingSnafu {
                        source_index,
                        rule: rule.clone(),
                    }
                    .build(),
                );
            }
        }
    }
}

fn check_archive_suspicious_skips(
    extract_details: &[ExtractDetail<'_>],
    reports: &mut Vec<CheckManifestWarnReport>,
) {
    for (source_index, extract_detail) in extract_details.iter().enumerate() {
        let Some(extract_detail) = extract_detail.as_archive() else {
            continue;
        };
        if !extract_detail.suspicious_skips.is_empty() {
            let skipped: BTreeSet<_> = extract_detail
                .suspicious_skips
                .iter()
                .map(|entry| entry.path_in_archive.clone())
                .collect();
            reports.push(
                ArchiveSuspiciousSkipSnafu {
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
    use crate::{engine::ExtractEntry, package::ArchiveOptions, util::testing};

    fn to_extract_entries(paths: &[&str]) -> Vec<ExtractEntry> {
        paths
            .iter()
            .copied()
            .map(|path| ExtractEntry {
                path_in_archive: path.into(),
            })
            .collect()
    }

    fn make_archive_extract_detail<'s>(
        options: &'s ArchiveOptions,
        included: &[&str],
        excluded: &[&str],
        suspicious_skips: &[&str],
    ) -> ExtractDetail<'s> {
        ExtractDetail::Archive(ArchiveExtractDetail {
            options,
            included: to_extract_entries(included),
            excluded: to_extract_entries(excluded),
            suspicious_skips: to_extract_entries(suspicious_skips),
        })
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

[sources.contents]
type = "archive"
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

[sources.contents]
type = "archive"
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

[sources.contents]
type = "archive"
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
    fn check_archive_fonts_extraction_reports_rules_matching_no_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = ["fonts/missing.ttf"]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/actual.ttf"],
            &[],
            &[],
        )];
        let mut reports = vec![];

        check_archive_fonts_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ArchiveFontsRuleMatchesNothing { source_index, rule }]
                if *source_index == 0 && rule.to_string() == "fonts/missing.ttf"
        );
    }

    #[test]
    fn check_archive_fonts_extraction_reports_rules_matching_only_ignored_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = ["fonts/skip.ttf"]
ignore = ["fonts/skip.ttf"]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/actual.ttf"],
            &["fonts/skip.ttf"],
            &[],
        )];
        let mut reports = vec![];

        check_archive_fonts_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ArchiveFontsRuleMatchesOnlyIgnoredPaths {
                source_index,
                rule,
                ignored,
            }] if *source_index == 0
                && rule.to_string() == "fonts/skip.ttf"
                && ignored == &BTreeSet::from([PathBuf::from("fonts/skip.ttf")])
        );
    }

    #[test]
    fn check_archive_fonts_extraction_reports_glob_rules() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "fonts/a.ttf" }]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/a.ttf"],
            &[],
            &[],
        )];
        let mut reports = vec![];

        check_archive_fonts_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ArchiveFontsGlob { source_index, glob, extracted }]
                if *source_index == 0
                    && glob.as_str() == "fonts/a.ttf"
                    && extracted == &BTreeSet::from([PathBuf::from("fonts/a.ttf")])
        );
    }

    #[test]
    fn check_archive_fonts_extraction_reports_multiple_rules_matching_same_path() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = ["fonts/a.ttf", { path = "fonts/a.ttf" }]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/a.ttf"],
            &[],
            &[],
        )];
        let mut reports = vec![];

        check_archive_fonts_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::MultipleArchiveFontsRulesMatchSamePath {
                source_index,
                path,
                rules,
            }] if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && rules.iter().map(ToString::to_string).collect::<Vec<_>>()
                    == vec!["fonts/a.ttf", "fonts/a.ttf"]
        );
    }

    #[test]
    fn check_archive_fonts_extraction_reports_glob_rule_and_duplicate_match_independently() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
fonts = [{ glob = "fonts/*.ttf" }, "fonts/a.ttf"]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/a.ttf"],
            &[],
            &[],
        )];
        let mut reports = vec![];

        check_archive_fonts_extraction(&pkg, &extract_details, &mut reports);

        assert!(reports.iter().any(|report| matches!(
            report,
            CheckManifestWarnReport::ArchiveFontsGlob {
                source_index,
                glob,
                extracted,
            } if *source_index == 0
                && glob.as_str() == "fonts/*.ttf"
                && extracted == &BTreeSet::from([PathBuf::from("fonts/a.ttf")])
        )));
        assert!(reports.iter().any(|report| matches!(
            report,
            CheckManifestWarnReport::MultipleArchiveFontsRulesMatchSamePath {
                source_index,
                path,
                rules,
            } if *source_index == 0
                && path == &PathBuf::from("fonts/a.ttf")
                && rules.iter().map(ToString::to_string).collect::<Vec<_>>()
                    == vec!["fonts/*.ttf", "fonts/a.ttf"]
        )));
    }

    #[test]
    fn check_archive_ignore_extraction_reports_rules_matching_no_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
ignore = ["fonts/missing.ttf"]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/actual.ttf"],
            &[],
            &[],
        )];
        let mut reports = vec![];

        check_archive_ignore_extraction(&pkg, &extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ArchiveIgnoreRuleMatchesNothing { source_index, rule }]
                if *source_index == 0 && rule.to_string() == "fonts/missing.ttf"
        );
    }

    #[test]
    fn check_archive_ignore_extraction_does_not_report_when_rule_matches_paths() {
        let pkg = testing::parse_manifest_to_definition(
            r#"
name = "example-font"
version = "0.1.0"

[[sources]]
url = "https://example.com/example-font-0.1.0.zip"
hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[sources.contents]
type = "archive"
ignore = ["fonts/skip.ttf"]
"#,
        );
        let options = pkg.sources[0].contents.as_archive().unwrap();
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/actual.ttf"],
            &["fonts/skip.ttf"],
            &[],
        )];
        let mut reports = vec![];

        check_archive_ignore_extraction(&pkg, &extract_details, &mut reports);

        assert!(reports.is_empty());
    }

    #[test]
    fn check_archive_suspicious_skips_reports_font_like_paths_not_covered_by_files_or_ignore() {
        let options = &ArchiveOptions {
            fonts: vec![],
            ignore: vec![],
        };
        let extract_details = [make_archive_extract_detail(
            options,
            &["fonts/actual.ttf"],
            &[],
            &["fonts/SKIP.TTF", "fonts/skip.otf"],
        )];
        let mut reports = vec![];

        check_archive_suspicious_skips(&extract_details, &mut reports);

        assert_matches!(
            reports.as_slice(),
            [CheckManifestWarnReport::ArchiveSuspiciousSkip { source_index, skipped }]
                if *source_index == 0
                    && skipped == &BTreeSet::from([
                        PathBuf::from("fonts/SKIP.TTF"),
                        PathBuf::from("fonts/skip.otf"),
                    ])
        );
    }
}
