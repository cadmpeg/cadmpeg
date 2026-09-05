// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::{
    bind_recipe_reference_candidates, contiguous_i32_program, decode_recipe_references,
    find_dimension_locus_groups, find_dimension_locus_pair, find_dimension_null_locus_pair,
    following_dimension_companion_record_index, indexed_record_containing,
    is_grouped_recipe_reference_frame, is_paired_recipe_reference_frame,
    parse_dimension_annotation_frame, parse_dimension_locus_group, parse_dimension_locus_pair,
    parse_dimension_null_locus_pair, parse_dimension_presentation_frame, recipe_record_prefix,
};
use crate::design::decode::parameters::parse_design_parameter;
use crate::design::dimensions::{
    bind_dimension_loci, null_locus_dimension_definition, remove_dimension_frame_relations,
};
use crate::design::test_support::{parameter_record, push_genesis_block, push_reference};
use crate::records::{
    DesignDimensionLocus, DesignParameterOwner, PersistentSubentityTag, SketchCurveIdentity,
    SketchPoint,
};
use crate::records::{
    SketchConstraintKind, SketchRelation, SketchRelationKind, SketchRelationMember,
    SketchRelationReturnMember,
};
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::{EdgeId, FaceId};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchAxis, SketchConstraintDefinition, SketchEntity, SketchEntityId, SketchGeometry, SketchId,
};
use std::collections::{HashMap, HashSet};

const TEST_LINEAR_TOLERANCE: f64 = 1.0e-6;

#[test]
fn dimension_recipe_uses_its_immediate_indexed_record_boundary() {
    let mut bytes = vec![0xaa; 5];
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"415");
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let recipe_offset = bytes.len();
    bytes.extend_from_slice(b"edge_recipe_data");
    bytes.extend_from_slice(&[0; 13]);
    let next_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"423");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);

    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), recipe_offset),
        Some((5, "415".into(), 40, next_offset))
    );
    assert_eq!(
        indexed_record_containing(&bytes, 5, bytes.len(), next_offset + 11),
        Some((next_offset, "423".into(), 41, bytes.len()))
    );
    assert_eq!(indexed_record_containing(&bytes, 6, bytes.len(), 7), None);
    assert_eq!(
        contiguous_i32_program(&[u8::MAX; 8], 0, 8),
        Some(vec![-1, -1])
    );
    assert_eq!(contiguous_i32_program(&[0; 7], 0, 7), None);

    let mut framed = vec![0; 11];
    framed.extend_from_slice(&[7, 8, 9]);
    framed.extend_from_slice(&16u32.to_le_bytes());
    let family_name_offset = framed.len();
    framed.extend_from_slice(b"edge_recipe_data");
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        Some((11, vec![7, 8, 9]))
    );
    framed[14..18].copy_from_slice(&15u32.to_le_bytes());
    assert_eq!(
        recipe_record_prefix(&framed, 0, family_name_offset, 16),
        None
    );
}

