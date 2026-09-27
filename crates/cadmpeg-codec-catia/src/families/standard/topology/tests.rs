use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopology};
use std::collections::HashMap;

fn standard_collection_limit_operation(
    max_collection_items: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError>,
) -> &'static str {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = run(&ctx).expect_err("standard topology allocation exceeds the collection limit");
    match error {
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems =>
        {
            limit.operation
        }
        other => panic!("expected collection resource limit, got {other:?}"),
    }
}

fn duplicate_face_slot_operation(max_collection_items: u64) -> &'static str {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    standard_collection_limit_operation(max_collection_items, |ctx| {
        let rows = (0..3)
            .map(|handle| EdgeRow {
                kind: 0,
                handles: vec![handle],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect::<Vec<_>>();
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &[[0, 1], [0, 1], [0, 0]],
            &[[0, 1], [1, 2], [2, 0]],
            2,
            None,
            None,
        )?;
        Ok(())
    })
}

#[test]
fn standard_endpoint_degrees_propagate_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(0),
        "catia standard endpoint degrees"
    );
}

#[test]
fn standard_unresolved_assignment_propagates_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(2),
        "catia standard unresolved edge assignment"
    );
}

#[test]
fn standard_unresolved_marks_propagate_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(3),
        "catia standard unresolved edge marks"
    );
}

#[test]
fn standard_duplicate_assignment_marks_propagate_collection_refusal() {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    let operation = standard_collection_limit_operation(7, |ctx| {
        let rows = (0..4)
            .map(|handle| EdgeRow {
                kind: 0,
                handles: vec![handle],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect::<Vec<_>>();
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &[[0, 1], [0, 1], [2, 2], [2, 2]],
            &[[0, 1], [1, 2], [2, 0], [0, 2]],
            3,
            Some(&[0, 1, 2, 2]),
            None,
        )?;
        Ok(())
    });
    assert_eq!(operation, "catia standard duplicate assignment marks");
}

#[test]
fn standard_face_edges_propagate_collection_refusal() {
    use super::{reconstruct_incidence, EdgeBoundaryLayout, EdgeRow};

    let operation = standard_collection_limit_operation(0, |ctx| {
        reconstruct_incidence(
            ctx,
            vec![EdgeRow {
                kind: 0,
                handles: vec![7, 7],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            }],
            vec![[1.0, 0.0, 0.0]],
            &[[0, 1]],
            &[[0, 0]],
            2,
        )?;
        Ok(())
    });
    assert_eq!(operation, "catia standard face edges");
}

#[test]
fn standard_boundary_constraints_propagate_collection_refusal() {
    let operation = standard_collection_limit_operation(0, |ctx| {
        solve_boundary_orientation_constraints(
            ctx,
            2,
            &HashMap::from([(0, vec![(0, false), (1, false)])]),
            true,
        )?;
        Ok(())
    });
    assert_eq!(operation, "catia standard boundary constraints");
}

#[test]
fn standard_boundary_flips_propagate_collection_refusal() {
    let operation = standard_collection_limit_operation(2, |ctx| {
        solve_boundary_orientation_constraints(
            ctx,
            2,
            &HashMap::from([(0, vec![(0, false), (1, false)])]),
            true,
        )?;
        Ok(())
    });
    assert_eq!(operation, "catia standard boundary flips");
}

#[test]
fn standard_edge_vertices_propagate_collection_refusal() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary::new(vec![CoedgeUse {
                edge_row: 0,
                reversed: false,
                start_vertex: 0,
                end_vertex: 1,
            }])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        topology
            .edge_vertices(&ctx)
            .expect("service resource budget"),
        Some(vec![[0, 1]])
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = topology
        .edge_vertices(&ctx)
        .expect_err("edge array exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia standard edge vertices"));
}

#[test]
fn standard_vertex_point_domains_propagate_collection_refusal() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary::new(vec![CoedgeUse {
                edge_row: 0,
                reversed: false,
                start_vertex: 0,
                end_vertex: 1,
            }])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = topology
        .bind_vertex_points(&ctx, &[[0, 1]])
        .expect_err("vertex domains exceed the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia standard vertex point domains"));
}

#[test]
fn body_kinds_rejects_an_overflowing_face_group_sum() {
    let topology = StandardTopology {
        faces: Vec::new(),
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };

    assert_eq!(topology.body_kinds(&[usize::MAX, 1]), None);
}

#[test]
fn incidence_cycles_rejects_duplicate_and_out_of_range_edges() {
    assert!(incidence_cycles(&[0, 0], &[[0, 0]]).is_none());
    assert!(incidence_cycles(&[1], &[[0, 0]]).is_none());
}

#[test]
fn boundary_orientation_constraints_retain_the_flip_assignment() {
    let edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, true)]),
    ]);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            solve_boundary_orientation_constraints(ctx, 4, &edge_uses, false)
        })
        .expect("service resource budget"),
        Some(vec![false, true, true, false])
    );
}
