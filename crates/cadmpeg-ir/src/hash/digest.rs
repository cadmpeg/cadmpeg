// SPDX-License-Identifier: Apache-2.0
//! Admitted SHA-256 digests.

use serde::{Deserialize, Deserializer, Serialize};

/// A SHA-256 digest spelled as exactly 64 lowercase hexadecimal characters.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Digest(String);

/// Text that does not spell a lowercase hexadecimal SHA-256 digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("sha256 digest must contain exactly 64 lowercase hexadecimal characters")]
pub struct InvalidSha256Digest;

impl Sha256Digest {
    /// Hash source bytes without a text admission step.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self::from_bytes(super::sha256(bytes))
    }

    /// Charge input hashing work and 64 retained bytes before allocating digest text.
    pub fn digest_for_decode(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        operation: &'static str,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        ctx.charge_work(cadmpeg_core::decode::u64_from_index(bytes.len()), operation)?;
        Self::from_bytes_for_decode(ctx, super::sha256(bytes), operation)
    }

    /// Encode the 32 bytes produced by a SHA-256 hasher.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self::encode_bytes(bytes, String::with_capacity(64))
    }

    /// Encode a completed hash after admitting its retained hexadecimal text.
    pub fn from_bytes_for_decode(ctx: &cadmpeg_core::decode::DecodeContext<'_>, bytes: [u8; 32], operation: &'static str) -> Result<Self, cadmpeg_core::CodecError> {
        let text = ctx.retained_string(64, operation)?;
        ctx.charge_work(64, operation)?;
        Ok(Self::encode_bytes(bytes, text))
    }

    fn encode_bytes(bytes: [u8; 32], mut text: String) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 15)]));
        }
        Self(text)
    }

    /// Copy the canonical spelling through the decode budget.
    pub fn try_clone_for_decode(&self, ctx: &cadmpeg_core::decode::DecodeContext<'_>, operation: &'static str) -> Result<Self, cadmpeg_core::CodecError> {
        Ok(Self(ctx.copy_retained_text(self.as_str(), operation)?))
    }

    /// Borrow the canonical hexadecimal spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<Sha256Digest> for String {
    fn from(digest: Sha256Digest) -> Self {
        digest.0
    }
}

impl TryFrom<String> for Sha256Digest {
    type Error = InvalidSha256Digest;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.len() == 64
            && text
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            Ok(Self(text))
        } else {
            Err(InvalidSha256Digest)
        }
    }
}

impl TryFrom<&str> for Sha256Digest {
    type Error = InvalidSha256Digest;

    fn try_from(text: &str) -> Result<Self, Self::Error> {
        Self::try_from(text.to_owned())
    }
}

impl std::fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Sha256Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Sha256Digest {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Sha256Digest".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string", "minLength": 64, "maxLength": 64,
            "pattern": "^[0-9a-f]{64}$"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Sha256Digest;

    #[test]
    fn digest_for_decode_refuses_before_retained_allocation() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 63;
        let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
        let result = Sha256Digest::digest_for_decode(&ctx, b"abc", "digest test");
        assert!(
            matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "digest test" && limit.additional == 64)
        );
    }

    #[test]
    fn digest_for_decode_succeeds_under_service_profile() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
        let digest = Sha256Digest::digest_for_decode(&ctx, b"abc", "digest test").unwrap();
        assert_eq!(digest, Sha256Digest::digest(b"abc"));
        assert_eq!(String::from(digest), Sha256Digest::digest(b"abc").as_str());
    }

    #[test]
    fn digest_for_decode_refuses_hashing_work() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
        assert!(
            matches!(Sha256Digest::digest_for_decode(&ctx, b"abc", "digest test"),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.additional == 3)
        );
    }

    #[cfg(feature = "schema")]
    #[test]
    fn digest_schema_does_not_admit_a_trailing_line_break() {
        let schema = serde_json::to_value(schemars::schema_for!(Sha256Digest)).unwrap();
        assert_eq!(schema["minLength"], 64);
        assert_eq!(schema["maxLength"], 64);
        assert_eq!(schema["pattern"], "^[0-9a-f]{64}$");
        assert!(Sha256Digest::try_from(format!("{}\n", "a".repeat(64))).is_err());
    }

    #[test]
    fn digest_matches_sha256_abc_vector() {
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert_eq!(Sha256Digest::digest(b"abc").as_str(), expected);
        assert_eq!(
            Sha256Digest::try_from(expected).unwrap(),
            Sha256Digest::digest(b"abc")
        );
    }

    #[test]
    fn text_and_serde_admission_require_canonical_hex() {
        for text in [
            String::new(),
            "a".repeat(63),
            "a".repeat(65),
            "A".repeat(64),
            "g".repeat(64),
            " ".repeat(64),
            "é".repeat(32),
        ] {
            assert!(Sha256Digest::try_from(text.as_str()).is_err());
            assert!(serde_json::from_value::<Sha256Digest>(serde_json::json!(text)).is_err());
        }
        let digest = Sha256Digest::from_bytes([0; 32]);
        assert_eq!(digest.as_str(), "0".repeat(64));
        assert_eq!(
            serde_json::from_value::<Sha256Digest>(serde_json::json!(digest)).unwrap(),
            digest
        );
    }
}
