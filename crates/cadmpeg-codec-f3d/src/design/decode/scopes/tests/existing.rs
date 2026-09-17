// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::super::coil::exact_coil_placement;
use super::super::parameter_scope::parse_parameter_scope;
use super::super::path_feature::exact_path_feature_construction;
use super::super::point_data::exact_work_point_construction;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::coil_compact_persistent_selection_prefix as coil_persist_selection;
use crate::layout::coil_legacy_placement_identity_frame as coil_legacy_identity;
use crate::layout::coil_modern_placement_matrix_frame as coil_modern_matrix;
use crate::records::{
    decal::DesignRecordHeader,
    feature::{
        coil::{DesignCoilExtent, DesignCoilSelection},
        extrude::DesignExtrudeOperation,
        path_features::DesignPathFeatureConstruction,
        scope::DesignParameterScope,
        work_geometry::DesignWorkPointRule,
    },
    recipes::{ConstructionRecipe, ConstructionRecipeKind},
};
use std::collections::HashMap;

fn lp_utf16(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for unit in units {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
}

fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

#[test]
fn compact_loft_prefix_reads_operation_at_offset_25_for_any_dynamic_class_tag() {
    for class_tag in ["301", "449"] {
        let mut bytes = Vec::new();
        let class_tag_bytes = class_tag
            .as_bytes()
            .try_into()
            .expect("three-byte class tag");
        indexed_header(&mut bytes, class_tag_bytes, 20);
        bytes.resize(128, 0);
        bytes[21..25].fill(1);
        bytes[25..29].copy_from_slice(&1u32.to_le_bytes());
        bytes[30..34].fill(0xff);

        let mut scope = DesignParameterScope::empty(
            "generated:loft#20",
            crate::records::feature::scope::DesignFeatureKind::Loft,
            20,
        );
        scope.class_tag =
            crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = 128;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let construction = exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[],
        )
        .expect("compact Loft operation");
        assert_eq!(
            construction,
            DesignPathFeatureConstruction::Loft(
                crate::records::feature::path_features::DesignLoftConstruction {
                    operation: DesignExtrudeOperation::Join,
                    operation_offset: 25,
                }
            )
        );

        bytes[24] = 0;
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                &scope,
                &[],
            ),
            None
        );
    }
}

