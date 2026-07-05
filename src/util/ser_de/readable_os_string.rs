use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::{OsStrExt as _, OsStringExt as _},
};

#[cfg(test)]
use serde::Serialize;
#[cfg(not(test))]
use serde::Serialize as _;
use serde::{Deserialize, Deserializer, Serializer};

#[cfg(test)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
struct Proxy(#[serde(with = "self")] OsString);

pub(crate) fn serialize<S>(s: &OsStr, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if let Some(s) = s.to_str() {
        s.serialize(serializer)
    } else {
        let wtf16_code_units = s.encode_wide().collect::<Vec<_>>();
        wtf16_code_units.serialize(serializer)
    }
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<OsString, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Repr {
        String(String),
        OsString(OsString),
        Wtf16CodeUnits(Vec<u16>),
    }

    let s = match Repr::deserialize(deserializer)? {
        Repr::String(v) => v.into(),
        Repr::OsString(v) => v,
        Repr::Wtf16CodeUnits(v) => OsString::from_wide(&v),
    };
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_uses_json_string_for_utf8_str() {
        let s = Proxy("foobar".into());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""foobar""#);
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_str() {
        let Proxy(s) = serde_json::from_str(r#""foobar""#).unwrap();
        assert_eq!(s, "foobar");
    }

    #[test]
    fn deserialize_accepts_platform_os_string_representation() {
        let json_value = serde_json::to_value(OsString::from("foobar")).unwrap();
        assert!(json_value.is_object());
        let Proxy(s) = serde_json::from_value(json_value).unwrap();
        assert_eq!(s, "foobar");
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let ws = Proxy(OsString::from_wide(&[0xD800, 0x0061, 0x0000]));
        let json_value = serde_json::to_value(&ws).unwrap();
        assert!(json_value.is_array());
        let Proxy(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, ws.0);
    }
}
