// SPDX-License-Identifier: Apache-2.0
//! Admission before native input inspection and actual vertex mutation.

use super::{DesignVertexRecipe, DesignVertexRecipeDraft, DesignVertexResolution, DesignWorkPointConstruction, DesignWorkPointInput, DesignWorkPointInputCarrier, DesignWorkPointRule};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn vertex_input(state_id: i64) -> DesignWorkPointInput {
    let recipe = DesignVertexRecipe::try_new(DesignVertexRecipeDraft {
        record_index: 22, byte_offset: 0,
        class_tag: "369".to_owned().try_into().expect("class tag"),
        paired_byte_offset: 16, paired_class_tag: "261".to_owned().try_into().expect("paired class tag"),
        recipe_record_index: 25, recipe_record_byte_offset: 32,
        recipe_id: "f3d:native/BulkStream.dat:construction-recipe#vertex".to_owned(),
        recipe_prefix_offset: 43, recipe_prefix_bytes: Vec::new(), recipe_references: Vec::new(),
        recipe_program_offset: 4, recipe_program: vec![0],
        resolution: DesignVertexResolution::new(state_id, 43),
        next_record_index: 27, next_byte_offset: 200,
    }).expect("vertex envelope");
    DesignWorkPointInput::try_new(22, 0, Some(Box::new(DesignWorkPointInputCarrier::VertexRecipe { recipe }))).expect("vertex input")
}

fn absent() -> DesignWorkPointInput {
    DesignWorkPointInput::try_new(23, 0, None).expect("absent carrier")
}

fn native_rule(inputs: Vec<DesignWorkPointInput>) -> DesignWorkPointRule {
    DesignWorkPointRule::from_serialized(99, inputs).expect("native rule")
}