fn compact_coil_placement_fixture(
    matrix: Option<[[f64; 4]; 4]>,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = Vec::new();
    let selection_record_index = 100;
    let transform_record_index = 200;
    indexed_header(&mut bytes, *b"333", selection_record_index);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(selection_record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"265", selection_record_index);
    indexed_header(&mut bytes, *b"301", selection_record_index + 1);
    indexed_header(&mut bytes, *b"446", selection_record_index + 2);
    indexed_header(&mut bytes, *b"429", selection_record_index + 3);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    indexed_header(&mut bytes, *b"311", selection_record_index + 4);

    let transform_start = bytes.len();
    indexed_header(&mut bytes, *b"270", transform_record_index);
    let frame_length = if matrix.is_some() { 341 } else { 213 };
    bytes.resize(transform_start + frame_length, 0);
    bytes[transform_start + 55] = 1;
    if let Some(matrix) = matrix {
        bytes[transform_start + 65] = 0;
        for (ordinal, value) in matrix.into_iter().flatten().enumerate() {
            let offset = transform_start + 66 + ordinal * 8;
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
    } else {
        bytes[transform_start + 65] = 1;
    }
    indexed_header(&mut bytes, *b"259", transform_record_index);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 442;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                selection_record_index,
                transform_record_index,
                300,
                301,
                302,
                303,
                304,
                305,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn modern_coil_matrix_placement_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    fn marked(bytes: &mut [u8], offset: usize, record_index: u32) {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
        bytes[offset + 5..offset + 11].fill(0);
    }

    fn u64_at(bytes: &mut [u8], offset: usize, value: u64) {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    let (mut bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    bytes.insert(coil_persist_selection::NESTED_SELECTION_MARKER, 0);
    let transform_start = transform_start + 1;
    bytes[4..7].copy_from_slice(b"286");
    bytes.truncate(transform_start);
    indexed_header(&mut bytes, *b"450", 200);
    bytes.resize(transform_start + coil_modern_matrix::LEN, 0);
    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let offset = transform_start + coil_modern_matrix::MATRIX + ordinal * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[transform_start + coil_modern_matrix::CONSTANT_512
        ..transform_start + coil_modern_matrix::CONSTANT_512 + 4]
        .copy_from_slice(&512u32.to_le_bytes());
    bytes[transform_start + coil_modern_matrix::CONSTANT_256
        ..transform_start + coil_modern_matrix::CONSTANT_256 + 4]
        .copy_from_slice(&256u32.to_le_bytes());
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::SELECTION_REFERENCE,
        100,
    );
    bytes[transform_start + coil_modern_matrix::SELECTION_FLAG
        ..transform_start + coil_modern_matrix::SELECTION_FLAG + 4]
        .copy_from_slice(&1u32.to_le_bytes());
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::AUXILIARY_REFERENCE,
        225,
    );
    u64_at(
        &mut bytes,
        transform_start + coil_modern_matrix::CONSTANT_1024,
        1024,
    );
    u64_at(
        &mut bytes,
        transform_start + coil_modern_matrix::IDENTITY_LANE_PREFIX,
        0x7000_0000_0000_0000,
    );
    u64_at(
        &mut bytes,
        transform_start + coil_modern_matrix::IDENTITY_LANE,
        0x703e_0000_0000_0001,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::SUCCESSOR_REFERENCE,
        202,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::PREDECESSOR_REFERENCE,
        201,
    );
    marked(
        &mut bytes,
        transform_start + coil_modern_matrix::OWNER_REFERENCE,
        scope.record_index,
    );
    indexed_header(&mut bytes, *b"259", 200);
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("353".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 427;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn compact_coil_owner_identity_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (mut bytes, scope, transform_start) = compact_coil_placement_fixture(None);
    let paired_start = transform_start + 213;
    let paired = bytes.split_off(paired_start);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&paired);
    (bytes, scope, transform_start)
}

fn legacy_coil_placement_identity_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    fn marked(bytes: &mut [u8], offset: usize, record_index: u32) {
        bytes[offset] = 1;
        bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
        bytes[offset + 5..offset + 11].fill(0);
    }

    fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    let (mut bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    bytes.truncate(transform_start);
    indexed_header(&mut bytes, *b"395", 200);
    bytes.resize(transform_start + coil_legacy_identity::LEN, 0);
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::LEADING_REFERENCE_MARKER,
        0,
    );
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::PROLOGUE_VALUE,
        2,
    );
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::PROLOGUE_FLAG,
        1,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::SELECTION_REFERENCE_MARKER,
        100,
    );
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::SELECTION_FLAG,
        1,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::AUXILIARY_REFERENCE_MARKER,
        350,
    );
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::TAIL_VALUE,
        4,
    );
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::INTERMEDIATE_SELECTOR,
        109,
    );
    bytes[transform_start + coil_legacy_identity::CARRIER_SCALAR
        ..transform_start + coil_legacy_identity::CARRIER_SCALAR + 8]
        .copy_from_slice(&6.64e-5f64.to_le_bytes());
    u32_at(
        &mut bytes,
        transform_start + coil_legacy_identity::TAIL_SELECTOR,
        109,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::SUCCESSOR_REFERENCE_MARKER,
        202,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::PREDECESSOR_REFERENCE_MARKER,
        201,
    );
    marked(
        &mut bytes,
        transform_start + coil_legacy_identity::OWNER_REFERENCE_MARKER,
        scope.record_index,
    );
    indexed_header(&mut bytes, *b"258", 200);
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("393".to_owned()).unwrap();
    scope.paired_class_tag =
        crate::records::references::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 427;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope, transform_start)
}