#[test]
fn dimension_recipe_decodes_ordered_persistent_reference_entries() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let first_token_at = prefix.len();
    prefix.extend_from_slice(b"13");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let first_reference_at = prefix.len();
    prefix.extend_from_slice(&331u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_token_at = prefix.len();
    prefix.extend_from_slice(&[b'9', 0, 0, 0]);
    prefix.push(0);
    prefix.extend_from_slice(&2u32.to_le_bytes());
    let second_reference_at = prefix.len();
    prefix.extend_from_slice(&303u32.to_le_bytes());
    let third_reference_at = prefix.len();
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert_eq!(references.len(), 3);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token, "13");
    assert_eq!(references[0].token_offset, 1_000 + first_token_at as u64);
    assert_eq!(references[0].design_reference, 331);
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + first_reference_at as u64
    );
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].selector_offset, 1_048);
    assert_eq!(references[1].token, "9");
    assert_eq!(references[1].token_offset, 1_000 + second_token_at as u64);
    assert_eq!(references[1].design_reference, 303);
    assert_eq!(
        references[1].design_reference_offset,
        1_000 + second_reference_at as u64
    );
    assert_eq!(references[2].selector, 2);
    assert_eq!(references[2].token, "9");
    assert_eq!(references[2].design_reference, 304);
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + third_reference_at as u64
    );
    let suffix_at = prefix.len() - 4;
    prefix.splice(
        suffix_at..,
        [1u32, 1, 0, 0, 2, 401, 402, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes),
    );
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    prefix.extend_from_slice(&[0; 2]);
    assert_eq!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000),
        references
    );
    let tags = [
        PersistentSubentityTag {
            id: "matching".into(),
            target: AttributeTarget::Face(FaceId::mint("face-b").expect("identity grammar")),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "other".into(),
            target: AttributeTarget::Face(FaceId::mint("face-a").expect("identity grammar")),
            selector: 1,
            token: "13".into(),
            design_references: vec![999],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-face".into(),
            target: AttributeTarget::Face(FaceId::mint("face-c").expect("identity grammar")),
            selector: 2,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "matching-edge".into(),
            target: AttributeTarget::Edge(EdgeId::mint("edge-b").expect("identity grammar")),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "alternate-edge".into(),
            target: AttributeTarget::Edge(EdgeId::mint("edge-c").expect("identity grammar")),
            selector: 2,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    let mut bound = references[0].clone();
    crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
        &mut bound, &tags, None,
    );
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("face-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.candidate_edges,
        [EdgeId::mint("edge-b").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_faces,
        [FaceId::mint("face-c").expect("identity grammar")]
    );
    assert_eq!(
        bound.alternate_selector_edges,
        [EdgeId::mint("edge-c").expect("identity grammar")]
    );
    let stream_tags = [
        PersistentSubentityTag {
            id: "f3d:xref/A/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId::mint("face-a").expect("identity grammar")),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
        PersistentSubentityTag {
            id: "f3d:xref/B/occurrence-0/design:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(FaceId::mint("face-b").expect("identity grammar")),
            selector: 1,
            token: "13".into(),
            design_references: vec![331],
            ordinal: 0,
        },
    ];
    crate::design::decode::dimension_frames::bind_recipe_reference_candidates(
        &mut bound,
        &stream_tags,
        Some("f3d:xref/A/occurrence-0/Asset/Design1/BulkStream.dat:dimension-recipe#1"),
    );
    assert_eq!(
        bound.candidate_faces,
        [FaceId::mint("face-a").expect("identity grammar")]
    );
}

#[test]
fn dimension_recipe_decodes_signed_decimal_reference_tokens() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&3u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());

    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-2");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&301u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(b"-1");
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&304u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].selector, 1);
    assert_eq!(references[0].token, "-2");
    assert_eq!(references[0].design_reference, 301);
    assert_eq!(references[1].selector, 2);
    assert_eq!(references[1].token, "-1");
    assert_eq!(references[1].design_reference, 304);
}

#[test]
fn face_recipe_decodes_paired_packed_reference_runs() {
    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    prefix.extend_from_slice(&1u32.to_le_bytes());
    let mut reference_offsets = Vec::new();
    for (ordinal, token) in b"23".iter().copied().enumerate() {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        if ordinal == 0 {
            prefix.push(token);
        } else {
            prefix.extend_from_slice(&1u32.to_le_bytes());
            prefix.push(token);
        }
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&2u32.to_le_bytes());
        reference_offsets.push(prefix.len());
        prefix.extend_from_slice(&305u32.to_le_bytes());
        prefix.extend_from_slice(&312u32.to_le_bytes());
        if ordinal == 1 {
            prefix.extend_from_slice(&0u32.to_le_bytes());
        }
    }

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert!(crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
    assert_eq!(references.len(), 4);
    assert_eq!(
        references
            .iter()
            .map(|reference| (reference.selector, reference.token.as_str()))
            .collect::<Vec<_>>(),
        [(1, "2"), (1, "2"), (1, "3"), (1, "3")]
    );
    assert_eq!(
        references
            .iter()
            .map(|reference| reference.design_reference)
            .collect::<Vec<_>>(),
        [305, 312, 305, 312]
    );
    assert_eq!(
        references[0].design_reference_offset,
        1_000 + reference_offsets[0] as u64
    );
    assert_eq!(
        references[2].design_reference_offset,
        1_000 + reference_offsets[1] as u64
    );

    let second_operand_at = reference_offsets[0] + 8;
    let mut packed_second = prefix.clone();
    packed_second.drain(second_operand_at + 4..second_operand_at + 8);
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&packed_second, 1_000)
            .is_empty()
    );
    let mut invalid_header = prefix.clone();
    invalid_header[0] = 1;
    assert!(
        !crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&invalid_header)
    );

    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&trailing, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&trailing));

    let mut mismatched_selector = prefix.clone();
    mismatched_selector[second_operand_at..second_operand_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &mismatched_selector,
            1_000,
        )
        .is_empty()
    );

    let second_run_at = reference_offsets[1];
    prefix[second_run_at..second_run_at + 4].copy_from_slice(&306u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_paired_recipe_reference_frame(&prefix));
}

