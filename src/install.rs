use std::{
    collections::HashSet,
    fs::File,
    io::{self, Cursor, Write as _},
    path::Path,
};

use bytes::Bytes;
use color_eyre::eyre::{self, WrapErr as _, bail, ensure, eyre};
use zip::ZipArchive;

use crate::{
    package::{FontEntry, Package, PackageDirs, PackageSpec},
    platform::windows::{
        font_validator::FontValidator,
        install::{self as platform_install},
    },
    util::{
        app_dirs::AppDirs,
        fs::{self as fs_util},
        hash,
        path::{AbsolutePath, FileName},
        reporter::{ReportErrorExt as _, ReportEyreErrorExt as _, Reporter},
    },
};

pub(crate) fn install_package(
    reporter: &mut Reporter<'_>,
    app_id: &str,
    spec: &PackageSpec,
    app_dirs: &AppDirs,
) -> eyre::Result<Package> {
    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), &spec.id);

    let package = match stage_package(reporter, spec, &pkg_dirs) {
        Ok(package) => package,
        Err(err) => {
            let _ = remove_package_dirs(reporter, &pkg_dirs).wrap_err_with(|| {
                format!("failed to remove package directory after install failure: {}; manual cleanup may be required", pkg_dirs.version_dir().display())
            }).report_err_as_warn(reporter);
            return Err(err);
        }
    };

    if let Err(err) = platform_install::install_package_fonts(reporter, app_id, &package) {
        let _ = platform_install::uninstall_package_fonts(reporter, app_id, &spec.id)
            .report_err_as_error(reporter);
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
    platform_install::uninstall_package_fonts(reporter, app_id, package.id())?;
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
) -> eyre::Result<Package> {
    fs_util::create_dir_all(pkg_dirs.name_dir())?;
    fs_util::create_dir(pkg_dirs.version_dir())?;
    fs_util::create_dir(pkg_dirs.fonts_dir())?;

    let bytes = download_archive(spec)?;
    let package_fonts_dir = pkg_dirs.fonts_dir();
    let file_paths = extract_archive(bytes, package_fonts_dir)?;

    let ValidationResult {
        unsupported_fonts,
        valid_entries,
    } = validate_fonts(package_fonts_dir, &file_paths)?;
    prune_invalid_fonts(reporter, package_fonts_dir, &unsupported_fonts);

    if valid_entries.is_empty() {
        bail!("no valid font files found in package");
    }

    let package = Package::new(spec.id.clone(), pkg_dirs.clone(), valid_entries);
    Ok(package)
}

fn download_archive(spec: &PackageSpec) -> eyre::Result<Bytes> {
    // TODO: Enforce a maximum downloaded response size, using Content-Length when available
    // and a running byte count while reading the response body.
    let response = reqwest::blocking::get(spec.url.clone())
        .wrap_err_with(|| format!("failed to download font archive from {}", spec.url))?
        .error_for_status()
        .wrap_err_with(|| format!("failed to download font archive from {}", spec.url))?;
    let content = response.bytes().wrap_err("failed to get response body")?;
    let digest = hash::digest_from_bytes(content.iter().as_slice());
    ensure!(
        digest == spec.sha256,
        "downloaded file hash mismatch: expected {}, got {}",
        spec.sha256,
        digest
    );
    Ok(content)
}

fn extract_archive(bytes: Bytes, fonts_dir: &AbsolutePath) -> eyre::Result<Vec<FileName>> {
    let mut files = vec![];
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader)?;

    // TODO: Enforce limits on extracted file count, per-file extracted size, and total
    // extracted size to guard against unexpectedly large or malicious ZIP archives.
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

    use std::fs;

    use semver::Version;
    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    use crate::package::{PackageId, PackageName};

    fn build_zip(entries: &[(&str, &[u8])]) -> Bytes {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            for (name, contents) in entries {
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .expect("failed to start zip entry");
                writer
                    .write_all(contents)
                    .expect("failed to write zip entry");
            }
            writer.finish().expect("failed to finish zip archive");
        }
        Bytes::from(cursor.into_inner())
    }

    fn extract_to_tempdir(bytes: Bytes) -> eyre::Result<(TempDir, Vec<FileName>)> {
        let tempdir = tempfile::tempdir().expect("failed to create temp dir");
        let fonts_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let files = extract_archive(bytes, &fonts_dir)?;
        Ok((tempdir, files))
    }

    fn make_package_dirs() -> (TempDir, PackageDirs) {
        let tempdir = tempfile::tempdir().expect("failed to create temp dir");
        let app_data_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let name = PackageName::new("hackgen").unwrap();
        let version = Version::new(2, 10, 0);
        let pkg_id = PackageId::new(name, version);
        let pkg_dirs = PackageDirs::new(app_data_dir, &pkg_id);
        (tempdir, pkg_dirs)
    }

    #[test]
    fn extract_archive_rejects_duplicate_font_file_names() {
        let bytes = build_zip(&[("a/font.ttf", b"font-a"), ("b/font.ttf", b"font-b")]);

        let err = extract_to_tempdir(bytes).expect_err("duplicate font file names should fail");

        assert!(format!("{err:?}").contains("extracted font file already exists"));
    }

    #[test]
    fn extract_archive_filters_non_font_files() {
        let bytes = build_zip(&[
            ("font.ttf", b"font"),
            ("font.ttc", b"collection"),
            ("font.otf", b"otf"),
            ("README.txt", b"readme"),
            ("dir/", b""),
        ]);

        let (_tempdir, files) =
            extract_to_tempdir(bytes).expect("font files should be extracted successfully");

        assert_eq!(files, vec!["font.ttf", "font.ttc", "font.otf"]);
    }

    #[test]
    fn remove_package_dirs_removes_empty_package_directories() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).expect("failed to create fonts dir");

        remove_package_dirs(&mut reporter, &pkg_dirs)
            .expect("empty package directories should be removed");

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
    }

    #[test]
    fn remove_package_dirs_stops_when_parent_directory_is_not_empty() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).expect("failed to create fonts dir");
        let sibling = pkg_dirs.name_dir().join("other-version");
        fs::create_dir(&sibling).expect("failed to create sibling version dir");

        remove_package_dirs(&mut reporter, &pkg_dirs)
            .expect("cleanup should succeed when package parent remains non-empty");

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.name_dir().exists());
        assert!(sibling.exists());
    }

    #[test]
    fn remove_package_dirs_ignores_missing_directories() {
        let mut reporter = Reporter::message_reporter();
        let (_tempdir, pkg_dirs) = make_package_dirs();

        remove_package_dirs(&mut reporter, &pkg_dirs)
            .expect("missing package directories should be ignored");

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
    }
}
