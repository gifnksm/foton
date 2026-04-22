use std::{
    ffi::OsString,
    fs::File,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use zip::{ZipArchive, result::ZipError};

use crate::{
    command::install::InstallConfig,
    util::path::{AbsolutePath, FileName},
};

#[derive(Debug, derive_more::Display, derive_more::Error)]
pub(crate) enum ExtractError {
    #[display("failed to read archive")]
    ReadArchive {
        #[error(source)]
        source: ZipError,
    },
    #[display("failed to extract file with index {index}")]
    ExtractFile {
        index: usize,
        #[error(source)]
        source: ZipError,
    },
    #[display(
        "archive entry `{file_path}` has extracted size {file_size} exceeding the maximum allowed size of {max_size}",
        file_path = file_path.display(),
    )]
    ExtractedFileExceedsMaxSize {
        file_path: PathBuf,
        file_size: u64,
        max_size: u64,
    },
    #[display("failed to get file name for archive entry with index {index}")]
    GetFileName { index: usize },
    #[display(
        "invalid file name `{file_name}` in archive entry with index {index}",
        file_name = file_name.display()
    )]
    InvalidFileName { file_name: OsString, index: usize },
    #[display("archive contains more than {max_files} extractable font files")]
    TooManyExtractableFiles { max_files: usize },
    #[display("extracted font file already exists: {path}", path = path.display())]
    ExtractedFileAlreadyExists {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to create font file: {path}", path = path.display())]
    CreateExtractedFile {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to copy extracted font file to destination: {path}", path = path.display())]
    CopyExtractedFile {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
    #[display("failed to flush font file: {path}", path = path.display())]
    FlushExtractedFile {
        path: AbsolutePath,
        #[error(source)]
        source: io::Error,
    },
}

pub(in crate::command::install) fn extract_archive(
    file: File,
    fonts_dir: &AbsolutePath,
    config: &InstallConfig,
) -> Result<Vec<FileName>, Box<ExtractError>> {
    let mut files = vec![];
    let mut archive =
        ZipArchive::new(file).map_err(|source| ExtractError::ReadArchive { source })?;

    for i in 0..archive.len() {
        let mut archive_file = archive.by_index(i).map_err(|source| {
            let index = i;
            ExtractError::ExtractFile { index, source }
        })?;

        if !archive_file.is_file() {
            continue;
        }
        let archive_path = Path::new(archive_file.name());
        let ext = archive_path.extension();
        let is_font = ext.is_some_and(|e| {
            e.eq_ignore_ascii_case("ttf")
                || e.eq_ignore_ascii_case("ttc")
                || e.eq_ignore_ascii_case("otf")
        });
        if !is_font {
            continue;
        }
        if archive_file.size() > config.max_extracted_file_size_bytes {
            return Err(ExtractError::ExtractedFileExceedsMaxSize {
                file_path: archive_path.to_owned(),
                file_size: archive_file.size(),
                max_size: config.max_extracted_file_size_bytes,
            }
            .into());
        }

        let file_name = archive_path
            .file_name()
            .ok_or(ExtractError::GetFileName { index: i })?
            .to_owned();
        let file_name = FileName::new(&file_name).ok_or_else(|| {
            let index = i;
            ExtractError::InvalidFileName { file_name, index }
        })?;
        let fs_path = fonts_dir.join(&file_name);

        if files.len() >= config.max_extracted_files {
            let max_files = config.max_extracted_files;
            return Err(ExtractError::TooManyExtractableFiles { max_files }.into());
        }

        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&fs_path)
            .map_err(|source| {
                let path = fs_path.clone();
                if source.kind() == io::ErrorKind::AlreadyExists {
                    ExtractError::ExtractedFileAlreadyExists { path, source }
                } else {
                    ExtractError::CreateExtractedFile { path, source }
                }
            })?;
        io::copy(&mut archive_file, &mut file).map_err(|source| {
            let path = fs_path.clone();
            ExtractError::CopyExtractedFile { path, source }
        })?;
        file.flush().map_err(|source| {
            let path = fs_path.clone();
            ExtractError::FlushExtractedFile { path, source }
        })?;

        files.push(file_name);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use std::io::Seek as _;

    use super::*;

    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn build_zip(entries: &[(&str, &[u8])]) -> File {
        let mut file = tempfile::tempfile().unwrap();
        {
            let mut writer = ZipWriter::new(&mut file);
            for (name, contents) in entries {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(contents).unwrap();
            }
            writer.finish().unwrap();
        }
        file.rewind().unwrap();
        file
    }

    fn extract_to_tempdir_with_config(
        archive: File,
        config: &InstallConfig,
    ) -> Result<(TempDir, Vec<FileName>), Box<ExtractError>> {
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let files = extract_archive(archive, &fonts_dir, config)?;
        Ok((tempdir, files))
    }

    fn extract_to_tempdir(archive: File) -> Result<(TempDir, Vec<FileName>), Box<ExtractError>> {
        let config = InstallConfig {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        };
        extract_to_tempdir_with_config(archive, &config)
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let archive = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);

        let err = extract_to_tempdir(archive).unwrap_err();
        assert!(matches!(
            *err,
            ExtractError::ExtractedFileAlreadyExists { .. }
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

        let (_tempdir, files) = extract_to_tempdir(archive).unwrap();

        assert_eq!(files, vec!["font.ttf", "font.ttc", "font.otf"]);
    }

    #[test]
    fn extract_archive_rejects_more_than_max_extracted_files() {
        let archive = build_zip(&[("a.ttf", b"font-a"), ("b.ttf", b"font-b")]);
        let config = InstallConfig {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        };

        let err = extract_to_tempdir_with_config(archive, &config).unwrap_err();
        assert!(matches!(*err, ExtractError::TooManyExtractableFiles { .. }));
    }

    #[test]
    fn extract_archive_rejects_entries_exceeding_max_extracted_file_size() {
        let archive = build_zip(&[("font.ttf", b"font")]);
        let config = InstallConfig {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 3,
        };

        let err = extract_to_tempdir_with_config(archive, &config).unwrap_err();
        assert!(matches!(
            *err,
            ExtractError::ExtractedFileExceedsMaxSize { .. }
        ));
    }
}
