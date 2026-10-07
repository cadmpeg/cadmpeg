// SPDX-License-Identifier: Apache-2.0
//! Admission limits for historical state indexes and change chains.

#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::{
    topology::collect_reference_edge_sets, topology::face_boundary_contexts_for_slots,
    topology::face_boundary_edge_index, topology::face_boundary_edges,
    topology::historical_edge_axis, topology::historical_face_support_contexts,
    topology::historical_loop_boundary, topology::preceding_support_face_slots,
    topology::treatment_edge_candidates, topology::treatment_face_supports,
};

use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistory};

use std::collections::HashSet;

use crate::history::test_support::{edge_context_topology, one_face_reference};

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
        if limit.operation == "index F3D boundary face loops")
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
    let error = treatment_edge_candidates::<true>(
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
fn empty_historical_support_query_does_not_index_faces() {
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
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let contexts =
        historical_face_support_contexts(&ctx, &[], &history, &preceding, &HashSet::new()).unwrap();
    assert!(contexts.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
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
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D face boundary loops",
        0,
        |ctx| face_boundary_contexts_for_slots(ctx, &[1], &topology),
    );
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
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D loop vertices",
        0,
        |ctx| historical_loop_boundary(ctx, coedges.clone(), &topology),
    );
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