#[test]
fn work_point_mutation_native_inputs_preserve_actual_vertex_order() {
    let mut rule = native_rule(vec![absent(), vertex_input(4), absent(), vertex_input(9)]);
    let mut policy = DecodePolicy::service();
    // Four reached raw slots and the iterator's end probe.
    policy.limits.max_work_units = 5;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut seen = Vec::new();
        rule.for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |recipe| {
            seen.push(recipe.resolution.expect("resolved fixture").state_id);
            recipe.resolution = None;
            Ok(())
        }).expect("admitted inputs");
        assert_eq!(seen, [4, 9]);
        assert!(rule.vertex_recipes_mut().all(|recipe| recipe.resolution.is_none()));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_mutation_refuses_unmatched_prefix_before_vertex_mutation() {
    let mut inputs: Vec<_> = (0..4096).map(|_| absent()).collect();
    inputs.push(vertex_input(4));
    let mut rule = native_rule(inputs);
    let before = rule.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visited = false;
        let Err(CodecError::ResourceLimit(original)) = rule.for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| { visited = true; Ok(()) }) else { panic!("vertex slot must refuse"); };
        assert_eq!((original.used, original.additional), (4096, 1));
        assert_eq!(original.operation, "visit native mutation fixture");
        assert!(!visited);
        assert_eq!(rule, before);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        let mut fixed = DesignWorkPointRule::from_serialized(10, vec![vertex_input(4)]).expect("fixed rule");
        assert!(matches!(fixed.for_each_vertex_recipe_for_decode(ctx, "later fixed mutation fixture", |_| { panic!("fused callback"); }), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn work_point_mutation_absent_native_inputs_pay_end_probe() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4097;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        native_rule((0..4096).map(|_| absent()).collect()).for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| { panic!("absent carrier callback"); }).expect("raw slots and end admitted");
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = native_rule((0..4096).map(|_| absent()).collect()).for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| { panic!("absent carrier callback"); }) else { panic!("end probe must refuse"); };
        assert_eq!((original.used, original.additional), (4096, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_mutation_empty_native_inputs_keep_original_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = native_rule(Vec::new()).for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| { panic!("empty callback"); }) else { panic!("native end probe must refuse"); };
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(native_rule(Vec::new()).for_each_vertex_recipe_for_decode(ctx, "later native mutation fixture", |_| Ok(())), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn work_point_mutation_fixed_arity_has_no_positive_input_work() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut rule = DesignWorkPointRule::from_serialized(10, vec![vertex_input(4)]).expect("fixed vertex");
        let mut visited = 0;
        rule.for_each_vertex_recipe_for_decode(ctx, "visit fixed mutation fixture", |recipe| { visited += 1; recipe.resolution = None; Ok(()) }).expect("fixed input is free");
        assert_eq!(visited, 1);
        for (kind, count) in [(7, 2), (8, 3)] {
            DesignWorkPointRule::from_serialized(kind, (0..count).map(|_| absent()).collect()).expect("fixed absent rule").for_each_vertex_recipe_for_decode(ctx, "visit fixed mutation fixture", |_| { panic!("nonvertex callback"); }).expect("fixed inputs are free");
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_mutation_callback_refusal_stops_before_native_tail() {
    let mut inputs = vec![vertex_input(4)];
    inputs.extend((0..4096).map(|_| vertex_input(9)));
    let mut rule = native_rule(inputs);
    let before = rule.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        let Err(CodecError::ResourceLimit(original)) = rule.for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| { visits += 1; ctx.charge_work(1, "original vertex callback refusal") }) else { panic!("callback must refuse"); };
        assert_eq!(visits, 1);
        assert_eq!(rule, before);
        assert_eq!(original.operation, "original vertex callback refusal");
        assert_eq!((original.used, original.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn scope(rule: DesignWorkPointRule) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut};
    let mut scope = DesignParameterScope::empty("f3d:native/BulkStream.dat:parameter-scope#20", DesignFeatureKind::WorkPoint, 20);
    let DesignScopePayloadMut::WorkPoint(slot) = scope.payload_mut() else { panic!("WorkPoint scope"); };
    *slot = Some(DesignWorkPointConstruction {
        point_record_index: 21, point_record_byte_offset: 0,
        position: crate::test_support::reals([0.0, 0.0, 0.0]), position_offset: 0,
        rule, reference_type_offset: 0,
    });
    scope
}

fn assert_real_binding_refusal(operation: &'static str, history: bool) {
    let mut scopes = [scope(native_rule(vec![absent(), vertex_input(4)]))];
    let before = scopes.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let error = if history {
            crate::history::bind_vertex_recipe_history(ctx, &mut scopes, &[], &[])
        } else {
            crate::design::decode::operands::bind_vertex_recipe_candidates(ctx, &mut scopes, &[])
        };
        let Err(CodecError::ResourceLimit(original)) = error else { panic!("actual raw input must refuse"); };
        assert_eq!(original.operation, operation);
        assert_eq!(original.additional, 1);
        assert_eq!(scopes, before);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(scopes[0].work_point_construction_mut().expect("construction").rule.for_each_vertex_recipe_for_decode(ctx, "later actual mutation fixture", |_| { panic!("fused callback"); }), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn work_point_mutation_real_candidate_binding_refuses_absent_input_before_mutation() {
    assert_real_binding_refusal("visit F3D WorkPoint candidate inputs", false);
}

#[test]
fn work_point_mutation_real_history_binding_refuses_absent_input_before_resolution_reset() {
    assert_real_binding_refusal("visit F3D WorkPoint vertex history inputs", true);
}

#[test]
fn work_point_mutation_real_history_binding_clears_all_native_vertex_resolutions_without_history() {
    let mut scopes = [scope(native_rule(vec![absent(), vertex_input(4), absent(), vertex_input(9)]))];
    crate::test_support::with_decode_policy(&DecodePolicy::service(), |ctx| {
        crate::history::bind_vertex_recipe_history(ctx, &mut scopes, &[], &[]).expect("no history clears resolutions");
        let construction = scopes[0].work_point_construction_mut().expect("construction");
        assert_eq!(construction.rule.inputs().len(), 4);
        assert!(construction.rule.vertex_recipes_mut().all(|recipe| recipe.resolution.is_none()));
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn work_point_mutation_callback_cannot_hide_original_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut rule = DesignWorkPointRule::from_serialized(10, vec![vertex_input(4)]).expect("fixed vertex");
        let mut original = None;
        let error = rule.for_each_vertex_recipe_for_decode(ctx, "visit fixed mutation fixture", |_| {
            let Err(CodecError::ResourceLimit(refusal)) = ctx.charge_work(1, "original swallowed callback refusal") else { panic!("callback must refuse"); };
            original = Some(refusal);
            Ok(())
        });
        let original = original.expect("actual callback refusal");
        assert!(matches!(error, Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn work_point_mutation_callback_error_cannot_replace_original_refusal() {
    let mut rule = native_rule(vec![vertex_input(4), vertex_input(9)]);
    let before = rule.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visits = 0;
        let mut original = None;
        let error = rule.for_each_vertex_recipe_for_decode(ctx, "visit native mutation fixture", |_| {
            visits += 1;
            let Err(CodecError::ResourceLimit(refusal)) = ctx.charge_work(1, "original replaced callback refusal") else { panic!("callback must refuse"); };
            original = Some(refusal);
            Err(CodecError::malformed("later callback error"))
        });
        let original = original.expect("actual callback refusal");
        assert!(matches!(error, Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!((original.used, original.additional), (1, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert_eq!(visits, 1);
        assert_eq!(rule, before);
    });
}
