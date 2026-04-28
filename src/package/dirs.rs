use crate::{
    package::PackageId,
    util::{
        app_dirs::AppDirs,
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
    pub(crate) fn new(app_dirs: &AppDirs, pkg_id: &PackageId) -> Self {
        let package_base_dir = app_dirs.data_local_dir().join("packages");
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
    use std::{fs, sync::LazyLock};

    use crate::util::testing;

    use super::*;

    static PKG_ID: LazyLock<PackageId> =
        LazyLock::new(|| "example-namespace/example-font@0.1.0".parse().unwrap());

    #[test]
    fn package_dirs_new_uses_app_dirs_data_local_dir() {
        let data_local_dir = AbsolutePath::new(r"C:\path\to\data").unwrap();
        let app_dirs = &AppDirs::new_for_test(data_local_dir.clone());
        let pkg_dirs = PackageDirs::new(app_dirs, &PKG_ID);
        assert_eq!(
            pkg_dirs.namespace_dir(),
            &data_local_dir.join("packages").join("example-namespace"),
        );
        assert_eq!(
            pkg_dirs.name_dir(),
            &pkg_dirs.namespace_dir().join("example-font"),
        );
        assert_eq!(pkg_dirs.version_dir(), &pkg_dirs.name_dir().join("0.1.0"));
        assert_eq!(pkg_dirs.fonts_dir(), &pkg_dirs.version_dir().join("fonts"));
    }

    #[test]
    fn remove_package_dirs_removes_empty_package_directories() {
        let (_tempdir, _app_dirs, pkg_dirs) = testing::make_package_dirs(&PKG_ID);
        fs::create_dir_all(pkg_dirs.fonts_dir()).unwrap();

        remove_package_dirs(&pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }

    #[test]
    fn remove_package_dirs_stops_when_parent_directory_is_not_empty() {
        let (_tempdir, _app_dirs, pkg_dirs) = testing::make_package_dirs(&PKG_ID);
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
        let (_tempdir, _app_dirs, pkg_dirs) = testing::make_package_dirs(&PKG_ID);

        remove_package_dirs(&pkg_dirs).unwrap();

        assert!(!pkg_dirs.fonts_dir().exists());
        assert!(!pkg_dirs.version_dir().exists());
        assert!(!pkg_dirs.name_dir().exists());
        assert!(!pkg_dirs.namespace_dir().exists());
    }
}