fn compact_coil_spiral_placement_fixture() -> (Vec<u8>, DesignParameterScope, usize) {
    let (bytes, mut scope, transform_start) = compact_coil_placement_fixture(None);
    scope
        .try_edit(|draft| {
            draft.frame_length = 411;
            draft.reference_members = {
                let mut values: Vec<u32> = draft.reference_members.values().copied().collect();
                values.pop();
                crate::records::identity::ReferenceRun::unlocated(values)
            };
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).coil_extent = Some(
            crate::records::identity::MaybeRecordedValue::Unlocated(DesignCoilExtent::Spiral),
        );
    }
    (bytes, scope, transform_start)
}

fn compact_coil_face_selection_fixture() -> (Vec<u8>, DesignParameterScope, Vec<ConstructionRecipe>)
{
    let mut bytes = Vec::new();
    let selection_record_index = 100;
    let transform_record_index = 200;
    indexed_header(&mut bytes, *b"333", selection_record_index);
    bytes.extend_from_slice(&[0; 12]);
    bytes.push(1);
    bytes.extend_from_slice(&(selection_record_index + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 3]);
    lp_utf16(&mut bytes, "11111111-1111-4111-8111-111111111111");
    lp_utf16(&mut bytes, "22222222-2222-4222-8222-222222222222");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"265", selection_record_index);
    indexed_header(&mut bytes, *b"301", selection_record_index + 1);
    indexed_header(&mut bytes, *b"446", selection_record_index + 2);
    indexed_header(&mut bytes, *b"429", selection_record_index + 3);
    bytes.extend_from_slice(&16u32.to_le_bytes());
    let recipe_byte_offset = bytes.len();
    bytes.extend_from_slice(b"face_recipe_data");
    bytes.extend_from_slice(&0i32.to_le_bytes());
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    let transform_start = bytes.len();

    indexed_header(&mut bytes, *b"270", transform_record_index);
    bytes.resize(transform_start + 341, 0);
    bytes[transform_start + 55] = 1;
    bytes[transform_start + 65] = 0;
    let transform: [[f64; 4]; 4] = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let offset = transform_start + 66 + ordinal * 8;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"259", transform_record_index);

    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#42",
        crate::records::feature::scope::DesignFeatureKind::CoilPrimitive,
        42,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 432;
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![
                selection_record_index,
                transform_record_index,
                300,
                301,
                302,
                303,
                304,
                305,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let recipes = vec![ConstructionRecipe {
        id: format!("{stream}:construction-recipe#{recipe_byte_offset}"),
        byte_offset: recipe_byte_offset as u64,
        kind: ConstructionRecipeKind::Face,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "body".into(),
                offset: 0,
            },
            selector: None,
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 103,
            offset: 0,
        }),
    }];
    (bytes, scope, recipes)
}

