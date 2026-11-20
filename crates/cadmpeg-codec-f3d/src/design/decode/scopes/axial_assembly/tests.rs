// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use super::super::parameter_scope::{decode_parameter_scopes, ParameterScopeSources};
use crate::layout::{
    joint_origin_direct_carrier_369 as carrier, joint_origin_scope_carrier_separation,
};
use crate::records::entity_header::SegmentType;
use crate::records::parameters::{DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::zip_test::with_scan;
use crate::test_support::{
    indexed_header, lp_utf16, push_marked_reference, write_marked_reference,
};
use cadmpeg_core::decode::u64_from_index;

const ASSEMBLY: u32 = 3;
const CONSTRUCTION: u32 = 90;
const ORIGIN: u32 = 91;
const CARRIER: u32 = ORIGIN + 1;
const ASSEMBLY_LENGTH: usize = 604;
const ORIGIN_LENGTH: usize = 315;
const BULK: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
const META: &str = "FusionAssetName[Active]/Design1/MetaStream.dat";
const LOSS_CODE: &str = "assembly.joint-origin-envelope-matrix-discarded";

fn write_scope_tail(bytes: &mut Vec<u8>, end: usize, kind: &str, ordinal: u32, members: &[u32]) {
    const TAIL_LENGTH: usize = 78;
    let kind_length = 4 + 2 * kind.encode_utf16().count();
    bytes.resize(end - TAIL_LENGTH - kind_length - 8 - members.len() * 11, 0);
    bytes.extend_from_slice(&u32::try_from(members.len()).unwrap().to_le_bytes());
    for &member in members {
        push_marked_reference(bytes, member);
    }
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(bytes, kind);
    let mut tail = [0; TAIL_LENGTH];
    tail[..4].copy_from_slice(&ordinal.to_le_bytes());
    tail[31..35].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes.extend_from_slice(&tail);
}

fn scope_archive(owners: &[DesignParameterOwner], carrier_x: f64) -> (Vec<u8>, u64) {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"276", ASSEMBLY);
    bytes.resize(24, 0);
    push_marked_reference(&mut bytes, CONSTRUCTION);
    bytes.push(0);
    for value in SketchPlacementMatrix::IDENTITY.rows().into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    push_marked_reference(&mut bytes, ORIGIN);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    let assembly_members: Vec<_> = [CONSTRUCTION, ORIGIN]
        .into_iter()
        .chain(owners.iter().map(DesignParameterOwner::record_index))
        .collect();
    write_scope_tail(
        &mut bytes,
        ASSEMBLY_LENGTH,
        "Assemble",
        1,
        &assembly_members,
    );
    indexed_header(&mut bytes, *b"258", ASSEMBLY);

    let origin_end = bytes.len() + ORIGIN_LENGTH;
    indexed_header(&mut bytes, *b"276", ORIGIN);
    write_scope_tail(&mut bytes, origin_end, "JointOrigin", 2, &[CARRIER]);
    indexed_header(&mut bytes, *b"258", ORIGIN);
    let start = origin_end + joint_origin_scope_carrier_separation::LEN;
    bytes.resize(start, 0);
    indexed_header(&mut bytes, *b"153", CARRIER);
    bytes.resize(start + carrier::SCOPE_BACKLINK, 0);
    push_marked_reference(&mut bytes, ORIGIN);
    indexed_header(&mut bytes, *b"249", CARRIER);
    indexed_header(&mut bytes, *b"301", CONSTRUCTION);
    write_marked_reference(
        &mut bytes,
        start + carrier::CONSTRUCTION_REFERENCE,
        CONSTRUCTION,
    );
    let matrix_at = start + carrier::MATRIX;
    let mut matrix = SketchPlacementMatrix::IDENTITY.rows();
    matrix[0][3] = carrier_x;
    for (lane, value) in bytes[matrix_at..]
        .chunks_exact_mut(size_of::<f64>())
        .zip(matrix.into_iter().flatten())
    {
        lane.copy_from_slice(&value.to_le_bytes());
    }

    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(BULK, stored).unwrap();
    zip.write_all(&bytes).unwrap();
    (
        zip.finish().unwrap().into_inner(),
        u64_from_index(matrix_at),
    )
}

#[test]
fn assembly_envelope_keeps_a_registered_joint_origin_with_either_matrix() {
    const EPS: f64 = 1.0e-12;
    let types = [
        ("02E8AA49-08B8-439C-A8C7-221BE20143D8", 2, ORIGIN),
        ("7B6C3C2D-4096-4E6D-B225-699E8C2D9355", 6, CARRIER),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (guid, version, record))| {
        let mut row = crate::design::test_support::design_type(
            guid,
            None,
            version,
            "Fusion",
            vec![u64::from(record)],
        );
        row.byte_offset = u64_from_index(ordinal) * 100;
        row.type_guid_offset = row.byte_offset + 4;
        row.version_offset = row.byte_offset + 44;
        SegmentType::try_new(
            crate::ids::native_design_type_id(META, row.byte_offset),
            row,
        )
        .unwrap()
    })
    .collect::<Vec<_>>();
    let owners: Vec<_> = (0..8)
        .map(|ordinal| {
            let record_index = 50 + ordinal * 3;
            let byte_offset = 2048 + 104 * u64::from(ordinal);
            DesignParameterOwner::try_from(DesignParameterOwnerWire {
                id: crate::design::decode::text::design_record_id(
                    BULK,
                    ":design-parameter-owner#",
                    byte_offset,
                ),
                byte_offset,
                frame_length: 104,
                class_tag: "457".to_owned().try_into().unwrap(),
                record_index,
                scope_record_index: ASSEMBLY,
                local_ordinal: ordinal,
                evaluated_value: 0.0,
                evaluated_value_offset: byte_offset + 40,
                parameter_record_index: record_index + 1,
                owned_ordinal: ordinal,
                variant: Some(0),
                companion_record_index: record_index + 2,
            })
            .unwrap()
        })
        .collect();
    let sources = ParameterScopeSources {
        types: &types,
        parameter_owners: &owners,
        ..Default::default()
    };
    for (x, expected_losses) in [(0.0, 0), (2.0, 1)] {
        let (archive, matrix_at) = scope_archive(&owners, x);
        let decoded = with_scan(&archive, |scan| {
            decode_parameter_scopes(
                &cadmpeg_core::decode::test_support::service_decode_context(),
                scan,
                &sources,
            )
            .unwrap()
        });
        let assembly = decoded
            .scopes
            .iter()
            .find(|scope| scope.record_index == ASSEMBLY)
            .unwrap();
        let origin = decoded
            .scopes
            .iter()
            .find(|scope| scope.record_index == ORIGIN)
            .unwrap();
        assert_eq!(
            assembly
                .assembly_alignment()
                .unwrap()
                .joint_origin_scope_record_index(),
            Some(ORIGIN),
            "carrier X = {x}"
        );
        assert_eq!(origin.joint_origin_transform_offset(), Some(matrix_at));
        assert_eq!(origin.joint_origin_reference(), Some(CONSTRUCTION));
        let actual_x = origin.joint_origin_transform().unwrap().rows()[0][3];
        assert!((actual_x - x).abs() < EPS);
        let losses = decoded
            .losses
            .iter()
            .filter(|note| note.code.local_code() == LOSS_CODE)
            .count();
        assert_eq!(losses, expected_losses);
    }
}
