use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use serde::Serializer;
#[cfg(test)]
use serde::{Deserialize, Deserializer, Serialize};

use crate::util::ser_de::readable_os_string;

#[cfg(test)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
struct Proxy(#[serde(with = "self")] PathBuf);

pub(crate) fn serialize<S>(s: &Path, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    readable_os_string::serialize(s.as_os_str(), serializer)
}

#[cfg(test)]
pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    readable_os_string::deserialize(deserializer).map(Into::into)
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt as _};

    use super::*;

    #[test]
    fn serialize_uses_json_string_for_utf8_str() {
        let s = Proxy("foobar".into());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""foobar""#);
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_str() {
        let Proxy(s) = serde_json::from_str(r#""foobar""#).unwrap();
        assert_eq!(&s, "foobar");
    }

    #[test]
    fn deserialize_accepts_platform_path_buf_representation() {
        let json_value = serde_json::to_value(PathBuf::from("foobar")).unwrap();
        assert!(json_value.is_string());
        let Proxy(s) = serde_json::from_value(json_value).unwrap();
        assert_eq!(&s, "foobar");
    }

    #[test]
    fn deserialize_accepts_platform_os_string_representation() {
        let json_value = serde_json::to_value(OsString::from("foobar")).unwrap();
        assert!(json_value.is_object());
        let Proxy(s) = serde_json::from_value(json_value).unwrap();
        assert_eq!(&s, "foobar");
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let ws = Proxy(OsString::from_wide(&[0xD800, 0x0061, 0x0000]).into());
        let json_value = serde_json::to_value(&ws).unwrap();
        assert!(json_value.is_array());
        let Proxy(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, ws.0);
    }
}
