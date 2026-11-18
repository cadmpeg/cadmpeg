// SPDX-License-Identifier: Apache-2.0

use super::super::work_geometry::direct_joint_origin_frame;
use super::bind_joint_origin_frames_from_assemblies;
use crate::design::test_support::indexed_record_offsets_for_test;
use crate::layout::{
    joint_origin_direct_carrier_369 as carrier, joint_origin_scope_carrier_separation,
};
use crate::records::feature::assembly::DesignAssemblyAlignment;
use crate::records::feature::scope::{
    DesignJointOriginReference, DesignParameterScope, DesignScopePayload,
};
use crate::records::identity::{Located, ReferenceRun};
use crate::records::sketch_placement::SketchPlacementMatrix;
use crate::test_support::{indexed_header, push_marked_reference, write_marked_reference};
use cadmpeg_core::decode::u64_from_index;
use std::collections::{HashMap, HashSet};

#[test]
fn assembly_envelope_keeps_a_registered_joint_origin_with_either_matrix() {
    const ASSEMBLY: u32 = 3;
    const CONSTRUCTION: u32 = 90;
    const ORIGIN: u32 = 91;
    const CARRIER: u32 = ORIGIN + 1;
    const ASSEMBLY_LENGTH: usize = 604;
    const EPS: f64 = 1.0e-12;

    let alignment = DesignAssemblyAlignment::try_new(
        0.0,
        [0.0; 3],
        (0_u32..4)
            .map(|ordinal| Located {
                value: 60 + ordinal,
                offset: 4096 + u64::from(ordinal) * 128,
            })
            .collect(),
        None,
    )
    .unwrap();
    let mut assembly = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#3",
        DesignScopePayload::Assemble(Some(alignment)),
        ASSEMBLY,
    );
    assembly.class_tag = "276".to_owned().try_into().unwrap();
    assembly.paired_class_tag = "258".to_owned().try_into().unwrap();
    assembly
        .try_edit(|draft| {
            draft.frame_length = u64_from_index(ASSEMBLY_LENGTH);
            draft.paired_byte_offset = draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut origin = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#91",
        DesignScopePayload::JointOrigin(None),
        ORIGIN,
    );
    origin
        .try_edit(|draft| {
            draft.byte_offset = 768;
            draft.frame_length = 315;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = draft.byte_offset + 132;
            draft.reference_members = ReferenceRun::unlocated(vec![CARRIER, CONSTRUCTION]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let start = usize::try_from(origin.paired_byte_offset()).unwrap()
        + joint_origin_scope_carrier_separation::LEN;
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
    bytes.resize(ASSEMBLY_LENGTH, 0);
    indexed_header(&mut bytes, *b"258", ASSEMBLY);
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
    let types = HashMap::from([
        (
            u64::from(ORIGIN),
            ("02E8AA49-08B8-439C-A8C7-221BE20143D8", 2),
        ),
        (
            u64::from(CARRIER),
            ("7B6C3C2D-4096-4E6D-B225-699E8C2D9355", 6),
        ),
    ]);

    let direct_origins = HashSet::from([origin.byte_offset()]);
    for x in [0.0, 2.0] {
        let mut matrix = SketchPlacementMatrix::IDENTITY.rows();
        matrix[0][3] = x;
        for (lane, value) in bytes[start + carrier::MATRIX..]
            .chunks_exact_mut(size_of::<f64>())
            .zip(matrix.into_iter().flatten())
        {
            lane.copy_from_slice(&value.to_le_bytes());
        }
        let records = indexed_record_offsets_for_test(&bytes);
        let frame = direct_joint_origin_frame(&bytes, &records, &origin, &types).unwrap();
        let mut scopes = [assembly.clone(), origin.clone()];
        scopes[1].with_joint_origin_transform(frame.transform);
        let placement = scopes[1].joint_origin_frame_mut().unwrap();
        placement.joint_origin_transform_offset = frame.transform_offset;
        placement.reference =
            frame
                .reference
                .map(|(record_index, offset)| DesignJointOriginReference {
                    joint_origin_reference: record_index,
                    joint_origin_reference_offset: offset,
                });
        bind_joint_origin_frames_from_assemblies(&bytes, &mut scopes, &direct_origins);

        let alignment = scopes[0].assembly_alignment().unwrap();
        assert_eq!(
            alignment.joint_origin_scope_record_index(),
            Some(ORIGIN),
            "carrier X = {x}"
        );
        assert_eq!(
            scopes[1].joint_origin_transform_offset(),
            Some(frame.transform_offset)
        );
        let actual_x = scopes[1].joint_origin_transform().unwrap().rows()[0][3];
        assert!((actual_x - x).abs() < EPS);
    }
}
