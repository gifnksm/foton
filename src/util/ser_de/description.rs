use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Proxy(#[serde(with = "self")] String);

impl From<Proxy> for String {
    fn from(proxy: Proxy) -> Self {
        proxy.0
    }
}

pub(crate) fn serialize<S>(s: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize(serializer)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    let t = s.trim();
    if t.is_empty() || t != s {
        return Err(de::Error::invalid_value(
            de::Unexpected::Str(&s),
            &"a non-empty string without leading or trailing whitespace",
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_uses_json_string_for_utf8_str() {
        let s = Proxy("foobar".to_owned());
        assert_eq!(serde_json::to_string(&s).unwrap(), r#""foobar""#);
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_str() {
        let Proxy(s) = serde_json::from_str(r#""foobar""#).unwrap();
        assert_eq!(s.as_str(), "foobar");
    }

    #[test]
    fn deserialize_rejects_empty_string() {
        let result = serde_json::from_str::<Proxy>(r#""""#);
        result.unwrap_err();
    }

    #[test]
    fn deserialize_rejects_string_with_surrounding_whitespaces() {
        let result = serde_json::from_str::<Proxy>(r#"" foobar ""#);
        result.unwrap_err();

        let result = serde_json::from_str::<Proxy>(r#"" foobar""#);
        result.unwrap_err();

        let result = serde_json::from_str::<Proxy>(r#""foobar ""#);
        result.unwrap_err();
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let s = Proxy("foobar".to_owned());
        let json_value = serde_json::to_value(&s).unwrap();
        assert!(json_value.is_string());
        let Proxy(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, s.0);
    }
}
