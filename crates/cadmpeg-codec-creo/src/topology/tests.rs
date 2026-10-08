// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
use crate::test_support::build_prt;
use crate::test_support::push_generated_plane_row;
use crate::test_support::push_generated_topology_row;
use crate::test_support::visibgeom_payload;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

use std::collections::BTreeSet;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self};
use crate::CreoCodec;

use super::{
    build, edge_start_vertex_pairs, edge_vertex_pairs, face_components, selected_body_count,
    vertex_incident_faces, vertex_orbits, HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence,
    TopologicalVertex,
};
use crate::curve::CurveTopologyRow;

fn row(id: u32, next: u32) -> CurveTopologyRow {
    CurveTopologyRow {
        id,
        type_byte: 0,
        feature_id: 0,
        directions: [1, 1],
        faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(20)],
        next_edges: [next, next],
        offset: 0,
    }
}

fn build_service(rows: &[CurveTopologyRow]) -> (Vec<HalfEdge>, Vec<super::Loop>) {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    build(&ctx, rows).expect("service topology build")
}

fn with_service_context<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    run(&ctx)
}

fn with_collection_limit<T>(
    max_collection_items: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    run(&ctx)
}

fn assert_collection_error(error: &CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

fn paired_incidence() -> [HalfEdgeVertexIncidence; 2] {
    [
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
            end_vertex_id: std::num::NonZeroU32::new(20),
        },
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
    ]
}

#[test]
fn start_vertex_pairs_refuse_group_node() {
    let error = with_collection_limit(0, |ctx| edge_start_vertex_pairs(ctx, &paired_incidence()))
        .expect_err("one curve needs a grouping node");
    assert_collection_error(&error, "creo start-vertex pair group nodes");
}

#[test]
fn start_vertex_pairs_refuse_output_node() {
    let error = with_collection_limit(1, |ctx| edge_start_vertex_pairs(ctx, &paired_incidence()))
        .expect_err("one curve needs an output node");
    assert_collection_error(&error, "creo start-vertex pair nodes");
}

#[test]
fn edge_vertex_pairs_refuse_group_node() {
    let error = with_collection_limit(0, |ctx| edge_vertex_pairs(ctx, &paired_incidence()))
        .expect_err("one curve needs a grouping node");
    assert_collection_error(&error, "creo edge-vertex pair group nodes");
}

#[test]
fn edge_vertex_pairs_refuse_output_node() {
    let error = with_collection_limit(1, |ctx| edge_vertex_pairs(ctx, &paired_incidence()))
        .expect_err("one curve needs an output node");
    assert_collection_error(&error, "creo edge-vertex pair nodes");
}

#[test]
fn edge_vertex_pairs_reject_duplicate_side_without_a_temporary_vector() {
    let [first, second] = paired_incidence();
    let incidence = [first.clone(), first, second];
    let start = with_service_context(|ctx| {
        edge_start_vertex_pairs(ctx, &incidence).expect("service start pairs")
    });
    let edge =
        with_service_context(|ctx| edge_vertex_pairs(ctx, &incidence).expect("service edge pairs"));
    assert!(!start.contains_key(&7));
    assert!(!edge.contains_key(&7));
}

fn one_incident_vertex() -> (Vec<TopologicalVertex>, Vec<HalfEdge>) {
    let edge = orphan_edge();
    (
        vec![crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::TopologicalVertex::new(ctx, 1, vec![edge.id])
        })
        .expect("vertex admission")
        .expect("valid vertex fixture")],
        vec![edge],
    )
}

#[test]
fn vertex_incident_faces_refuse_half_edge_lookup_node() {
    let (vertices, edges) = one_incident_vertex();
    let error = with_collection_limit(0, |ctx| vertex_incident_faces(ctx, &vertices, &edges))
        .expect_err("one edge needs a lookup node");
    assert_collection_error(&error, "creo incident-face half-edge lookup nodes");
}