#[test]
fn compact_coil_placement_accepts_identity_and_matrix_frames() {
    let explicit = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, (matrix, expected_offset)) in [(None, None), (Some(explicit), Some(66))]
        .into_iter()
        .enumerate()
    {
        let (bytes, mut scope, transform_start) = compact_coil_placement_fixture(matrix);
        scope
            .try_edit(|draft| {
                draft.frame_length = if ordinal == 0 { 432 } else { 442 };
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let records = IndexedRecordOffsets::build(&bytes);
        let placement =
            exact_coil_placement(&bytes, &records, &scope, &[]).expect("compact Coil placement");
        assert_eq!(placement.selection_record_index, 100);
        assert_eq!(placement.transform_record_index, 200);
        assert_eq!(
            placement.selection,
            DesignCoilSelection::Persistent {
                asset_id: "11111111-1111-4111-8111-111111111111"
                    .to_owned()
                    .try_into()
                    .expect("GUID"),
                context_id: "22222222-2222-4222-8222-222222222222"
                    .to_owned()
                    .try_into()
                    .expect("GUID"),
                identity_record_index: 103,
                primary_identity: 1331,
                secondary: Some(crate::records::identity::DesignSecondaryIdentity {
                    identity: 183,
                    curve_identity: None
                }),
            }
        );
        assert_eq!(
            placement.explicit_transform.map(|matrix| matrix.offset),
            expected_offset.map(|offset| (transform_start + offset) as u64)
        );
        assert_eq!(
            *placement.transform(),
            matrix
                .unwrap_or([
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ])
                .try_into()
                .unwrap()
        );
    }
}

#[test]
fn modern_coil_placement_accepts_class_450_matrix_frame() {
    let (bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    let placement = exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[])
        .expect("modern Coil matrix placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.selection_class_tag.as_str(), "286");
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(placement.transform_class_tag.as_str(), "450");
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        Some((transform_start + coil_modern_matrix::MATRIX) as u64)
    );
}

#[test]
fn modern_coil_placement_requires_exact_class_450_matrix_carrier() {
    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::CONSTANT_512 + 1] = 0;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::IDENTITY_LANE + 7] = 0;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (mut bytes, scope, transform_start) = modern_coil_matrix_placement_fixture();
    bytes[transform_start + coil_modern_matrix::OWNER_REFERENCE + 1] = 0;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );
}

#[test]
fn compact_coil_placement_accepts_owner_referenced_identity_frame() {
    let (bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    let placement = exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[])
        .expect("owner-referenced compact Coil placement");
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        None
    );
    assert_eq!(
        placement.transform_record_byte_offset,
        transform_start as u64
    );
}

#[test]
fn legacy_coil_placement_accepts_identity_frame() {
    let (bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    let placement = exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[])
        .expect("legacy Coil placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(
        placement.explicit_transform.map(|matrix| matrix.offset),
        None
    );
    assert_eq!(
        placement.transform_record_byte_offset,
        transform_start as u64
    );
    assert_eq!(
        *placement.transform(),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );
}

#[test]
fn legacy_coil_placement_requires_exact_identity_carrier() {
    let (mut bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    bytes[transform_start + coil_legacy_identity::TAIL_VALUE] = 5;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (bytes, mut scope, _) = legacy_coil_placement_identity_fixture();
    scope.class_tag =
        crate::records::references::DesignClassTag::try_from("432".to_owned()).unwrap();
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (mut bytes, scope, transform_start) = legacy_coil_placement_identity_fixture();
    bytes[transform_start + coil_legacy_identity::SUCCESSOR_RECORD_INDEX] = 203;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );
}

#[test]
fn compact_coil_spiral_placement_accepts_seven_reference_form() {
    let (bytes, scope, transform_start) = compact_coil_spiral_placement_fixture();
    let placement = exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[])
        .expect("seven-reference compact Coil spiral placement");
    assert_eq!(placement.selection_record_index, 100);
    assert_eq!(placement.transform_record_index, 200);
    assert_eq!(
        placement.transform_record_byte_offset,
        transform_start as u64
    );
}

#[test]
fn compact_coil_seven_reference_form_requires_spiral_extent() {
    let (bytes, mut scope, _) = compact_coil_spiral_placement_fixture();
    if let crate::records::feature::scope::DesignScopePayloadMut::SpirePrimitive(slot)
    | crate::records::feature::scope::DesignScopePayloadMut::CoilPrimitive(slot) =
        scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).coil_extent =
            Some(crate::records::identity::MaybeRecordedValue::Unlocated(
                DesignCoilExtent::RevolutionsHeight,
            ));
    }
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );
}

