// SPDX-License-Identifier: Apache-2.0
//! Compact Parasolid offset-surface carriers.

use std::collections::HashMap;

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::FiniteReal;

use super::LEN_TO_MM;

use crate::layout::offset_surface_00_3c as offset_surf;

const TAG: [u8; 2] = [0x00, 0x3c];
const COMMON_REFERENCE_COUNT: usize = 5;

/// One exact offset-surface construction, keyed by its stream-local attribute.
#[derive(Debug, Clone, Copy)]
pub(super) struct OffsetCarrier {
    /// Attribute of the support-surface carrier.
    pub(super) support: u16,
    /// Signed offset distance in millimetres.
    pub(super) distance: FiniteReal,
    /// Byte offset of the `00 3c` tag.
    pub(super) offset: usize,
    pub(super) transferable: bool,
}

fn parse_payload(
    body: &[u8],
    tail: usize,
    tripled_support: bool,
    offset: usize,
) -> Option<OffsetCarrier> {
    let discriminator = *body.get(tail)?;
    matches!(discriminator, b'V' | b'I' | b'U').then_some(())?;
    match body.get(tail + (offset_surf::TRUE_OFFSET - offset_surf::DISCRIMINATOR))? {
        0 | 1 => {}
        _ => return None,
    }
    let support_at = tail.checked_add(offset_surf::SUPPORT - offset_surf::DISCRIMINATOR)?;
    let support = View::u16_be_at(body, support_at)?;
    (support > 1 && support <= i16::MAX.cast_unsigned()).then_some(())?;
    if tripled_support && body.get(support_at + 2) != Some(&1) {
        return None;
    }
    let distance_at = if tripled_support {
        support_at + 3
    } else {
        tail + (offset_surf::DISTANCE - offset_surf::DISCRIMINATOR)
    };
    let scale_at = distance_at.checked_add(offset_surf::SCALE - offset_surf::DISTANCE)?;
    View::f64_be_at(body, scale_at)?.is_finite().then_some(())?;
    let distance = View::f64_be_at(body, distance_at)? * LEN_TO_MM;
    Some(OffsetCarrier {
        support,
        distance: FiniteReal::new(distance)?,
        offset,
        transferable: discriminator == b'V' && distance != 0.0,
    })
}

fn parse_at(body: &[u8], offset: usize) -> Option<(u16, OffsetCarrier)> {
    (body.get(offset..offset + TAG.len())? == TAG).then_some(())?;
    let mut header = offset.checked_add(TAG.len())?;
    if body.get(header) == Some(&0xff) {
        header += 1;
    }
    let attr = View::u16_be_at(body, header + offset_surf::ATTR)?;
    (attr > 1 && attr <= i16::MAX.cast_unsigned()).then_some(())?;

    let references = header.checked_add(offset_surf::REFS)?;
    let partition_marker = header.checked_add(offset_surf::MARKER)?;
    let short_references = |stride| {
        (0..COMMON_REFERENCE_COUNT).all(|index| {
            View::u16_be_at(body, references + index * stride)
                .is_some_and(|value| value > 0 && value <= i16::MAX.cast_unsigned())
        })
    };
    let partition = (short_references(2)
        && matches!(body.get(partition_marker), Some(0x2b | 0x2d)))
    .then(|| parse_payload(body, partition_marker + 1, false, offset))
    .flatten();

    let tripled_marker = references.checked_add(COMMON_REFERENCE_COUNT * 3)?;
    let tripled_references = short_references(3)
        && (0..COMMON_REFERENCE_COUNT)
            .all(|index| body.get(references + index * 3 + 2) == Some(&1));
    let tripled = (tripled_references && matches!(body.get(tripled_marker), Some(0x2b | 0x2d)))
        .then(|| parse_payload(body, tripled_marker + 1, true, offset))
        .flatten();

    match (partition, tripled) {
        (Some(carrier), None) | (None, Some(carrier)) => Some((attr, carrier)),
        (None, None) | (Some(_), Some(_)) => None,
    }
}

