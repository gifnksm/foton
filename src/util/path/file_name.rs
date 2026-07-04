use std::{
    borrow::Cow,
    ffi::{OsStr, OsString, os_str},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize, de};

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

    pub(crate) fn display(&self) -> os_str::Display<'_> {
        self.0.display()
    }
}

impl Serialize for FileName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let Some(s) = self.0.to_str() {
            str::serialize(s, serializer)
        } else {
            OsStr::serialize(&self.0, serializer)
        }
    }
}

impl<'de> Deserialize<'de> for FileName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            String(String),
            OsString(OsString),
        }

        let name = match Repr::deserialize(deserializer)? {
            Repr::String(v) => v.into(),
            Repr::OsString(v) => v,
        };
        FileName::new(name).ok_or_else(|| de::Error::custom("invalid file name"))
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
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _};

    use super::*;

    fn make_non_utf8_os_string() -> OsString {
        OsString::from_wide(&[
            0xD800,
            u16::from(b'.'),
            u16::from(b't'),
            u16::from(b't'),
            u16::from(b'f'),
        ])
    }

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

    #[test]
    fn serialize_uses_json_string_for_utf8_file_name() {
        let file_name = FileName::new("example-font.ttf").unwrap();

        assert_eq!(
            serde_json::to_string(&file_name).unwrap(),
            r#""example-font.ttf""#
        );
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_file_name() {
        let file_name: FileName = serde_json::from_str(r#""example-font.ttf""#).unwrap();

        assert_eq!(file_name, FileName::new("example-font.ttf").unwrap());
    }

    #[test]
    fn deserialize_accepts_platform_os_string_representation() {
        let value = serde_json::to_value(OsString::from("example-font.ttf")).unwrap();
        let file_name: FileName = serde_json::from_value(value).unwrap();

        assert_eq!(file_name, FileName::new("example-font.ttf").unwrap());
    }

    #[test]
    fn non_utf8_file_name_roundtrips_via_json() {
        let os_string = make_non_utf8_os_string();
        let file_name = FileName::new(&os_string).unwrap();

        assert_eq!(
            serde_json::to_value(&file_name).unwrap(),
            serde_json::to_value(&os_string).unwrap(),
        );
        assert_eq!(
            serde_json::from_value::<FileName>(serde_json::to_value(&file_name).unwrap()).unwrap(),
            file_name,
        );
    }
}
