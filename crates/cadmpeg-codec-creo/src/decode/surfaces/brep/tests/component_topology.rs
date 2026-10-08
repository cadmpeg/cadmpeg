// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::BrepComponentTopology;

fn topology_result(limit: u64, two_faces: bool) -> Result<BrepComponentTopology, CodecError> {
    let first_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::Zero,
        }],
    );
    let second_loop = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(6),
        vec![crate::topology::HalfEdgeId {
            curve_id: 10,
            side: crate::topology::Side::One,
        }],
    );
    let mut eligible_faces = BTreeMap::from([(5, vec![&first_loop])]);
    if two_faces {
        eligible_faces.insert(6, vec![&second_loop]);
    }
    let faces: &[u32] = if two_faces { &[5, 6] } else { &[5] };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    BrepComponentTopology::from_component(
        &ctx,
        &BTreeSet::from([10, 11]),
        &BTreeSet::from([10]),
        faces,
        &eligible_faces,
        &BTreeMap::from([(10, [1, 2])]),
    )
}

fn assert_refusal(limit: u64, two_faces: bool, operation: &'static str) {
    let error = topology_result(limit, two_faces)
        .err()
        .expect("topology node refused");
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

#[test]
fn brep_component_face_curve_nodes_refuse_collection_limit() {
    assert_refusal(0, false, "creo B-rep component face curve nodes");
}

#[test]
fn brep_component_wire_curve_nodes_refuse_collection_limit() {
    assert_refusal(1, false, "creo B-rep component wire curve nodes");
}

#[test]
fn brep_adjacency_face_nodes_refuse_collection_limit() {
    assert_refusal(2, false, "creo B-rep adjacency face nodes");
}

#[test]
fn brep_face_vertex_map_nodes_refuse_collection_limit() {
    assert_refusal(3, false, "creo B-rep face vertex map nodes");
}

#[test]
fn brep_curve_incidence_map_nodes_refuse_collection_limit() {
    assert_refusal(4, false, "creo B-rep curve incidence map nodes");
}

#[test]
fn brep_curve_incident_face_nodes_refuse_collection_limit() {
    assert_refusal(5, false, "creo B-rep curve incident face nodes");
}

#[test]
fn brep_face_vertex_nodes_refuse_collection_limit() {
    assert_refusal(6, false, "creo B-rep face vertex nodes");
}

#[test]
fn brep_vertex_incidence_map_nodes_refuse_collection_limit() {
    assert_refusal(7, false, "creo B-rep vertex incidence map nodes");
}

#[test]
fn brep_vertex_incident_face_nodes_refuse_collection_limit() {
    assert_refusal(8, false, "creo B-rep vertex incident face nodes");
}

#[test]
fn brep_adjacency_neighbour_nodes_refuse_collection_limit() {
    assert_refusal(19, true, "creo B-rep adjacency neighbour nodes");
}

#[test]
fn brep_component_topology_preserves_service_incidence() {
    let topology = topology_result(u64::MAX, true).expect("service topology admitted");
    assert_eq!(topology.component_face_curves, BTreeSet::from([10]));
    assert_eq!(topology.wire_curves, BTreeSet::from([11]));
    assert_eq!(topology.face_adjacency[&5], BTreeSet::from([6]));
    assert_eq!(topology.face_adjacency[&6], BTreeSet::from([5]));
    assert_eq!(topology.face_vertices[&5], BTreeSet::from([1, 2]));
    assert_eq!(topology.face_vertices[&6], BTreeSet::from([1, 2]));
}

fn closed_component_lookup_fixture() -> (
    BTreeSet<u32>,
    BTreeSet<crate::topology::HalfEdgeId>,
    BTreeMap<crate::topology::HalfEdgeId, crate::topology::HalfEdge>,
) {
    let first = crate::topology::HalfEdgeId {
        curve_id: 7,
        side: crate::topology::Side::Zero,
    };
    let second = crate::topology::HalfEdgeId {
        curve_id: 7,
        side: crate::topology::Side::One,
    };
    let edges = BTreeMap::from([
        (
            first,
            crate::topology::HalfEdge {
                id: first,
                face_id: std::num::NonZeroU32::new(5),
                next: None,
            },
        ),
        (
            second,
            crate::topology::HalfEdge {
                id: second,
                face_id: std::num::NonZeroU32::new(5),
                next: None,
            },
        ),
    ]);
    (BTreeSet::from([7]), BTreeSet::from([first, second]), edges)
}

#[test]
fn closed_component_face_membership_refuses_work_and_preserves_service_result() {
    const FIRST_LOOKUP: &str = "creo closed component first face lookup";
    const SECOND_LOOKUP: &str = "creo closed component second face lookup";
    let (component_face_curves, emitted_half_edges, edges) = closed_component_lookup_fixture();
    let half_edges = edges
        .iter()
        .map(|(id, edge)| (*id, edge))
        .collect::<BTreeMap<_, _>>();
    let faces = [5];
    let closed =
        crate::test_support::assert_work_boundaries(&[FIRST_LOOKUP, SECOND_LOOKUP], |ctx| {
            super::super::component_is_closed(
                ctx,
                &component_face_curves,
                &emitted_half_edges,
                &half_edges,
                &faces,
            )
        });
    assert!(closed);
}

#[test]
fn closed_component_first_face_miss_short_circuits_second_lookup() {
    const FIRST_LOOKUP: &str = "creo closed component first face lookup";
    const SECOND_LOOKUP: &str = "creo closed component second face lookup";
    let (component_face_curves, emitted_half_edges, edges) = closed_component_lookup_fixture();
    let half_edges = edges
        .iter()
        .map(|(id, edge)| (*id, edge))
        .collect::<BTreeMap<_, _>>();
    let faces = [6];
    let closed = crate::test_support::assert_work_boundaries(&[FIRST_LOOKUP], |ctx| {
        let result = super::super::component_is_closed(
            ctx,
            &component_face_curves,
            &emitted_half_edges,
            &half_edges,
            &faces,
        );
        if matches!(
            &result,
            Err(CodecError::ResourceLimit(resource)) if resource.operation == SECOND_LOOKUP
        ) {
            panic!("second face lookup must be skipped after the first face misses");
        }
        result
    });
    assert!(!closed);
}
