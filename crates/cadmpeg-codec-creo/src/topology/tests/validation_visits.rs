// SPDX-License-Identifier: Apache-2.0
use super::super::{FaceComponent, HalfEdge, HalfEdgeId, Loop, Side, TopologicalVertex};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn id(curve_id: u32) -> HalfEdgeId {
    HalfEdgeId {
        curve_id,
        side: Side::Zero,
    }
}

fn with_work<T>(work: u64, f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    f(&ctx)
}

#[test]
fn fixed_and_empty_topology_validation_is_free_and_keeps_original_refusal() {
    let members = vec![id(1); 128];
    let graph = vec![
        HalfEdge {
            id: id(1),
            face_id: None,
            next: None
        };
        128
    ];
    with_work(0, |ctx| {
        assert!(Loop::new(ctx, None, vec![], &graph)
            .expect("empty ring")
            .is_none());
        assert!(FaceComponent::new(ctx, vec![], vec![1; 128])
            .expect("empty faces")
            .is_none());
        assert!(TopologicalVertex::new(ctx, 0, members.clone())
            .expect("zero identity")
            .is_none());
        assert!(TopologicalVertex::new(ctx, 1, vec![])
            .expect("empty members")
            .is_none());
        let vertex = TopologicalVertex::new(ctx, 7, vec![id(1)])
            .expect("no adjacent pair")
            .expect("one valid member");
        assert_eq!(vertex.id.get(), 7);
        assert_eq!(vertex.half_edges(), [id(1)]);
        let original = ctx
            .charge_work_limit(1, "seed fixed topology refusal")
            .expect_err("zero cap");
        assert_eq!((original.used, original.additional), (0, 1));
        assert!(matches!(Loop::new(ctx, None, vec![], &graph),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(FaceComponent::new(ctx, vec![], vec![]),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        for (number, members) in [(0, members), (1, vec![]), (7, vec![id(1)])] {
            assert!(matches!(TopologicalVertex::new(ctx, number, members),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn closed_ring_validation_admits_actual_members_prefixes_and_graph_candidates() {
    let face = std::num::NonZeroU32::new(10);
    let graph = vec![
        HalfEdge {
            id: id(1),
            face_id: face,
            next: Some(id(2)),
        },
        HalfEdge {
            id: id(2),
            face_id: face,
            next: Some(id(1)),
        },
    ];
    let mut duplicate_graph = vec![graph[0].clone(), graph[0].clone()];
    duplicate_graph.extend(vec![graph[1].clone(); 128]);
    for (ring, graph, valid) in [
        (vec![id(1), id(2)], graph.clone(), true),
        (vec![id(1), id(2), id(1)], graph.clone(), false),
        (vec![id(1), id(2)], duplicate_graph, false),
        (vec![id(3), id(1)], graph, false),
        (vec![id(1)], vec![], false),
    ] {
        let result = super::work_output(|ctx| Loop::new(ctx, face, ring.clone(), &graph));
        if valid {
            let result = result.expect("closed ring");
            assert_eq!(result.face_id(), face);
            assert_eq!(result.half_edges(), ring);
        } else {
            assert!(result.is_none());
        }
    }
}

#[test]
fn face_component_validation_admits_only_executed_zero_and_order_queries() {
    for (faces, curves, valid) in [
        (vec![0, 1, 2], vec![1; 128], false),
        (vec![2, 0, 1], vec![1; 128], false),
        (vec![2, 1], vec![1; 128], false),
        (vec![1], vec![1, 1, 2], false),
        (vec![1, 2], vec![0, 1], true),
    ] {
        let result =
            super::work_output(|ctx| FaceComponent::new(ctx, faces.clone(), curves.clone()));
        if valid {
            let result = result.expect("ordered component");
            assert_eq!(result.face_ids(), faces);
            assert_eq!(result.curve_ids(), curves);
        } else {
            assert!(result.is_none());
        }
    }
}

#[test]
fn vertex_orbit_validation_admits_adjacent_pairs_and_stops_at_first_conflict() {
    for (members, valid) in [
        (vec![id(1), id(2), id(3)], true),
        (vec![id(1), id(1), id(3)], false),
        (vec![id(2), id(1), id(3)], false),
        (vec![id(1), id(3), id(2)], false),
    ] {
        let result = super::work_output(|ctx| TopologicalVertex::new(ctx, 7, members.clone()));
        if valid {
            let result = result.expect("ordered vertex orbit");
            assert_eq!(result.id.get(), 7);
            assert_eq!(result.half_edges(), members);
        } else {
            assert!(result.is_none());
        }
    }
}