#[test]
fn vertex_incident_faces_refuse_face_node() {
    let (vertices, edges) = one_incident_vertex();
    let error = with_collection_limit(1, |ctx| vertex_incident_faces(ctx, &vertices, &edges))
        .expect_err("one incident face needs a set node");
    assert_collection_error(&error, "creo incident face nodes");
}

#[test]
fn vertex_incident_faces_refuse_vertex_node() {
    let (vertices, edges) = one_incident_vertex();
    let error = with_collection_limit(2, |ctx| vertex_incident_faces(ctx, &vertices, &edges))
        .expect_err("one vertex needs an output node");
    assert_collection_error(&error, "creo incident-face vertex nodes");
}

fn build_with_collection_limit(
    rows: &[CurveTopologyRow],
    max_collection_items: u64,
) -> Result<(Vec<HalfEdge>, Vec<super::Loop>), CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    build(&ctx, rows)
}

fn assert_build_collection_refusal(operation: &'static str) {
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some(operation),
        |limit| build_with_collection_limit(&[row(1, 1)], limit),
    );
    let error = build_with_collection_limit(&[row(1, 1)], limit)
        .expect_err("one closed face-side ring exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

fn orbit_with_collection_limit(
    edges: &[HalfEdge],
    max_collection_items: u64,
) -> Result<super::VertexOrbits, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    vertex_orbits(&ctx, edges)
}

fn orphan_edge() -> HalfEdge {
    HalfEdge {
        id: HalfEdgeId {
            curve_id: 1,
            side: crate::topology::Side::Zero,
        },
        face_id: std::num::NonZeroU32::new(10),
        next: None,
    }
}

fn assert_orbit_collection_refusal(operation: &'static str) {
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some(operation),
        |limit| orbit_with_collection_limit(&[orphan_edge()], limit),
    );
    let error = orbit_with_collection_limit(&[orphan_edge()], limit)
        .expect_err("one half-edge exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

fn components_with_collection_limit(
    rows: &[CurveTopologyRow],
    max_collection_items: u64,
) -> Result<Vec<super::FaceComponent>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    face_components(&ctx, rows)
}

fn assert_component_collection_refusal(operation: &'static str) {
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some(operation),
        |limit| components_with_collection_limit(&[row(1, 1)], limit),
    );
    let error = components_with_collection_limit(&[row(1, 1)], limit)
        .expect_err("one two-face component exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

#[test]
fn face_components_refuse_unique_row_count_node() {
    assert_component_collection_refusal( "creo unique-row count nodes");
}

#[test]
fn face_components_refuse_unique_row_projection() {
    assert_component_collection_refusal( "creo unique-row projection");
}

#[test]
fn face_components_refuse_adjacency_node() {
    assert_component_collection_refusal( "creo face adjacency nodes");
}

#[test]
fn face_components_refuse_curve_group_node() {
    assert_component_collection_refusal( "creo face curve group nodes");
}

#[test]
fn face_components_refuse_curve_member_node() {
    assert_component_collection_refusal( "creo face curve member nodes");
}

#[test]
fn face_components_refuse_adjacency_link() {
    assert_component_collection_refusal( "creo face adjacency links");
}

#[test]
fn face_components_refuse_seen_start_node() {
    assert_component_collection_refusal( "creo seen component faces");
}

#[test]
fn face_components_refuse_pending_start() {
    assert_component_collection_refusal( "creo pending component faces");
}

#[test]
fn face_components_refuse_face_member_node() {
    assert_component_collection_refusal( "creo component face nodes");
}

#[test]
fn face_components_refuse_curve_node() {
    assert_component_collection_refusal( "creo component curve nodes");
}

#[test]
fn face_components_refuse_seen_neighbor_node() {
    assert_component_collection_refusal( "creo seen component faces");
}

#[test]
fn face_components_refuse_pending_neighbor() {
    assert_component_collection_refusal( "creo pending component faces");
}

#[test]
fn face_components_refuse_face_id_vector() {
    assert_component_collection_refusal( "creo component face IDs");
}

#[test]
fn face_components_refuse_curve_id_vector() {
    assert_component_collection_refusal( "creo component curve IDs");
}

#[test]
fn face_components_refuse_component_vector() {
    assert_component_collection_refusal( "creo face components");
}

#[test]
fn vertex_orbits_refuse_half_edge_lookup_node() {
    assert_orbit_collection_refusal( "creo vertex-orbit half-edge lookup nodes");
}

#[test]
fn vertex_orbits_refuse_adjacency_node() {
    assert_orbit_collection_refusal( "creo vertex adjacency nodes");
}

#[test]
fn vertex_orbits_refuse_pending_seed() {
    assert_orbit_collection_refusal( "creo vertex orbit pending edges");
}

#[test]
fn vertex_orbits_refuse_visited_node() {
    assert_orbit_collection_refusal( "creo visited vertex-orbit edges");
}

#[test]
fn vertex_orbits_refuse_member_node() {
    assert_orbit_collection_refusal( "creo vertex orbit member nodes");
}

#[test]
fn vertex_orbits_refuse_half_edge_vector() {
    assert_orbit_collection_refusal( "creo vertex orbit half-edges");
}

#[test]
fn vertex_orbits_refuse_vertex_vector() {
    assert_orbit_collection_refusal( "creo topological vertices");
}

#[test]
fn vertex_orbits_refuse_start_vertex_lookup_node() {
    assert_orbit_collection_refusal( "creo start-vertex lookup nodes");
}

#[test]
fn vertex_orbits_refuse_incidence_vector() {
    assert_orbit_collection_refusal( "creo half-edge vertex incidence");
}

#[test]
fn vertex_orbits_refuse_predecessor_group_node() {
    let mut edge = orphan_edge();
    edge.next = Some(edge.id);
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some("creo predecessor group nodes"),
        |limit| orbit_with_collection_limit(&[edge.clone()], limit),
    );
    let error = orbit_with_collection_limit(&[edge], limit)
        .expect_err("one successor needs a predecessor node");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo predecessor group nodes"));
}

