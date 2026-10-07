// SPDX-License-Identifier: Apache-2.0
//! Admission limits for historical state indexes and change chains.

use crate::history::resolve_pattern_face_by_surface_radius;
use crate::history::selection::{
    bind_entity_selection_history, historical_identity_edge,
    historical_pattern_identity_axes_for_selection, unique_entity_selection_edge,
    HistoricalIdentityIndex,
};
use crate::history::{
    collect_reference_edge_sets, face_boundary_contexts_for_slots, face_boundary_edge_index,
    face_boundary_edges, historical_edge_axis, historical_face_support_contexts,
    historical_loop_boundary, preceding_support_face_slots, selection::boundary_edges_in_changes,
    selection::faces_in_topology, selection::historical_edge_context,
    selection::recipe_selector_candidates, terminal_edge_recipe_faces,
    terminal_edge_recipe_reference_faces, treatment_edge_candidates, treatment_face_supports,
};
use crate::history::{
    edge_changes_across_state_chain, face_changes_across_state_chain, history_state_index,
    unique_history_state,
};
use crate::history_records::{
    AsmBulletinBoard, AsmEntityChange, AsmEntityChangeKind, AsmHistoricalCarrierBinding,
    AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalSurfaceRadius,
};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalEntityDelta, AsmHistoricalTopology, AsmHistoricalTopologyDelta,
    AsmHistoricalTransition, AsmHistory,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::HashMap;
use std::collections::HashSet;

fn recipe_selector_storage_fixture() -> (
    crate::records::topology::edge_recipe::DesignEdgeRecipeStructure,
    Vec<crate::records::topology::historical_context::DesignHistoricalEdgeContext>,
) {
    use crate::records::topology::edge_recipe::{
        DesignEdgeRecipeStructure, DesignTopologyRecipeEntry, DesignTopologyRecipeSide,
        DesignTopologyRecipeTriplet,
    };
    use crate::records::topology::historical_context::{
        DesignHistoricalEdgeContext, DesignHistoricalEdgeLoopContext,
    };

    let triplet = || DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(1).unwrap(),
        middle: 0,
        incident: None,
    };
    let structure = DesignEdgeRecipeStructure {
        root: 1,
        sides: vec![DesignTopologyRecipeSide {
            header_value: 0,
            scalars: Vec::new(),
            payload_prefix: Vec::new(),
            entries: vec![DesignTopologyRecipeEntry {
                selector: 1,
                boundary_edge_count: std::num::NonZeroU32::new(1).unwrap(),
                topology_triplets: [triplet(), triplet()],
            }],
        }],
    };
    let contexts = vec![DesignHistoricalEdgeContext {
        edge_slot: 1,
        incident_loops: vec![DesignHistoricalEdgeLoopContext {
            coedge_slot: 2,
            loop_slot: 3,
            face_slot: 4,
            boundary_edge_count: 1,
            coedge_ordinal: 0,
            previous_edge_slot: 5,
            next_edge_slot: 6,
        }],
    }];
    (structure, contexts)
}

