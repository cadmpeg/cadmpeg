// SPDX-License-Identifier: Apache-2.0
//! Admission limits for historical state indexes and change chains.

use super::{
    edge_changes_across_state_chain, face_changes_across_state_chain,
    history_state_index, AsmDeltaState, AsmHistoricalTopology, AsmHistoricalEntityDelta,
    AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory, HashMap,
};
use crate::history::resolve_pattern_face_by_surface_radius;
use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalSurfaceRadius};
use std::collections::HashSet;

pub(super) fn change_state(state_id: i64) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(
            AsmHistoricalTopology::default(),
        ),
        transition: None,
    }
}

#[test]
fn history_state_index_refuses_collection_limit() {
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = history_state_index(Some(&ctx), &history).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D history states"));
}

#[test]
fn face_change_chain_refuses_visited_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    result.transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    });
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = face_changes_across_state_chain(Some(&ctx), &result, 1, &states).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D face change state chain"));
}

#[test]
fn face_change_chain_refuses_changed_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.faces.updated.push(10);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = face_changes_across_state_chain(Some(&ctx), &result, 1, &states).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D changed faces"));
}

#[test]
fn edge_change_chain_refuses_visited_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    result.transition = Some(AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    });
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = edge_changes_across_state_chain(Some(&ctx), &result, 1, &states).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D edge change state chain"));
}

#[test]
fn edge_change_chain_refuses_deleted_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.edges.deleted.push(20);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = edge_changes_across_state_chain(Some(&ctx), &result, 1, &states).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D deleted edges"));
}

#[test]
fn edge_change_chain_refuses_updated_collection_limit() {
    let preceding = change_state(1);
    let mut result = change_state(2);
    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: AsmHistoricalEntityDelta::default(),
        topology: AsmHistoricalTopologyDelta::default(),
    };
    transition.topology.edges.updated.push(20);
    result.transition = Some(transition);
    let states = HashMap::from([(1, Some(&preceding)), (2, Some(&result))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    let error = edge_changes_across_state_chain(Some(&ctx), &result, 1, &states).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D updated edges"));
}

fn pattern_face_limit_case(max_items: u64) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let candidate = crate::ids::brep_face_id(21);
    let preceding = AsmHistoricalTopology::default();
    let result = AsmHistoricalTopology {
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 21,
            carrier: 201,
        }],
        surface_radii: vec![AsmHistoricalSurfaceRadius {
            surface: 201,
            radius: 2.5,
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .unwrap();
    resolve_pattern_face_by_surface_radius(
        Some(&ctx),
        &[candidate],
        &preceding,
        &result,
        &HashSet::new(),
    )
}

#[test]
fn pattern_face_candidate_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(0).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern face candidates"));
}

#[test]
fn pattern_face_bound_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(1).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern bound faces"));
}