#[test]
fn vertex_orbits_refuse_adjacency_links() {
    let mut first = orphan_edge();
    first.next = Some(HalfEdgeId {
        curve_id: 2,
        side: crate::topology::Side::Zero,
    });
    let second = HalfEdge {
        id: HalfEdgeId {
            curve_id: 1,
            side: crate::topology::Side::One,
        },
        ..orphan_edge()
    };
    let third = HalfEdge {
        id: HalfEdgeId {
            curve_id: 2,
            side: crate::topology::Side::Zero,
        },
        ..orphan_edge()
    };
    let limit = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some("creo vertex adjacency links"),
        |limit| orbit_with_collection_limit(&[first.clone(), second.clone(), third.clone()], limit),
    );
    let error = orbit_with_collection_limit(&[first, second, third], limit)
        .expect_err("linked predecessor needs an adjacency edge");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo vertex adjacency links"));
}

#[test]
fn topology_build_refuses_unique_row_count_node() {
    assert_build_collection_refusal( "creo unique-row count nodes");
}

#[test]
fn topology_build_refuses_unique_row_projection() {
    assert_build_collection_refusal( "creo unique-row projection");
}

#[test]
fn topology_build_refuses_face_side_group_node() {
    assert_build_collection_refusal( "creo topology successor index");
}

#[test]
fn topology_build_refuses_half_edge_vector() {
    assert_build_collection_refusal( "creo topology half-edges");
}

#[test]
fn topology_build_refuses_ring_visit_node() {
    assert_build_collection_refusal( "creo topology ring visit nodes");
}

#[test]
fn topology_build_refuses_ring_half_edge() {
    assert_build_collection_refusal( "creo topology ring half-edges");
}

#[test]
fn topology_build_refuses_consumed_half_edge_node() {
    assert_build_collection_refusal( "creo consumed topology half-edges");
}

#[test]
fn topology_build_refuses_loop_vector() {
    assert_build_collection_refusal( "creo topology loops");
}

