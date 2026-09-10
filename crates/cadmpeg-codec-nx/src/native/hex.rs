// SPDX-License-Identifier: Apache-2.0
//! Fixed-width lowercase hexadecimal identities.

use serde::{Deserialize, Serialize};

/// A SHA-256 digest encoded as 64 lowercase hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Hex(String);

impl Sha256Hex {
    /// SHA-256 of the supplied bytes.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self(cadmpeg_ir::hash::sha256_hex(bytes))
    }

    /// The digest of a completed SHA-256 computation.
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(encoded)
    }

    /// Borrow the digest text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Sha256Hex {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !is_lowercase_hex(&value, 64) {
            return Err("sha256 must contain 64 lowercase hexadecimal digits");
        }
        Ok(Self(value))
    }
}

impl std::fmt::Display for Sha256Hex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<Sha256Hex> for String {
    fn from(value: Sha256Hex) -> Self {
        value.0
    }
}

/// A saved-toggle identity encoded as 32 lowercase hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct ToggleId(String);

impl TryFrom<String> for ToggleId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !is_lowercase_hex(&value, 32) {
            return Err("SavedToggleEntry.toggle_id must be 32 lowercase hexadecimal digits");
        }
        Ok(Self(value))
    }
}

impl From<ToggleId> for String {
    fn from(value: ToggleId) -> Self {
        value.0
    }
}

impl std::fmt::Display for ToggleId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn is_lowercase_hex(value: &str, digits: usize) -> bool {
    value.len() == digits
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::{Sha256Hex, ToggleId};

    #[test]
    fn digest_preserves_canonical_hex_and_rejects_other_strings(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let digest = Sha256Hex::digest(&[]);
        assert_eq!(serde_json::to_value(&digest)?, serde_json::json!(expected));
        assert_eq!(
            serde_json::from_value::<Sha256Hex>(serde_json::json!(expected))?,
            digest
        );
        for invalid in [
            "0".repeat(63),
            "0".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            "é".repeat(32),
        ] {
            assert!(Sha256Hex::try_from(invalid.clone()).is_err());
            assert!(serde_json::from_value::<Sha256Hex>(serde_json::json!(invalid)).is_err());
        }
        Ok(())
    }

    #[test]
    fn toggle_identity_preserves_hex_and_rejects_other_strings(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let expected = "0123456789abcdef0123456789abcdef";
        let identity = ToggleId::try_from(expected.to_owned())?;
        assert_eq!(
            serde_json::to_value(&identity)?,
            serde_json::json!(expected)
        );
        assert_eq!(
            serde_json::from_value::<ToggleId>(serde_json::json!(expected))?,
            identity
        );
        for invalid in [
            "0".repeat(31),
            "0".repeat(33),
            "A".repeat(32),
            "g".repeat(32),
            "é".repeat(16),
        ] {
            assert!(ToggleId::try_from(invalid.clone()).is_err());
            assert!(serde_json::from_value::<ToggleId>(serde_json::json!(invalid)).is_err());
        }
        Ok(())
    }
}
