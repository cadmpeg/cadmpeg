// SPDX-License-Identifier: Apache-2.0
//! SHA-256 digest admission.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Sha256Hex(String);

impl Serialize for Sha256Hex {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("SHA-256 digest must contain 64 hexadecimal characters");
        }
        Ok(Self(value))
    }
}

impl From<Sha256Hex> for String {
    fn from(value: Sha256Hex) -> Self {
        value.0
    }
}

impl Sha256Hex {
    pub(crate) fn digest(bytes: &[u8]) -> Self {
        Self(cadmpeg_ir::hash::sha256_hex(bytes))
    }
}