#[test]
fn builds_closed_face_side_rings_without_guessing() {
    let (half_edges, loops) = build_service(&[row(1, 2), row(2, 3), row(3, 1)]);
    assert_eq!(half_edges.len(), 6);
    assert_eq!(loops.len(), 2);
    assert_eq!(loops[0].face_id, std::num::NonZeroU32::new(10));
    assert_eq!(
        loops[0].half_edges(),
        vec![
            HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero
            },
            HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero
            },
            HalfEdgeId {
                curve_id: 3,
                side: crate::topology::Side::Zero
            }
        ]
    );
}

#[test]
fn duplicate_curve_identities_do_not_contribute_derived_topology() {
    let rows = [row(1, 2), row(2, 1), row(2, 1)];

    let (half_edges, loops) = build_service(&rows);
    assert_eq!(half_edges.len(), 2);
    assert!(half_edges.iter().all(|edge| edge.id.curve_id == 1));
    assert!(half_edges.iter().all(|edge| edge.next.is_none()));
    assert!(loops.is_empty());

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let components = face_components(&ctx, &rows).expect("service face components");
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].face_ids, [10, 20]);
    assert_eq!(components[0].curve_ids, [1]);
}
#[test]
fn withholds_ambiguous_successors() {
    let (half_edges, loops) = build_service(&[
        row(1, 2),
        CurveTopologyRow {
            faces: [std::num::NonZeroU32::new(10), std::num::NonZeroU32::new(10)],
            ..row(2, 1)
        },
    ]);
    assert!(half_edges.iter().any(|edge| edge.id
        == HalfEdgeId {
            curve_id: 1,
            side: crate::topology::Side::Zero
        }
        && edge.next.is_none()));
    assert!(loops.is_empty());
}

#[test]
fn vertex_orbits_close_predecessor_relations_in_both_directions() {
    let edges = vec![
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: Some(HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            }),
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
    ];

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let vertices = vertex_orbits(&ctx, &edges)
        .expect("service vertex orbits")
        .vertices;
    assert!(vertices.iter().any(|vertex| vertex.half_edges()
        == vec![
            HalfEdgeId {
                curve_id: 1,
                side: crate::topology::Side::Zero,
            },
            HalfEdgeId {
                curve_id: 2,
                side: crate::topology::Side::Zero,
            },
        ]));
}

#[test]
fn vertex_incident_faces_include_both_sides_of_each_orbit_edge() {
    let edges = vec![
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(20),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::Zero,
            },
            face_id: std::num::NonZeroU32::new(10),
            next: None,
        },
        HalfEdge {
            id: HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::One,
            },
            face_id: std::num::NonZeroU32::new(30),
            next: None,
        },
    ];
    let vertex = crate::decode::with_test_decode_ctx(|ctx| {
        crate::topology::TopologicalVertex::new(
            ctx,
            1,
            vec![
                HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                },
                HalfEdgeId {
                    curve_id: 8,
                    side: crate::topology::Side::Zero,
                },
            ],
        )
    })
    .expect("vertex admission")
    .expect("valid vertex fixture");

    let incident_faces = with_service_context(|ctx| {
        vertex_incident_faces(ctx, &[vertex], &edges).expect("service incident faces")
    });
    assert_eq!(
        incident_faces
            .get(&std::num::NonZeroU32::new(1).expect("one-based vertex fixture"))
            .cloned(),
        Some(BTreeSet::from([10, 20, 30]))
    );
}

