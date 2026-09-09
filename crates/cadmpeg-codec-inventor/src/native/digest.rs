// SPDX-License-Identifier: Apache-2.0
//! SHA-256 digest admission.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct Sha256Hex(String);

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
