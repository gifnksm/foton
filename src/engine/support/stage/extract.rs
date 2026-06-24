use std::{
    ffi::OsString,
    fs::File,
    io::{self, Write as _},
    marker::PhantomData,
    path::PathBuf,
};

use snafu::{IntoError as _, OptionExt as _, ResultExt as _, Snafu};
use url::Url;
use zip::{ZipArchive, result::ZipError};

use crate::{
    cli::{
        config::FotonConfig,
        context::ReportContext,
        reporter::{NeverReport, ReportScope, ScopeResultErrorExt as _, SubReportScope},
    },
    engine::{ArchiveExtractDetail, ExtractDetail, ExtractEntry, ExtractedFile},
    package::{ArchiveOptions, FontFileOptions, PackageSource, PackageSourceContents},
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
    #[snafu(display("failed to get file name from URL: {url}"))]
    GetFileNameFromUrl { url: Url },
    #[snafu(display("invalid file name `{file_name}` in URL: {url}", file_name = file_name.to_string_lossy()))]
    InvalidFileNameInUrl { file_name: OsString, url: Url },
    #[snafu(display(
        "source file has size {file_size} exceeding the maximum allowed size of {max_size}"
    ))]
    SourceFileExceedsMaxSize { file_size: u64, max_size: u64 },
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
    InvalidFileNameInArchive { file_name: OsString, index: usize },
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

#[derive(Debug)]
pub(super) struct ExtractResult<'s> {
    pub(super) files: Vec<ExtractedFile>,
    pub(super) detail: ExtractDetail<'s>,
}

pub(super) fn extract_contents<'s, S>(
    cx: &ReportContext<S>,
    file: File,
    file_size: u64,
    source: &'s PackageSource,
    fonts_dir: &AbsolutePath,
) -> Result<ExtractResult<'s>, S::Error>
where
    S: ReportScope,
{
    let cx = ExtractScope::start_with_report(
        cx,
        format_args!("Extracting font files to {}...", fonts_dir.display()),
    );

    let config = cx.config();
    match &source.contents {
        PackageSourceContents::FontFile(options) => {
            extract_font_file(file, file_size, &source.url, options, fonts_dir, config)
        }
        PackageSourceContents::Archive(options) => {
            extract_archive(file, options, fonts_dir, config)
        }
    }
    .report_error(&cx)
}

fn extract_font_file<'s>(
    mut file: File,
    file_size: u64,
    url: &Url,
    options: &'s FontFileOptions,
    fonts_dir: &AbsolutePath,
    config: &FotonConfig,
) -> Result<ExtractResult<'s>, ExtractErrorReport> {
    let FontFileOptions { file_name, title } = options;
    let file_name = match file_name {
        Some(name) => name.clone(),
        None => url_to_filename(url)?,
    };

    snafu::ensure!(
        file_size <= config.install.max_font_file_size_bytes.get(),
        SourceFileExceedsMaxSizeSnafu {
            file_size,
            max_size: config.install.max_font_file_size_bytes.get(),
        }
    );

    create_font_file(fonts_dir, &file_name, &mut file)?;
    let files = vec![ExtractedFile {
        file_name,
        title: title.clone(),
    }];
    Ok(ExtractResult {
        files,
        detail: ExtractDetail::FontFile {},
    })
}

fn extract_archive<'s>(
    file: File,
    options: &'s ArchiveOptions,
    fonts_dir: &AbsolutePath,
    config: &FotonConfig,
) -> Result<ExtractResult<'s>, ExtractErrorReport> {
    let ArchiveOptions { fonts, ignore } = options;
    let mut files = vec![];
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
        let Some(include_rule) = fonts.iter().find(|rule| rule.matches(&path_in_archive)) else {
            if let Some(ext) = path_in_archive.extension()
                && (ext.eq_ignore_ascii_case("ttf")
                    || ext.eq_ignore_ascii_case("otf")
                    || ext.eq_ignore_ascii_case("ttc")
                    || ext.eq_ignore_ascii_case("otc"))
            {
                suspicious_skips.push(ExtractEntry { path_in_archive });
            }
            continue;
        };
        snafu::ensure!(
            archive_file.size() <= config.install.max_font_file_size_bytes.get(),
            ExtractedFileExceedsMaxSizeSnafu {
                file_path: path_in_archive,
                file_size: archive_file.size(),
                max_size: config.install.max_font_file_size_bytes,
            }
        );

        let file_name = if let Some(file_name) = include_rule.file_name() {
            file_name.clone()
        } else {
            let file_name = path_in_archive
                .file_name()
                .context(GetFileNameSnafu { index: i })?
                .to_owned();
            FileName::new(&file_name).context(InvalidFileNameInArchiveSnafu {
                file_name,
                index: i,
            })?
        };

        snafu::ensure!(
            included.len() < config.install.max_fonts_per_package_source.get(),
            TooManyExtractableFilesSnafu {
                max_files: config.install.max_fonts_per_package_source
            }
        );

        create_font_file(fonts_dir, &file_name, &mut archive_file)?;

        files.push(ExtractedFile {
            file_name,
            title: include_rule.title().map(ToOwned::to_owned),
        });
        included.push(ExtractEntry { path_in_archive });
    }
    Ok(ExtractResult {
        files,
        detail: ExtractDetail::Archive(ArchiveExtractDetail {
            options,
            included,
            excluded,
            suspicious_skips,
        }),
    })
}

