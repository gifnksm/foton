use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read, Seek as _, Write as _},
    path::Path,
};

use color_eyre::eyre::{self, WrapErr as _, bail, ensure, eyre};
use sha2::{Digest as _, Sha256};
use zip::ZipArchive;

use crate::{
    package::{FontEntry, Package, PackageDirs, PackageSpec},
    platform::windows::services::{font::FontValidator, registration},
    util::{
        app_dirs::AppDirs,
        fs::{self as fs_util},
        hash::Sha256Digest,
        path::{AbsolutePath, FileName},
        reporter::{ReportErrorExt as _, ReportEyreErrorExt as _, Reporter},
    },
};

#[derive(Debug)]
#[expect(clippy::struct_field_names)]
pub(crate) struct InstallConfig {
    pub(crate) max_archive_size_bytes: u64,
    pub(crate) max_extracted_files: usize,
    pub(crate) max_extracted_file_size_bytes: u64,
}

pub(crate) fn install_package(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    spec: &PackageSpec,
    app_dirs: &AppDirs,
    config: &InstallConfig,
) -> eyre::Result<Package> {
    reporter.report_step(format_args!("Installing {}...", spec.id));

    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), &spec.id);

    reporter.report_step(format_args!("Staging package files..."));
    let package = match stage_package(reporter, spec, &pkg_dirs, config) {
        Ok(package) => package,
        Err(err) => {
            let _ = remove_package_dirs(reporter, &pkg_dirs).wrap_err_with(|| {
                format!("failed to remove package directory after install failure: {}; manual cleanup may be required", pkg_dirs.version_dir().display())
            }).report_err_as_warn(reporter);
            return Err(err);
        }
    };

    reporter.report_step(format_args!("Registering fonts..."));
    if let Err(err) = registration::install_package_fonts(reporter, app_id, &package) {
        let _ = registration::uninstall_package_fonts(reporter, app_id, &spec.id);
        let _ = remove_package_dirs(reporter, &pkg_dirs).wrap_err_with(|| {
                format!("failed to remove package directory after install failure: {}; manual cleanup may be required", pkg_dirs.version_dir().display())
            }).report_err_as_warn(reporter);
        return Err(err.into());
    }

    Ok(package)
}

pub(crate) fn uninstall_package(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    package: &Package,
) -> eyre::Result<()> {
    reporter.report_step(format_args!("Uninstalling {}...", package.id()));

    reporter.report_step(format_args!("Unregistering fonts..."));
    registration::uninstall_package_fonts(reporter, app_id, package.id())?;
    reporter.report_step(format_args!("Removing package files..."));
    remove_package_dirs(reporter, package.dirs())?;

    Ok(())
}

fn remove_package_dirs(reporter: &mut Reporter<'_>, pkg_dirs: &PackageDirs) -> eyre::Result<()> {
    fs_util::remove_dir_all_if_exists(pkg_dirs.fonts_dir())?;

    // remove the package version / package name directory if it's empty after uninstall, ignoring errors
    let ancestors = [pkg_dirs.version_dir(), pkg_dirs.name_dir()];
    for ancestor in ancestors {
        if let Some(res) = fs_util::remove_dir_if_empty(ancestor)
            .report_err_as_warn(reporter)
            .ok()
            && res.is_not_empty()
        {
            return Ok(());
        }
    }

    Ok(())
}

fn stage_package(
    reporter: &mut Reporter<'_>,
    spec: &PackageSpec,
    pkg_dirs: &PackageDirs,
    config: &InstallConfig,
) -> eyre::Result<Package> {
    fs_util::create_dir_all(pkg_dirs.name_dir())?;
    fs_util::create_dir(pkg_dirs.version_dir())?;
    fs_util::create_dir(pkg_dirs.fonts_dir())?;

    let file = download_archive(reporter, spec, config)?;
    let package_fonts_dir = pkg_dirs.fonts_dir();
    let file_paths = extract_archive(reporter, file, package_fonts_dir, config)?;

    let ValidationResult {
        unsupported_fonts,
        valid_entries,
    } = validate_fonts(package_fonts_dir, &file_paths)?;
    prune_invalid_fonts(reporter, package_fonts_dir, &unsupported_fonts);

    if valid_entries.is_empty() {
        bail!("no valid font files found in package");
    }

    reporter.report_info(format_args!(
        "{} valid font(s) found in package",
        valid_entries.len()
    ));

    let package = Package::new(spec.id.clone(), pkg_dirs.clone(), valid_entries);
    Ok(package)
}

