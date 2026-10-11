// SPDX-License-Identifier: Apache-2.0
//! Reached `WorkPoint` queries retain native identity and the caller's refusal.

use super::{construction, edge_operand, input, scope, vertex_construction};
use crate::design::feature_project::{work_point_edge_operand, work_point_recipe_state_id};
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayloadMut};
use crate::records::feature::work_geometry::{
    DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRuleForm,
};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn edge_input() -> DesignWorkPointInput {
    input(
        42,
        DesignWorkPointInputCarrier::EdgeRecipe {
            operand_id: edge_operand(false).id,
        },
    )
}

fn vertex_input(state_id: i64) -> DesignWorkPointInput {
    let construction = vertex_construction(true);
    let DesignWorkPointRuleForm::Vertex { input } = construction.rule.form() else {
        panic!("vertex fixture");
    };
    let Some(DesignWorkPointInputCarrier::VertexRecipe { mut recipe }) = input.carrier().cloned()
    else {
        panic!("vertex carrier fixture");
    };
    recipe.resolution = Some(
        crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, 43).unwrap(),
    );
    super::input(22, DesignWorkPointInputCarrier::VertexRecipe { recipe })
}

fn native_scope(inputs: Vec<DesignWorkPointInput>) -> DesignParameterScope {
    let mut scope = scope();
    let DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() else {
        panic!("WorkPoint fixture");
    };
    *slot = Some(construction(DesignWorkPointRuleForm::Native {
        reference_type: 99,
        inputs,
    }));
    scope
}

