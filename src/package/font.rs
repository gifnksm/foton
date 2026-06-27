use crate::{
    package::PackageDirs,
    util::path::{AbsolutePath, FileName},
};

#[derive(Debug, Clone)]
pub(crate) struct PackageFont {
    file_name: FileName,
}

impl PackageFont {
    pub(crate) fn new<F>(file_name: F) -> Self
    where
        F: Into<FileName>,
    {
        let file_name = file_name.into();
        Self { file_name }
    }

    pub(crate) fn file_name(&self) -> &FileName {
        &self.file_name
    }

    pub(crate) fn path(&self, pkg_dirs: &PackageDirs) -> AbsolutePath {
        pkg_dirs.fonts_dir().join(&self.file_name)
    }
}