fn url_to_filename(url: &Url) -> Result<FileName, ExtractErrorReport> {
    let file_name = url
        .path_segments()
        .and_then(Iterator::last)
        .with_context(|| GetFileNameFromUrlSnafu { url: url.clone() })?;
    FileName::new(file_name).with_context(|| InvalidFileNameInUrlSnafu {
        file_name,
        url: url.clone(),
    })
}

fn create_font_file<R>(
    fonts_dir: &AbsolutePath,
    file_name: &FileName,
    reader: &mut R,
) -> Result<(), ExtractErrorReport>
where
    R: io::Read,
{
    let path = &fonts_dir.join(file_name);
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| {
            if source.kind() == io::ErrorKind::AlreadyExists {
                ExtractedFileAlreadyExistsSnafu { path }.into_error(source)
            } else {
                CreateExtractedFileSnafu { path }.into_error(source)
            }
        })?;
    io::copy(reader, &mut file).context(CopyExtractedFileSnafu { path })?;
    file.flush().context(FlushExtractedFileSnafu { path })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        assert_matches,
        io::{Seek as _, Write as _},
        num::NonZero,
    };

    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;
    use crate::{
        cli::config::InstallConfig,
        package::{FontFileOptions, FontRule, IgnoreRule},
        util::glob::PathGlob,
    };

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

    fn build_file(contents: &[u8]) -> File {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(contents).unwrap();
        file.rewind().unwrap();
        file
    }

    fn default_archive_options() -> ArchiveOptions {
        ArchiveOptions {
            fonts: default_fonts(),
            ignore: default_ignore(),
        }
    }

    fn default_fonts() -> Vec<FontRule> {
        vec![
            FontRule::glob(PathGlob::new("**/*.ttf").unwrap()),
            FontRule::glob(PathGlob::new("**/*.otf").unwrap()),
            FontRule::glob(PathGlob::new("**/*.ttc").unwrap()),
            FontRule::glob(PathGlob::new("**/*.otc").unwrap()),
        ]
    }

    fn default_ignore() -> Vec<IgnoreRule> {
        vec![]
    }

    fn extract_archive_to_tempdir<'s>(
        archive: File,
        options: &'s ArchiveOptions,
        config: &FotonConfig,
    ) -> Result<(TempDir, ExtractResult<'s>), Box<ExtractErrorReport>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let res = extract_archive(archive, options, &fonts_dir, config)?;
        Ok((tempdir, res))
    }

    fn extract_font_file_to_tempdir<'s>(
        file: File,
        file_size: u64,
        url: &Url,
        options: &'s FontFileOptions,
        config: &FotonConfig,
    ) -> Result<(TempDir, ExtractResult<'s>), Box<ExtractErrorReport>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let res = extract_font_file(file, file_size, url, options, &fonts_dir, config)?;
        Ok((tempdir, res))
    }

    impl ExtractResult<'_> {
        fn file_names(&self) -> Vec<FileName> {
            self.files.iter().map(|e| e.file_name.clone()).collect()
        }
    }

    #[test]
    fn extract_font_file_uses_url_file_name_when_not_overridden() {
        let file = build_file(b"font");
        let url = Url::parse("https://example.com/fonts/Example-Regular.ttf").unwrap();
        let options = FontFileOptions::default();

        let (tempdir, res) =
            extract_font_file_to_tempdir(file, 4, &url, &options, &FotonConfig::default()).unwrap();

        assert_eq!(res.file_names(), ["Example-Regular.ttf"]);
        assert!(matches!(res.detail, ExtractDetail::FontFile {}));
        assert_eq!(res.files[0].title(), None);
        assert!(tempdir.path().join("Example-Regular.ttf").exists());
    }

    #[test]
    fn extract_font_file_uses_overridden_file_name_and_title() {
        let file = build_file(b"font");
        let url = Url::parse("https://example.com/download?id=123").unwrap();
        let options = FontFileOptions {
            file_name: Some(FileName::new("Renamed.ttf").unwrap()),
            title: Some("Example Regular".to_string()),
        };

        let (tempdir, res) =
            extract_font_file_to_tempdir(file, 4, &url, &options, &FotonConfig::default()).unwrap();

        assert_eq!(res.file_names(), ["Renamed.ttf"]);
        assert_eq!(res.files[0].title(), Some("Example Regular"));
        assert!(tempdir.path().join("Renamed.ttf").exists());
    }

    #[test]
    fn extract_font_file_rejects_source_files_exceeding_max_font_file_size_bytes() {
        let file = build_file(b"font");
        let url = Url::parse("https://example.com/fonts/Example-Regular.ttf").unwrap();
        let options = FontFileOptions::default();
        let config = FotonConfig {
            install: InstallConfig {
                max_font_file_size_bytes: NonZero::new(3).unwrap(),
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };

        let err = extract_font_file_to_tempdir(file, 4, &url, &options, &config).unwrap_err();

        assert_matches!(*err, ExtractErrorReport::SourceFileExceedsMaxSize { .. });
    }

    #[test]
    fn extract_archive_does_not_match_plain_globs_across_directories() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::glob(PathGlob::new("*.ttf").unwrap())],
            ignore: vec![],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        assert!(res.files.is_empty());
    }

    #[test]
    fn extract_archive_matches_single_directory_globs_against_nested_paths() {
        let archive = build_zip(&[("a/font.ttf", b"font")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::glob(PathGlob::new("*/*.ttf").unwrap())],
            ignore: vec![],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();
        assert_eq!(res.file_names(), ["font.ttf"]);
    }

    #[test]
    fn extract_archive_matches_recursive_globs_against_nested_paths() {
        let archive = build_zip(&[("a/b/font.ttf", b"font")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::glob(PathGlob::new("**/*.ttf").unwrap())],
            ignore: vec![],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        assert_eq!(res.file_names(), vec!["font.ttf"]);
    }

    #[test]
    fn extract_archive_skips_files_matching_exclude_patterns() {
        let archive = build_zip(&[("fonts/keep.ttf", b"keep"), ("fonts/skip.ttf", b"skip")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::glob(PathGlob::new("fonts/*.ttf").unwrap())],
            ignore: vec![IgnoreRule::path("fonts/skip.ttf").unwrap()],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        assert_eq!(res.file_names(), vec!["keep.ttf"]);
    }

    #[test]
    fn extract_archive_exclude_takes_precedence_over_include() {
        let archive = build_zip(&[("fonts/font.ttf", b"font")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::path("fonts/font.ttf").unwrap()],
            ignore: vec![IgnoreRule::path("fonts/font.ttf").unwrap()],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        assert!(res.files.is_empty());
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let archive = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);
        let options = default_archive_options();

        let err =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap_err();
        assert_matches!(*err, ExtractErrorReport::ExtractedFileAlreadyExists { .. });
    }

    #[test]
    fn extract_archive_filters_non_font_files() {
        let archive = build_zip(&[
            ("font.ttf", b"font"),
            ("font.otf", b"otf"),
            ("font.ttc", b"collection"),
            ("font.otc", b"otc-collection"),
            ("README.txt", b"readme"),
            ("dir/", b""),
        ]);
        let options = default_archive_options();

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        assert_eq!(
            res.file_names(),
            vec!["font.ttf", "font.otf", "font.ttc", "font.otc"]
        );
    }

    #[test]
    fn extract_archive_rejects_more_than_max_fonts_per_package_source() {
        let archive = build_zip(&[("a.ttf", b"font-a"), ("b.ttf", b"font-b")]);
        let options = default_archive_options();
        let config = FotonConfig {
            install: InstallConfig {
                max_fonts_per_package_source: NonZero::new(1).unwrap(),
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };

        let err = extract_archive_to_tempdir(archive, &options, &config).unwrap_err();
        assert_matches!(*err, ExtractErrorReport::TooManyExtractableFiles { .. });
    }

    #[test]
    fn extract_archive_rejects_entries_exceeding_max_font_file_size_bytes() {
        let archive = build_zip(&[("font.ttf", b"font")]);
        let options = default_archive_options();
        let config = FotonConfig {
            install: InstallConfig {
                max_font_file_size_bytes: NonZero::new(3).unwrap(),
                ..InstallConfig::default()
            },
            ..FotonConfig::default()
        };

        let err = extract_archive_to_tempdir(archive, &options, &config).unwrap_err();
        assert_matches!(*err, ExtractErrorReport::ExtractedFileExceedsMaxSize { .. });
    }

    #[test]
    fn extract_archive_records_suspicious_skips_for_font_like_paths_case_insensitively() {
        let archive = build_zip(&[("fonts/SKIP.TTF", b"font"), ("fonts/readme.txt", b"readme")]);
        let options = ArchiveOptions {
            fonts: vec![FontRule::path("fonts/include.ttf").unwrap()],
            ignore: vec![],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        let detail = res.detail.as_archive().unwrap();
        assert_eq!(
            detail
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
        let options = ArchiveOptions {
            fonts: vec![],
            ignore: vec![IgnoreRule::path("fonts/skip.ttf").unwrap()],
        };

        let (_tempdir, res) =
            extract_archive_to_tempdir(archive, &options, &FotonConfig::default()).unwrap();

        let detail = res.detail.as_archive().unwrap();
        assert!(detail.suspicious_skips.is_empty());
        assert_eq!(
            detail
                .excluded
                .iter()
                .map(|entry| entry.path_in_archive.as_path())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("fonts/skip.ttf").as_path()]
        );
    }
}
