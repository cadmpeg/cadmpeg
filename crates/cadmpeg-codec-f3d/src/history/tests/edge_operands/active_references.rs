// SPDX-License-Identifier: Apache-2.0
//! Active edge reference reuse, native evidence and candidate lifetimes.

use crate::history::bind_active_edge_operand_candidates;
use crate::history_records::AsmHistoricalTopology;
use crate::records::dimensions::DesignRecipeReference;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::num::NonZeroU32;

fn operand(references: Vec<DesignRecipeReference>) -> DesignEdgeOperand {
    let mut operand: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:design-edge-operand#20",
        "scope_record_index": 10,
        "scope_reference_ordinal": 0,
        "record_index": 20,
        "byte_offset": 0,
        "class_tag": "297",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 23,
        "recipe_record_byte_offset": 32,
        "recipe_id": "f3d:test:construction-recipe#23",
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [],
        "changed_boundary_edge_slots": [],
        "deleted_boundary_edge_slots": [],
        "next_record_index": 24,
        "next_byte_offset": 160
    })).expect("edge operand framing");
    operand.recipe_references = references;
    operand
}

fn reference(face: Option<i64>) -> DesignRecipeReference {
    let mut reference = crate::history::test_support::one_face_reference();
    reference.candidate_faces = face.map(crate::ids::brep_face_id).into_iter().collect();
    reference
}

fn native_inputs() -> (DesignEdgeOperand, AsmHistoricalTopology) {
    let mut alternate = reference(None);
    alternate.alternate_selector_faces.push(crate::ids::brep_face_id(8));
    let operand = operand(vec![reference(Some(4)), alternate]);
    let mut topology = crate::history::test_support::edge_context_topology();
    topology.faces = vec![4, 8];
    topology.edges = vec![7];
    topology.face_loops.push(crate::history_records::AsmHistoricalRelation {
        owner_ref: 8,
        member_refs: vec![5],
    });
    (operand, topology)
}

fn ordinals(values: &[u32]) -> Vec<NonZeroU32> {
    values.iter().map(|value| NonZeroU32::new(*value).expect("nonzero ordinal")).collect()
}

#[test]
fn active_edge_absent_local_references_preserve_native_identity_and_exact_alternate_order() {
    let (mut operand, topology) = native_inputs();
    let original_id = operand.id.clone();
    let original_references = operand.recipe_references.clone();
    crate::test_support::with_decode_context(|ctx| {
        bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]).expect("active edge");
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(operand.id, original_id);
    assert_eq!(operand.recipe_references, original_references);
    assert_eq!(operand.recipe_state_id, Some(99));
    assert_eq!(operand.resolved_edge_slot, Some(7));
    assert_eq!(operand.terminal_candidate_faces, vec![crate::ids::brep_face_id(4), crate::ids::brep_face_id(8)]);
    assert_eq!(operand.terminal_boundary_edge_slots, vec![7]);
    assert_eq!(operand.terminal_reference_edge_slots, vec![vec![7], vec![7]]);
    assert_eq!(operand.terminal_boundary_edge_contexts.len(), 1);
    let context = &operand.terminal_boundary_edge_contexts[0];
    assert_eq!(context.edge_slot, 7);
    assert_eq!(context.incident_loops.len(), 1);
    let incident = &context.incident_loops[0];
    assert_eq!((incident.coedge_slot, incident.loop_slot, incident.face_slot), (6, 5, 4));
    assert_eq!((incident.boundary_edge_count, incident.coedge_ordinal), (1, 0));
    assert_eq!((incident.previous_edge_slot, incident.next_edge_slot), (7, 7));
    assert!(operand.recipe_selectors.is_empty());
}

#[test]
fn active_edge_explicit_local_ordinals_keep_subset_duplicate_and_empty_semantics() {
    for (selected, faces, edge) in [
        (vec![2], vec![8], None),
        (vec![2, 1, 2], vec![4, 8], Some(7)),
        (Vec::new(), Vec::new(), None),
        (vec![3], Vec::new(), None),
    ] {
        let (mut operand, topology) = native_inputs();
        operand.local_topology_references = Some(ordinals(&selected));
        crate::test_support::with_decode_context(|ctx| {
            bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]).expect("explicit local references");
        });
        assert_eq!(operand.local_topology_references, Some(ordinals(&selected)));
        assert_eq!(operand.terminal_candidate_faces, faces.into_iter().map(crate::ids::brep_face_id).collect::<Vec<_>>());
        assert_eq!(operand.terminal_reference_edge_slots, vec![vec![7], vec![7]]);
        assert_eq!(operand.recipe_state_id, Some(99));
        assert_eq!(operand.resolved_edge_slot, edge);
    }
}