#[test]
fn face_recipe_decodes_five_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, references: &[u32]) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(
            &u32::try_from(references.len())
                .expect("synthetic reference count")
                .to_le_bytes(),
        );
        for reference in references {
            prefix.extend_from_slice(&reference.to_le_bytes());
        }
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&5u32.to_le_bytes());
    prefix.extend_from_slice(&2u32.to_le_bytes());
    operand(&mut prefix, 2, "-4", &[401, 402]);
    let first_operand_end = prefix.len();
    operand(&mut prefix, 3, "8", &[501]);

    let second_group_count_at = prefix.len();
    prefix.extend_from_slice(&1u32.to_le_bytes());
    operand(&mut prefix, 4, "-9", &[601, 602]);

    for (selector, token, reference) in [(5, "7", 701), (6, "-1", 801), (7, "0", 901)] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, &[reference]);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| {
                (
                    reference.selector,
                    reference.token.as_str(),
                    reference.design_reference,
                )
            })
            .collect::<Vec<_>>(),
        [
            (2, "-4", 401),
            (2, "-4", 402),
            (3, "8", 501),
            (4, "-9", 601),
            (4, "-9", 602),
            (5, "7", 701),
            (6, "-1", 801),
            (7, "0", 901),
        ]
    );
    assert_eq!(references[0].selector_offset, 1_022);
    assert_eq!(references[0].token_offset, 1_026);
    assert_eq!(references[0].design_reference_offset, 1_036);
    assert_eq!(references[1].design_reference_offset, 1_040);

    let mut wrong_first_group_count = prefix.clone();
    wrong_first_group_count[18..22].copy_from_slice(&3u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &wrong_first_group_count,
            1_000,
        )
        .is_empty()
    );
    let mut wrong_group_count = prefix.clone();
    wrong_group_count[14..18].copy_from_slice(&4u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &wrong_group_count,
            1_000,
        )
        .is_empty()
    );
    let mut empty_group = prefix.clone();
    empty_group[second_group_count_at..second_group_count_at + 4]
        .copy_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&empty_group, 1_000)
            .is_empty()
    );
    let mut length_prefixed_token = prefix.clone();
    length_prefixed_token.splice(26..26, 2u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &length_prefixed_token,
            1_000,
        )
        .is_empty()
    );
    let mut locally_terminated_operand = prefix.clone();
    locally_terminated_operand.splice(first_operand_end..first_operand_end, 0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(
            &locally_terminated_operand,
            1_000,
        )
        .is_empty()
    );
    let mut trailing = prefix.clone();
    trailing.extend_from_slice(&0u32.to_le_bytes());
    assert!(
        crate::design::decode::dimension_frames::decode_recipe_references(&trailing, 1_000)
            .is_empty()
    );
    assert!(!crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&trailing));
}

#[test]
fn face_recipe_decodes_dynamic_group_reference_sequence() {
    fn operand(prefix: &mut Vec<u8>, selector: u32, token: &str, reference: u32) {
        prefix.extend_from_slice(&selector.to_le_bytes());
        prefix.extend_from_slice(token.as_bytes());
        prefix.extend_from_slice(&[0; 4]);
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&reference.to_le_bytes());
    }

    let mut prefix = vec![0; 10];
    prefix.extend_from_slice(&1u32.to_le_bytes());
    prefix.extend_from_slice(&4u32.to_le_bytes());
    for (selector, token, reference) in [
        (1, "97", 302),
        (2, "88", 302),
        (3, "10", 302),
        (1, "8", 302),
    ] {
        prefix.extend_from_slice(&1u32.to_le_bytes());
        operand(&mut prefix, selector, token, reference);
    }
    prefix.extend_from_slice(&0u32.to_le_bytes());

    let references =
        crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000);
    assert!(crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(&prefix));
    assert_eq!(
        references
            .iter()
            .map(|reference| (
                reference.selector,
                reference.token.as_str(),
                reference.design_reference
            ))
            .collect::<Vec<_>>(),
        [
            (1, "97", 302),
            (2, "88", 302),
            (3, "10", 302),
            (1, "8", 302)
        ]
    );
}