#[test]
fn edge_vertex_pair_accepts_one_closed_face_and_rejects_disagreement() {
    let incidence = |reverse_end: Option<u32>| {
        vec![
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                },
                start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
                end_vertex_id: std::num::NonZeroU32::new(20),
            },
            HalfEdgeVertexIncidence {
                half_edge: HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::One,
                },
                start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
                end_vertex_id: reverse_end
                    .map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")),
            },
        ]
    };

    assert_eq!(
        with_service_context(|ctx| edge_vertex_pairs(ctx, &incidence(None)).expect("service pairs"))
            .get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert_eq!(
        with_service_context(|ctx| {
            edge_vertex_pairs(ctx, &incidence(Some(10))).expect("service pairs")
        })
        .get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert!(!with_service_context(|ctx| {
        edge_vertex_pairs(ctx, &incidence(Some(30))).expect("service pairs")
    })
    .contains_key(&7));
}

#[test]
fn edge_start_vertex_pair_survives_an_unresolved_successor() {
    let incidence = vec![
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero,
            },
            start_vertex_id: std::num::NonZeroU32::new(10).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
        HalfEdgeVertexIncidence {
            half_edge: HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::One,
            },
            start_vertex_id: std::num::NonZeroU32::new(20).expect("one-based vertex fixture"),
            end_vertex_id: None,
        },
    ];

    assert_eq!(
        with_service_context(|ctx| {
            edge_start_vertex_pairs(ctx, &incidence).expect("service start pairs")
        })
        .get(&7),
        Some(&[10, 20].map(|id| std::num::NonZeroU32::new(id).expect("one-based vertex fixture")))
    );
    assert!(!with_service_context(|ctx| {
        edge_vertex_pairs(ctx, &incidence).expect("service edge pairs")
    })
    .contains_key(&7));
}

#[test]
fn scan_groups_connected_nonzero_face_references() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3\x08\x08\x04\x01\xf6\x0b\x0c\x08\x08\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes_ok(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.face_components.len(), 1);
    assert_eq!(
        scan.topology.face_components[0].face_ids(),
        vec![10, 11, 12]
    );
    assert_eq!(scan.topology.face_components[0].curve_ids(), vec![7, 8]);
}

#[test]
fn selects_body_count_in_metadata_precedence_order() {
    assert_eq!(selected_body_count(Some(2), Some(0), 7), Some(2));
    assert_eq!(selected_body_count(None, Some(0), 7), Some(1));
    assert_eq!(selected_body_count(None, None, 7), Some(7));
    assert_eq!(selected_body_count(None, Some(9), 7), None);
    assert_eq!(selected_body_count(None, Some(9), 1), Some(1));
    assert_eq!(selected_body_count(None, Some(0), 0), Some(1));
    assert_eq!(selected_body_count(Some(0), None, 7), None);
}

#[test]
fn scan_builds_topological_vertex_orbits_and_incidence() {
    let mut payload = visibgeom_payload(0, 2);
    payload.extend_from_slice(
        b"topol_ref_data\0\x07\x08\x04\x01\xf6\x0a\x0b\x08\x08\0\0\xe3\xe1\xe3\
          \x08\x08\x04\x01\xf6\x0a\x0b\x07\x07\0\0\xe3\xe1\xe3",
    );
    let scan = container::scan_bytes_ok(build_prt("c", &[("VisibGeom", payload)]));

    assert_eq!(scan.topology.vertices.len(), 2);
    assert_eq!(
        scan.topology.vertices[0].half_edges(),
        vec![
            crate::topology::HalfEdgeId {
                curve_id: 7,
                side: crate::topology::Side::Zero
            },
            crate::topology::HalfEdgeId {
                curve_id: 8,
                side: crate::topology::Side::One
            },
        ]
    );
    let incidence = scan
        .topology
        .half_edge_vertex_incidence
        .iter()
        .find(|incidence| {
            incidence.half_edge
                == crate::topology::HalfEdgeId {
                    curve_id: 7,
                    side: crate::topology::Side::Zero,
                }
        })
        .expect("half-edge incidence");
    assert_eq!(incidence.start_vertex_id.get(), 1);
    assert_eq!(
        incidence.end_vertex_id.map(std::num::NonZeroU32::get),
        Some(2)
    );
}

