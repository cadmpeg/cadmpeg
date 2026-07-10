// SPDX-License-Identifier: Apache-2.0
//! Serde adapter for byte vectors represented as base64 strings.
//!
//! Apply this module with `#[serde(with = "crate::bytes")]` on a `Vec<u8>`
//! field.

use std::fmt;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{de::Visitor, Deserializer, Serializer};

/// Serializes bytes as a standard, padded base64 string.
pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&STANDARD.encode(bytes))
}

/// Deserializes a standard, padded base64 string into bytes.
pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Base64Visitor;

    impl<'de> Visitor<'de> for Base64Visitor {
        type Value = Vec<u8>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a standard, padded base64 string")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            STANDARD
                .decode(value)
                .map_err(|error| E::custom(format_args!("invalid base64 byte payload: {error}")))
        }

        fn visit_borrowed_str<E>(self, value: &'de str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(value)
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(&value)
        }
    }

    deserializer.deserialize_str(Base64Visitor)
}

/// Serde adapter for optional byte vectors.
pub mod option {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize optional bytes as an optional base64 string.
    pub fn serialize<S>(bytes: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match bytes {
            Some(value) => serializer.serialize_some(&STANDARD.encode(value)),
            None => serializer.serialize_none(),
        }
    }

    /// Deserialize an optional base64 string.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| STANDARD.decode(value).map_err(serde::de::Error::custom))
            .transpose()
    }
}
