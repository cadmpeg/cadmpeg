// SPDX-License-Identifier: Apache-2.0
//! Source-frame evidence, with parser bytes retained outside document content.

use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::source_fidelity::SourceEvidence;
use serde::{Deserialize, Serialize};

/// Runtime parser storage. Its wire representation is the shared, schema-backed
/// `SourceEvidence`; source bytes never serialize into the native document.
#[derive(Debug, Clone)]
pub(crate) struct SourceFrame {
    evidence: SourceEvidence,
    bytes: Option<ParserFrame>,
}

/// Transient source parser storage with no serialization implementation.
#[derive(Debug, Clone)]
struct ParserFrame(Vec<u8>);

impl Serialize for SourceFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.evidence.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SourceFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            evidence: SourceEvidence::deserialize(deserializer)?,
            bytes: None,
        })
    }
}

impl PartialEq for SourceFrame {
    fn eq(&self, other: &Self) -> bool {
        self.evidence == other.evidence
    }
}
impl Eq for SourceFrame {}

impl From<Vec<u8>> for SourceFrame {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            evidence: SourceEvidence::of(&bytes),
            bytes: Some(ParserFrame(bytes)),
        }
    }
}

impl SourceFrame {
    pub(crate) fn byte_len(&self) -> u64 {
        self.evidence.length
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.evidence.length == 0
    }
    pub(crate) fn data(&self) -> Option<&[u8]> {
        self.bytes.as_ref().map(|frame| frame.0.as_slice())
    }
    pub(crate) fn digest(&self) -> &Sha256Digest {
        &self.evidence.sha256
    }

    /// Retain actual parser bytes and their source location. A document parsed
    /// from CADIR supplies neither bytes nor a location; no frame is fabricated.
    pub(crate) fn retain(
        &self,
        fidelity: &mut cadmpeg_ir::SourceFidelity,
        id: &str,
        stream: &str,
        offset: Option<u64>,
        untyped: cadmpeg_ir::document::ArenaClasses,
    ) -> Result<(), cadmpeg_core::CodecError> {
        let (Some(bytes), Some(offset)) = (&self.bytes, offset) else {
            return Ok(());
        };
        if self.byte_len() != cadmpeg_core::decode::u64_from_index(bytes.0.len())
            || self.digest() != &Sha256Digest::digest(&bytes.0)
        {
            return Err(cadmpeg_core::CodecError::malformed(
                "source frame bytes disagree with their typed evidence",
            ));
        }
        let retained = cadmpeg_ir::RetainedSourceRecord::from_bytes(
            stream,
            offset,
            cadmpeg_ir::source_fidelity::RetainedBytes::Inline {
                data: bytes.0.clone(),
            },
        )?
        .with_untyped(untyped);
        fidelity
            .insert_retained_record(
                id.try_into().map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format!("source frame identity: {error}"))
                })?,
                retained,
            )
            .map_err(|error| cadmpeg_core::CodecError::malformed(error.to_string()))
    }
}

rewrite_native_scalar!(SourceFrame);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn document_round_trip_keeps_evidence_without_reconstituting_parser_bytes() {
        let frame = SourceFrame::from(b"opaque source frame".to_vec());
        let wire = serde_json::to_value(&frame).unwrap();
        assert_eq!(wire.as_object().unwrap().len(), 2);
        assert_eq!(wire["length"], 19);
        let restored: SourceFrame = serde_json::from_value(wire).unwrap();
        assert_eq!(frame, restored);
        assert!(restored.data().is_none());
        assert!(!restored.is_empty());
        let mut fidelity = cadmpeg_ir::SourceFidelity::for_decode(
            &cadmpeg_test_support::service_decode_context(),
            cadmpeg_ir::Annotations::default(),
        );
        restored
            .retain(
                &mut fidelity,
                "f3d:source:frame#one",
                "Design",
                None,
                cadmpeg_ir::ArenaClasses::default(),
            )
            .unwrap();
        assert!(fidelity.retained_records().is_empty());
    }
    #[test]
    fn retained_parser_bytes_charge_only_declared_untyped_content() {
        let frame = SourceFrame::from(vec![1, 2, 3]);
        let mut fidelity = cadmpeg_ir::SourceFidelity::for_decode(
            &cadmpeg_test_support::service_decode_context(),
            cadmpeg_ir::Annotations::default(),
        );
        frame
            .retain(
                &mut fidelity,
                "f3d:source:frame#one",
                "ASM",
                Some(7),
                [cadmpeg_ir::document::ArenaClass::Design].into(),
            )
            .unwrap();
        let record = fidelity.retained_record("f3d:source:frame#one").unwrap();
        assert_eq!(record.data(), Some(&[1, 2, 3][..]));
        assert_eq!(record.offset(), 7);
        assert!(fidelity
            .unrepresented()
            .contains(&cadmpeg_ir::document::ArenaClass::Design));
    }
}