fn closed_plane_intersection_data(geomlists: Option<&[u8]>) -> Vec<u8> {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    push_generated_plane_row(
        &mut payload,
        1,
        true,
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        2,
        false,
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        3,
        false,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0],
    );
    push_generated_plane_row(
        &mut payload,
        4,
        false,
        [-2.0, -1.0, 2.0],
        [2.0, -2.0, 1.0],
        [1.0, 0.0, 0.0],
    );
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        push_generated_topology_row(&mut payload, curve, faces, next);
    }

    let allfeatur = b"\x04\xeb\x04\x00\x10\x01\x00\xe5\xe3\xf6\x83\x91\xe1\
        \xe0\x21geoms_affected\0\xf8\x01\x63\
        \xe0\x21edgs_affected\0\xf8\x02\x0a\x0b"
        .to_vec();
    let mut sections = vec![
        ("VisibGeom", payload),
        ("AllFeatur", allfeatur),
        ("MdlStatus", b"Round id 4\0".to_vec()),
    ];
    if let Some(geomlists) = geomlists {
        sections.push(("Geomlists", geomlists.to_vec()));
    }
    build_prt("c", &sections)
}

#[test]
fn decode_transfers_closed_plane_intersection_brep() {
    let data = closed_plane_intersection_data(None);
    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(scan.planes.local_systems.len(), 4);
    assert_eq!(scan.curves.topology_rows.len(), 6);
    assert!(
        scan.features.affected_ids.iter().any(|record| {
            record.feature_id == 4
                && record.kind == crate::feature::rows::AffectedIdKind::Edges
                && record.ids == [10, 11]
        }),
        "affected ids: {:#?}",
        scan.features.affected_ids
    );
    assert_eq!(scan.topology.loops.len(), 4);
    assert_eq!(scan.topology.vertices.len(), 4);
    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let model = &result.ir().model;
    let namespace = result.ir().native.namespace("creo").unwrap();
    assert_eq!(namespace.arenas()["half_edges"].len(), 12);
    assert_eq!(namespace.arenas()["loops"].len(), 4);
    assert_eq!(namespace.arenas()["topological_vertices"].len(), 4);
    assert_eq!(namespace.arenas()["half_edge_vertex_incidence"].len(), 12);
    assert_eq!(namespace.arenas()["face_components"].len(), 1);
    assert_eq!(namespace.arenas()["half_edges"][0].fields()["curve_id"], 10);
    assert_eq!(namespace.arenas()["half_edges"][0].fields()["side"], 0);

    assert_eq!(model.points.len(), 4);
    assert_eq!(model.vertices.len(), 4);
    assert_eq!(model.edges.len(), 6);
    assert_eq!(model.curves.len(), 6);
    assert!(model.edges.iter().all(|edge| edge.curve().is_some()));
    assert!(model.edges.iter().all(|edge| edge.param_range().is_some()));
    for edge in &model.edges {
        let [start_parameter, end_parameter] = edge.param_range().expect("line edge range").get();
        assert_eq!(start_parameter, 0.0);
        assert!(end_parameter > 0.0);
        let curve = model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .expect("edge curve");
        let Some(SolvedCurveGeometry::Line(line_curve)) = curve.geometry.solved() else {
            panic!("edge line: {curve:#?}");
        };
        let origin = line_curve.origin().get();
        let direction = *line_curve.direction().as_raw();
        let start = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.start)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge start point")
            .position()
            .get();
        let end = model
            .vertices
            .iter()
            .find(|vertex| vertex.id == edge.end)
            .and_then(|vertex| model.points.iter().find(|point| point.id == vertex.point))
            .expect("edge end point")
            .position()
            .get();
        assert_eq!(origin, start);
        let evaluated = [
            origin.x + direction.x * end_parameter,
            origin.y + direction.y * end_parameter,
            origin.z + direction.z * end_parameter,
        ];
        assert!(evaluated
            .into_iter()
            .zip([end.x, end.y, end.z])
            .all(|(evaluated, expected)| (evaluated - expected).abs() < 1.0e-10));
    }
    assert_eq!(model.faces.len(), 4);
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#1")
            .expect("reversed face")
            .sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        model
            .faces
            .iter()
            .find(|face| face.id.as_str() == "creo:visibgeom:face#2")
            .expect("forward face")
            .sense,
        cadmpeg_ir::topology::Sense::Forward
    );
    assert_eq!(model.loops.len(), 4);
    assert!(model.loops.iter().all(|lp| {
        model
            .faces
            .iter()
            .find(|face| face.id == lp.face)
            .map(|face| face.loop_role(&lp.id))
            .unwrap_or_default()
            == cadmpeg_ir::topology::LoopBoundaryRole::Outer
    }));
    assert_eq!(model.coedges.len(), 12);
    assert_eq!(model.pcurves.len(), 12);
    assert!(model.coedges.iter().all(|coedge| coedge.pcurves.len() == 1));
    for coedge in &model.coedges {
        let pcurve = model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == coedge.pcurves[0].pcurve)
            .expect("projected plane pcurve");
        assert!(matches!(
            pcurve.geometry,
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(_)
        ));
        let edge = model
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("pcurve edge");
        assert_eq!(pcurve.parameter_range(), edge.param_range());
    }
    assert_eq!(model.shells.len(), 1);
    assert_eq!(model.regions.len(), 1);
    assert_eq!(model.bodies.len(), 1);
    assert_eq!(model.bodies[0].kind, cadmpeg_ir::topology::BodyKind::Solid);
    let feature = model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == "creo:model:feature#4")
        .expect("feature 4");
    assert_eq!(
        *feature.evaluation.outputs(),
        vec![model.bodies[0].id.clone()]
    );
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Fillet { groups },
    ) = feature.evaluation.definition()
    else {
        panic!("round definition: {:#?}", feature.evaluation.definition());
    };
    let [cadmpeg_ir::features::edge_treatments::FilletGroup { edges, .. }] = groups.as_slice()
    else {
        panic!("round groups: {groups:#?}");
    };
    let cadmpeg_ir::features::EdgeSelection::Resolved { edges, native } = edges else {
        panic!("round edges: {edges:#?}");
    };
    assert_eq!(
        edges,
        &[
            cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#10".to_string())
                .expect("identity grammar"),
            cadmpeg_ir::ids::EdgeId::mint("creo:visibgeom:edge#11".to_string())
                .expect("identity grammar"),
        ]
    );
    assert_eq!(native, "creo:allfeatur:edgs_affected#4:10,11");
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone())
        .expect("resource allocation did not fail");
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn decode_withholds_native_brep_when_declared_body_count_disagrees() {
    let data = closed_plane_intersection_data(Some(b"n_bodies\0\x02"));
    let scan = container::scan_bytes_ok(data.clone());
    assert_eq!(scan.framing.declared_body_count, Some(2));
    assert_eq!(scan.topology.face_components.len(), 1);

    let result = CreoCodec
        .decode(&mut Cursor::new(data), &DecodeOptions::default())
        .expect("decode");
    let model = &result.ir().model;
    assert_eq!(model.points.len(), 4);
    assert!(model.vertices.is_empty());
    assert!(model.edges.is_empty());
    assert!(model.faces.is_empty());
    assert!(model.loops.is_empty());
    assert!(model.coedges.is_empty());
    assert!(model.shells.is_empty());
    assert!(model.regions.is_empty());
    assert!(model.bodies.is_empty());
}