#[test]
fn dimension_recipe_rejects_non_decimal_reference_tokens() {
    for token in [b"-".as_slice(), b"+1", b"1-"] {
        let mut prefix = vec![0; 10];
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&3u32.to_le_bytes());
        prefix.extend_from_slice(&4u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(
            &u32::try_from(token.len())
                .expect("synthetic token length")
                .to_le_bytes(),
        );
        prefix.extend_from_slice(token);
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&1u32.to_le_bytes());
        prefix.extend_from_slice(&301u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());
        prefix.extend_from_slice(&0u32.to_le_bytes());

        assert!(
            crate::design::decode::dimension_frames::decode_recipe_references(&prefix, 1_000)
                .is_empty(),
            "accepted token {token:?}"
        );
    }
}

#[test]
fn dimension_locus_pair_resolves_two_typed_geometry_records() {
    let mut bytes = vec![0; 80];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&233u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&3u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&4u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&192u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&0u32.to_le_bytes());
    bytes[54] = 1;
    bytes[55..59].copy_from_slice(&194u32.to_le_bytes());
    bytes[65..69].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&233u32.to_le_bytes());

    let mut pair = parse_dimension_locus_pair(&bytes, 0, 228, &HashSet::from([192, 194]))
        .expect("paired dimension locus frame");
    pair.id = "f3d:Design/BulkStream.dat:design-dimension-locus-pair#0".into();
    assert_eq!(pair.companion_record_index, 228);
    assert_eq!(pair.record_index, 233);
    assert_eq!(pair.frame_length, 80);
    assert_eq!(pair.first_geometry_record_index, 192);
    assert_eq!(pair.first_role, 0);
    assert_eq!(pair.second_geometry_record_index, 194);
    assert_eq!(pair.second_role, 1);
    assert_eq!(pair.paired_class_tag, "273");
    let mut parameter = parse_design_parameter(&parameter_record(
        Some(300),
        "40 mm",
        "Linear Dimension-3",
        Some("mm"),
        "d3",
        4.0,
    ))
    .unwrap();
    parameter.id = "f3d:Design/BulkStream.dat:design-parameter#301".into();
    parameter.record_index = 301;
    let owner = DesignParameterOwner {
        id: "f3d:Design/BulkStream.dat:design-parameter-owner#300".into(),
        byte_offset: pair.paired_byte_offset + 59,
        frame_length: 104,
        class_tag: "292".into(),
        record_index: 300,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 4.0,
        evaluated_value_offset: pair.paired_byte_offset + 99,
        parameter_record_index: 301,
        owned_ordinal: 3,
        variant: Some(0),
        companion_record_index: 302,
    };
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            std::slice::from_ref(&owner),
            std::slice::from_ref(&parameter),
        ),
        Some(302)
    );
    assert_eq!(
        crate::design::decode::dimension_frames::following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset,
            &[owner.clone(), owner],
            std::slice::from_ref(&parameter),
        ),
        None
    );

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested = find_dimension_locus_pair(&nested, 0, nested_end, 228, &HashSet::from([192, 194]))
        .expect("nested paired dimension locus frame");
    assert_eq!(nested.byte_offset, 11);
    assert_eq!(nested.paired_byte_offset, 91);

    let mut competing = bytes.clone();
    competing.extend_from_slice(&bytes);
    assert!(find_dimension_locus_pair(
        &competing,
        0,
        competing.len(),
        228,
        &HashSet::from([192, 194]),
    )
    .is_none());
}