#[test]
fn recipe_selector_temporary_vectors_refuse_materialized_limit() {
    for operation in [
        "collect F3D recipe side counts",
        "collect F3D incident loop counts",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            operation,
            0,
            |decode| {
                let (structure, contexts) = recipe_selector_storage_fixture();
                recipe_selector_candidates(decode, Some(&structure), &contexts).map(|_| ())
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn identity_index_refuses_history_collection_limit() {
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = HistoricalIdentityIndex::build(
        &ctx,
        &[history],
        ctx.admit_iter(&[1], "scan F3D identity local IDs")
            .expect("test identity local ID admission"),
        |local_id| std::iter::once(*local_id).chain(None),
    )
    .err()
    .expect("limit refusal");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D identity histories")
    );
}

fn complete_record_binding_fixture() -> (Vec<u8>, Vec<AsmDeltaState>) {
    use crate::history_records::{AsmHistoryRecord, AsmHistoryRecordFraming};
    let mut bytes = crate::test_support::smbh_header_test::smbh_header_prefix();
    let start = bytes.len();
    crate::test_support::tokens_test::t_ident(&mut bytes, "body");
    crate::test_support::tokens_test::t_end(&mut bytes);
    let framed = cadmpeg_asm::test_support::sab::frame(
        &bytes,
        start,
        bytes.len(),
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let record = &framed[0];
    let limit = record.offset.checked_add(record.len).unwrap();
    let record_bytes = bytes[record.offset..limit].to_vec();
    let state_id = "complete-state".to_owned();
    let state = AsmDeltaState {
        id: state_id.clone(),
        parent: "history".to_owned(),
        byte_offset: 0,
        state_id: 1,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 1,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: vec![AsmHistoryRecord {
            id: "record".to_owned(),
            parent: state_id,
            revision_id: Some(1),
            byte_offset: cadmpeg_core::decode::u64_from_index(record.offset),
            framing: AsmHistoryRecordFraming::Framed {
                index: cadmpeg_core::decode::u64_from_index(record.index),
                name: record.name.clone(),
                entity_references: Vec::new(),
            },
            raw_bytes: record_bytes,
        }],
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Absent,
        transition: None,
    };
    (bytes, vec![state])
}

fn complete_record_binding_refusal(operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            let (bytes, mut states) = complete_record_binding_fixture();
            crate::history::bind_complete_record_tables(
                ctx,
                &mut states,
                &bytes,
                cadmpeg_asm::kernel_header::RefWidth::Eight,
                &cadmpeg_core::decode::ResourceLimits::service(),
            )
            .map(|_| ())
        },
    )
}

#[test]
fn complete_record_state_scan_refuses_work() {
    let operation = "scan F3D complete record states";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_state_record_scan_refuses_work() {
    let operation = "scan F3D complete state records";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_record_byte_comparison_refuses_work() {
    let operation = "compare F3D archived record bytes";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn complete_record_name_comparison_refuses_work() {
    let operation = "compare F3D archived record names";
    let error = complete_record_binding_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn identity_index_refuses_local_id_scan_work() {
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D identity local IDs",
        0,
        |decode| {
            let local_ids = decode
                .admit_iter(&[1], "scan F3D identity local IDs")
                .map_err(cadmpeg_core::CodecError::ResourceLimit)?;
            HistoricalIdentityIndex::build(decode, &[], local_ids, |local_id| {
                std::iter::once(*local_id).chain(None)
            })
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D identity local IDs"
    ));
}

#[test]
fn entity_selection_previous_state_comparison_propagates_work_refusal() {
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#42"),
        crate::records::feature::scope::DesignScopePayload::Sweep(None),
        42,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let history = AsmHistory {
        id: format!("{stream}/BREP.selection.smbh:asm-1"),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    let scopes = [scope];
    let histories = [history];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D selection previous state ID",
        0,
        |decode| {
            let mut operands = [
                crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
                    crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                        id: format!("{stream}:design-entity-selection-operand#200"),
                        scope_record_index: 42,
                        group_record_index: 100,
                        group_member_ordinal: 0,
                        record_index: 200,
                        byte_offset: 0,
                        class_tag: "377".to_owned().try_into().unwrap(),
                        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        asset_id_offset: 0,
                        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        context_id_offset: 0,
                        identity_record_index: 203,
                        identity_record_offset: 0,
                        primary_identity: 7,
                        primary_identity_offset: 21,
                        secondary: None,
                        historical_edge_candidates: Vec::new(),
                        historical_face_candidates: Vec::new(),
                        resolved_edge_slot: None,
                        next_record_index: 202,
                        next_byte_offset: 29,
                    },
                )
                .unwrap(),
            ];
            bind_entity_selection_history(decode, &mut operands, &scopes, &histories)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compare F3D selection previous state ID"
    ));
}

#[test]
fn identity_edges_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        edges: vec![7],
        ..Default::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        historical_identity_edge(&ctx, AsmHistoricalEntityKind::Edge, 7, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D identity edges")
    );
}

#[test]
fn identity_edge_membership_propagates_work_refusal() {
    let topology = AsmHistoricalTopology {
        edges: vec![7],
        ..Default::default()
    };
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D identity edge",
        0,
        |decode| {
            historical_identity_edge(decode, AsmHistoricalEntityKind::Edge, 7, &topology)
                .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D identity edge")
    );
}

#[test]
fn edge_axis_candidate_scan_propagates_work_refusal() {
    let mut state = change_state(3);
    state.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            edges: vec![7],
            ..Default::default()
        });
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let state_ids = [3];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D edge axis candidates",
        0,
        |decode| {
            historical_pattern_identity_axes_for_selection(
                decode,
                Some((AsmHistoricalEntityKind::Edge, 7, &state_ids)),
                &history,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D edge axis candidates")
    );
}