#[test]
fn closed_ring_constructor_rejects_empty_repeated_disconnected_and_mixed_faces() {
    let a = HalfEdgeId {
        curve_id: 1,
        side: crate::topology::Side::Zero,
    };
    let b = HalfEdgeId {
        curve_id: 2,
        side: crate::topology::Side::Zero,
    };
    let face = std::num::NonZeroU32::new(10);
    let graph = [
        HalfEdge {
            id: a,
            face_id: face,
            next: Some(b),
        },
        HalfEdge {
            id: b,
            face_id: face,
            next: Some(a),
        },
    ];
    with_service_context(|ctx| {
        for ring in [vec![], vec![a, a], vec![a], vec![b]] {
            assert!(super::Loop::new(ctx, face, ring, &graph)
                .expect("ring validation")
                .is_none());
        }
        let ring = super::Loop::new(ctx, face, vec![a, b], &graph)
            .expect("ring validation")
            .expect("closed ring");
        assert_eq!(ring.half_edges(), [a, b]);
        let mut mixed = graph.clone();
        mixed[1].face_id = std::num::NonZeroU32::new(11);
        assert!(super::Loop::new(ctx, face, vec![a, b], &mixed)
            .expect("ring validation")
            .is_none());
        let mut duplicate = graph.to_vec();
        duplicate.push(graph[0].clone());
        assert!(super::Loop::new(ctx, face, vec![a, b], &duplicate)
            .expect("ring validation")
            .is_none());
    });
}

