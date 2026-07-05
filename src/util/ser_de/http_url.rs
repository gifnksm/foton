use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use url::Url;

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Proxy(#[serde(with = "self")] Url);

impl From<Proxy> for Url {
    fn from(proxy: Proxy) -> Self {
        proxy.0
    }
}

pub(crate) fn serialize<S>(url: &Url, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    url.serialize(serializer)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Url, D::Error>
where
    D: Deserializer<'de>,
{
    let url = Url::deserialize(deserializer)?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(de::Error::invalid_value(
            de::Unexpected::Str(url.as_str()),
            &"a URL with http or https scheme",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_uses_json_string_for_utf8_str() {
        let url = Proxy("http://example.com/".parse().unwrap());
        assert_eq!(
            serde_json::to_string(&url).unwrap(),
            r#""http://example.com/""#
        );
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_str() {
        let Proxy(url) = serde_json::from_str(r#""http://example.com/""#).unwrap();
        assert_eq!(url.as_str(), "http://example.com/");
    }

    #[test]
    fn deserialize_rejects_non_http_scheme() {
        let result = serde_json::from_str::<Proxy>(r#""ftp://example.com/""#);
        result.unwrap_err();
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let url = Proxy("http://example.com".parse().unwrap());
        let json_value = serde_json::to_value(&url).unwrap();
        assert!(json_value.is_string());
        let Proxy(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, url.0);
    }
}