#[test]
fn pattern_identity_face_surface_binding_scan_propagates_work_refusal() {
    let mut state = change_state(3);
    state.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            faces: vec![11],
            face_surfaces: vec![crate::history_records::AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 41,
            }],
            surface_axes: vec![crate::history_records::AsmHistoricalSurfaceAxis {
                surface: 41,
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
            }],
            ..Default::default()
        });
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let state_ids = [3];
    let operation = "find F3D pattern face surface binding";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| {
            historical_pattern_identity_axes_for_selection(
                decode,
                Some((AsmHistoricalEntityKind::Face, 11, &state_ids)),
                &history,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn pattern_identity_surface_axis_scan_propagates_work_refusal() {
    let mut state = change_state(3);
    state.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            surface_axes: vec![crate::history_records::AsmHistoricalSurfaceAxis {
                surface: 41,
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
            }],
            ..Default::default()
        });
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let state_ids = [3];
    let operation = "find F3D pattern surface axis";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| {
            historical_pattern_identity_axes_for_selection(
                decode,
                Some((AsmHistoricalEntityKind::Surface, 41, &state_ids)),
                &history,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn pattern_identity_surface_plane_scan_propagates_work_refusal() {
    let mut state = change_state(3);
    state.topology_cache =
        crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            surface_planes: vec![crate::history_records::AsmHistoricalPlane {
                surface: 41,
                origin: cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0),
                normal: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 2.0),
            }],
            ..Default::default()
        });
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![state],
    };
    let state_ids = [3];
    let operation = "find F3D pattern surface plane";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| {
            historical_pattern_identity_axes_for_selection(
                decode,
                Some((AsmHistoricalEntityKind::Surface, 41, &state_ids)),
                &history,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

fn edge_context_topology() -> AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalRelation,
        AsmHistoricalSurfaceAxis,
    };
    AsmHistoricalTopology {
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 5,
            member_refs: vec![6],
        }],
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 4,
            member_refs: vec![5],
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 8,
        }],
        surface_axes: vec![AsmHistoricalSurfaceAxis {
            surface: 8,
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
        }],
        ..Default::default()
    }
}

#[test]
fn historical_edge_context_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_edge_context(&ctx, 7, &edge_context_topology()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical incident loops")
    );
}

#[test]
fn historical_edge_axis_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_edge_axis(&ctx, 7, &edge_context_topology()).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical incident loops")
    );
}

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
fn released_projection_cache_scan_propagates_work_refusal() {
    // Clearing released entity versions is free; the state scan pays the work.
    let mut history = two_state_entity_history();
    for state in &mut history.states {
        state.topology_cache = crate::history_records::AsmTopologyCache::Released;
    }
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D projection cache states",
        0,
        |decode| {
            let mut histories = [history.clone()];
            crate::history::discard_projection_caches(decode, &mut histories)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "scan F3D projection cache states"
    ));
    let mut histories = [history];
    crate::test_support::with_decode_context(|decode| {
        crate::history::discard_projection_caches(decode, &mut histories)
    })
    .unwrap();
    assert!(histories[0]
        .states
        .iter()
        .all(|state| state.entity_versions.is_empty()));
}

#[test]
fn historical_entity_version_retain_propagates_work_refusal() {
    let mut history = two_state_entity_history();
    for state in &mut history.states {
        state.topology_cache =
            crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
                faces: vec![42],
                face_loops: vec![crate::history_records::AsmHistoricalRelation {
                    owner_ref: 42,
                    member_refs: vec![1],
                }],
                ..Default::default()
            });
    }
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "retain F3D historical entity versions",
        0,
        |decode| {
            let mut histories = [history.clone()];
            crate::history::discard_projection_caches(decode, &mut histories)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "retain F3D historical entity versions"
    ));
}

#[test]
fn historical_entity_version_membership_propagates_work_refusal() {
    let mut history = two_state_entity_history();
    for state in &mut history.states {
        state.topology_cache =
            crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
                faces: vec![42],
                face_loops: vec![crate::history_records::AsmHistoricalRelation {
                    owner_ref: 42,
                    member_refs: vec![1],
                }],
                ..Default::default()
            });
    }
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D historical entity version slot",
        0,
        |decode| {
            let mut histories = [history.clone()];
            crate::history::discard_projection_caches(decode, &mut histories)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "find F3D historical entity version slot"
    ));
}