fn download_archive(
    reporter: &mut Reporter<'_>,
    spec: &PackageSpec,
    config: &InstallConfig,
) -> eyre::Result<File> {
    reporter.report_step(format_args!("Downloading {} archive...", spec.id));

    let mut response = reqwest::blocking::get(spec.url.clone())
        .wrap_err_with(|| format!("failed to download font archive from {}", spec.url))?
        .error_for_status()
        .wrap_err_with(|| format!("failed to download font archive from {}", spec.url))?;

    let len = response.content_length();
    if let Some(len) = len
        && len > config.max_archive_size_bytes
    {
        bail!(
            "server-reported archive size {len} exceeds maximum allowed size of {}",
            config.max_archive_size_bytes
        );
    }
    let (mut output, digest) = reporter.with_download_progress_bar(len, |pb| {
        stream_archive_to_tempfile(&mut response, config, pb)
    })?;
    ensure!(
        digest == spec.sha256,
        "downloaded archive hash mismatch for {}: expected {}, got {}",
        spec.id,
        spec.sha256,
        digest
    );
    output
        .rewind()
        .wrap_err("failed to rewind temporary file for downloaded archive")?;
    Ok(output)
}

fn stream_archive_to_tempfile<R>(
    reader: &mut R,
    config: &InstallConfig,
    pb: &indicatif::ProgressBar,
) -> eyre::Result<(File, Sha256Digest)>
where
    R: Read,
{
    let mut output =
        tempfile::tempfile().wrap_err("failed to create temporary file for downloaded archive")?;
    let mut buffer = [0; 8096];
    let mut hasher = Sha256::new();
    let mut total_size = 0;
    loop {
        let n = reader
            .read(&mut buffer)
            .wrap_err("failed to read response body while downloading archive")?;
        total_size += n as u64;
        if total_size > config.max_archive_size_bytes {
            bail!(
                "downloaded archive size exceeds maximum allowed size of {}",
                config.max_archive_size_bytes
            );
        }
        if n == 0 {
            break;
        }
        let chunk = &buffer[..n];
        hasher.update(chunk);
        output
            .write_all(chunk)
            .wrap_err("failed to write chunk to temporary file for downloaded archive")?;
        pb.inc(chunk.len() as u64);
    }
    let digest = Sha256Digest::new(hasher.finalize());
    Ok((output, digest))
}

fn extract_archive(
    reporter: &mut Reporter<'_>,
    file: File,
    fonts_dir: &AbsolutePath,
    config: &InstallConfig,
) -> eyre::Result<Vec<FileName>> {
    reporter.report_step(format_args!(
        "Extracting archive to {}...",
        fonts_dir.display()
    ));

    let mut files = vec![];
    let mut archive = ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut archive_file = archive
            .by_index(i)
            .wrap_err_with(|| format!("failed to extract file with index {i}"))?;
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
            bail!(
                "archive entry `{}` has extracted size {} exceeding the maximum allowed size of {}",
                archive_file.name(),
                archive_file.size(),
                config.max_extracted_file_size_bytes,
            );
        }

        let file_name = archive_path
            .file_name()
            .ok_or_else(|| eyre!("failed to get file name for archive entry with index {i}"))?
            .to_owned();
        let file_name = FileName::new(&file_name).ok_or_else(|| {
            eyre!(
                "invalid file name `{}` in archive entry with index {i}",
                file_name.to_string_lossy(),
            )
        })?;
        let fs_path = fonts_dir.join(&file_name);

        if files.len() >= config.max_extracted_files {
            bail!(
                "archive contains more than {} extractable font files",
                config.max_extracted_files
            );
        }

        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&fs_path)
            .map_err(|err| {
                if err.kind() == io::ErrorKind::AlreadyExists {
                    eyre!("extracted font file already exists: {}", fs_path.display())
                } else {
                    eyre!(err)
                }
            })
            .wrap_err_with(|| format!("failed to create font file: {}", fs_path.display()))?;
        io::copy(&mut archive_file, &mut file)
            .wrap_err_with(|| format!("failed to write font file: {}", fs_path.display()))?;
        file.flush()
            .wrap_err_with(|| format!("failed to flush font file: {}", fs_path.display()))?;

        files.push(file_name);
    }
    Ok(files)
}

