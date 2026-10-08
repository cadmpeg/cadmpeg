// SPDX-License-Identifier: Apache-2.0

use crate::history::test_support::{active_body, base_feature, output_binding_inputs};
fn empty_transition_history() -> crate::history_records::AsmHistory {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
        AsmHistoricalTransition, AsmHistory, AsmTopologyCache,
    };
    AsmHistory {
        id: "f3d:asm-history#1".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:asm-delta-state#1".into(),
            parent: "f3d:asm-history#1".into(),
            byte_offset: 0,
            state_id: 1,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology::default()),
            transition: Some(AsmHistoricalTransition {
                previous_state_id: None,
                records: AsmHistoricalEntityDelta::default(),
                topology: AsmHistoricalTopologyDelta::default(),
            }),
        }],
    }
}

fn feature_output_error(
    histories: &[crate::history_records::AsmHistory],
    bodies: &[cadmpeg_ir::topology::Body],
    max_items: u64,
    max_materialized_bytes: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = u64::MAX;
    // Temporary copied body IDs stay in the live scoped reservation.
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::bind_feature_outputs(&ctx, &mut [], &[], histories, bodies).unwrap_err()
}

#[test]
fn feature_output_history_nodes_refuse_collection_limit() {
    let history = empty_transition_history();
    let error = feature_output_error(&[history], &[], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D feature output history nodes")
    );
}

#[test]
fn feature_output_states_refuse_collection_limit() {
    let operation = "index F3D feature output states";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let history = empty_transition_history();
            Err::<(), _>(feature_output_error(&[history], &[], cap, u64::MAX))
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn feature_output_active_body_refuses_collection_limit() {
    let body = active_body();
    let error = feature_output_error(&[], &[body], 0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D active feature output bodies")
    );
}

fn base_feature_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::bind_base_feature_output_selection(&ctx, &mut base_feature()).unwrap_err()
}

#[test]
fn base_feature_output_bodies_refuse_collection_limit() {
    let error = base_feature_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D BaseFeature output bodies")
    );
}

#[test]
fn base_feature_output_body_id_refuses_retained_limit() {
    let error = base_feature_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D BaseFeature body identity")
    );
}

#[test]
fn base_feature_native_selection_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D BaseFeature native selection",
        |cap| Err::<(), cadmpeg_core::CodecError>(base_feature_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D BaseFeature native selection")
    );
}

fn bound_output_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (mut feature, scope, history, body) = output_binding_inputs();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::bind_feature_outputs(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[scope],
        &[history],
        &[body],
    )
    .unwrap_err()
}

#[test]
fn feature_output_bodies_refuse_collection_limit() {
    let operation = "collect F3D feature output bodies";
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        |cap| Err::<(), _>(bound_output_error(cap, u64::MAX)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn feature_output_body_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D feature output body identity",
        |cap| Err::<(), cadmpeg_core::CodecError>(bound_output_error(u64::MAX, cap)),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D feature output body identity")
    );
}

#[test]
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    let ctx = cadmpeg_test_support::service_decode_context();
    crate::history::bind_feature_outputs(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[scope],
        &[history],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().len(), 1);
    assert_eq!(feature.evaluation.outputs()[0], body.id);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == "native:scope"));
}

#[test]
fn affected_history_body_scan_refuses_work() {
    let operation = "scan F3D affected history bodies";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let (mut feature, scope, history, body) = output_binding_inputs();
            crate::history::bind_feature_outputs(
                ctx,
                std::slice::from_mut(&mut feature),
                &[scope],
                &[history],
                &[body],
            )
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn feature_output_binding_uses_first_scope_with_repeated_id() {
    let (mut feature, scope, history, body) = output_binding_inputs();
    let other = crate::records::feature::scope::DesignParameterScope::empty(
        &scope.id,
        crate::records::feature::scope::DesignFeatureKind::Combine,
        scope.record_index + 1,
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    crate::history::bind_feature_outputs(
        &ctx,
        std::slice::from_mut(&mut feature),
        &[scope, other],
        &[history],
        std::slice::from_ref(&body),
    ).unwrap();
    assert_eq!(feature.evaluation.outputs().as_slice(), &[body.id]);
}
