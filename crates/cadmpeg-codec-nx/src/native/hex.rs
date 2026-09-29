// SPDX-License-Identifier: Apache-2.0
//! Fixed-width lowercase hexadecimal identities.

use serde::{Deserialize, Serialize};

/// A saved-toggle identity encoded as 32 lowercase hexadecimal digits.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub(super) struct ToggleId(String);

impl Serialize for ToggleId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl TryFrom<String> for ToggleId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if !Self::is_valid(&value) {
            return Err("SavedToggleEntry.toggle_id must be 32 lowercase hexadecimal digits");
        }
        Ok(Self(value))
    }
}

impl ToggleId {
    pub(super) fn is_valid(value: &str) -> bool {
        is_lowercase_hex(value, 32)
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
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
    use super::ToggleId;
    use cadmpeg_ir::hash::digest::Sha256Digest;

    #[test]
    fn sha256_hex_streams_once_with_native_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row {
            id: &'static str,
            sha256: Sha256Digest,
        }

        let sha256 = Sha256Digest::digest(&[]);
        let expected = serde_json::json!({
            "id": "nx:hash:digest#1", "sha256": sha256.as_str()
        });
        let row = Row {
            id: "nx:hash:digest#1",
            sha256,
        };
        cadmpeg_test_support::native_serialization::assert_native_limit(&row, expected);
    }

    #[test]
    fn toggle_id_streams_once_with_native_retained_limit() {
        #[derive(serde::Serialize)]
        struct Row {
            id: &'static str,
            toggle: ToggleId,
        }

        let value = "a".repeat(32);
        let row = Row {
            id: "nx:toggle:record#1",
            toggle: ToggleId::try_from(value.clone()).unwrap(),
        };
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &row,
            serde_json::json!({"id": row.id, "toggle": value}),
        );
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
