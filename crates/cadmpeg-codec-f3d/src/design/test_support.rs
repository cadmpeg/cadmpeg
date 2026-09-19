// SPDX-License-Identifier: Apache-2.0
//! Shared helpers for design-owner unit tests.

use crate::design::decode::parameters::design_parameter_discriminator;
use crate::test_support::lp_utf16;

pub(crate) fn parameter_record(
    owner: Option<u32>,
    expression: &str,
    source_kind: &str,
    unit: Option<&str>,
    name: &str,
    evaluated_value: f64,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(b"305");
    out.extend_from_slice(&71u32.to_le_bytes());
    out.extend_from_slice(&[0; 11]);
    out.extend_from_slice(&design_parameter_discriminator(source_kind).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&9u32.to_le_bytes());
    match owner {
        Some(owner) => {
            out.push(1);
            out.extend_from_slice(&owner.to_le_bytes());
            out.extend_from_slice(&[0; 6]);
        }
        None => out.push(0),
    }
    lp_utf16(&mut out, expression);
    out.extend_from_slice(if owner.is_some() {
        &[0; 9]
    } else {
        &[0, 0, 0, 0, 0, 0, 0, 0, 1]
    });
    lp_utf16(&mut out, source_kind);
    out.extend_from_slice(&0u32.to_le_bytes());
    if let Some(unit) = unit {
        lp_utf16(&mut out, unit);
    }
    lp_utf16(&mut out, name);
    out.extend_from_slice(&evaluated_value.to_le_bytes());
    out.extend_from_slice(&[0, 1, 19, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out
}

pub(crate) fn parameter_owner_frame() -> Vec<u8> {
    let mut frame = vec![0; 104];
    frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    frame[4..7].copy_from_slice(b"292");
    frame[7..11].copy_from_slice(&44u32.to_le_bytes());
    frame[19] = 1;
    frame[20..24].copy_from_slice(&1u32.to_le_bytes());
    frame[24] = 1;
    frame[25..29].copy_from_slice(&12u32.to_le_bytes());
    frame[35..39].copy_from_slice(&2u32.to_le_bytes());
    frame[40..48].copy_from_slice(&6.0f64.to_le_bytes());
    frame[48] = 1;
    frame[49..53].copy_from_slice(&45u32.to_le_bytes());
    frame[59..63].copy_from_slice(&9u32.to_le_bytes());
    frame[67] = 1;
    frame[68..72].copy_from_slice(&12u32.to_le_bytes());
    frame[78] = 1;
    frame[79] = 1;
    frame[81] = 1;
    frame[82..86].copy_from_slice(&46u32.to_le_bytes());
    frame[93] = 1;
    frame[94..98].copy_from_slice(&12u32.to_le_bytes());
    frame
}

pub(crate) fn identity_matrix() -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(crate) fn design_type(
    type_guid: &str,
    base_type_guid: Option<&str>,
    version: u32,
    module: &str,
    entity_ids: Vec<u64>,
) -> crate::records::entity_header::SegmentType {
    crate::records::entity_header::SegmentType {
        id: String::new(),
        byte_offset: 0,
        type_guid: type_guid.to_owned().try_into().expect("type GUID"),
        type_guid_offset: 0,
        base_type_guid: base_type_guid.map_or(
            crate::records::entity_header::BaseTypeGuid::Absent,
            |value| crate::records::entity_header::BaseTypeGuid::Guid {
                value: value.to_owned().try_into().expect("base GUID"),
                offset: 0,
            },
        ),
        version,
        version_offset: 0,
        module: module.into(),
        entities: crate::records::identity::ReferenceRun::unlocated(entity_ids),
    }
}

pub(crate) fn primary_record(
    entity_id: u64,
    bulk_offset: usize,
) -> crate::metastream::RecordIndexEntry {
    crate::metastream::RecordIndexEntry {
        entity_id,
        bulk_offset: bulk_offset as u64,
    }
}

pub(crate) fn push_reference(out: &mut Vec<u8>, reference: u32) {
    out.push(1);
    out.extend_from_slice(&reference.to_le_bytes());
}

pub(crate) fn push_genesis_block(out: &mut Vec<u8>, genesis: u64) {
    out.push(1);
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&13u32.to_le_bytes());
    out.extend_from_slice(b"EntityGenesis");
    out.extend_from_slice(&23u32.to_le_bytes());
    out.extend_from_slice(b"IntrinsicMetaTypeuint64");
    out.extend_from_slice(&genesis.to_le_bytes());
}

pub(crate) fn assembly_operand_frame_fixture(scope_record_index: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 648];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"273");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[20] = 1;
    bytes[25] = 1;
    for (reference_at, transform_at, reference, translation) in [
        (28, 40, 70_u32, [1.0_f64, 2.0, 3.0]),
        (168, 180, 80_u32, [4.0, 5.0, 6.0]),
    ] {
        bytes[reference_at] = 1;
        bytes[reference_at + 1..reference_at + 5].copy_from_slice(&reference.to_le_bytes());
        for (ordinal, value) in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[transform_at + ordinal * 8..transform_at + ordinal * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[637..641].copy_from_slice(&3_u32.to_le_bytes());
    bytes[641..644].copy_from_slice(b"259");
    bytes[644..648].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes
}