fn two_state_entity_history() -> AsmHistory {
    let make_state = |state_id| {
        let mut state = change_state(state_id);
        state.entity_versions = vec![crate::history_records::AsmEntityVersion {
            entity_ref: 42,
            record_ref: 700,
        }];
        state.topology_cache =
            crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
                faces: vec![42],
                ..Default::default()
            });
        state
    };
    AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![make_state(3), make_state(5)],
    }
}

#[test]
fn identity_revision_state_membership_propagates_work_refusal() {
    let history = two_state_entity_history();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D revision membership state",
        0,
        |decode| {
            HistoricalIdentityIndex::build(
                decode,
                std::slice::from_ref(&history),
                decode.admit_iter(&[700], "scan F3D identity local IDs")?,
                |local_id| std::iter::once(*local_id).chain(None),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D revision membership state")
    );
}

#[test]
fn identity_membership_state_propagates_work_refusal() {
    let history = two_state_entity_history();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D identity membership state",
        0,
        |decode| {
            HistoricalIdentityIndex::build(
                decode,
                std::slice::from_ref(&history),
                decode.admit_iter(&[700], "scan F3D identity local IDs")?,
                |local_id| std::iter::once(*local_id).chain(None),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D identity membership state")
    );
}

#[test]
fn identity_index_history_limit_scan_propagates_work_refusal() {
    let histories = [AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: Vec::new(),
    }];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D identity history limits",
        0,
        |decode| {
            HistoricalIdentityIndex::build(
                decode,
                &histories,
                decode.admit_iter(&[1], "scan F3D identity local IDs")?,
                |local_id| std::iter::once(*local_id).chain(None),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D identity history limits")
    );
}

#[test]
fn identity_index_bulletin_scan_propagates_work_refusal() {
    let mut history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    };
    history.states[0].bulletin_boards.push(AsmBulletinBoard {
        id: "board".into(),
        parent: "state".into(),
        byte_offset: 0,
        owner_ref: 1,
        number: 0,
        changes: vec![AsmEntityChange {
            id: "change".into(),
            parent: "board".into(),
            byte_offset: 0,
            kind: AsmEntityChangeKind::Update { old: 1, new: 2 },
        }],
    });
    let histories = [history];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "scan F3D identity bulletin changes",
        0,
        |decode| {
            HistoricalIdentityIndex::build(
                decode,
                &histories,
                decode.admit_iter(&[1], "scan F3D identity local IDs")?,
                |local_id| std::iter::once(*local_id).chain(None),
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "scan F3D identity bulletin changes")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = history_state_index(&ctx, &history).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D history states")
    );
}

#[test]
fn unique_history_state_propagates_work_refusal() {
    let histories = [AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![change_state(1)],
    }];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D history state",
        0,
        |decode| unique_history_state(decode, &histories, 1).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D history state")
    );
}

#[test]
fn unique_entity_selection_edge_propagates_membership_refusal() {
    use crate::records::topology::entity_selection::DesignEntitySelectionEdgeCandidate;
    let candidates = [
        DesignEntitySelectionEdgeCandidate {
            identity_ordinal: 0,
            local_id: 700,
            historical_entity_kind: AsmHistoricalEntityKind::Edge,
            historical_entity_ref: 42,
            edge_slots: vec![17],
        },
        DesignEntitySelectionEdgeCandidate {
            identity_ordinal: 1,
            local_id: 800,
            historical_entity_kind: AsmHistoricalEntityKind::Vertex,
            historical_entity_ref: 50,
            edge_slots: vec![17, 18],
        },
    ];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "find F3D common candidate edge",
        0,
        |decode| unique_entity_selection_edge(decode, &candidates).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "find F3D common candidate edge")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D face change state chain")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D changed faces")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "track F3D edge change state chain")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D deleted edges")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = edge_changes_across_state_chain(&ctx, &result, 1, &states).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D updated edges")
    );
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
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    resolve_pattern_face_by_surface_radius(&ctx, &[candidate], &preceding, &result, &HashSet::new())
}

#[test]
fn pattern_face_candidate_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern face candidates")
    );
}

#[test]
fn pattern_face_bound_index_refuses_collection_limit() {
    let error = pattern_face_limit_case(1).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D pattern bound faces")
    );
}

fn one_face_reference() -> crate::records::dimensions::DesignRecipeReference {
    crate::records::dimensions::DesignRecipeReference {
        selector: 0,
        selector_offset: 0,
        token: "face".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces: vec![crate::ids::brep_face_id(1)],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn topology_face_index_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        faces_in_topology(&ctx, &one_face_reference().candidate_faces, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D topology faces")
    );
}

#[test]
fn topology_face_copy_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        faces_in_topology(&ctx, &one_face_reference().candidate_faces, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D faces in topology")
    );
}