struct ValidationResult {
    unsupported_fonts: Vec<FileName>,
    valid_entries: Vec<FontEntry>,
}

fn validate_fonts(
    fonts_dir: &AbsolutePath,
    file_names: &[FileName],
) -> eyre::Result<ValidationResult> {
    let mut unsupported_fonts = vec![];
    let mut valid_entries = vec![];
    let mut valid_entry_titles = HashSet::new();
    let validator = FontValidator::new()?;
    for file_name in file_names {
        let Some(entry) = validator.validate_font(fonts_dir, file_name)? else {
            unsupported_fonts.push(file_name.clone());
            continue;
        };
        if !valid_entry_titles.insert(entry.title().to_lowercase()) {
            bail!("duplicate font name found in package: {}", entry.title());
        }
        valid_entries.push(entry);
    }
    Ok(ValidationResult {
        unsupported_fonts,
        valid_entries,
    })
}

fn prune_invalid_fonts(
    reporter: &mut Reporter<'_>,
    fonts_dir: &AbsolutePath,
    invalid_files: &[FileName],
) {
    for file_name in invalid_files {
        let path = fonts_dir.join(file_name);
        reporter.report_warn(eyre!("removing invalid font file: {}", path.display()).as_ref());
        let _ = fs_util::remove_file(&path).report_err_as_warn(reporter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{fs, io::Cursor};

    use indicatif::ProgressBar;
    use semver::Version;
    use tempfile::TempDir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use crate::package::{PackageId, PackageName};

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
    ) -> eyre::Result<(TempDir, Vec<FileName>)> {
        let mut reporter = Reporter::message_reporter();
        let tempdir = tempfile::tempdir().unwrap();
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let files = extract_archive(&mut reporter, archive, &fonts_dir, config)?;
        Ok((tempdir, files))
    }

    fn extract_to_tempdir(archive: File) -> eyre::Result<(TempDir, Vec<FileName>)> {
        let config = InstallConfig {
            max_archive_size_bytes: 100 * 1024 * 1024, // 100 MiB
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        };
        extract_to_tempdir_with_config(archive, &config)
    }

    fn make_package_dirs() -> (TempDir, PackageDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let app_data_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let name = PackageName::new("hackgen").unwrap();
        let version = Version::new(2, 10, 0);
        let pkg_id = PackageId::new(name, version);
        let pkg_dirs = PackageDirs::new(app_data_dir, &pkg_id);
        (tempdir, pkg_dirs)
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let archive = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);

        let err = extract_to_tempdir(archive).unwrap_err();

        assert!(format!("{err:?}").contains("extracted font file already exists"));
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

        assert!(format!("{err:?}").contains("archive contains more than 1 extractable font files"));
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

        assert!(format!("{err:?}").contains(
            "archive entry `font.ttf` has extracted size 4 exceeding the maximum allowed size of 3"
        ));
    }

    #[test]
    fn stream_archive_to_tempfile_rejects_download_size_exceeding_limit() {
        let mut reader = Cursor::new(b"font".to_vec());
        let config = InstallConfig {
            max_archive_size_bytes: 3,
            max_extracted_files: 1000,
            max_extracted_file_size_bytes: 50 * 1024 * 1024, // 50 MiB
        };
        let pb = ProgressBar::hidden();

        let err = stream_archive_to_tempfile(&mut reader, &config, &pb).unwrap_err();

        assert!(
            format!("{err:?}")
                .contains("downloaded archive size exceeds maximum allowed size of 3")
        );
    }

    #[test]
    fn remove_package_dirs_removes_empty_package_directories() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();

        remove_package_dirs(&mut reporter, &pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
    }

    #[test]
    fn remove_package_dirs_stops_when_parent_directory_is_not_empty() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        let sibling = pkg_dirs.name_dir().join("other-version");
        fs::create_dir(&sibling).unwrap();

        remove_package_dirs(&mut reporter, &pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.name_dir().exists());
        assert!(sibling.exists());
    }

    #[test]
    fn remove_package_dirs_ignores_missing_directories() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();

        remove_package_dirs(&mut reporter, &pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
    }
}
