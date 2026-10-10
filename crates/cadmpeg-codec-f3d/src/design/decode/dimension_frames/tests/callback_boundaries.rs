// SPDX-License-Identifier: Apache-2.0

use super::super::{
    matching_edge_operand_ids, parse_dimension_presentation_frame, PresentationStreamTables,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::dimensions::DesignDimensionRecipeRecord;
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::edge_identity::{DesignEdgeOperand, DesignEdgeOperandDraft};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::cell::Cell;
use std::collections::HashMap;

const ORIGINAL: &str = "original F3D dimension callback refusal";

/// Typed record inputs for the edge matcher, without a byte-file admission claim.
pub(super) fn edge_inputs() -> (DesignDimensionRecipeRecord, DesignEdgeOperand) {
    let operand = DesignEdgeOperand::try_new(DesignEdgeOperandDraft {
        id: "f3d:Design/BulkStream.dat:edge-operand#100".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 0,
        class_tag: "316".to_owned().try_into().unwrap(),
        paired_byte_offset: 20,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        recipe_record_index: 103,
        recipe_record_byte_offset: 40,
        recipe_id: "f3d:Design/BulkStream.dat:edge-recipe#103".into(),
        recipe_prefix_offset: 51,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_program_offset: 60,
        recipe_program: vec![0, 0, 0, 0, 0, 0, 0, 1, 2],
        recipe_structure: None,
        surface_patch_recipe_structure: None,
        local_topology_references: None,
        candidate_faces: Vec::new(),
        result_candidate_faces: Vec::new(),
        result_boundary_edge_slots: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        terminal_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        preceding_boundary_edge_slots: Vec::new(),
        terminal_boundary_edge_slots: Vec::new(),
        changed_boundary_edge_slots: Vec::new(),
        deleted_boundary_edge_slots: Vec::new(),
        updated_boundary_edge_slots: Vec::new(),
        treatment_radius_candidates: Vec::new(),
        changed_boundary_edge_contexts: Vec::new(),
        terminal_boundary_edge_contexts: Vec::new(),
        terminal_reference_edge_slots: Vec::new(),
        recipe_reference_contexts: Vec::new(),
        recipe_selectors: Vec::new(),
        recipe_state_id: None,
        resolved_edge_slot: None,
        resolved_axis: None,
        next_record_index: 104,
        next_byte_offset: 100,
    })
    .unwrap();
    let record = DesignDimensionRecipeRecord {
        id: "f3d:Design/BulkStream.dat:dimension-recipe#1".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#1".into(),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: "423".to_owned().try_into().unwrap(),
        record_index: 1,
        frame_length: 100,
        prefix_offset: 11,
        prefix_bytes: Vec::new(),
        references: Vec::new(),
        program_offset: 60,
        program: vec![1, 2],
        matching_edge_operand_ids: Vec::new(),
    };
    (record, operand)
}

#[test]
fn dimension_edge_scope_callback_keeps_original_refusal_before_bool_or_ordinary_error() {
    let (record, operand) = edge_inputs();
    let native = serde_json::to_value(&operand).unwrap();
    for outcome in 0..3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let calls = Cell::new(0);
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
            let result =
                matching_edge_operand_ids(ctx, &record, [Ok(&operand), Ok(&operand)], |_| {
                    calls.set(calls.get() + 1);
                    assert!(matches!(
                        ctx.charge_work(1, ORIGINAL),
                        Err(CodecError::ResourceLimit(_))
                    ));
                    match outcome {
                        0 => Ok(false),
                        1 => Ok(true),
                        _ => Err(CodecError::Malformed(
                            "ordinary dimension callback error".into(),
                        )),
                    }
                });
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("original edge scope refusal");
            };
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(original.additional, 1);
            assert_eq!(calls.get(), 1);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(
                matches!(ctx.charge_work(0, "later edge scope boundary"), Err(CodecError::ResourceLimit(later)) if later == original)
            );
        });
    }
    assert_eq!(serde_json::to_value(&operand).unwrap(), native);
}