#[test]
fn dimension_null_locus_pair_preserves_null_and_typed_roles() {
    let mut bytes = vec![0; 74];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"277");
    bytes[7..11].copy_from_slice(&1394u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[35..39].copy_from_slice(&10u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&1109u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&7u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"273");
    bytes.extend_from_slice(&1394u32.to_le_bytes());

    let pair = parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1109]))
        .expect("null-locus dimension frame");
    assert_eq!(pair.companion_record_index, 1290);
    assert_eq!(pair.governing_companion_record_index, 1290);
    assert_eq!(pair.record_index, 1394);
    assert_eq!(pair.frame_length, 74);
    assert_eq!(pair.null_role, 10);
    assert_eq!(pair.geometry_record_index, 1109);
    assert_eq!(pair.geometry_role, 7);
    assert_eq!(pair.paired_class_tag, "273");

    assert!(parse_dimension_null_locus_pair(&bytes, 0, 1290, &HashSet::from([1110]),).is_none());

    let mut nested = Vec::new();
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"341");
    nested.extend_from_slice(&229u32.to_le_bytes());
    nested.extend_from_slice(&bytes);
    let nested_end = nested.len();
    let nested =
        find_dimension_null_locus_pair(&nested, 0, nested_end, 1290, &HashSet::from([1109]))
            .expect("null-locus frame following another indexed frame");
    assert_eq!(nested.byte_offset, 11);
    assert_eq!(nested.paired_byte_offset, 85);

    let mut axis_pair = pair.clone();
    axis_pair.null_role = 14;
    axis_pair.geometry_role = 3;
    let entity = SketchEntity::new(
        SketchEntityId("f3d:model:sketch-entity#line".into()),
        SketchId("f3d:model:sketch#axis-angle".into()),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 1.0),
        },
    );
    let parameter = cadmpeg_ir::features::ParameterId("f3d:model:parameter#angle".into());
    assert!(matches!(
        null_locus_dimension_definition(
            &axis_pair,
            &entity,
            "Angular Dimension-2",
            std::f64::consts::FRAC_PI_4,
            parameter.clone(),
            TEST_LINEAR_TOLERANCE,
        ),
        Some(SketchConstraintDefinition::AngleToAxis {
            entity: ref actual_entity,
            axis: SketchAxis::Horizontal,
            parameter: ref actual_parameter,
        }) if actual_entity == entity.id() && actual_parameter == &parameter
    ));
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        0.5,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE,
    )
    .is_none());
    axis_pair.null_role = 13;
    assert!(null_locus_dimension_definition(
        &axis_pair,
        &entity,
        "Angular Dimension-2",
        std::f64::consts::FRAC_PI_4,
        parameter.clone(),
        TEST_LINEAR_TOLERANCE,
    )
    .is_none());

    let radial_entity = SketchEntity::new(
        SketchEntityId("f3d:model:sketch-entity:circle".into()),
        SketchId("f3d:model:sketch#radial".into()),
        SketchGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            radius: cadmpeg_ir::features::Length(1.000_000_014_901_161_2),
        },
    );
    assert!(matches!(
        null_locus_dimension_definition(
            &pair,
            &radial_entity,
            "Diameter Dimension-2",
            0.2,
            parameter.clone(),
            TEST_LINEAR_TOLERANCE,
        ),
        Some(SketchConstraintDefinition::Diameter {
            entity: ref actual_entity,
            parameter: ref actual_parameter,
        }) if actual_entity == radial_entity.id() && actual_parameter == &parameter
    ));
    assert!(null_locus_dimension_definition(
        &pair,
        &radial_entity,
        "Diameter Dimension-2",
        0.2,
        parameter,
        0.0,
    )
    .is_none());
}

#[test]
fn dimension_locus_group_preserves_roles_owner_state_and_return_order() {
    let mut bytes = vec![0; 101];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"286");
    bytes[7..11].copy_from_slice(&249u32.to_le_bytes());
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[25..29].copy_from_slice(&175u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&2u32.to_le_bytes());
    bytes[39] = 1;
    bytes[40..44].copy_from_slice(&217u32.to_le_bytes());
    bytes[50..54].copy_from_slice(&1u32.to_le_bytes());
    bytes[55] = 1;
    bytes[56..60].copy_from_slice(&172u32.to_le_bytes());
    bytes[66..70].copy_from_slice(&1u32.to_le_bytes());
    bytes[74..78].copy_from_slice(&2u32.to_le_bytes());
    bytes[78] = 1;
    bytes[79..83].copy_from_slice(&217u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90..94].copy_from_slice(&175u32.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&250u32.to_le_bytes());

    let group = parse_dimension_locus_group(
        &bytes,
        0,
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    )
    .expect("counted dimension locus frame");
    assert_eq!(group.companion_record_index, 240);
    assert_eq!(group.record_index, 249);
    assert_eq!(group.frame_length, 101);
    assert_eq!(group.owner_reference, 172);
    assert_eq!(group.owner_role, 1);
    assert_eq!(group.state, 0);
    assert_eq!(group.loci[0].geometry_record_index, 175);
    assert_eq!(group.loci[0].role, 2);
    assert_eq!(group.loci[1].geometry_record_index, 217);
    assert_eq!(group.loci[1].role, 1);
    assert_eq!(group.return_members, [217, 175]);
    assert_eq!(group.next_class_tag, "314");
    assert_eq!(group.next_record_index, 250);

    let relation_at = |stream: &str, byte_offset| SketchRelation {
        id: format!("f3d:{stream}:sketch-relation#{byte_offset}"),
        record_index: 249,
        class_tag: "286".into(),
        byte_offset,
        state_offset: 66,
        owner_reference: 172,
        owner_entity_id: "0_172".into(),
        auxiliary_references: Vec::new(),
        auxiliary_reference_offsets: Vec::new(),
        rectangular_counted_reference_count: None,
        members: crate::records::zip_relation_members(
            vec![175, 217],
            vec![25, 40],
            Vec::new(),
            Vec::new(),
        )
        .expect("members"),
        owner_reference_offset: 56,
        state: 0,
        entity_genesis: None,
        kind: SketchRelationKind::Unpatterned,
        return_members: crate::records::zip_return_members(
            vec![217, 175],
            vec![79, 90],
            Vec::new(),
        )
        .expect("return members"),
        raw_bytes: bytes[..101].to_vec(),
    };
    let mut relations = vec![relation_at("native", 0), relation_at("other", 0)];
    let mut group = group;
    group.id = "f3d:native:design-dimension-locus-group#0".into();
    remove_dimension_frame_relations(&mut relations, &[], &[group], &[]);
    assert_eq!(relations.len(), 1);
    assert!(relations[0].id.starts_with("f3d:other:"));

    let body = bytes[11..101].to_vec();
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"315");
    bytes.extend_from_slice(&251u32.to_le_bytes());
    let groups = find_dimension_locus_groups(
        &bytes,
        0,
        bytes.len(),
        240,
        &HashSet::from([175, 217]),
        &HashSet::from([172]),
    );
    assert_eq!(
        groups
            .iter()
            .map(|group| group.record_index)
            .collect::<Vec<_>>(),
        [249, 250]
    );
}

