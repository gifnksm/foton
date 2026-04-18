use std::{
    fmt::{self, Display},
    path::{self, Path, PathBuf},
    sync::LazyLock,
};

use color_eyre::eyre::{self, ensure};
use regex::Regex;
use reqwest::Url;
use semver::Version;

use crate::util::hash::Sha256Digest;

#[derive(Debug, Clone)]
pub(crate) struct PackageId {
    name: String,
    version: Version,
}

impl PackageId {
    pub(crate) fn new<N, V>(name: N, version: V) -> eyre::Result<Self>
    where
        N: Into<String>,
        V: Into<Version>,
    {
        static NAME_REGEX: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"^[a-zA-Z][-_0-9a-zA-Z]+$").unwrap());

        let name = name.into();
        let version = version.into();

        ensure!(NAME_REGEX.is_match(&name), "invalid package name: {name}");

        Ok(Self { name, version })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn version(&self) -> &Version {
        &self.version
    }
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PackageSpec {
    pub(crate) id: PackageId,
    pub(crate) url: Url,
    pub(crate) sha256: Sha256Digest,
}

#[derive(Debug, Clone)]
#[expect(clippy::struct_field_names)]
pub(crate) struct PackageDirs {
    name_dir: PathBuf,
    version_dir: PathBuf,
    fonts_dir: PathBuf,
}

impl PackageDirs {
    pub(crate) fn new<P>(app_data_dir: P, pkg_id: &PackageId) -> eyre::Result<Self>
    where
        P: Into<PathBuf>,
    {
        let app_data_dir = app_data_dir.into();
        ensure!(
            app_data_dir.is_absolute(),
            "data directory path must be absolute: {}",
            app_data_dir.display()
        );
        let package_base_dir = app_data_dir.join("packages");
        let name_dir = package_base_dir.join(pkg_id.name());
        let version_dir = name_dir.join(pkg_id.version().to_string());
        let fonts_dir = version_dir.join("fonts");
        Ok(Self {
            name_dir,
            version_dir,
            fonts_dir,
        })
    }

    pub(crate) fn name_dir(&self) -> &Path {
        &self.name_dir
    }

    pub(crate) fn version_dir(&self) -> &Path {
        &self.version_dir
    }

    pub(crate) fn fonts_dir(&self) -> &Path {
        &self.fonts_dir
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Package {
    id: PackageId,
    dirs: PackageDirs,
    entries: Vec<FontEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct FontEntry {
    title: String,
    file_name: String,
}

impl Package {
    pub(crate) fn new(id: PackageId, dirs: PackageDirs, entries: Vec<FontEntry>) -> Self {
        Self { id, dirs, entries }
    }

    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    pub(crate) fn dirs(&self) -> &PackageDirs {
        &self.dirs
    }

    pub(crate) fn entries(&self) -> &[FontEntry] {
        &self.entries
    }
}

impl FontEntry {
    pub(crate) fn new<T, F>(title: T, file_name: F) -> eyre::Result<Self>
    where
        T: Into<String>,
        F: Into<String>,
    {
        let title = title.into();
        let file_name = file_name.into();
        validate_font_file_name(&file_name)?;
        Ok(Self { title, file_name })
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn file_name(&self) -> &str {
        &self.file_name
    }
}

fn validate_font_file_name(file_name: &str) -> eyre::Result<()> {
    let path = Path::new(file_name);
    ensure!(
        !path.as_os_str().is_empty(),
        "font file name must not be empty"
    );
    ensure!(
        !file_name.contains(path::is_separator),
        "font file name must not contain path separators: {file_name}",
    );
    ensure!(
        file_name != "." && file_name != "..",
        "font file name must not be `.` or `..`: {file_name}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_package_id() -> PackageId {
        PackageId::new("example-package", Version::new(0, 1, 0)).unwrap()
    }

    #[test]
    fn package_dirs_new_accepts_absolute_base_path() {
        let pkg_dirs = PackageDirs::new(r"C:\path\to\package", &test_package_id()).unwrap();
        assert_eq!(
            pkg_dirs.name_dir(),
            Path::new(r"C:\path\to\package\packages\example-package")
        );
        assert_eq!(
            pkg_dirs.version_dir(),
            Path::new(r"C:\path\to\package\packages\example-package\0.1.0")
        );
        assert_eq!(
            pkg_dirs.fonts_dir(),
            Path::new(r"C:\path\to\package\packages\example-package\0.1.0\fonts")
        );
    }

    #[test]
    fn package_dirs_new_rejects_relative_base_path() {
        let _ = PackageDirs::new(r"relative\package", &test_package_id()).unwrap_err();
    }

    #[test]
    fn package_id_new_accepts_valid_names() {
        for name in ["hackgen", "HackGen", "hackgen-nerd", "hackgen_nerd", "a0"] {
            let pkg_id = PackageId::new(name, Version::new(0, 1, 0))
                .expect("valid package name should be accepted");
            assert_eq!(pkg_id.name(), name);
        }
    }

    #[test]
    fn package_id_new_rejects_invalid_names() {
        for name in [
            "",
            "0hackgen",
            "-hackgen",
            "_hackgen",
            "hackgen/nerd",
            r"hackgen\nerd",
            "hackgen:nerd",
        ] {
            let _ = PackageId::new(name, Version::new(0, 1, 0))
                .expect_err("invalid package name should be rejected");
        }
    }

    #[test]
    fn font_entry_new_accepts_plain_file_name() {
        let entry = FontEntry::new("Example Font", "example-font.ttf")
            .expect("plain file name should work");

        assert_eq!(entry.title(), "Example Font");
        assert_eq!(entry.file_name(), "example-font.ttf");
    }

    #[test]
    fn font_entry_new_rejects_invalid_file_names() {
        for file_name in [
            "",
            ".",
            "..",
            "dir/example-font.ttf",
            r"dir\example-font.ttf",
            r"example-font.ttf\",
        ] {
            let _ = FontEntry::new("Example Font", file_name).unwrap_err();
        }
    }
}