#[test]
fn terminal_edge_recipe_face_union_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        terminal_edge_recipe_faces(&ctx, &one_face_reference().candidate_faces, &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D terminal edge recipe faces")
    );
}

#[test]
fn terminal_reference_group_refuses_collection_limit() {
    let mut reference = one_face_reference();
    reference.candidate_faces.clear();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = terminal_edge_recipe_reference_faces(&ctx, &[reference], None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D terminal reference groups")
    );
}

#[test]
fn terminal_reference_face_copy_refuses_retained_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        terminal_edge_recipe_reference_faces(&ctx, &[one_face_reference()], None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical face identity")
    );
}

#[test]
fn reference_edge_set_groups_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = collect_reference_edge_sets(&ctx, &[Vec::new()], &AsmHistoricalTopology::default())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D reference edge sets")
    );
}

#[test]
fn boundary_face_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_boundary_edges(
        &ctx,
        &one_face_reference().candidate_faces,
        &AsmHistoricalTopology::default(),
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D boundary faces")
    );
}

#[test]
fn boundary_change_slots_refuse_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = boundary_edges_in_changes(&ctx, &[3], &[3]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D boundary edges in changes")
    );
}

#[test]
fn edge_recipe_selector_index_refuses_collection_limit() {
    use crate::records::topology::edge_recipe::{
        DesignEdgeRecipeStructure, DesignTopologyRecipeEntry, DesignTopologyRecipeSide,
        DesignTopologyRecipeTriplet,
    };
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(1).unwrap(),
        middle: 0,
        incident: None,
    };
    let structure = DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![DesignTopologyRecipeSide {
            header_value: 0,
            scalars: Vec::new(),
            payload_prefix: Vec::new(),
            entries: vec![DesignTopologyRecipeEntry {
                selector: 1,
                boundary_edge_count: std::num::NonZeroU32::new(1).unwrap(),
                topology_triplets: [triplet, triplet],
            }],
        }],
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = recipe_selector_candidates(&ctx, Some(&structure), &[]).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D edge recipe selectors")
    );
}

#[test]
fn treatment_boundary_relation_index_refuses_collection_limit() {
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_boundary_edge_index(&ctx, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D boundary relations")
    );
}

#[test]
fn treatment_preceding_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let empty_boundaries =
        face_boundary_edge_index(&ctx, &AsmHistoricalTopology::default()).unwrap();
    let error = treatment_face_supports(
        &ctx,
        &[],
        &AsmHistoricalTopology::default(),
        &preceding,
        &empty_boundaries,
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D treatment preceding faces")
    );
}

#[test]
fn treatment_deleted_edge_index_refuses_collection_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = treatment_edge_candidates(
        &ctx,
        None,
        &[],
        &AsmHistoricalTopology::default(),
        &AsmHistoricalTopology::default(),
        &[1],
    )
    .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D treatment deleted edges")
    );
}

#[test]
fn historical_support_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let history = AsmHistory {
        id: "history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: Vec::new(),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_face_support_contexts(&ctx, &[], &history, &preceding, &HashSet::new())
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical support faces")
    );
}

#[test]
fn face_boundary_loop_contexts_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: vec![2],
        }],
        loop_coedges: vec![AsmHistoricalRelation {
            owner_ref: 2,
            member_refs: Vec::new(),
        }],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = face_boundary_contexts_for_slots(&ctx, &[1], &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D face boundary loops")
    );
}

#[test]
fn historical_loop_vertices_refuse_collection_limit() {
    let topology = AsmHistoricalTopology {
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 2,
            start_vertex: 1,
            end_vertex: 1,
        }],
        ..AsmHistoricalTopology::default()
    };
    let coedges = vec![
        crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
            coedge_slot: 3,
            edge_slot: 2,
        },
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = historical_loop_boundary(&ctx, coedges, &topology).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D loop vertices")
    );
}

#[test]
fn preceding_support_face_index_refuses_collection_limit() {
    let preceding = AsmHistoricalTopology {
        faces: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        preceding_support_face_slots(&ctx, &[], &AsmHistoricalTopology::default(), &preceding)
            .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D preceding support faces")
    );
}

