// SPDX-License-Identifier: Apache-2.0
use super::super::{build, face_components, vertex_orbits};
use super::{build_service, row};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn empty_topology_builders_are_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (edges, loops) = build(&ctx, &[]).expect("empty graph");
    assert!(edges.is_empty());
    assert!(loops.is_empty());
    assert!(face_components(&ctx, &[]).expect("empty faces").is_empty());
    let orbits = vertex_orbits(&ctx, &[]).expect("empty orbits");
    assert!(orbits.vertices.is_empty());
    assert!(orbits.incidence.is_empty());
    assert!(orbits.unstatable_orbits.is_empty());
}

#[test]
fn topology_build_withholds_rings_for_repeated_missing_and_mixed_face_successors() {
    let mut mixed = row(2, 1);
    mixed.faces = [std::num::NonZeroU32::new(11), std::num::NonZeroU32::new(21)];
    for rows in [
        vec![],
        vec![row(1, 2), row(1, 2), row(2, 1)],
        vec![row(1, 2), row(2, 99)],
        vec![row(1, 2), mixed],
    ] {
        let (_, loops) = build_service(&rows);
        assert!(loops.is_empty());
    }
    let (_, loops) = crate::test_support::assert_work_boundaries(
        &[
            "creo topology successor lookup",
            "creo topology ring successor lookup",
            "creo topology consumed membership",
        ],
        |ctx| build(ctx, &[row(1, 2), row(2, 1)]),
    );
    assert_eq!(loops.len(), 2);
    for ring in loops {
        assert_eq!(ring.half_edges().len(), 2);
        assert_eq!(ring.half_edges()[0].curve_id, 1);
        assert_eq!(ring.half_edges()[1].curve_id, 2);
        assert_eq!(ring.half_edges()[0].side, ring.half_edges()[1].side);
        assert_eq!(
            ring.face_id().expect("nonzero face").get(),
            if ring.half_edges()[0].side.index() == 0 {
                10
            } else {
                20
            }
        );
    }
}

#[test]
fn face_graph_projects_sorted_unique_nonzero_faces_from_unordered_rows() {
    let mut left = row(0, 1);
    left.faces = [std::num::NonZeroU32::new(2), std::num::NonZeroU32::new(1)];
    let mut right = row(1, 0);
    right.faces = [std::num::NonZeroU32::new(3), std::num::NonZeroU32::new(2)];
    let mut faceless = row(2, 2);
    faceless.faces = [None; 2];
    let rows = [right, faceless, left];
    let components = crate::test_support::assert_work_boundaries(
        &[
            "creo face graph assembly",
            "creo face graph traversal",
            "creo face component projection",
        ],
        |ctx| face_components(ctx, &rows),
    );
    assert_eq!(components.len(), 1);
    assert_eq!(components[0].face_ids(), [1, 2, 3]);
    assert_eq!(components[0].curve_ids(), [0, 1]);
}

#[test]
fn vertex_graph_projects_sorted_unique_orbit_members_from_unordered_edges() {
    let (mut edges, _) = build_service(&[row(1, 2), row(2, 1)]);
    let expected = crate::decode::with_test_decode_ctx(|ctx| vertex_orbits(ctx, &edges))
        .expect("ordered graph");
    edges.reverse();
    let actual = crate::test_support::assert_work_boundaries(
        &[
            "creo vertex graph assembly",
            "creo vertex graph traversal",
            "creo vertex orbit projection",
        ],
        |ctx| vertex_orbits(ctx, &edges),
    );
    assert_eq!(actual.vertices, expected.vertices);
    for (index, vertex) in actual.vertices.iter().enumerate() {
        assert_eq!(
            vertex.id.get(),
            u32::try_from(index + 1).expect("one-based fixture identity")
        );
        assert!(!vertex.half_edges().is_empty());
        assert!(vertex.half_edges().windows(2).all(|pair| pair[0] < pair[1]));
    }
    assert_eq!(actual.incidence.len(), edges.len());
}

#[test]
fn one_open_half_edge_has_exact_graph_work_and_retained_output() {
    use super::super::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, Side, TopologicalVertex};
    use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
    fn node_bytes<K, V>() -> u64 {
        let alignment = std::mem::align_of::<K>()
            .max(std::mem::align_of::<V>())
            .max(std::mem::align_of::<usize>());
        u64_from_index(
            11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
                + 16 * std::mem::size_of::<usize>()
                + 2 * alignment,
        )
    }
    let edges = [HalfEdge {
        id: HalfEdgeId {
            curve_id: 1,
            side: Side::Zero,
        },
        face_id: None,
        next: None,
    }];
    // Eight row/member visits, two five-byte lookups, and five first-node insertions.
    // A first insertion pays three node passes under the core B-tree contract.
    let expected_work = 8
        + 2 * 5
        + 3 * (node_bytes::<HalfEdgeId, &HalfEdge>()
            + node_bytes::<HalfEdgeId, std::collections::BTreeSet<HalfEdgeId>>()
            + 2 * node_bytes::<HalfEdgeId, ()>()
            + node_bytes::<HalfEdgeId, std::num::NonZeroU32>());
    let persistent = node_bytes::<HalfEdgeId, &HalfEdge>()
        + node_bytes::<HalfEdgeId, std::collections::BTreeSet<HalfEdgeId>>()
        + node_bytes::<HalfEdgeId, ()>();
    let orbit_scratch =
        node_bytes::<HalfEdgeId, ()>() + u64_from_index(4 * std::mem::size_of::<HalfEdgeId>());
    let peak = persistent + orbit_scratch.max(node_bytes::<HalfEdgeId, std::num::NonZeroU32>());
    let retained = std::mem::size_of::<HalfEdgeId>()
        + 4 * std::mem::size_of::<TopologicalVertex>()
        + 4 * std::mem::size_of::<HalfEdgeVertexIncidence>();
    let required =
        crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_materialized_bytes = peak;
            policy.limits.max_retained_bytes = u64_from_index(retained);
            policy.limits.max_collection_items = 9;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            vertex_orbits(&ctx, &edges)
        });
    assert_eq!(required, expected_work);
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::MaterializedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = expected_work;
        policy.limits.max_retained_bytes = u64_from_index(retained);
        policy.limits.max_materialized_bytes = peak;
        policy.limits.max_collection_items = 9;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = vertex_orbits(&ctx, &edges).expect("one open half-edge");
        assert_eq!(result.vertices.len(), 1);
        assert_eq!(result.vertices[0].id.get(), 1);
        assert_eq!(result.vertices[0].half_edges(), [edges[0].id]);
        assert_eq!(result.incidence.len(), 1);
        assert_eq!(result.incidence[0].half_edge, edges[0].id);
        assert_eq!(result.incidence[0].start_vertex_id.get(), 1);
        assert!(result.incidence[0].end_vertex_id.is_none());
        assert!(result.unstatable_orbits.is_empty());
        let (refusal, used, additional) = match dimension {
            ResourceDimension::RetainedBytes => (
                ctx.charge_retained_limit(1, "graph output backing probe")
                    .expect_err("exact retained backing"),
                u64_from_index(retained),
                1,
            ),
            ResourceDimension::MaterializedBytes => (
                ctx.reserve_scoped_limit(peak + 1, "graph scratch release probe")
                    .expect_err("scratch released before graph returns"),
                0,
                peak + 1,
            ),
            _ => unreachable!("two storage probes"),
        };
        assert_eq!(refusal.dimension, dimension);
        assert_eq!(refusal.used, used);
        assert_eq!(refusal.additional, additional);
    }
}
