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
    serializer.collect_str(&Base64Text(bytes))
}

struct Base64Text<'a>(&'a [u8]);

impl fmt::Display for Base64Text<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        const INPUT_CHUNK: usize = 768;
        const OUTPUT_CHUNK: usize = 1024;
        let mut output = [0u8; OUTPUT_CHUNK];
        for chunk in self.0.chunks(INPUT_CHUNK) {
            let used = STANDARD
                .encode_slice(chunk, &mut output)
                .map_err(|_| fmt::Error)?;
            let text = std::str::from_utf8(&output[..used]).map_err(|_| fmt::Error)?;
            formatter.write_str(text)?;
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests;