/// Scan all structurally valid type-60 offset-surface records.
pub(super) fn scan(body: &[u8]) -> HashMap<u16, OffsetCarrier> {
    let mut out = HashMap::new();
    let Some(last_offset) = body.len().checked_sub(1) else {
        return out;
    };
    for offset in 0..last_offset {
        if let Some((attr, carrier)) = parse_at(body, offset) {
            out.insert(attr, carrier);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::TAG;

    fn partition(discriminator: u8, flag: u8, support: u16, distance: f64) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&907u32.to_be_bytes());
        for reference in [1u16; 5] {
            bytes.extend_from_slice(&reference.to_be_bytes());
        }
        bytes.push(b'+');
        bytes.push(discriminator);
        bytes.push(flag);
        bytes.extend_from_slice(&support.to_be_bytes());
        bytes.extend_from_slice(&distance.to_be_bytes());
        bytes.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
        bytes
    }

    fn deltas(distance: f64) -> Vec<u8> {
        let mut bytes = TAG.to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(&12u16.to_be_bytes());
        bytes.extend_from_slice(&907u32.to_be_bytes());
        for reference in [1u16, 1, 1, 1, 1] {
            bytes.extend_from_slice(&reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(b'-');
        bytes.push(b'I');
        bytes.push(0);
        bytes.extend_from_slice(&6u16.to_be_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&distance.to_be_bytes());
        bytes.extend_from_slice(&(-31_415_800_000_000.0f64).to_be_bytes());
        bytes
    }

    #[test]
    fn parses_partition_and_deltas_framing() {
        let partition = super::scan(&partition(b'V', 1, 6, -0.0025));
        let carrier = partition.get(&12).expect("partition offset surface");
        assert_eq!(carrier.support, 6);
        assert!((carrier.distance.get() + 2.5).abs() < 1.0e-12);

        let deltas = super::scan(&deltas(0.0045));
        let carrier = deltas.get(&12).expect("deltas offset surface");
        assert_eq!(carrier.support, 6);
        assert!((carrier.distance.get() - 4.5).abs() < 1.0e-12);
    }

    #[test]
    fn rejects_invalid_fields_and_nonfinite_converted_distance() {
        assert!(super::scan(&partition(b'X', 1, 6, 1.0)).is_empty());
        assert!(super::scan(&partition(b'V', 2, 6, 1.0)).is_empty());
        assert!(super::scan(&partition(b'V', 1, 1, 1.0)).is_empty());
        assert!(super::scan(&partition(b'V', 1, 6, f64::INFINITY)).is_empty());
        assert!(super::scan(&partition(b'V', 1, 6, f64::MAX)).is_empty());

        let mut malformed_deltas = deltas(1.0);
        let support_terminator = malformed_deltas.len() - 17;
        malformed_deltas[support_terminator] = 0;
        assert!(super::scan(&malformed_deltas).is_empty());
    }
    #[test]
    fn scale_lane_is_complete_finite_and_not_a_distance_multiplier() {
        let original = partition(b'V', 0, 6, -0.0025);
        assert_eq!(original.len(), 39);
        for end in 0..original.len() {
            assert!(!super::scan(&original[..end]).contains_key(&12));
        }
        for scale in [-31_415_800_000_000.0f64, 0.0, 0.002_921, -7.4] {
            let mut bytes = original.clone();
            let at = bytes.len() - 8;
            bytes[at..].copy_from_slice(&scale.to_be_bytes());
            let records = super::scan(&bytes);
            let carrier = records.get(&12).unwrap();
            assert_eq!(carrier.distance.get(), -2.5);
            assert!(carrier.transferable);
        }
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = original.clone();
            let at = bytes.len() - 8;
            bytes[at..].copy_from_slice(&scale.to_be_bytes());
            assert!(super::scan(&bytes).is_empty());
        }
        let delta = deltas(0.0045);
        for end in 0..delta.len() {
            assert!(!super::scan(&delta[..end]).contains_key(&12));
        }
    }

    #[test]
    fn invalid_unchecked_and_zero_offsets_keep_claims_without_typed_transfer() {
        for status in *b"VIU" {
            for distance in [-0.0025, 0.0, -0.0] {
                let bytes = partition(status, 0, 6, distance);
                let records = super::scan(&bytes);
                let carrier = records.get(&12).expect("complete native offset claim");
                assert_eq!(carrier.transferable, status == b'V' && distance != 0.0);
            }
        }
    }

    #[test]
    fn short_grammar_refuses_wide_or_null_header_aliases() {
        for at in [2, 8, 10, 12, 14, 16, 21] {
            let mut bytes = partition(b'V', 0, 6, 0.0025);
            bytes[at..at + 2].copy_from_slice(&0x8001u16.to_be_bytes());
            assert!(super::scan(&bytes).is_empty());
        }
        let mut bytes = partition(b'V', 0, 6, 0.0025);
        bytes[8..10].copy_from_slice(&0u16.to_be_bytes());
        assert!(super::scan(&bytes).is_empty());
    }

    #[test]
    fn unused_logical_and_face_independent_sense_do_not_flip_distance() {
        for logical in [0, 1] {
            for sense in *b"+-" {
                let mut bytes = partition(b'V', logical, 6, -0.0025);
                bytes[18] = sense;
                let records = super::scan(&bytes);
                let carrier = records.get(&12).unwrap();
                assert_eq!(carrier.distance.get(), -2.5);
                assert!(carrier.transferable);
            }
        }
    }
}
