use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    path::{self, Path, PathBuf},
};

use snafu::Snafu;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct AbsolutePath(PathBuf);

#[derive(Debug, Snafu)]
pub(crate) enum AbsolutePathError {
    #[snafu(display("path is not absolute: {path}", path = path.display()))]
    NotAbsolute { path: PathBuf },
}

impl AbsolutePath {
    pub(crate) fn new<P>(path: P) -> Option<Self>
    where
        P: AsRef<Path>,
    {
        Self::try_new(path).ok()
    }

    pub(crate) fn try_new<P>(path: P) -> Result<Self, AbsolutePathError>
    where
        P: AsRef<Path>,
    {
        let path = dunce::simplified(path.as_ref());
        if !path.is_absolute() {
            return Err(NotAbsoluteSnafu { path }.build());
        }
        Ok(Self(path.to_path_buf()))
    }

    pub(crate) fn join<P>(&self, path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let path = self.0.join(path);
        let path = dunce::simplified(&path);
        Self(path.to_path_buf())
    }

    pub(crate) fn parent(&self) -> Option<Self> {
        let parent = self.0.parent()?;
        parent.is_absolute().then(|| Self(parent.to_path_buf()))
    }

    pub(crate) fn display(&self) -> path::Display<'_> {
        self.0.display()
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn exists(&self) -> bool {
        self.0.exists()
    }
}

impl From<&AbsolutePath> for AbsolutePath {
    fn from(path: &AbsolutePath) -> Self {
        path.clone()
    }
}

impl From<&AbsolutePath> for PathBuf {
    fn from(path: &AbsolutePath) -> Self {
        path.0.clone()
    }
}

impl From<AbsolutePath> for PathBuf {
    fn from(path: AbsolutePath) -> Self {
        path.0
    }
}

impl TryFrom<&Path> for AbsolutePath {
    type Error = AbsolutePathError;

    fn try_from(path: &Path) -> Result<Self, Self::Error> {
        AbsolutePath::try_new(path)
    }
}

impl TryFrom<PathBuf> for AbsolutePath {
    type Error = AbsolutePathError;

    fn try_from(path: PathBuf) -> Result<Self, Self::Error> {
        AbsolutePath::try_new(path)
    }
}

impl AsRef<Path> for AbsolutePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

macro_rules! impl_partial_eq_for_absolute_path {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialEq<$ty> for AbsolutePath {
                fn eq(&self, other: &$ty) -> bool {
                    &self.0 == other
                }
            }

            impl PartialEq<AbsolutePath> for $ty {
                fn eq(&self, other: &AbsolutePath) -> bool {
                    self == &other.0
                }
            }
        )*
    };
}

impl_partial_eq_for_absolute_path!(
    OsString,
    OsStr,
    &OsStr,
    Cow<'_, OsStr>,
    PathBuf,
    Path,
    &Path,
    Cow<'_, Path>,
    String,
    str,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_new_returns_some_for_absolute_paths() {
        let abs_path = AbsolutePath::new(r"C:\absolute\path").unwrap();
        assert_eq!(abs_path, Path::new(r"C:\absolute\path"));
    }

    #[test]
    fn absolute_path_new_returns_none_for_relative_paths() {
        assert!(AbsolutePath::new("relative/path").is_none());
    }

    #[test]
    fn absolute_path_new_simplifies_path_if_possible() {
        let path = AbsolutePath::new(r"\\?\C:\absolute\path").unwrap();
        assert_eq!(path, Path::new(r"C:\absolute\path"));

        // path with reserved name component should not be simplified
        let path = AbsolutePath::new(r"\\?\C:\CON").unwrap();
        assert_eq!(path, Path::new(r"\\?\C:\CON"));

        // path with UNC prefix should not be simplified
        let path = AbsolutePath::new(r"\\?\UNC\server\share").unwrap();
        assert_eq!(path, Path::new(r"\\?\UNC\server\share"));
    }

    #[test]
    fn absolute_path_join_returns_simplified_absolute_path_if_possible() {
        let path = AbsolutePath::new(r"C:\absolute\path").unwrap();
        let joined = path.join(r"..\joined\path");
        assert_eq!(joined, Path::new(r"C:\absolute\path\..\joined\path"));

        let path = AbsolutePath::new(r"D:\absolute\path").unwrap();
        let joined = path.join(r"\\?\C:\joined\path");
        assert_eq!(joined, Path::new(r"C:\joined\path"));

        let path = AbsolutePath::new(r"C:\absolute\path").unwrap();
        let joined = path.join(r"\\?\UNC\server\share");
        assert_eq!(joined, Path::new(r"\\?\UNC\server\share"));
    }

    #[test]
    fn absolute_path_parent_returns_parent_for_nested_path() {
        let path = AbsolutePath::new(r"C:\absolute\path").unwrap();
        assert_eq!(path.parent().unwrap(), Path::new(r"C:\absolute"));
    }

    #[test]
    fn absolute_path_parent_returns_root_for_single_component_path() {
        let path = AbsolutePath::new(r"C:\absolute").unwrap();
        assert_eq!(path.parent().unwrap(), Path::new(r"C:\"));
    }

    #[test]
    fn absolute_path_parent_returns_none_for_root_path() {
        let path = AbsolutePath::new(r"C:\").unwrap();
        assert!(path.parent().is_none());
    }
}