#[test]
fn dimension_annotation_frame_links_nullable_loci_to_governing_owner() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"298");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    for (reference, role) in [(0u32, 6u32), (354, 2), (376, 3)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    push_genesis_block(&mut bytes, 0x202);
    let annotation_byte_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    push_reference(&mut bytes, 390);
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for reference in [376u32, 354] {
        push_reference(&mut bytes, reference);
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 4]);
    let paired_byte_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"287");
    bytes.extend_from_slice(&388u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    push_reference(&mut bytes, 201);
    bytes.extend_from_slice(&[0; 6]);
    bytes.resize(paired_byte_offset + 59, 0);

    let frame = parse_dimension_annotation_frame(
        &bytes,
        0,
        Some(383),
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("annotated dimension frame");
    assert_eq!(frame.companion_record_index, Some(383));
    assert_eq!(frame.governing_companion_record_index, 391);
    assert_eq!(frame.entity_genesis, 0x202);
    assert_eq!(frame.annotation_byte_offset, annotation_byte_offset as u64);
    assert_eq!(frame.annotation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index, 0);
    assert_eq!(frame.return_members.iter().map(|member| member.value).collect::<Vec<_>>(), [376, 354]);
    assert_eq!(frame.paired_byte_offset, paired_byte_offset as u64);
    assert_eq!(frame.owner_reference, 201);

    let leading = parse_dimension_annotation_frame(
        &bytes,
        0,
        None,
        &HashMap::from([(390, 391)]),
        &HashSet::from([354, 376]),
        &HashSet::from([201]),
    )
    .expect("scope-prefix dimension frame");
    assert_eq!(leading.companion_record_index, None);
    assert_eq!(leading.governing_owner_record_index, 390);
}

#[test]
fn dimension_presentation_frame_requires_registered_geometry_and_paired_sketch_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (reference, role) in [(306u32, 1u32), (331, 0)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    let presentation_offset = bytes.len();
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    let paired_offset = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&332u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&270u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 35]);

    let frame = parse_dimension_presentation_frame(
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306, 331]),
        &HashSet::from([270]),
        &HashSet::from([String::from("281")]),
    )
    .expect("direct dimension presentation frame");
    assert_eq!(frame.class_tag, "314");
    assert_eq!(frame.record_index, 332);
    assert_eq!(frame.frame_length, paired_offset as u64);
    assert_eq!(frame.presentation_byte_offset, presentation_offset as u64);
    assert_eq!(frame.presentation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index, 306);
    assert_eq!(frame.operands[1].geometry_record_index, 331);
    assert_eq!(frame.owner_reference, 270);

    assert!(parse_dimension_presentation_frame(
        &bytes,
        0,
        "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
        &HashSet::from([306]),
        &HashSet::from([270]),
        &HashSet::from([String::from("281")]),
    )
    .is_none());
}
