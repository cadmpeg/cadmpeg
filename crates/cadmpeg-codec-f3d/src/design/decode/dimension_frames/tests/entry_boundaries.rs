// SPDX-License-Identifier: Apache-2.0

use super::super::{
    annotation_return_members, annotation_tail, contiguous_i32_program,
    decode_grouped_recipe_references, decode_paired_recipe_references,
    decode_recipe_references_charged, decode_standard_recipe_references,
    is_decimal_integer_token, is_grouped_recipe_reference_frame, is_paired_recipe_reference_frame,
    length_prefixed_token, parse_dimension_annotation_frame, parse_dimension_locus_group,
    parse_dimension_locus_pair, parse_dimension_null_locus_pair, program_has_operand_tail,
    recipe_reference_suffix, record_containing, scan_recipe_reference_operand,
    AnnotationFrameInputs, RecipeReferenceTokenFrame,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashMap;
use std::num::NonZeroU32;

const ORIGINAL: &str = "original F3D dimension entry refusal";

fn with_entry_context(test: impl Fn(&DecodeContext<'_>, bool)) {
    for refused in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        test(&ctx, refused);
        if refused {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original.operation == ORIGINAL));
        } else {
            assert!(ctx.finish_session().is_ok());
        }
    }
}

fn check_gate<T>(
    ctx: &DecodeContext<'_>,
    refused: bool,
    call: impl FnOnce() -> Result<T, CodecError>,
    healthy: impl FnOnce(T),
) {
    if refused {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, ORIGINAL) else {
            panic!("the real zero-work limit must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.limit, 0);
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        assert!(matches!(call(), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(ctx.charge_work(0, "later dimension entry"), Err(CodecError::ResourceLimit(later)) if later == original));
    } else {
        healthy(call().unwrap());
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn dimension_recipe_decode_entry_keeps_original_before_incomplete_header() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || decode_recipe_references_charged(ctx, &[], u64::MAX), |references| assert!(references.is_empty()));
    });
}

#[test]
fn dimension_standard_reference_entry_keeps_original_before_missing_operand_count() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || decode_standard_recipe_references(ctx, &[], u64::MAX), |references| assert!(references.is_none()));
    });
}

#[test]
fn dimension_paired_reference_entry_keeps_original_before_missing_pair_count() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || decode_paired_recipe_references(ctx, &[], u64::MAX), |references| assert!(references.is_none()));
    });
}

#[test]
fn dimension_grouped_reference_entry_keeps_original_before_incomplete_extent() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || decode_grouped_recipe_references(ctx, &[], u64::MAX, 4), |references| assert!(references.is_none()));
    });
}

#[test]
fn dimension_paired_frame_entry_keeps_original_before_incomplete_header() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || is_paired_recipe_reference_frame(ctx, &[]), |paired| assert!(!paired));
    });
}

#[test]
fn dimension_grouped_frame_entry_keeps_original_before_incomplete_header() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || is_grouped_recipe_reference_frame(ctx, &[]), |grouped| assert!(!grouped));
    });
}

#[test]
fn dimension_operand_scan_entry_keeps_original_before_unreadable_selector() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || scan_recipe_reference_operand(ctx, &[], usize::MAX, RecipeReferenceTokenFrame::Packed), |operand| assert!(operand.is_none()));
    });
}

#[test]
fn dimension_counted_token_entry_keeps_original_before_unreadable_count() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || length_prefixed_token(ctx, &[], usize::MAX), |token| assert!(token.is_none()));
    });
}

#[test]
fn dimension_decimal_token_entry_keeps_original_before_empty_digits() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || is_decimal_integer_token(ctx, &[]), |decimal| assert!(!decimal));
    });
}

#[test]
fn dimension_suffix_entry_keeps_original_before_free_constant_terminator() {
    let bytes = [0; 4];
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || recipe_reference_suffix(ctx, &bytes), |terminator| assert!(terminator));
    });
    assert_eq!(bytes, [0; 4]);
}

#[test]
fn dimension_operand_tail_entry_keeps_original_before_missing_tail() {
    // Trusted typed inputs share the existing matcher's fixture; no file-admission claim.
    let (record, mut operand) = super::callback_boundaries::edge_inputs();
    operand.recipe_program.truncate(7);
    let native = serde_json::to_value((&record, &operand)).unwrap();
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || program_has_operand_tail(ctx, &record, &operand), |matching| assert!(!matching));
    });
    assert_eq!(serde_json::to_value((&record, &operand)).unwrap(), native);
}

#[test]
fn dimension_containing_record_entry_keeps_original_before_ineligible_interval() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || record_containing(ctx, &[], 2, 1, 1), |record| assert!(record.is_none()));
    });
}

#[test]
fn dimension_program_entry_keeps_original_before_unreadable_extent() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || contiguous_i32_program(ctx, &[], usize::MAX, usize::MAX), |program| assert!(program.is_none()));
    });
}

#[test]
fn dimension_locus_pair_entry_keeps_original_before_unreadable_header() {
    with_entry_context(|ctx, refused| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        check_gate(ctx, refused, || parse_dimension_locus_pair(ctx, &[], usize::MAX, 42, &[], &records), |pair| assert!(pair.is_none()));
    });
}

#[test]
fn dimension_null_locus_pair_entry_keeps_original_before_unreadable_header() {
    with_entry_context(|ctx, refused| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        check_gate(ctx, refused, || parse_dimension_null_locus_pair(ctx, &[], usize::MAX, 42, &[], &records), |pair| assert!(pair.is_none()));
    });
}

#[test]
fn dimension_annotation_entry_keeps_original_before_unreadable_header() {
    with_entry_context(|ctx, refused| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        let owners = HashMap::new();
        let inputs = AnnotationFrameInputs { governed_owners: &owners, geometry_indices: &[], sketch_entities: &[], records: &records };
        check_gate(ctx, refused, || parse_dimension_annotation_frame(ctx, &[], usize::MAX, 0, Some(42), &inputs), |frame| assert!(frame.is_none()));
    });
}

#[test]
fn dimension_annotation_tail_entry_keeps_original_before_ineligible_interval() {
    with_entry_context(|ctx, refused| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        let owners = HashMap::new();
        let inputs = AnnotationFrameInputs { governed_owners: &owners, geometry_indices: &[], sketch_entities: &[], records: &records };
        check_gate(ctx, refused, || annotation_tail(ctx, &[], 1, 0, &[], &inputs), |tail| assert!(tail.is_none()));
    });
}

#[test]
fn dimension_annotation_members_entry_keeps_original_before_free_empty_run() {
    with_entry_context(|ctx, refused| {
        let records = IndexedRecordOffsets::build(ctx, &[]).unwrap();
        let owners = HashMap::new();
        let inputs = AnnotationFrameInputs { governed_owners: &owners, geometry_indices: &[], sketch_entities: &[], records: &records };
        check_gate(ctx, refused, || annotation_return_members(ctx, &[], usize::MAX, 0, &inputs), |members| assert_eq!(members, Some([NonZeroU32::MIN; 64])));
    });
}

#[test]
fn dimension_locus_group_entry_keeps_original_before_unreadable_header() {
    with_entry_context(|ctx, refused| {
        check_gate(ctx, refused, || parse_dimension_locus_group(ctx, &[], usize::MAX, 42, &[], &[]), |group| assert!(group.is_none()));
    });
}