#[test]
fn work_point_query_duplicate_stops_before_unvisited_operand_tail() {
    let scope = scope();
    let input = edge_input();
    let operand = edge_operand(false);
    let mut tail = operand.clone();
    tail.id = format!("f3d:{}:edge#42", "x".repeat(4096));
    let operands = [operand.clone(), operand, tail];
    let scope_stream = crate::ids::native_stream(&scope.id).unwrap();
    let operand_stream = crate::ids::native_stream(&operands[0].id).unwrap();
    // Scope reverse scan, then two raw slots, full equal IDs, operand reverse
    // scans and equal streams. The duplicate ends the query before any tail.
    let reached = u64::try_from(
        scope.id.len() - scope_stream.len()
            + 2 * (1 + operands[0].id.len() + operands[0].id.len() - operand_stream.len()
                + operand_stream.len()),
    )
    .unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = reached;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(work_point_edge_operand(ctx, &scope, &input, &operands)
            .unwrap()
            .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            work_point_edge_operand(ctx, &scope, &input, &operands)
        else {
            panic!("last compared byte must refuse");
        };
        assert_eq!(original.operation, "compare F3D WorkPoint operand stream");
        assert_eq!((original.used, original.additional), (reached - 1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_query_single_match_returns_actual_reference_and_missing_stays_absent() {
    let scope = scope();
    let input = edge_input();
    let operand = edge_operand(false);
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        let selected = work_point_edge_operand(ctx, &scope, &input, std::slice::from_ref(&operand))
            .unwrap()
            .unwrap();
        assert!(std::ptr::eq(selected, &raw const operand));
        assert!(work_point_edge_operand(ctx, &scope, &input, &[])
            .unwrap()
            .is_none());
        let mut wrong_owner = operand.clone();
        wrong_owner.scope_record_index = 21;
        assert!(work_point_edge_operand(ctx, &scope, &input, &[wrong_owner])
            .unwrap()
            .is_none());
    });
}

#[test]
fn work_point_query_identity_mismatch_skips_stream_and_identity_suffix() {
    let scope = scope();
    let input = edge_input();
    let mut operand = edge_operand(false);
    operand.id.replace_range(..1, "u");
    let stream = crate::ids::native_stream(&scope.id).unwrap();
    let mut policy = DecodePolicy::service();
    // Scope reverse scan, raw slot, first unequal ID byte and source end.
    policy.limits.max_work_units = u64::try_from(scope.id.len() - stream.len() + 3).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(work_point_edge_operand(ctx, &scope, &input, &[operand])
            .unwrap()
            .is_none());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

fn assert_query_refusal(operation: &'static str) {
    let scope = scope();
    let input = edge_input();
    let operands = [edge_operand(false)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let Err(CodecError::ResourceLimit(original)) =
            work_point_edge_operand(ctx, &scope, &input, &operands)
        else {
            panic!("actual query phase must refuse");
        };
        assert_eq!(original.operation, operation);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_query_scope_stream_preserves_original_refusal() {
    assert_query_refusal("resolve F3D WorkPoint scope stream");
}

#[test]
fn work_point_query_source_visit_preserves_original_refusal() {
    assert_query_refusal("visit F3D WorkPoint edge operands");
}

#[test]
fn work_point_query_identity_comparison_preserves_original_refusal() {
    assert_query_refusal("compare F3D WorkPoint operand identity");
}

#[test]
fn work_point_query_operand_stream_preserves_original_refusal() {
    assert_query_refusal("compare F3D WorkPoint operand stream");
}

#[test]
fn work_point_query_fixed_vertex_state_uses_no_positive_work() {
    let mut scope = scope();
    let DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() else {
        panic!("WorkPoint fixture");
    };
    *slot = Some(vertex_construction(true));
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            work_point_recipe_state_id(ctx, &scope, &[]).unwrap(),
            Some(4)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_query_native_states_keep_absence_and_agreement_in_source_order() {
    let absent = || DesignWorkPointInput::try_new(23, 0, None).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    let scope = native_scope(vec![absent(), vertex_input(4), vertex_input(4)]);
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            work_point_recipe_state_id(ctx, &scope, &[]).unwrap(),
            Some(4)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 3;
    let scope = native_scope(vec![absent(), absent()]);
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(work_point_recipe_state_id(ctx, &scope, &[]).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units = 1;
    let scope = native_scope(Vec::new());
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(work_point_recipe_state_id(ctx, &scope, &[]).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_query_native_conflict_stops_before_unvisited_input_tail() {
    let mut inputs = vec![
        DesignWorkPointInput::try_new(23, 0, None).unwrap(),
        vertex_input(4),
        vertex_input(5),
        edge_input(),
    ];
    inputs.extend((0..4096).map(|_| vertex_input(4)));
    let scope = native_scope(inputs);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(work_point_recipe_state_id(ctx, &scope, &[]).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = work_point_recipe_state_id(ctx, &scope, &[])
        else {
            panic!("third reached input must refuse");
        };
        assert_eq!(original.operation, "visit F3D WorkPoint history inputs");
        assert_eq!((original.used, original.additional), (2, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_query_native_input_refuses_before_its_carrier() {
    let scope = native_scope(vec![edge_input()]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            work_point_recipe_state_id(ctx, &scope, &[edge_operand(true)])
        else {
            panic!("first input must refuse before edge lookup");
        };
        assert_eq!(original.operation, "visit F3D WorkPoint history inputs");
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_query_real_input_topology_caller_keeps_history_input_refusal() {
    use cadmpeg_ir::features::{
        Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, UnresolvedFamily,
    };
    let scope = native_scope(vec![vertex_input(4)]);
    let feature = Feature {
        id: "f3d:model:feature#input".to_owned().try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            },
        )),
        native_ref: Some(scope.id.clone()),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let project = || {
            crate::history::project_feature_input_topologies(
                ctx,
                std::slice::from_ref(&feature),
                std::slice::from_ref(&scope),
                &[],
                &[],
            )
        };
        assert!(project().unwrap().is_empty());
        let _probe = RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "visit F3D WorkPoint history inputs",
            None,
        );
        let Err(CodecError::ResourceLimit(original)) = project() else {
            panic!("real caller must propagate input refusal");
        };
        assert_eq!(original.operation, "visit F3D WorkPoint history inputs");
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(project(), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn work_point_query_fixed_two_and_three_inputs_skip_positive_source_work() {
    let mut two = scope();
    let DesignScopePayloadMut::WorkPoint(slot) = two.payload_mut() else {
        panic!("WorkPoint fixture");
    };
    *slot = Some(construction(DesignWorkPointRuleForm::TwoEdgeIntersection {
        inputs: [
            DesignWorkPointInput::try_new(22, 0, None).unwrap(),
            DesignWorkPointInput::try_new(23, 0, None).unwrap(),
        ],
    }));
    let mut three = scope();
    let DesignScopePayloadMut::WorkPoint(slot) = three.payload_mut() else {
        panic!("WorkPoint fixture");
    };
    *slot = Some(super::plane_construction());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(work_point_recipe_state_id(ctx, &two, &[]).unwrap(), None);
        assert_eq!(work_point_recipe_state_id(ctx, &three, &[]).unwrap(), None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_query_duplicate_edge_state_is_absent_before_later_vertex_state() {
    let operand = edge_operand(true);
    let scope = native_scope(vec![edge_input(), vertex_input(9)]);
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        assert_eq!(
            work_point_recipe_state_id(ctx, &scope, &[operand.clone(), operand]).unwrap(),
            Some(9)
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

mod callback_refusal;
