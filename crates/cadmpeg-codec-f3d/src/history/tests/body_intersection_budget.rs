// SPDX-License-Identifier: Apache-2.0

fn simple_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: Vec::new(),
        }],
        ..Default::default()
    }
}

fn intersection_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(Some(&ctx), &simple_topology(), &BTreeSet::from([1]))
        .unwrap_err()
}

#[test]
fn historical_relation_index_refuses_collection_limit() {
    let error = intersection_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical relations")
    );
}

#[test]
fn historical_body_closure_refuses_collection_limit() {
    let error = intersection_error(1);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical body closure")
    );
}

#[test]
fn affected_topology_bodies_refuse_collection_limit() {
    let error = intersection_error(2);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D affected topology bodies")
    );
}

fn index_error(
    topology: &crate::history_records::AsmHistoricalTopology,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(Some(&ctx), topology, &std::collections::BTreeSet::new())
        .unwrap_err()
}

#[test]
fn historical_coedge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.coedge_topology.push(AsmHistoricalCoedge {
        coedge: 1,
        owner_loop: 2,
        edge: 3,
        next: 1,
        previous: 1,
        radial_next: 1,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical coedges")
    );
}

#[test]
fn historical_edge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalEdge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 1,
        start_vertex: 2,
        end_vertex: 3,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical edge vertices")
    );
}

#[test]
fn historical_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 1,
        carrier: 2,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical carriers")
    );
}

#[test]
fn historical_optional_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalOptionalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology
        .edge_curves
        .push(AsmHistoricalOptionalCarrierBinding {
            entity: 1,
            carrier: None,
        });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical optional carriers")
    );
}

fn wire_topology(free_vertex: bool) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![7])],
        shell_free_vertices: vec![relation(3, if free_vertex { vec![8] } else { Vec::new() })],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 28,
            },
            AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 29,
            },
        ],
        ..Default::default()
    }
}

fn wire_error(max_items: u64, free_vertex: bool) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bodies_intersecting(
        Some(&ctx),
        &wire_topology(free_vertex),
        &std::collections::BTreeSet::from([1]),
    )
    .unwrap_err()
}

#[test]
fn historical_shell_edges_refuse_collection_limit() {
    let error = wire_error(11, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical shell edges")
    );
}

#[test]
fn historical_shell_vertices_refuse_collection_limit() {
    let error = wire_error(12, true);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D historical shell vertices")
    );
}

#[test]
fn historical_edge_vertices_refuse_collection_limit() {
    let error = wire_error(13, false);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D historical shell vertices")
    );
}
