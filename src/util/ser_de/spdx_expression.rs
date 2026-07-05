use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use spdx::Expression;

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Proxy(#[serde(with = "self")] Expression);

impl From<Proxy> for Expression {
    fn from(proxy: Proxy) -> Self {
        proxy.0
    }
}

pub(crate) fn serialize<S>(s: &Expression, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.to_string().serialize(serializer)
}

pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Expression, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse()
        .map_err(|e| de::Error::custom(format!("invalid SPDX expression: {e}")))
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn serialize_uses_json_string_for_utf8_str() {
        let expr = Proxy("MIT OR Apache-2.0".parse().unwrap());
        assert_eq!(
            serde_json::to_string(&expr).unwrap(),
            r#""MIT OR Apache-2.0""#
        );
    }

    #[test]
    fn deserialize_accepts_json_string_for_utf8_str() {
        let Proxy(expr) = serde_json::from_str(r#""MIT OR Apache-2.0""#).unwrap();
        assert_eq!(expr.to_string(), "MIT OR Apache-2.0");
    }

    #[test]
    fn deserialize_rejects_empty_string() {
        let result = serde_json::from_str::<Proxy>(r#""""#);
        result.unwrap_err();
    }

    #[test]
    fn deserialize_rejects_invalid_expression() {
        let result = serde_json::from_str::<Proxy>(r#""MIT WITH Apache-2.0""#);
        result.unwrap_err();

        let result = serde_json::from_str::<Proxy>(r#""invalid license""#);
        result.unwrap_err();
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let expr = Proxy("MIT OR Apache-2.0".parse().unwrap());
        let json_value = serde_json::to_value(&expr).unwrap();
        assert!(json_value.is_string());
        let Proxy(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, expr.0);
    }
}