#[test]
fn active_edge_absent_local_reference_pays_each_group_once_and_retains_only_final_sets() {
    let mut operand = operand(vec![reference(None)]);
    let topology = AsmHistoricalTopology::default();
    // One copied reference group plus one final edge-set group. The two Vecs
    // each allocate the amortized minimum of four slots; only edge sets escape.
    let backing = 4 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Vec<i64>>());
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    policy.limits.max_retained_bytes = backing;
    policy.limits.max_materialized_bytes = 2 * backing;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]).expect("each group once");
        assert_eq!(operand.terminal_reference_edge_slots, vec![Vec::<i64>::new()]);
        let scratch = ctx.reserve_scoped(2 * backing, "after active edge scratch drops").expect("all scratch released while native output lives");
        drop(scratch);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_retained(1, "after final active edge sets") else { panic!("exact retained backing") };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.used, backing);
        assert_eq!(original.additional, 1);
    });
    assert_eq!(operand.recipe_state_id, Some(99));
    assert_eq!(operand.resolved_edge_slot, None);
}

#[test]
fn active_edge_one_below_group_count_preserves_original_refusal_and_native_input() {
    let mut operand = operand(vec![reference(None)]);
    let original_operand = operand.clone();
    let topology = AsmHistoricalTopology::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]) else { panic!("second actual group refuses") };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "collect F3D reference edge sets");
        assert_eq!(original.used, 1);
        assert_eq!(original.additional, 1);
        assert_eq!(operand, original_operand);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        let Err(CodecError::ResourceLimit(sticky)) = bind_active_edge_operand_candidates(ctx, &mut operand, &[]) else { panic!("original refusal before empty source") };
        assert_eq!(sticky, original);
        assert_eq!(operand, original_operand);
    });
}

#[test]
fn active_edge_explicit_empty_local_lane_remains_a_separate_empty_query() {
    let mut operand = operand(vec![reference(None)]);
    operand.local_topology_references = Some(Vec::new());
    let topology = AsmHistoricalTopology::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]).expect("explicit empty selection");
        assert_eq!(ctx.resource_refusal(), None);
    });
    assert_eq!(operand.local_topology_references, Some(Vec::new()));
    assert_eq!(operand.terminal_reference_edge_slots, vec![Vec::<i64>::new()]);
    assert_eq!(operand.recipe_state_id, Some(99));
    assert_eq!(operand.resolved_edge_slot, None);
}

#[test]
fn active_edge_ambiguous_topologies_release_discarded_candidates_across_repeated_calls() {
    let (mut operand, topology) = native_inputs();
    let original_operand = operand.clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 8192;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology), (100, &topology)]).expect("ambiguous candidate scratch released");
            assert_eq!(operand, original_operand);
            let scratch = ctx.reserve_scoped(8192, "after ambiguous active edge candidates").expect("both complete candidates destroyed before refunds");
            drop(scratch);
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn active_edge_actual_query_and_commit_phases_preserve_original_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_collection_items = u64::MAX;
    policy.limits.max_retained_bytes = u64::MAX;
    for (dimension, operation, additional) in [
        (ResourceDimension::WorkUnits, "scan F3D terminal edge recipe references", Some(2)),
        (ResourceDimension::WorkUnits, "copy F3D terminal reference faces", Some(1)),
        (ResourceDimension::CollectionItems, "collect F3D terminal reference groups", Some(1)),
        (ResourceDimension::WorkUnits, "scan F3D terminal reference face groups", Some(2)),
        (ResourceDimension::WorkUnits, "scan F3D topology faces", Some(2)),
        (ResourceDimension::CollectionItems, "collect F3D faces in topology", Some(1)),
        (ResourceDimension::CollectionItems, "collect F3D boundary edges", Some(1)),
        (ResourceDimension::WorkUnits, "collect F3D historical incident loops", Some(1)),
        (ResourceDimension::WorkUnits, "scan F3D recipe reference faces", Some(2)),
        (ResourceDimension::CollectionItems, "collect F3D reference edge sets", Some(1)),
        (ResourceDimension::WorkUnits, "scan F3D shared edge sets", Some(1)),
        (ResourceDimension::RetainedBytes, "collect F3D active edge candidate", None),
    ] {
        let (mut operand, topology) = native_inputs();
        let original_operand = operand.clone();
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(dimension, operation, additional);
            let Err(CodecError::ResourceLimit(original)) = bind_active_edge_operand_candidates(ctx, &mut operand, &[(99, &topology)]) else { panic!("reached active edge phase {operation}") };
            assert_eq!(original.operation, operation);
            assert_eq!(original.dimension, dimension);
            assert!(original.additional > 0);
            if let Some(additional) = additional { assert_eq!(original.additional, additional); }
            assert_eq!(operand, original_operand);
            assert_eq!(ctx.resource_refusal(), Some(original.clone()));
            let Err(CodecError::ResourceLimit(sticky)) = bind_active_edge_operand_candidates(ctx, &mut operand, &[]) else { panic!("sticky active edge phase") };
            assert_eq!(sticky, original);
            assert_eq!(operand, original_operand);
        });
    }
}
