use std::{
    ffi::{OsStr, OsString},
    fmt::{self, Display},
    path::{Path, PathBuf},
};

use color_eyre::eyre::{self, ensure};

#[derive(Debug, Clone)]
pub(crate) struct PackageId {
    pub(crate) name: String,
    pub(crate) version: String,
}

impl Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}", self.name, self.version)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Package {
    id: PackageId,
    base_path: PathBuf,
    entries: Vec<FontEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct FontEntry {
    name: String,
    file_name: OsString,
}

impl Package {
    pub(crate) fn new<P>(id: PackageId, base_path: P, entries: Vec<FontEntry>) -> eyre::Result<Self>
    where
        P: Into<PathBuf>,
    {
        let base_path = base_path.into();
        ensure!(
            base_path.is_absolute(),
            "base path must be absolute: {}",
            base_path.display()
        );
        Ok(Self {
            id,
            base_path,
            entries,
        })
    }

    pub(crate) fn id(&self) -> &PackageId {
        &self.id
    }

    pub(crate) fn base_path(&self) -> &Path {
        &self.base_path
    }

    pub(crate) fn entries(&self) -> &[FontEntry] {
        &self.entries
    }
}

impl FontEntry {
    pub(crate) fn new<N, F>(name: N, file_name: F) -> eyre::Result<Self>
    where
        N: Into<String>,
        F: Into<OsString>,
    {
        let name = name.into();
        let file_name = file_name.into();
        validate_font_file_name(&file_name)?;
        Ok(Self { name, file_name })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn file_name(&self) -> &OsStr {
        &self.file_name
    }
}

fn validate_font_file_name(file_name: &OsStr) -> eyre::Result<()> {
    let path = Path::new(file_name);
    ensure!(
        !path.as_os_str().is_empty(),
        "font file name must not be empty"
    );
    let file_name_str = file_name.to_string_lossy();
    ensure!(
        !file_name_str.contains(['\\', '/']),
        "font file name must not contain path separators: {}",
        file_name.display(),
    );
    ensure!(
        file_name != OsStr::new(".") && file_name != OsStr::new(".."),
        "font file name must not be `.` or `..`: {}",
        file_name.display(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_package_id() -> PackageId {
        PackageId {
            name: "example-package".to_string(),
            version: "0.1.0".to_string(),
        }
    }

    #[test]
    fn package_new_accepts_absolute_base_path() {
        let package = Package::new(
            test_package_id(),
            r"C:\path\to\package",
            vec![FontEntry::new("Example Font", "example-font.ttf").unwrap()],
        )
        .expect("absolute base path should be accepted");

        assert_eq!(package.base_path(), Path::new(r"C:\path\to\package"));
    }

    #[test]
    fn package_new_rejects_relative_base_path() {
        let _ = Package::new(
            test_package_id(),
            r"relative\package",
            vec![FontEntry::new("Example Font", "example-font.ttf").unwrap()],
        )
        .expect_err("relative base path should be rejected");
    }

    #[test]
    fn font_entry_new_accepts_plain_file_name() {
        let entry = FontEntry::new("Example Font", "example-font.ttf")
            .expect("plain file name should work");

        assert_eq!(entry.name(), "Example Font");
        assert_eq!(entry.file_name(), OsStr::new("example-font.ttf"));
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
