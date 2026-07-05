use serde::{Deserialize, Deserializer, de};
#[cfg(test)]
use serde::{Serialize, Serializer};

#[cfg(test)]
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: Deserialize<'de>"))]
struct Proxy<T>(#[serde(with = "self")] Vec<T>);

#[cfg(test)]
pub(crate) fn serialize<T, S>(s: &[T], serializer: S) -> Result<S::Ok, S::Error>
where
    T: Serialize,
    S: Serializer,
{
    s.serialize(serializer)
}

pub(crate) fn deserialize<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    let v = Vec::<T>::deserialize(deserializer)?;
    if v.is_empty() {
        return Err(de::Error::invalid_value(
            de::Unexpected::Seq,
            &"a non-empty array",
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_uses_json_array() {
        let v = Proxy(vec![1, 2, 3]);
        assert_eq!(serde_json::to_string(&v).unwrap(), r"[1,2,3]");
    }

    #[test]
    fn deserialize_accepts_json_array() {
        let Proxy::<i32>(v) = serde_json::from_str(r"[1,2,3]").unwrap();
        assert_eq!(v, [1, 2, 3]);
    }

    #[test]
    fn deserialize_rejects_empty_array() {
        let result = serde_json::from_str::<Proxy<i32>>(r"[]");
        result.unwrap_err();
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let v = Proxy(vec![1, 2, 3]);
        let json_value = serde_json::to_value(&v).unwrap();
        assert!(json_value.is_array());
        let Proxy::<i32>(deserialized) = serde_json::from_value(json_value).unwrap();
        assert_eq!(deserialized, v.0);
    }
}
