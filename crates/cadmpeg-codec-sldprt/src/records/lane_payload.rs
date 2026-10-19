// SPDX-License-Identifier: Apache-2.0
//! Transient input-lane bytes and their document-safe evidence.

use cadmpeg_ir::hash::digest::Sha256Digest;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Bytes exist only during source parsing or a fidelity-backed patch.
/// Serialization carries their length and digest, never their content.
#[derive(Debug, Clone)]
pub(crate) struct LanePayload {
    bytes: Vec<u8>,
    evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    byte_len: u64,
    sha256: Sha256Digest,
}

impl LanePayload {
    fn evidence(&self) -> Evidence {
        if self.bytes.is_empty() {
            self.evidence.clone()
        } else {
            Evidence {
                byte_len: cadmpeg_core::decode::u64_from_index(self.bytes.len()),
                sha256: Sha256Digest::digest(&self.bytes),
            }
        }
    }

    pub(crate) fn byte_len(&self) -> u64 {
        self.evidence().byte_len
    }

    pub(crate) fn sha256(&self) -> Sha256Digest {
        self.evidence().sha256
    }

    pub(crate) fn into_inner(self) -> Vec<u8> {
        self.bytes
    }

    /// Rehydrate layout only after the sidecar's extent and digest agree.
    pub(crate) fn attach(&mut self, bytes: &[u8]) -> Result<(), cadmpeg_core::CodecError> {
        if cadmpeg_core::decode::u64_from_index(bytes.len()) != self.byte_len()
            || Sha256Digest::digest(bytes) != self.sha256()
        {
            return Err(cadmpeg_core::CodecError::malformed(
                "SLDPRT retained input-lane bytes disagree with their document evidence",
            ));
        }
        self.bytes = bytes.to_vec();
        Ok(())
    }
}

impl From<Vec<u8>> for LanePayload {
    fn from(bytes: Vec<u8>) -> Self {
        let evidence = Evidence {
            byte_len: cadmpeg_core::decode::u64_from_index(bytes.len()),
            sha256: Sha256Digest::digest(&bytes),
        };
        Self { bytes, evidence }
    }
}

impl Default for LanePayload {
    fn default() -> Self {
        Vec::new().into()
    }
}

impl PartialEq for LanePayload {
    fn eq(&self, other: &Self) -> bool {
        self.evidence() == other.evidence()
    }
}

impl std::ops::Deref for LanePayload {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl std::ops::DerefMut for LanePayload {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bytes
    }
}

impl Serialize for LanePayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.evidence().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LanePayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            bytes: Vec::new(),
            evidence: Evidence::deserialize(deserializer)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::LanePayload;

    #[test]
    fn document_wire_contains_evidence_and_fidelity_restores_bytes() {
        let payload = LanePayload::from(b"source-only lane".to_vec());
        let wire = serde_json::to_value(&payload).expect("payload evidence serializes");
        assert_eq!(wire.as_object().expect("evidence object").len(), 2);
        assert!(wire.get("byte_len").is_some());
        assert!(wire.get("sha256").is_some());
        let mut parsed: LanePayload = serde_json::from_value(wire).expect("evidence parses");
        assert!(parsed.is_empty());
        assert_eq!(parsed, payload);
        assert!(parsed.attach(b"other bytes").is_err());
        parsed.attach(&payload).expect("matching retained image");
        assert_eq!(parsed.as_slice(), payload.as_slice());
    }
}

/// The retained image belongs to a lane identity, outside the document.
pub(crate) fn retained_id(
    lane: &str,
) -> Result<cadmpeg_ir::ids::UnknownId, cadmpeg_core::CodecError> {
    cadmpeg_ir::ids::UnknownId::mint(format!(
        "sldprt:source:feature-input#{}",
        Sha256Digest::digest(lane.as_bytes())
    ))
    .map_err(cadmpeg_core::CodecError::malformed)
}
