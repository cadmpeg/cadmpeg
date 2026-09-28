// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand {
    use crate::records::{
        feature::work_geometry::{DesignEdgeTreatmentVertexOperand, DesignVertexRecipe,
            DesignVertexRecipeDraft},
        references::DesignClassTag,
    };
    DesignEdgeTreatmentVertexOperand {
        id: "f3d:Design/BulkStream.dat:edge-treatment-vertex-operand#100".into(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        group_record_index: 20,
        group_member_ordinal: 0,
        recipe: DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
            record_index: 100,
            byte_offset: 1_000,
            class_tag: DesignClassTag::try_from("306".to_owned()).unwrap(),
            paired_byte_offset: 1_016,
            paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
            recipe_record_index: 103,
            recipe_record_byte_offset: 1_032,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            recipe_prefix_offset: 1_043,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 1_050,
            recipe_program: vec![0],
            resolution: None,
            next_record_index: 105,
            next_byte_offset: 1_054,
        }).unwrap(),
    }
}

fn nested_items(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Array(values) =>
            u64::try_from(values.len()).unwrap() + values.iter().map(nested_items).sum::<u64>(),
        serde_json::Value::Object(values) =>
            u64::try_from(values.len()).unwrap() + values.values().map(nested_items).sum::<u64>(),
        _ => 0,
    }
}

fn reload_items(ir: &cadmpeg_ir::CadIr) -> u64 {
    let record = &ir.native.namespace("f3d").unwrap()
        .arenas().get("design_edge_treatment_vertex_operands").unwrap()[0];
    let fields = record.fields();
    1 + u64::try_from(fields.len()).unwrap() + fields.values().map(nested_items).sum::<u64>()
}

fn vertex_error(stored: bool, extra_items: u64, max_retained: u64)
    -> cadmpeg_core::CodecError
{
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = crate::native::F3dNative {
        design_edge_treatment_vertex_operands: vec![operand()],
        ..Default::default()
    };
    if stored {
        ir.native.namespace_mut("f3d").set_arena(
            &cadmpeg_test_support::service_decode_context(),
            "design_edge_treatment_vertex_operands",
            &native.design_edge_treatment_vertex_operands,
        ).unwrap();
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = if stored {
        reload_items(&ir) + extra_items
    } else {
        extra_items
    };
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_edge_treatment_vertex_operands(
        Some(&decode), &ctx, &mut Vec::new()).unwrap_err()
}

#[test]
fn vertex_operand_expected_index_refuses_collection_limit() {
    let error = vertex_error(true, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge treatment vertex operands"));
}

#[test]
fn vertex_operand_generated_id_refuses_retained_limit() {
    let error = vertex_error(false, u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D native record ID"));
}

#[test]
fn vertex_operand_invalid_finding_refuses_collection_limit() {
    let error = vertex_error(false, 0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings"));
}

#[test]
fn vertex_operand_invalid_entity_refuses_retained_limit() {
    let native = operand();
    let stream = super::super::design_stream(&native.id);
    let expected_id = crate::ids::native_scoped_id(
        stream, "edge-treatment-vertex-operand", native.recipe.byte_offset());
    let error = vertex_error(false, u64::MAX, u64::try_from(expected_id.len()).unwrap());
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity"));
}