#[test]
fn face_component_constructor_rejects_zero_repeated_and_unordered_identity_sets() {
    with_service_context(|ctx| {
        for faces in [vec![], vec![0], vec![2, 0, 2, 1], vec![2, 1], vec![1, 1]] {
            assert!(super::FaceComponent::new(ctx, faces, vec![1])
                .expect("component validation")
                .is_none());
        }
        for curves in [vec![2, 1], vec![1, 1]] {
            assert!(super::FaceComponent::new(ctx, vec![1], curves)
                .expect("component validation")
                .is_none());
        }
        let component = super::FaceComponent::new(ctx, vec![1, 2], vec![0, 1])
            .expect("component validation")
            .expect("ordered component");
        assert_eq!(component.face_ids(), [1, 2]);
        assert_eq!(component.curve_ids(), [0, 1]);
    });
}

#[test]
fn checked_topology_constructors_propagate_work_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let id = HalfEdgeId {
        curve_id: 1,
        side: crate::topology::Side::Zero,
    };
    let graph = [HalfEdge {
        id,
        face_id: None,
        next: Some(id),
    }];
    let error = super::Loop::new(&ctx, None, vec![id], &graph).expect_err("ring work refused");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error =
        super::FaceComponent::new(&ctx, vec![1], vec![]).expect_err("component work refused");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn vertex_orbit_constructor_rejects_zero_empty_repeated_and_unordered_members() {
    let a = HalfEdgeId {
        curve_id: 1,
        side: crate::topology::Side::Zero,
    };
    let b = HalfEdgeId {
        curve_id: 2,
        side: crate::topology::Side::Zero,
    };
    with_service_context(|ctx| {
        assert!(super::TopologicalVertex::new(ctx, 0, vec![a])
            .expect("vertex validation")
            .is_none());
        for members in [vec![], vec![a, a], vec![b, a]] {
            assert!(super::TopologicalVertex::new(ctx, 1, members)
                .expect("vertex validation")
                .is_none());
        }
        let vertex = super::TopologicalVertex::new(ctx, 1, vec![a, b])
            .expect("vertex validation")
            .expect("ordered orbit");
        assert_eq!(vertex.id.get(), 1);
        assert_eq!(vertex.half_edges(), [a, b]);
    });
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let error = super::TopologicalVertex::new(&ctx, 1, vec![a]).expect_err("orbit work refused");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn topology_successor_and_open_tail_walks_refuse_work() {
    let rows = [row(1, 2), row(2, 3), row(3, 99)];
    let (edges, loops) = crate::test_support::assert_work_boundaries(
        &[
            "creo topology successor lookup",
            "creo topology ring successor lookup",
            "creo topology open tail lookup",
        ],
        |ctx| build(ctx, &rows),
    );
    assert_eq!(edges.len(), 6);
    assert!(loops.is_empty());
}

mod work_admission;

#[test]
fn half_edge_id_cost_excludes_struct_padding() {
    use cadmpeg_core::decode::cost::DecodeCost;
    with_service_context(|ctx| {
        // Four identifier bytes and one side tag exclude struct padding.
        for side in [super::Side::Zero, super::Side::One] {
            assert_eq!(
                side.decode_cost(ctx, "half-edge side cost")
                    .expect("side cost"),
                1
            );
            for curve_id in [0, 7, u32::MAX] {
                assert_eq!(
                    HalfEdgeId { curve_id, side }
                        .decode_cost(ctx, "half-edge id cost")
                        .expect("id cost"),
                    5
                );
            }
        }
        assert_eq!(<HalfEdgeId as DecodeCost>::FIXED_BYTES, Some(5));
    });
}