#[test]
fn dimension_edge_iterator_keeps_original_refusal_before_end_error_or_value() {
    let (record, operand) = edge_inputs();
    for outcome in 0..3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let next_calls = Cell::new(0);
            let scope_calls = Cell::new(0);
            let iterator = std::iter::from_fn(|| {
                next_calls.set(next_calls.get() + 1);
                assert!(matches!(
                    ctx.charge_work(1, ORIGINAL),
                    Err(CodecError::ResourceLimit(_))
                ));
                match outcome {
                    0 => None,
                    1 => Some(Err(CodecError::Malformed("ordinary iterator error".into()))),
                    _ => Some(Ok(&operand)),
                }
            });
            let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
            let result = matching_edge_operand_ids(ctx, &record, iterator, |_| {
                scope_calls.set(scope_calls.get() + 1);
                Ok(true)
            });
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("original iterator refusal");
            };
            assert_eq!(original.operation, ORIGINAL);
            assert_eq!(original.additional, 1);
            assert_eq!(next_calls.get(), 1);
            assert_eq!(scope_calls.get(), 0);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            assert!(
                matches!(ctx.charge_work(0, "later iterator boundary"), Err(CodecError::ResourceLimit(later)) if later == original)
            );
        });
    }
}

#[test]
fn dimension_edge_ordinary_error_and_nonbinding_match_preserve_native_identity() {
    let (record, operand) = edge_inputs();
    let native = serde_json::to_value(&operand).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        assert!(
            matches!(matching_edge_operand_ids(ctx, &record, [Ok(&operand)], |_| Err(CodecError::Malformed("ordinary dimension callback error".into()))), Err(CodecError::Malformed(message)) if message == "ordinary dimension callback error")
        );
        assert!(ctx.resource_refusal().is_none());
        assert_eq!(
            matching_edge_operand_ids(ctx, &record, [Ok(&operand)], |_| Ok(true)).unwrap(),
            [operand.id.clone()]
        );
        assert_eq!(serde_json::to_value(&operand).unwrap(), native);
    });
}

fn presentation_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"314");
    bytes.extend_from_slice(&332_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    for (reference, role) in [(306_u32, 1_u32), (331, 0)] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&role.to_le_bytes());
    }
    bytes.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&332_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&270_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 35]);
    bytes
}

#[test]
fn dimension_paired_class_callback_keeps_original_refusal_before_bool_or_ordinary_error() {
    let bytes = presentation_bytes();
    let native = bytes.clone();
    for outcome in 0..3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
        let tables = PresentationStreamTables {
            ctx: &ctx,
            geometry_indices: &[306, 331],
            sketch_entities: &[270],
            records: &records,
            owners_by_scope: HashMap::new(),
        };
        let calls = Cell::new(0);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, ORIGINAL, Some(1));
        let result = parse_dimension_presentation_frame(&bytes, 0, &tables, |class| {
            calls.set(calls.get() + 1);
            assert_eq!(class, 281);
            assert!(matches!(
                ctx.charge_work(1, ORIGINAL),
                Err(CodecError::ResourceLimit(_))
            ));
            match outcome {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(CodecError::Malformed("ordinary paired class error".into())),
            }
        });
        let Err(CodecError::ResourceLimit(original)) = result else {
            panic!("original paired class refusal");
        };
        assert_eq!(original.operation, ORIGINAL);
        assert_eq!(original.additional, 1);
        assert_eq!(calls.get(), 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(ctx.charge_work(0, "later paired class boundary"), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    }
    assert_eq!(bytes, native);
}

#[test]
fn dimension_paired_class_ordinary_error_and_nonbinding_output_preserve_native_bytes() {
    let bytes = presentation_bytes();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
    let tables = PresentationStreamTables {
        ctx: &ctx,
        geometry_indices: &[306, 331],
        sketch_entities: &[270],
        records: &records,
        owners_by_scope: HashMap::new(),
    };
    assert!(
        matches!(parse_dimension_presentation_frame(&bytes, 0, &tables, |_| Err(CodecError::Malformed("ordinary paired class error".into()))), Err(CodecError::Malformed(message)) if message == "ordinary paired class error")
    );
    assert!(ctx.resource_refusal().is_none());
    let frame = parse_dimension_presentation_frame(&bytes, 0, &tables, |class| Ok(class == 281))
        .unwrap()
        .unwrap();
    assert_eq!(frame.class_tag.as_str(), "314");
    assert_eq!(frame.record_index, 332);
    assert_eq!(frame.frame_length, 57);
    assert_eq!(frame.presentation_byte_offset, 54);
    assert_eq!(frame.presentation_bytes, [0xaa, 0xbb, 0xcc]);
    assert_eq!(frame.operands[0].geometry_record_index.get(), 306);
    assert_eq!(frame.operands[1].geometry_record_index.get(), 331);
    assert_eq!(frame.owner_reference, 270);
    assert_eq!(frame.paired_class_tag.as_str(), "281");
    assert!(ctx.resource_refusal().is_none());
}
