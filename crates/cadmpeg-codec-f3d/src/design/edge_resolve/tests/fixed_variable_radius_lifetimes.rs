// SPDX-License-Identifier: Apache-2.0
//! Distinct typed radius candidates own their retained backing until acceptance.

use super::*;
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::refusal_probe::RefusalProbe;

const RADIUS_QUERY_SCRATCH: u64 = 4096;

fn variable_scope_with_position(position: f64) -> DesignParameterScope {
    let mut value = serde_json::to_value(fixed_scope()).unwrap();
    let group = &mut value["fixed_fillet_parameters"]["groups"][0];
    // Fixed lanes place both endpoints before the intermediate radius.
    group["radii"] = serde_json::json!([1.0, 2.0, 3.0]);
    group["radius_record_indexes"] = serde_json::json!([5, 6, 7]);
    group["radius_offsets"] = serde_json::json!([0, 8, 16]);
    group["intermediate_parameters"] = serde_json::json!([position]);
    group["intermediate_parameter_record_indexes"] = serde_json::json!([8]);
    group["intermediate_parameter_offsets"] = serde_json::json!([24]);
    serde_json::from_value(value).unwrap()
}

fn complete_operand() -> DesignEdgeOperand {
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.resolved_edge_slot = Some(17);
    operand
}

fn typed_radius_bytes() -> u64 {
    use cadmpeg_ir::features::edge_treatments::VariableRadius;
    use cadmpeg_ir::scalar::{Fraction, NonNegativeLength};
    // IntoIter has an exact three-element size hint. The installed constructor
    // reserves exactly three slots in a NEW typed vector, separately from raw.
    u64::try_from(3 * std::mem::size_of::<VariableRadius<Fraction, NonNegativeLength>>()).unwrap()
}

#[test]
fn fixed_variable_radius_rejected_order_refunds_actual_typed_backing() {
    // Native admits this intermediate parameter; neutral strict order rejects
    // [0, 0, 1] after the complete typed vector has actually been constructed.
    let scope = variable_scope_with_position(0.0);
    let group = group(2, 10);
    let operand = complete_operand();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = typed_radius_bytes();
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            assert!(project_fixed_fillet(&scope, std::slice::from_ref(&group), std::slice::from_ref(&operand), &[], ctx).unwrap().is_none());
            let all = ctx.reserve_scoped(RADIUS_QUERY_SCRATCH, "reuse rejected fixed variable radius scratch").unwrap();
            drop(all);
        }
        ctx.charge_retained(typed_radius_bytes(), "reuse rejected fixed typed radius retention").unwrap();
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn fixed_variable_radius_typed_allocation_refuses_one_below_before_conversion() {
    let scope = variable_scope_with_position(0.5);
    let group = group(2, 10);
    let operand = complete_operand();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = typed_radius_bytes() - 1;
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = project_fixed_fillet(&scope, std::slice::from_ref(&group), std::slice::from_ref(&operand), &[], ctx)
        else { panic!("typed radius allocation must refuse before conversion"); };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "IR variable radius admitted samples");
        assert_eq!((original.used, original.additional), (0, typed_radius_bytes()));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(project_fixed_fillet(&scope, &[], &[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn fixed_variable_radius_order_work_refusal_remains_original_after_candidate_drop() {
    let scope = variable_scope_with_position(0.5);
    let group = group(2, 10);
    let operand = complete_operand();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "IR variable radius parameter comparison", None);
        let Err(CodecError::ResourceLimit(original)) = project_fixed_fillet(&scope, std::slice::from_ref(&group), std::slice::from_ref(&operand), &[], ctx)
        else { panic!("typed radius comparison must refuse before execution"); };
        assert_eq!(original.operation, "IR variable radius parameter comparison");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(project_fixed_fillet(&scope, &[], &[], &[], ctx), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn fixed_variable_radius_allowed_projection_preserves_native_samples_and_edges() {
    let scope = variable_scope_with_position(0.5);
    let group = group(2, 10);
    let operand = complete_operand();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = RADIUS_QUERY_SCRATCH;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = project_fixed_fillet(&scope, std::slice::from_ref(&group), std::slice::from_ref(&operand), &[], ctx).unwrap().unwrap();
        let cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Fillet { groups }) = definition else { panic!("typed fixed Fillet"); };
        let [group] = groups.as_slice() else { panic!("one fixed radius group"); };
        let cadmpeg_ir::features::edge_treatments::RadiusSpec::Variable { points } = &group.radius else { panic!("variable radius samples"); };
        assert_eq!(points.as_slice().iter().map(|point| (point.parameter.get(), point.radius.get())).collect::<Vec<_>>(), [(0.0, 10.0), (0.5, 30.0), (1.0, 20.0)]);
        assert_eq!(group.tangency_weight.unwrap().get(), 1.0);
        let cadmpeg_ir::features::EdgeSelection::Historical { edges, native, .. } = &group.edges else { panic!("exact historical edge"); };
        assert_eq!(native.as_str(), "f3d:test:group-2");
        let prefix = crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("10:f3d%3Atest6:Fillet1:1"), 7);
        assert_eq!(edges.as_slice(), [crate::ids::history_input_edge_id(&prefix, 17)]);
        drop(groups);
        let all = ctx.reserve_scoped(RADIUS_QUERY_SCRATCH, "reuse allowed fixed variable radius scratch").unwrap();
        drop(all);
    });
}
