// SPDX-License-Identifier: Apache-2.0
//! Complete owner frames, occurrence coordinates, and byte-consistent fields.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use crate::families::b2::records::b2_owner_packet_from_frame;
use crate::wire::bytes::{allocation_reference, compact_int, persistent_ref};
use crate::wire::records::{ConsolidatedFrame, ConsolidatedRawFrame, ConsolidatedRecord};

use super::projection::native_allocation_reference_encoding;
use super::{
    CatiaConsolidatedOwnerPacket, CatiaFaceNodeRelation, CatiaFaceNodeTargetEncoding,
    CatiaOwnerPacketPayload,
};

pub(super) fn source_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record: &ConsolidatedRecord,
) -> Result<ConsolidatedRawFrame<u64>, CodecError> {
    let payload = record
        .payload()
        .and_then(|range| bytes.get(range))
        .ok_or_else(|| CodecError::malformed("owner occurrence has no complete physical frame"))?;
    Ok(ConsolidatedRawFrame::from_record(
        record,
        ctx.copy_slice(payload, "catia_native_owner_frame_payload")?,
    )?
    .into())
}

pub(super) fn face_node(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    record: &ConsolidatedRecord,
    node: crate::families::b2::records::B2FaceNode5f,
) -> Result<CatiaFaceNodeRelation, CodecError> {
    Ok(CatiaFaceNodeRelation {
        source_offset: u64_from_index(record.source_range().start),
        frame: source_frame(ctx, bytes, record)?,
        target_encoding: match node.target_encoding {
            crate::families::b2::records::B2FaceNode5fTargetEncoding::Compact => {
                CatiaFaceNodeTargetEncoding::Compact
            }
            crate::families::b2::records::B2FaceNode5fTargetEncoding::TaggedU16Strong => {
                CatiaFaceNodeTargetEncoding::TaggedU16Strong
            }
        },
        target: node.target,
        terminal: node.terminal.into(),
    })
}

pub(super) fn valid_face_node(node: &CatiaFaceNodeRelation) -> bool {
    let payload = &node.frame.payload;
    if payload.len() > usize::from(u8::MAX) || payload.first() != Some(&0x82) {
        return false;
    }
    let mut at = 1;
    let parsed = if payload.get(at) == Some(&0x0a) {
        persistent_ref(payload, &mut at)
            .map(|value| (CatiaFaceNodeTargetEncoding::TaggedU16Strong, value))
    } else {
        compact_int(payload, &mut at).map(|value| (CatiaFaceNodeTargetEncoding::Compact, value))
    };
    parsed == Some((node.target_encoding, node.target))
        && payload.get(at..) == Some(node.terminal.as_slice())
}

pub(super) fn valid_owner_packet(packet: &CatiaConsolidatedOwnerPacket) -> bool {
    let payload = &packet.frame.payload;
    if payload.len() > usize::from(u8::MAX) {
        return false;
    }
    let frame = ConsolidatedFrame {
        pos: 0,
        payload: 0,
        end: payload.len(),
        header_token: packet.frame.header_token(),
    };
    let fixed = b2_owner_packet_from_frame(payload, frame, packet.source_index);
    let valid_payload = match (&packet.payload, fixed) {
        (
            CatiaOwnerPacketPayload::FixedNine {
                reference_encoding,
                references,
                identity_encodings,
                numeric_tail,
                ..
            },
            Some(parsed),
        ) => {
            *reference_encoding == parsed.reference_encoding.into()
                && *references == parsed.references
                && *identity_encodings == parsed.identity_encodings.map(Into::into)
                && *numeric_tail == parsed.numeric_tail
        }
        (
            CatiaOwnerPacketPayload::Counted {
                references,
                reference_encodings,
                tail,
            },
            None,
        ) => {
            if references.is_empty()
                || references.len() > 127
                || references.len() != reference_encodings.len()
                || payload.first().copied().map(usize::from) != Some(128 + references.len())
            {
                return false;
            }
            let mut at = 1;
            references
                .iter()
                .zip(reference_encodings)
                .all(|(value, encoding)| {
                    allocation_reference(payload, &mut at).is_some_and(|reference| {
                        reference.value == *value
                            && native_allocation_reference_encoding(reference.encoding) == *encoding
                    })
                })
                && payload.get(at..) == Some(tail.as_slice())
        }
        _ => false,
    };
    valid_payload
        && packet.face_node.as_ref().is_none_or(|node| {
            let length = 4
                + u64::from(u8::from(node.frame.width()))
                + u64_from_index(node.frame.payload.len());
            let dialect = match &packet.payload {
                CatiaOwnerPacketPayload::Counted { references, .. } => {
                    references.len() >= 5
                        && references.len() % 2 == 1
                        && node.terminal.as_slice() == [0x03, 0x05]
                }
                CatiaOwnerPacketPayload::FixedNine {
                    reference_encoding, ..
                } => {
                    node.terminal.as_slice() == [0x03, 0x05]
                        || node.terminal.as_slice() == [0x03, 0x03]
                            && *reference_encoding == super::CatiaOwnerReferenceEncoding::AllCompact
                }
            };
            valid_face_node(node)
                && dialect
                && node.source_offset.checked_add(length) == Some(packet.source_offset)
                && node.target.checked_add(1) == packet.payload.final_reference()
        })
}

#[cfg(test)]
mod tests;
