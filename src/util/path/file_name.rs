use std::{
    borrow::Cow,
    ffi::{OsStr, OsString, os_str},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FileName(OsString);

impl FileName {
    pub(crate) fn new<N>(name: N) -> Option<Self>
    where
        N: Into<OsString>,
    {
        let name = name.into();
        if name.is_empty() {
            return None;
        }
        let path = Path::new(&name);
        if path.file_name() != Some(&name) || path.components().count() != 1 {
            return None;
        }
        Some(Self(name))
    }

    pub(crate) fn to_os_string(&self) -> OsString {
        self.0.clone()
    }

    pub(crate) fn display(&self) -> os_str::Display<'_> {
        self.0.display()
    }
}

impl From<&FileName> for FileName {
    fn from(name: &FileName) -> Self {
        name.clone()
    }
}

impl From<&FileName> for PathBuf {
    fn from(name: &FileName) -> Self {
        PathBuf::from(name.0.clone())
    }
}

impl From<FileName> for PathBuf {
    fn from(name: FileName) -> Self {
        PathBuf::from(name.0)
    }
}

impl AsRef<Path> for FileName {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

macro_rules! impl_partial_eq_for_file_name {
    ($($ty:ty),* $(,)?) => {
        $(
            impl PartialEq<$ty> for FileName {
                fn eq(&self, other: &$ty) -> bool {
                    &self.0 == other
                }
            }

            impl PartialEq<FileName> for $ty {
                fn eq(&self, other: &FileName) -> bool {
                    self == &other.0
                }
            }
        )*
    };
}

impl_partial_eq_for_file_name!(
    OsString,
    OsStr,
    &OsStr,
    Cow<'_, OsStr>,
    PathBuf,
    Path,
    &Path,
    Cow<'_, Path>,
    str,
    &str,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_name_new_accepts_plain_file_name() {
        let file_name = FileName::new("example-font.ttf").unwrap();
        assert_eq!(file_name.0, "example-font.ttf");
    }

    #[test]
    fn file_name_new_rejects_invalid_file_names() {
        for file_name in [
            "",
            ".",
            "..",
            "dir/example-font.ttf",
            r"dir\example-font.ttf",
            r"example-font.ttf\",
        ] {
            assert!(FileName::new(file_name).is_none());
        }
    }
}