#[test]
fn compact_coil_placement_rejects_ambiguous_or_reflected_frames() {
    let (mut bytes, scope, transform_start) = compact_coil_placement_fixture(Some([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.7],
        [0.0, 0.0, 0.0, 1.0],
    ]));
    let matrix_value_offset = transform_start + 66 + 10 * 8;
    bytes[matrix_value_offset..matrix_value_offset + 8].copy_from_slice(&(-1.0f64).to_le_bytes());
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (mut bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    bytes[transform_start + 65] = 0;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );

    let (mut bytes, scope, transform_start) = compact_coil_owner_identity_fixture();
    bytes[transform_start + 223] ^= 1;
    assert_eq!(
        exact_coil_placement(&bytes, &IndexedRecordOffsets::build(&bytes), &scope, &[]),
        None
    );
}

#[test]
fn compact_coil_placement_accepts_face_recipe_selection() {
    let (bytes, scope, recipes) = compact_coil_face_selection_fixture();
    let placement = exact_coil_placement(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &recipes,
    )
    .expect("compact Coil face placement");
    assert_eq!(
        placement.selection,
        DesignCoilSelection::FaceRecipe {
            asset_id: "11111111-1111-4111-8111-111111111111"
                .to_owned()
                .try_into()
                .expect("GUID"),
            context_id: "22222222-2222-4222-8222-222222222222"
                .to_owned()
                .try_into()
                .expect("GUID"),
            recipe_record_index: 103,
            recipe_record_byte_offset: recipes[0].byte_offset - 15,
            recipe_id: recipes[0].id.clone(),
            recipe_kind: crate::records::feature::scope::DesignFaceRecipeKind::Face,
            design: Some(crate::records::recipes::ConstructionRecipeDesign {
                id: "body".into(),
                selector: None
            }),
        }
    );
}

/// A `WorkPoint` scope record, its paired header, and one point-data record
/// frame: the indexed header, the payload prologue with an optional property
/// block, the class-level members of `version`, a base-level run of `inputs`
/// references, and the second header that closes the frame.
fn work_point_stream(
    class_tag: &str,
    version: u32,
    property: bool,
    pick_point: Option<u64>,
    position: [f64; 3],
    reference_type: u32,
    inputs: u32,
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"427");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "WorkPoint");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);

    bytes.extend_from_slice(&(class_tag.len() as u32).to_le_bytes());
    bytes.extend_from_slice(class_tag.as_bytes());
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    if property {
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(b"pt_tag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&9u64.to_le_bytes());
    } else {
        bytes.push(0);
    }
    if version >= 2 {
        bytes.extend_from_slice(&0i32.to_le_bytes());
    }
    for _ in 0..2 {
        bytes.extend_from_slice(&f64::to_le_bytes(0.0));
    }
    if version >= 1 {
        match pick_point {
            Some(target) => {
                bytes.push(1);
                bytes.extend_from_slice(&target.to_le_bytes());
                bytes.extend_from_slice(&[0, 0]);
            }
            None => bytes.push(0),
        }
    }
    let position_at = bytes.len();
    for value in position {
        bytes.extend_from_slice(&f64::to_le_bytes(value));
    }
    bytes.extend_from_slice(&reference_type.to_le_bytes());
    if version >= 3 {
        for _ in 0..3 {
            bytes.extend_from_slice(&f64::to_le_bytes(-1.0));
        }
    }
    bytes.extend_from_slice(&inputs.to_le_bytes());
    for input in 0..inputs {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(70 + input).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&55u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: crate::records::references::DesignClassTag::try_from("427".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("WorkPoint scope");
    (bytes, scope, position_at)
}

#[test]
fn work_point_position_survives_a_property_block_and_a_present_pick_point() {
    let (bytes, scope, position_at) =
        work_point_stream("282", 3, true, Some(9), [1.25, -2.5, 3.75], 20, 1);

    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(frame.position, [1.25, -2.5, 3.75]);
    assert_eq!(frame.position_offset, position_at as u64);
    assert_eq!(work_point_input_indices(&frame.rule), [70]);
}

#[test]
fn work_point_position_reads_every_class_version_that_stores_one() {
    for version in 0..=3 {
        let (bytes, scope, position_at) =
            work_point_stream("282", version, false, None, [4.0, 5.0, 6.0], 5, 1);

        let frame = exact_work_point_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .unwrap_or_else(|| panic!("class version {version}"));
        assert_eq!(frame.position, [4.0, 5.0, 6.0], "class version {version}");
        assert_eq!(
            frame.position_offset, position_at as u64,
            "class version {version}"
        );
    }
}

#[test]
fn work_point_rejects_a_registered_entity_of_another_type() {
    // The tag says `282`, but the type table names a different class for
    // this entity, so the record is not point data whatever its tag reads.
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [4.0, 5.0, 6.0], 5, 1);
    let records = IndexedRecordOffsets::build(&bytes);

    assert_eq!(
        exact_work_point_construction(
            &bytes,
            &records,
            &scope,
            &HashMap::from([(55, ("A0A15D26-1F3B-4120-A3F1-9CDDA189AB74", 2))])
        ),
        None
    );
}

#[test]
fn work_point_uses_the_serialized_input_count_for_every_rule() {
    // The input count is a member of the point-data level. It frames the
    // run independently of the rule selector, including three-input
    // constructions.
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 1);

    let records = IndexedRecordOffsets::build(&bytes);
    let frame = exact_work_point_construction(&bytes, &records, &scope, &HashMap::new())
        .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70]);
    assert_eq!(frame.rule.reference_type(), 18);
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 14, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71]);

    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 8, 3);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71, 72]);

    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 18, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    assert_eq!(work_point_input_indices(&frame.rule), [70, 71]);
}

