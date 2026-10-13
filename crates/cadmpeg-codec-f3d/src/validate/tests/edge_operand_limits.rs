// SPDX-License-Identifier: Apache-2.0

fn operand() -> crate::records::topology::edge_identity::DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-edge-operand#10",
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": 2,
        "byte_offset": 10,
        "class_tag": "346",
        "paired_byte_offset": 20,
        "paired_class_tag": "262",
        "recipe_record_index": 5,
        "recipe_record_byte_offset": 30,
        "recipe_id": "f3d:Design/BulkStream.dat:construction-recipe#0",
        "recipe_prefix_offset": 41,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 50,
        "recipe_program": [],
        "next_record_index": 6,
        "next_byte_offset": 100
    }))
    .unwrap()
}

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        identity::{RecordedValue, ReferenceRun},
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
        references::DesignClassTag,
    };
    let mut native = crate::native::F3dNative {
        design_edge_operands: vec![operand()],
        ..Default::default()
    };
    if valid {
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members = ReferenceRun::unlocated(vec![2]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        native.design_parameter_scopes.push(scope);
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#2".into(),
            record_index: 2,
            class_tag: DesignClassTag::try_from("346".to_owned()).unwrap(),
            byte_offset: 10,
        });
        native.construction_recipes.push(ConstructionRecipe {
            id: "f3d:Design/BulkStream.dat:construction-recipe#0".into(),
            byte_offset: 45,
            kind: ConstructionRecipeKind::Edge,
            design: None,
            recipe_index: 0,
            record_index: Some(RecordedValue {
                value: 5,
                offset: 0,
            }),
        });
    }
    native
}

fn edge_error(valid: bool, max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context(|service_ctx| {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(valid);
        if valid {
            ir.native
                .namespace_mut("f3d")
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "design_edge_operands",
                    &native.design_edge_operands,
                )
                .unwrap();
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = max_items;
        policy.limits.max_retained_bytes = max_retained;
        let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ctx = super::super::Ctx::new(&ir, &native, service_ctx).unwrap();
        ctx.decode = &decode;
        super::super::validate_edge_operands(&ctx, &mut Vec::new()).unwrap_err()
    })
}

#[test]
fn edge_operand_expected_index_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D expected edge operands",
        |cap| Err::<(), cadmpeg_core::CodecError>(edge_error(true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D expected edge operands")
    );
}

#[test]
fn edge_operand_slot_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D edge operand slots",
        |cap| Err::<(), cadmpeg_core::CodecError>(edge_error(true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge operand slots")
    );
}

#[test]
fn edge_operand_record_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "index F3D edge operand records",
        |cap| Err::<(), cadmpeg_core::CodecError>(edge_error(true, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge operand records")
    );
}

#[test]
fn edge_operand_invalid_finding_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D native validation findings",
        |cap| Err::<(), cadmpeg_core::CodecError>(edge_error(false, cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn edge_operand_invalid_entity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D validation entity",
        |cap| Err::<(), cadmpeg_core::CodecError>(edge_error(false, u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn edge_operand_full_value_comparison_refuses_work_limit() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D expected edge operand",
        0,
        |decode| {
            let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let native = native(true);
            ir.native
                .namespace_mut("f3d")
                .set_arena(
                    &cadmpeg_test_support::service_decode_context(),
                    "design_edge_operands",
                    &native.design_edge_operands,
                )
                .unwrap();
            let ctx = super::super::Ctx::new(&ir, &native, decode)?;
            super::super::validate_edge_operands(&ctx, &mut Vec::new()).map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compare F3D expected edge operand"
                && limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
    ));
}
