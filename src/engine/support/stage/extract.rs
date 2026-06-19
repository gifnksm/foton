use std::{
    ffi::OsString,
    fs::File,
    io::{self, Write as _},
    marker::PhantomData,
    path::PathBuf,
};

use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use zip::{ZipArchive, result::ZipError};

use crate::{
    cli::{
        config::FotonConfig,
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope},
    },
    package::{FileRule, IgnoreRule},
    util::path::{AbsolutePath, FileName},
};

#[derive(Debug, Default)]
struct ExtractScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ExtractScope<S>
where
    S: ReportScope,
{
    type NoticeReportValue = NeverReport;
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ExtractErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ExtractScope<S> where S: ReportScope {}

#[derive(Debug, Snafu)]
enum ExtractErrorReport {
    #[snafu(display("failed to read archive"))]
    ReadArchive { source: ZipError },
    #[snafu(display("failed to extract file with index {index}"))]
    ExtractFile { index: usize, source: ZipError },
    #[snafu(display(
        "archive entry `{file_path}` has extracted size {file_size} exceeding the maximum allowed size of {max_size}",
        file_path = file_path.display(),
    ))]
    ExtractedFileExceedsMaxSize {
        file_path: PathBuf,
        file_size: u64,
        max_size: u64,
    },
    #[snafu(display("failed to get file name for archive entry with index {index}"))]
    GetFileName { index: usize },
    #[snafu(display(
        "invalid file name `{file_name}` in archive entry with index {index}",
        file_name = file_name.display()
    ))]
    InvalidFileName { file_name: OsString, index: usize },
    #[snafu(display("archive contains more than {max_files} extractable font files"))]
    TooManyExtractableFiles { max_files: usize },
    #[snafu(display("extracted font file already exists: {path}", path = path.display()))]
    ExtractedFileAlreadyExists { path: PathBuf, source: io::Error },
    #[snafu(display("failed to create font file: {path}", path = path.display()))]
    CreateExtractedFile { path: PathBuf, source: io::Error },
    #[snafu(display("failed to copy extracted font file to destination: {path}", path = path.display()))]
    CopyExtractedFile { path: PathBuf, source: io::Error },
    #[snafu(display("failed to flush font file: {path}", path = path.display()))]
    FlushExtractedFile { path: PathBuf, source: io::Error },
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractedFile<'r> {
    file_name: FileName,
    rule: &'r FileRule,
}