#[test]
fn entity_selection_scope_stream_comparison_propagates_work_refusal() {
    let stream = "f3d:Design/BulkStream.dat";
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#42"),
        crate::records::feature::scope::DesignScopePayload::Sweep(None),
        42,
    );
    let scopes = [scope];
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "compare F3D selection entity scope stream",
        0,
        |decode| {
            let mut operands = [
                crate::records::topology::entity_selection::DesignEntitySelectionOperand::try_new(
                    crate::records::topology::entity_selection::DesignEntitySelectionOperandDraft {
                        id: format!("{stream}:design-entity-selection-operand#200"),
                        scope_record_index: 42,
                        group_record_index: 100,
                        group_member_ordinal: 0,
                        record_index: 200,
                        byte_offset: 0,
                        class_tag: "377".to_owned().try_into().unwrap(),
                        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        asset_id_offset: 0,
                        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                        context_id_offset: 0,
                        identity_record_index: 203,
                        identity_record_offset: 0,
                        primary_identity: 7,
                        primary_identity_offset: 21,
                        secondary: None,
                        historical_edge_candidates: Vec::new(),
                        historical_face_candidates: Vec::new(),
                        resolved_edge_slot: None,
                        next_record_index: 202,
                        next_byte_offset: 29,
                    },
                )
                .unwrap(),
            ];
            bind_entity_selection_history(decode, &mut operands, &scopes, &[])
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "compare F3D selection entity scope stream"
    ));
}

fn treatment_face_support_scan_fixture() -> ([i64; 1], AsmHistoricalTopology, AsmHistoricalTopology)
{
    let inserted_faces = [1];
    let result = AsmHistoricalTopology {
        faces: vec![1, 2],
        face_loops: vec![
            AsmHistoricalRelation {
                owner_ref: 1,
                member_refs: vec![10],
            },
            AsmHistoricalRelation {
                owner_ref: 2,
                member_refs: vec![20],
            },
        ],
        loop_coedges: vec![
            AsmHistoricalRelation {
                owner_ref: 10,
                member_refs: vec![100, 101],
            },
            AsmHistoricalRelation {
                owner_ref: 20,
                member_refs: vec![200, 201],
            },
        ],
        coedge_topology: vec![
            crate::history_records::AsmHistoricalCoedge {
                coedge: 100,
                owner_loop: 10,
                edge: 30,
                next: 101,
                previous: 101,
                radial_next: 200,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 101,
                owner_loop: 10,
                edge: 31,
                next: 100,
                previous: 100,
                radial_next: 201,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 200,
                owner_loop: 20,
                edge: 30,
                next: 201,
                previous: 201,
                radial_next: 100,
            },
            crate::history_records::AsmHistoricalCoedge {
                coedge: 201,
                owner_loop: 20,
                edge: 31,
                next: 200,
                previous: 200,
                radial_next: 101,
            },
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 1,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 2,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![2],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 2,
            carrier: 200,
        }],
        ..AsmHistoricalTopology::default()
    };
    (inserted_faces, result, preceding)
}

fn treatment_supports_for_fixture(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
) -> Result<Vec<(i64, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    let result_boundaries = face_boundary_edge_index(decode, result)?;
    treatment_face_supports(
        decode,
        inserted_faces,
        result,
        preceding,
        &result_boundaries,
    )
}

#[test]
fn history_treatment_support_fixture_keeps_neighbor_face() {
    let (inserted_faces, result, preceding) = treatment_face_support_scan_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (decode, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let supports =
        treatment_supports_for_fixture(&decode, &inserted_faces, &result, &preceding).unwrap();
    assert_eq!(supports, vec![(1, 100, vec![2])]);
}

fn treatment_face_support_refusal(operation: &str) -> cadmpeg_core::CodecError {
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |decode| {
            let (inserted_faces, result, preceding) = treatment_face_support_scan_fixture();
            treatment_supports_for_fixture(decode, &inserted_faces, &result, &preceding).map(|_| ())
        },
    )
}

#[test]
fn history_treatment_result_boundary_edge_scan_refuses_work() {
    let operation = "scan F3D result face boundary edges";
    let error = treatment_face_support_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_treatment_inserted_boundary_scan_refuses_work() {
    let operation = "scan F3D inserted boundary edges";
    let error = treatment_face_support_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_treatment_adjacent_face_lists_refuse_work() {
    let operation = "scan F3D adjacent treatment faces";
    let error = treatment_face_support_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn history_treatment_support_face_dedup_refuses_work() {
    let operation = "deduplicate F3D treatment support faces";
    let error = treatment_face_support_refusal(operation);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