#[test]
fn work_point_rule_codes_select_typed_input_arities() {
    for (reference_type, arity) in [(5, 1), (7, 2), (8, 3), (10, 1), (14, 2), (20, 1)] {
        let (bytes, scope, _) = work_point_stream(
            "282",
            2,
            false,
            None,
            [1.0, 2.0, 3.0],
            reference_type,
            arity,
        );
        let frame = exact_work_point_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &HashMap::new(),
        )
        .expect("work point frame");
        assert_eq!(frame.rule.reference_type(), reference_type);
        assert_eq!(u32::try_from(frame.rule.inputs().len()).unwrap(), arity);
        assert!(match frame.rule.form() {
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::CircleCenter { .. } =>
                reference_type == 5,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::TwoEdgeIntersection { .. } =>
                reference_type == 7,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::ThreePlaneIntersection { .. } =>
                reference_type == 8,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::Vertex { .. } => reference_type == 10,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::EdgePlaneIntersection { .. } =>
                reference_type == 14,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::DistanceOnEdge { .. } =>
                reference_type == 20,
            crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native { .. } => false,
        });
    }
}

#[test]
fn work_point_rule_code_with_wrong_arity_remains_native() {
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 5, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");

    assert!(matches!(
        frame.rule.form(),
        crate::records::feature::work_geometry::DesignWorkPointRuleForm::Native {
            reference_type: 5,
            ref inputs,
        } if inputs.len() == 2
    ));
}

#[test]
fn work_point_rule_rejects_an_incompatible_input_carrier() {
    let (bytes, scope, _) = work_point_stream("282", 2, false, None, [1.0, 2.0, 3.0], 14, 2);
    let frame = exact_work_point_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &HashMap::new(),
    )
    .expect("work point frame");
    let mut wire = serde_json::to_value(&frame.rule).expect("serialize valid rule");

    wire["inputs"][1]["carrier"] = serde_json::json!({
        "kind": "edge_recipe", "operand_id": "f3d:native:edge-operand#wrong-role"
    });
    assert!(serde_json::from_value::<DesignWorkPointRule>(wire).is_err());
}

fn work_point_input_indices(rule: &DesignWorkPointRule) -> Vec<u32> {
    rule.inputs()
        .iter()
        .map(crate::records::feature::work_geometry::DesignWorkPointInput::record_index)
        .collect()
}
