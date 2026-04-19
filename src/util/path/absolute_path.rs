use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    path::{self, Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct AbsolutePath(PathBuf);

impl AbsolutePath {
    pub(crate) fn new<P>(path: P) -> Option<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        path.is_absolute().then_some(Self(path))
    }

    pub(crate) fn join<P>(&self, path: P) -> Self
    where
        P: AsRef<Path>,
    {
        Self(self.0.join(path))
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
    fn absolute_path_buf_new_returns_some_for_absolute_paths() {
        let abs_path = AbsolutePath::new(r"C:\absolute\path").unwrap();
        assert_eq!(abs_path, Path::new(r"C:\absolute\path"));
    }

    #[test]
    fn absolute_path_buf_new_returns_none_for_relative_paths() {
        assert!(AbsolutePath::new("relative/path").is_none());
    }
}
