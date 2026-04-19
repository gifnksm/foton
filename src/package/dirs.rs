use crate::{package::PackageId, util::path::AbsolutePath};

#[derive(Debug, Clone)]
#[expect(clippy::struct_field_names)]
pub(crate) struct PackageDirs {
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
        let name_dir = package_base_dir.join(pkg_id.name());
        let version_dir = name_dir.join(pkg_id.version().to_string());
        let fonts_dir = version_dir.join("fonts");
        Self {
            name_dir,
            version_dir,
            fonts_dir,
        }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use semver::Version;

    use crate::package::PackageName;

    use super::*;

    fn test_package_id() -> PackageId {
        let name = PackageName::new("example-package").unwrap();
        PackageId::new(name, Version::new(0, 1, 0))
    }

    #[test]
    fn package_dirs_new_accepts_absolute_base_path() {
        let app_data_dir = AbsolutePath::new(r"C:\path\to\package").unwrap();
        let pkg_dirs = PackageDirs::new(app_data_dir, &test_package_id());
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
}