impl<'r> ExtractedFile<'r> {
    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }

    pub(crate) fn rule(&self) -> &'r FileRule {
        self.rule
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractEntry {
    pub(crate) path_in_archive: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ExtractDetail {
    pub(crate) included: Vec<ExtractEntry>,
    pub(crate) excluded: Vec<ExtractEntry>,
    pub(crate) suspicious_skips: Vec<ExtractEntry>,
}

pub(super) fn extract_archive<'r, S>(
    cx: &ReportContext<S>,
    file: File,
    files: &'r [FileRule],
    ignore: &[IgnoreRule],
    fonts_dir: &AbsolutePath,
) -> Result<(Vec<ExtractedFile<'r>>, ExtractDetail), S::Error>
where
    S: ReportScope,
{
    let cx = ExtractScope::start_with_report(
        cx,
        format_args!("Extracting archive to {}...", fonts_dir.display()),
    );
    extract_archive_impl(file, files, ignore, fonts_dir, cx.config()).report_error(&cx)
}

fn extract_archive_impl<'r>(
    file: File,
    files: &'r [FileRule],
    ignore: &[IgnoreRule],
    fonts_dir: &AbsolutePath,
    config: &FotonConfig,
) -> Result<(Vec<ExtractedFile<'r>>, ExtractDetail), ExtractErrorReport> {
    let mut extracted = vec![];
    let mut included = vec![];
    let mut excluded = vec![];
    let mut suspicious_skips = vec![];
    let mut archive = ZipArchive::new(file).context(ReadArchiveSnafu)?;

    for i in 0..archive.len() {
        let mut archive_file = archive.by_index(i).context(ExtractFileSnafu { index: i })?;

        if !archive_file.is_file() {
            continue;
        }
        let Some(path_in_archive) = archive_file.enclosed_name() else {
            continue;
        };
        let is_excluded = ignore.iter().any(|rule| rule.matches(&path_in_archive));
        if is_excluded {
            excluded.push(ExtractEntry { path_in_archive });
            continue;
        }
        let Some(include_rule) = files.iter().find(|rule| rule.matches(&path_in_archive)) else {
            if let Some(ext) = path_in_archive.extension()
                && (ext.eq_ignore_ascii_case("ttf")
                    || ext.eq_ignore_ascii_case("otf")
                    || ext.eq_ignore_ascii_case("ttc"))
            {
                suspicious_skips.push(ExtractEntry { path_in_archive });
            }
            continue;
        };
        snafu::ensure!(
            archive_file.size() <= config.install.max_extracted_file_size_bytes,
            ExtractedFileExceedsMaxSizeSnafu {
                file_path: path_in_archive,
                file_size: archive_file.size(),
                max_size: config.install.max_extracted_file_size_bytes,
            }
        );

        let file_name = path_in_archive
            .file_name()
            .context(GetFileNameSnafu { index: i })?
            .to_owned();
        let file_name = FileName::new(&file_name).context(InvalidFileNameSnafu {
            file_name,
            index: i,
        })?;
        let fs_path = fonts_dir.join(&file_name);

        snafu::ensure!(
            included.len() < config.install.max_extracted_files,
            TooManyExtractableFilesSnafu {
                max_files: config.install.max_extracted_files
            }
        );

        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&fs_path)
            .map_err(|source| {
                if source.kind() == io::ErrorKind::AlreadyExists {
                    ExtractedFileAlreadyExistsSnafu { path: &fs_path }.into_error(source)
                } else {
                    CreateExtractedFileSnafu { path: &fs_path }.into_error(source)
                }
            })?;
        io::copy(&mut archive_file, &mut file)
            .context(CopyExtractedFileSnafu { path: &fs_path })?;
        file.flush()
            .context(FlushExtractedFileSnafu { path: &fs_path })?;

        extracted.push(ExtractedFile {
            file_name,
            rule: include_rule,
        });
        included.push(ExtractEntry { path_in_archive });
    }
    Ok((
        extracted,
        ExtractDetail {
            included,
            excluded,
            suspicious_skips,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, io::Seek as _};

    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::{cli::config::InstallConfig, util::glob::PathGlob};

    fn build_zip(entries: &[(&str, &[u8])]) -> File {
        let mut file = tempfile::tempfile().unwrap();
        let mut writer = ZipWriter::new(&mut file);
        for (name, contents) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(contents).unwrap();
        }
        writer.finish().unwrap();
        file.rewind().unwrap();
        file
    }

    fn default_files() -> Vec<FileRule> {
        vec![
            FileRule::glob(PathGlob::new("**/*.ttf").unwrap()),
            FileRule::glob(PathGlob::new("**/*.ttc").unwrap()),
            FileRule::glob(PathGlob::new("**/*.otf").unwrap()),
        ]
    }

    fn default_ignore() -> Vec<IgnoreRule> {
        vec![]
    }

    fn extract_to_tempdir(
        archive: File,
        files: &[FileRule],
        ignore: &[IgnoreRule],
        config: &FotonConfig,
    ) -> Result<(TempDir, Vec<FileName>), Box<ExtractErrorReport>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let (extracted, _extract_detail) =
            extract_archive_impl(archive, files, ignore, &fonts_dir, config)?;
        let files = extracted
            .into_iter()
            .map(|e| e.file_name)
            .collect::<Vec<_>>();
        Ok((tempdir, files))
    }

    fn extract_detail_to_tempdir(
        archive: File,
        files: &[FileRule],
        ignore: &[IgnoreRule],
        config: &FotonConfig,
    ) -> Result<(TempDir, ExtractDetail), Box<ExtractErrorReport>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let (_file_names, extract_detail) =
            extract_archive_impl(archive, files, ignore, &fonts_dir, config)?;
        Ok((tempdir, extract_detail))
    }

    #[test]
    fn extract_archive_does_not_match_plain_globs_across_directories() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let files = [FileRule::glob(PathGlob::new("*.ttf").unwrap())];
        let ignore = [];

        let (_tempdir, files) =
            extract_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn extract_archive_matches_single_directory_globs_against_nested_paths() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let files = [FileRule::glob(PathGlob::new("*/*.ttf").unwrap())];
        let ignore = [];

        let (_tempdir, files) =
            extract_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();
        assert_eq!(files, ["font.ttf"]);
    }

    #[test]
    fn extract_archive_matches_recursive_globs_against_nested_paths() {
        let archive = build_zip(&[("a/b/font.ttf", b"font")]);
        let files = [FileRule::glob(PathGlob::new("**/*.ttf").unwrap())];
        let ignore = [];

        let (_tempdir, files) =
            extract_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert_eq!(files, vec!["font.ttf"]);
    }

    #[test]
    fn extract_archive_skips_files_matching_exclude_patterns() {
        let archive = build_zip(&[("fonts/keep.ttf", b"keep"), ("fonts/skip.ttf", b"skip")]);
        let files = [FileRule::glob(PathGlob::new("fonts/*.ttf").unwrap())];
        let ignore = [IgnoreRule::path("fonts/skip.ttf").unwrap()];

        let (_tempdir, files) =
            extract_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert_eq!(files, vec!["keep.ttf"]);
    }

    #[test]
    fn extract_archive_exclude_takes_precedence_over_include() {
        let archive = build_zip(&[("fonts/font.ttf", b"font")]);
        let files = [FileRule::path("fonts/font.ttf").unwrap()];
        let ignore = [IgnoreRule::path("fonts/font.ttf").unwrap()];

        let (_tempdir, files) =
            extract_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let archive = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);

        let err = extract_to_tempdir(
            archive,
            &default_files(),
            &default_ignore(),
            &FotonConfig::default(),
        )
        .unwrap_err();
        assert_matches!(*err, ExtractErrorReport::ExtractedFileAlreadyExists { .. });
    }

    #[test]
    fn extract_archive_filters_non_font_files() {
        let archive = build_zip(&[
            ("font.ttf", b"font"),
            ("font.ttc", b"collection"),
            ("font.otf", b"otf"),
            ("README.txt", b"readme"),
            ("dir/", b""),
        ]);

        let (_tempdir, files) = extract_to_tempdir(
            archive,
            &default_files(),
            &default_ignore(),
            &FotonConfig::default(),
        )
        .unwrap();

        assert_eq!(files, vec!["font.ttf", "font.ttc", "font.otf"]);
    }

    #[test]
    fn extract_archive_rejects_more_than_max_extracted_files() {
        let archive = build_zip(&[("a.ttf", b"font-a"), ("b.ttf", b"font-b")]);
        let config = FotonConfig {
            install: InstallConfig {
                max_extracted_files: 1,
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };

        let err =
            extract_to_tempdir(archive, &default_files(), &default_ignore(), &config).unwrap_err();
        assert_matches!(*err, ExtractErrorReport::TooManyExtractableFiles { .. });
    }

    #[test]
    fn extract_archive_rejects_entries_exceeding_max_extracted_file_size() {
        let archive = build_zip(&[("font.ttf", b"font")]);
        let config = FotonConfig {
            install: InstallConfig {
                max_extracted_file_size_bytes: 3,
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };

        let err =
            extract_to_tempdir(archive, &default_files(), &default_ignore(), &config).unwrap_err();
        assert_matches!(*err, ExtractErrorReport::ExtractedFileExceedsMaxSize { .. });
    }

    #[test]
    fn extract_archive_records_suspicious_skips_for_font_like_paths_case_insensitively() {
        let archive = build_zip(&[("fonts/SKIP.TTF", b"font"), ("fonts/readme.txt", b"readme")]);
        let files = [FileRule::path("fonts/include.ttf").unwrap()];
        let ignore = [];

        let (_tempdir, extract_detail) =
            extract_detail_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert_eq!(
            extract_detail
                .suspicious_skips
                .iter()
                .map(|entry| entry.path_in_archive.as_path())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("fonts/SKIP.TTF").as_path()]
        );
    }

    #[test]
    fn extract_archive_does_not_record_suspicious_skips_for_excluded_font_like_paths() {
        let archive = build_zip(&[("fonts/skip.ttf", b"font")]);
        let files = [];
        let ignore = [IgnoreRule::path("fonts/skip.ttf").unwrap()];

        let (_tempdir, extract_detail) =
            extract_detail_to_tempdir(archive, &files, &ignore, &FotonConfig::default()).unwrap();

        assert!(extract_detail.suspicious_skips.is_empty());
        assert_eq!(
            extract_detail
                .excluded
                .iter()
                .map(|entry| entry.path_in_archive.as_path())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("fonts/skip.ttf").as_path()]
        );
    }
}
