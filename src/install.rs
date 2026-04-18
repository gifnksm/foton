use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Cursor, Write as _},
    path::Path,
};

use bytes::Bytes;
use color_eyre::eyre::{self, WrapErr as _, bail, ensure, eyre};
use zip::ZipArchive;

use crate::{
    cli::message::warn,
    package::{FontEntry, Package, PackageDirs, PackageSpec},
    platform::windows::{self, font_info::FontInspector},
    util::{
        app_dirs::AppDirs,
        error::{FormatErrorChain as _, IgnoreNotFound as _},
        hash,
    },
};

pub(crate) fn install_package(
    app_id: &str,
    spec: &PackageSpec,
    app_dirs: &AppDirs,
) -> eyre::Result<Package> {
    let pkg_dirs = PackageDirs::new(app_dirs.data_dir(), &spec.id)?;

    match try_install_package(app_id, spec, &pkg_dirs) {
        Ok(package) => Ok(package),
        Err(err) => {
            if let Err(rollback_err) = remove_package_dirs(&pkg_dirs) {
                let rollback_err = eyre!(rollback_err).wrap_err(format!(
                    "failed to remove package directory after install failure: {}; manual cleanup may be required",
                    pkg_dirs.version_dir().display()
                ));
                warn!("{}", rollback_err.format_error_chain());
            }
            Err(err)
        }
    }
}

pub(crate) fn uninstall_package(app_id: &str, package: &Package) -> eyre::Result<()> {
    windows::install::uninstall_package_fonts(app_id, package)?;
    remove_package_dirs(package.dirs())?;
    Ok(())
}

fn remove_package_dirs(pkg_dirs: &PackageDirs) -> eyre::Result<()> {
    fs::remove_dir_all(pkg_dirs.fonts_dir())
        .ignore_not_found()
        .wrap_err_with(|| {
            format!(
                "failed to remove package directory: {}",
                pkg_dirs.fonts_dir().display()
            )
        })?;

    // remove the package version / package name directory if it's empty after uninstall, ignoring errors
    if let Err(err) = fs::remove_dir(pkg_dirs.version_dir()).ignore_not_found() {
        if err.kind() == io::ErrorKind::DirectoryNotEmpty {
            return Ok(());
        }
        let err = eyre!(err).wrap_err(format!(
            "failed to remove package directory: {}",
            pkg_dirs.version_dir().display()
        ));
        warn!("{}", err.format_error_chain());
        return Ok(());
    }

    if let Err(err) = fs::remove_dir(pkg_dirs.name_dir()).ignore_not_found() {
        if err.kind() != io::ErrorKind::DirectoryNotEmpty {
            let err = eyre!(err).wrap_err(format!(
                "failed to remove package directory: {}",
                pkg_dirs.name_dir().display()
            ));
            warn!("{}", err.format_error_chain());
        }
        return Ok(());
    }

    Ok(())
}

fn try_install_package(
    app_id: &str,
    spec: &PackageSpec,
    pkg_dirs: &PackageDirs,
) -> eyre::Result<Package> {
    fs::create_dir_all(pkg_dirs.name_dir()).wrap_err_with(|| {
        format!(
            "failed to create package name directory: {}",
            pkg_dirs.name_dir().display()
        )
    })?;
    fs::create_dir(pkg_dirs.version_dir()).wrap_err_with(|| {
        format!(
            "failed to create package version directory: {}",
            pkg_dirs.version_dir().display()
        )
    })?;
    fs::create_dir(pkg_dirs.fonts_dir()).wrap_err_with(|| {
        format!(
            "failed to create package fonts directory: {}",
            pkg_dirs.fonts_dir().display()
        )
    })?;

    let bytes = download_archive(spec)?;
    let package_fonts_dir = pkg_dirs.fonts_dir();
    let file_paths = extract_archive(bytes, package_fonts_dir)?;

    let ValidationResult {
        unsupported_fonts,
        valid_entries,
    } = validate_fonts(package_fonts_dir, &file_paths)?;
    prune_invalid_fonts(package_fonts_dir, &unsupported_fonts);

    if valid_entries.is_empty() {
        bail!("no valid font files found in package");
    }

    let package = Package::new(spec.id.clone(), pkg_dirs.clone(), valid_entries);
    windows::install::install_package_fonts(app_id, &package)?;

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

fn extract_archive(bytes: Bytes, fonts_dir: &Path) -> eyre::Result<Vec<String>> {
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

        files.push(file_name.into_string().unwrap());
    }
    Ok(files)
}

struct ValidationResult {
    unsupported_fonts: Vec<String>,
    valid_entries: Vec<FontEntry>,
}

fn validate_fonts(fonts_dir: &Path, file_names: &[String]) -> eyre::Result<ValidationResult> {
    let mut unsupported_fonts = vec![];
    let mut valid_entries = vec![];
    let mut valid_entry_titles = HashSet::new();
    let inspector = FontInspector::new()?;
    for file_name in file_names {
        let Some(entry) = validate_font(&inspector, fonts_dir, file_name)? else {
            unsupported_fonts.push(file_name.to_owned());
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

fn validate_font(
    inspector: &FontInspector,
    fonts_dir: &Path,
    file_name: &str,
) -> eyre::Result<Option<FontEntry>> {
    let path = fonts_dir.join(file_name);
    let supported = inspector.is_supported_font_file(&path).wrap_err_with(|| {
        format!(
            "failed to check if font file is supported: {}",
            path.display()
        )
    })?;
    if !supported {
        return Ok(None);
    }

    let title = windows::font_info::get_font_title(&path)
        .wrap_err_with(|| format!("failed to get font title for file: {}", path.display()))?
        .unwrap_or_else(|| file_name.to_owned());
    Ok(Some(FontEntry::new(title, file_name)?))
}

fn prune_invalid_fonts(fonts_dir: &Path, invalid_files: &[String]) {
    for file_name in invalid_files {
        let path = fonts_dir.join(file_name);
        warn!("removing invalid font file: {}", path.display());
        if let Err(err) = fs::remove_file(&path) {
            let err = eyre!(err).wrap_err(format!("failed to remove file: {}", path.display()));
            warn!("{}", err.format_error_chain());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use zip::write::SimpleFileOptions;

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

    fn extract_to_tempdir(bytes: Bytes) -> eyre::Result<(tempfile::TempDir, Vec<String>)> {
        let tempdir = tempfile::tempdir().expect("failed to create temp dir");
        let files = extract_archive(bytes, tempdir.path())?;
        Ok((tempdir, files))
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

        assert_eq!(
            files,
            vec![
                String::from("font.ttf"),
                String::from("font.ttc"),
                String::from("font.otf"),
            ]
        );
    }
}
