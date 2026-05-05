use std::{
    ffi::OsString,
    fs::File,
    io::{self, Write as _},
    marker::PhantomData,
    path::PathBuf,
};

use glob::MatchOptions;
use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use zip::{ZipArchive, result::ZipError};

use crate::{
    cli::{
        config::FotonConfig,
        context::ReportContext,
        reporter::{
            NeverReport, ReportScope, ReportValue, ScopeResultErrorExt as _, SubReportScope,
        },
    },
    util::path::{AbsolutePath, FileName},
};

#[derive(Debug)]
struct ExtractScope<S> {
    _base_scope: PhantomData<S>,
}

impl<S> ReportScope for ExtractScope<S>
where
    S: ReportScope,
{
    type WarnReportValue = NeverReport;
    type ErrorReportValue = ExtractErrorReport;
    type Error = S::Error;
}

impl<S> SubReportScope<S> for ExtractScope<S>
where
    S: ReportScope,
{
    fn new() -> Self {
        Self {
            _base_scope: PhantomData,
        }
    }
}

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

impl From<ExtractErrorReport> for ReportValue<'static> {
    fn from(report: ExtractErrorReport) -> Self {
        Self::BoxedError(report.into())
    }
}

pub(super) fn extract_archive<S>(
    cx: &ReportContext<S>,
    file: File,
    include: &[glob::Pattern],
    fonts_dir: &AbsolutePath,
) -> Result<Vec<FileName>, S::Error>
where
    S: ReportScope,
{
    let cx = ExtractScope::start_with_report(
        cx,
        format_args!("Extracting archive to {}...", fonts_dir.display()),
    );
    extract_archive_impl(file, include, fonts_dir, cx.config()).report_error(cx.reporter())
}

fn extract_archive_impl(
    file: File,
    include: &[glob::Pattern],
    fonts_dir: &AbsolutePath,
    config: &FotonConfig,
) -> Result<Vec<FileName>, ExtractErrorReport> {
    const MATCH_OPTIONS: MatchOptions = MatchOptions {
        case_sensitive: false,
        require_literal_separator: true,
        require_literal_leading_dot: true,
    };

    let mut files = vec![];
    let mut archive = ZipArchive::new(file).context(ReadArchiveSnafu)?;

    for i in 0..archive.len() {
        let mut archive_file = archive.by_index(i).context(ExtractFileSnafu { index: i })?;

        if !archive_file.is_file() {
            continue;
        }
        let Some(archive_path) = archive_file.enclosed_name() else {
            continue;
        };
        let matches = include
            .iter()
            .any(|pattern| pattern.matches_path_with(&archive_path, MATCH_OPTIONS));
        if !matches {
            continue;
        }
        snafu::ensure!(
            archive_file.size() <= config.install.max_extracted_file_size_bytes,
            ExtractedFileExceedsMaxSizeSnafu {
                file_path: archive_path,
                file_size: archive_file.size(),
                max_size: config.install.max_extracted_file_size_bytes,
            }
        );

        let file_name = archive_path
            .file_name()
            .context(GetFileNameSnafu { index: i })?
            .to_owned();
        let file_name = FileName::new(&file_name).context(InvalidFileNameSnafu {
            file_name,
            index: i,
        })?;
        let fs_path = fonts_dir.join(&file_name);

        snafu::ensure!(
            files.len() < config.install.max_extracted_files,
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

        files.push(file_name);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::io::Seek as _;

    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::cli::config::InstallConfig;

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

    fn default_include() -> Vec<glob::Pattern> {
        vec![
            glob::Pattern::new("**/*.ttf").unwrap(),
            glob::Pattern::new("**/*.ttc").unwrap(),
            glob::Pattern::new("**/*.otf").unwrap(),
        ]
    }

    fn extract_to_tempdir(
        archive: File,
        include: &[glob::Pattern],
        config: &FotonConfig,
    ) -> Result<(TempDir, Vec<FileName>), Box<ExtractErrorReport>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let files = extract_archive_impl(archive, include, &fonts_dir, config)?;
        Ok((tempdir, files))
    }

    #[test]
    fn extract_archive_does_not_match_plain_globs_across_directories() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let include = [glob::Pattern::new("*.ttf").unwrap()];

        let (_tempdir, files) = {
            let include: &[glob::Pattern] = &include;
            extract_to_tempdir(archive, include, &FotonConfig::default())
        }
        .unwrap();

        assert!(files.is_empty());
    }

    #[test]
    fn extract_archive_matches_single_directory_globs_against_nested_paths() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let include = [glob::Pattern::new("*/*.ttf").unwrap()];

        let (_tempdir, files) = {
            let include: &[glob::Pattern] = &include;
            extract_to_tempdir(archive, include, &FotonConfig::default())
        }
        .unwrap();

        assert_eq!(files, vec!["font.ttf"]);
    }

    #[test]
    fn extract_archive_matches_recursive_globs_against_nested_paths() {
        let archive = build_zip(&[("a/b/font.ttf", b"font")]);
        let include = [glob::Pattern::new("**/*.ttf").unwrap()];

        let (_tempdir, files) = {
            let include: &[glob::Pattern] = &include;
            extract_to_tempdir(archive, include, &FotonConfig::default())
        }
        .unwrap();

        assert_eq!(files, vec!["font.ttf"]);
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let archive = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);

        let err =
            extract_to_tempdir(archive, &default_include(), &FotonConfig::default()).unwrap_err();
        assert!(matches!(
            *err,
            ExtractErrorReport::ExtractedFileAlreadyExists { .. }
        ));
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

        let (_tempdir, files) =
            extract_to_tempdir(archive, &default_include(), &FotonConfig::default()).unwrap();

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

        let err = extract_to_tempdir(archive, &default_include(), &config).unwrap_err();
        assert!(matches!(
            *err,
            ExtractErrorReport::TooManyExtractableFiles { .. }
        ));
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

        let err = extract_to_tempdir(archive, &default_include(), &config).unwrap_err();
        assert!(matches!(
            *err,
            ExtractErrorReport::ExtractedFileExceedsMaxSize { .. }
        ));
    }
}
