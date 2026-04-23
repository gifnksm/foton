use crate::{
    package::PackageId,
    util::{
        fs::{self as fs_util, FsError},
        path::AbsolutePath,
    },
};

#[derive(Debug, Clone)]
#[expect(clippy::struct_field_names)]
pub(crate) struct PackageDirs {
    namespace_dir: AbsolutePath,
    name_dir: AbsolutePath,
    version_dir: AbsolutePath,
    fonts_dir: AbsolutePath,
}

impl PackageDirs {
    pub(crate) fn new<P>(app_data_dir: P, pkg_id: &PackageId) -> Self
    where
        P: Into<AbsolutePath>,
    {
        let app_data_dir = app_data_dir.into();
        let package_base_dir = app_data_dir.join("packages");
        let namespace_dir = package_base_dir.join(pkg_id.namespace());
        let name_dir = namespace_dir.join(pkg_id.name());
        let version_dir = name_dir.join(pkg_id.version().to_string());
        let fonts_dir = version_dir.join("fonts");
        Self {
            namespace_dir,
            name_dir,
            version_dir,
            fonts_dir,
        }
    }

    pub(crate) fn namespace_dir(&self) -> &AbsolutePath {
        &self.namespace_dir
    }

    pub(crate) fn name_dir(&self) -> &AbsolutePath {
        &self.name_dir
    }

    pub(crate) fn version_dir(&self) -> &AbsolutePath {
        &self.version_dir
    }

    pub(crate) fn fonts_dir(&self) -> &AbsolutePath {
        &self.fonts_dir
    }
}

pub(crate) fn create_new_package_dirs(pkg_dirs: &PackageDirs) -> Result<(), FsError> {
    fs_util::create_dir_all(pkg_dirs.name_dir())?;
    // fails if version_dir already exists, preventing overwriting existing package versions
    fs_util::create_dir(pkg_dirs.version_dir())?;
    fs_util::create_dir(pkg_dirs.fonts_dir())?;
    Ok(())
}

pub(crate) fn remove_package_dirs(pkg_dirs: &PackageDirs) -> Result<(), FsError> {
    fs_util::remove_dir_all_if_exists(pkg_dirs.fonts_dir())?;

    let ancestors = [
        pkg_dirs.version_dir(),
        pkg_dirs.name_dir(),
        pkg_dirs.namespace_dir(),
    ];
    for ancestor in ancestors {
        let res = fs_util::remove_dir_if_empty(ancestor)?;
        if res.is_not_empty() {
            return Ok(());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use semver::Version;
    use tempfile::TempDir;

    use crate::package::{PackageName, PackageNamespace};

    use super::*;

    fn test_package_id() -> PackageId {
        let namespace = PackageNamespace::new("example-namespace").unwrap();
        let name = PackageName::new("example-package").unwrap();
        PackageId::new(namespace, name, Version::new(0, 1, 0))
    }

    #[test]
    fn package_dirs_new_accepts_absolute_base_path() {
        let app_data_dir = AbsolutePath::new(r"C:\path\to\package").unwrap();
        let pkg_dirs = PackageDirs::new(app_data_dir, &test_package_id());
        assert_eq!(
            pkg_dirs.namespace_dir(),
            Path::new(r"C:\path\to\package\packages\example-namespace")
        );
        assert_eq!(
            pkg_dirs.name_dir(),
            Path::new(r"C:\path\to\package\packages\example-namespace\example-package")
        );
        assert_eq!(
            pkg_dirs.version_dir(),
            Path::new(r"C:\path\to\package\packages\example-namespace\example-package\0.1.0")
        );
        assert_eq!(
            pkg_dirs.fonts_dir(),
            Path::new(r"C:\path\to\package\packages\example-namespace\example-package\0.1.0\fonts")
        );
    }

    fn make_package_dirs() -> (TempDir, PackageDirs) {
        let tempdir = tempfile::tempdir().unwrap();
        let app_data_dir = AbsolutePath::new(tempdir.path()).unwrap();
        let namespace = PackageNamespace::new("yuru7").unwrap();
        let name = PackageName::new("hackgen").unwrap();
        let version = Version::new(2, 10, 0);
        let pkg_id = PackageId::new(namespace, name, version);
        let pkg_dirs = PackageDirs::new(app_data_dir, &pkg_id);
        (tempdir, pkg_dirs)
    }

    #[test]
    fn remove_package_dirs_removes_empty_package_directories() {
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();

        remove_package_dirs(&pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }

    #[test]
    fn remove_package_dirs_stops_when_parent_directory_is_not_empty() {
        let (_tempdir, pkg_dirs) = make_package_dirs();
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();
        let sibling = pkg_dirs.name_dir().join("other-version");
        fs::create_dir(&sibling).unwrap();

        remove_package_dirs(&pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(pkg_dirs.name_dir().exists());
        assert!(pkg_dirs.namespace_dir().exists());
        assert!(sibling.exists());
    }

    #[test]
    fn remove_package_dirs_ignores_missing_directories() {
        let (_tempdir, pkg_dirs) = make_package_dirs();

        remove_package_dirs(&pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }
}
