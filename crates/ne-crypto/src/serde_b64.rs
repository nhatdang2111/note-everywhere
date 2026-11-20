//! Serde helper that stores byte arrays as unpadded URL-safe base64 strings,
//! so server records stay compact and readable in JSON.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serializer};

pub(crate) fn encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub(crate) fn decode(text: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(text).ok()
}

pub(crate) fn serialize<S, T>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: AsRef<[u8]>,
{
    serializer.serialize_str(&encode(value.as_ref()))
}

pub(crate) fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: TryFrom<Vec<u8>>,
{
    let text = String::deserialize(deserializer)?;
    let bytes = decode(&text).ok_or_else(|| D::Error::custom("invalid base64"))?;
    T::try_from(bytes).map_err(|_| D::Error::custom("unexpected byte length"))
}
