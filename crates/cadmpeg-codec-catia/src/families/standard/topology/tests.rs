use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopology};
use std::collections::HashMap;

#[test]
fn standard_topology_copy_refuses_each_nested_collection_limit() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology, NonEmptyCoedges};

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: NonEmptyCoedges::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 0,
                }),
            }],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![7],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: vec![[0.0, 0.0, 0.0]],
        logical_vertex_count: 1,
    };
    let copy = crate::test_support::with_service_context(|ctx| topology.clone_charged(ctx))
        .expect("service resource budget");
    assert_eq!(copy, topology);
    for (limit, operation) in [
        "catia_standard_topology_copy_coedges",
        "catia_standard_topology_copy_boundaries",
        "catia_standard_topology_copy_faces",
        "catia_standard_edge_row_copy_handles",
        "catia_standard_topology_copy_edge_rows",
        "catia_standard_topology_copy_vertex_points",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            standard_collection_limit_operation(limit as u64, |ctx| {
                topology.clone_charged(ctx)?;
                Ok(())
            }),
            operation
        );
    }
}

#[test]
fn standard_native_vertex_binding_refuses_before_identity_and_edge_growth() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology, NonEmptyCoedges};

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: NonEmptyCoedges::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![7],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    };
    let bound = crate::test_support::with_service_context(|ctx| {
        topology.with_native_edge_vertices(ctx, &[[4, 5]])
    })
    .expect("service resource budget")
    .expect("native edge identities bind");
    assert_eq!(bound.logical_vertex_count, 2);
    assert_eq!(bound.faces[0].boundaries[0].coedges[0].start_vertex, 0);
    assert_eq!(bound.faces[0].boundaries[0].coedges[0].end_vertex, 1);
    assert_eq!(
        standard_collection_limit_operation(0, |ctx| {
            topology.with_native_edge_vertices(ctx, &[[4, 5]])?;
            Ok(())
        }),
        "catia_standard_native_vertex_identities"
    );
    assert_eq!(
        standard_collection_limit_operation(2, |ctx| {
            topology.with_native_edge_vertices(ctx, &[[4, 5]])?;
            Ok(())
        }),
        "catia_standard_native_edge_vertices"
    );
    assert!(crate::test_support::with_collection_limit(0, |ctx| {
        topology.with_native_edge_vertices(ctx, &[])
    })
    .expect("invalid length requires no allocation")
    .is_none());
}

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

fn duplicate_face_slot_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

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
    )
}

fn duplicate_face_slot_operation(max_collection_items: u64) -> &'static str {
    standard_collection_limit_operation(max_collection_items, |ctx| {
        duplicate_face_slot_fixture(ctx)?;
        Ok(())
    })
}

#[test]
fn standard_endpoint_degrees_propagate_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(4),
        "catia standard endpoint degrees"
    );
}

#[test]
fn standard_duplicate_unresolved_edges_refuse_before_growth() {
    assert_eq!(
        duplicate_face_slot_operation(3),
        "catia_standard_duplicate_unresolved_edges"
    );
}

#[test]
fn standard_endpoint_degree_entries_refuse_before_growth() {
    use super::set_duplicate_degree;

    let operation = standard_collection_limit_operation(0, |ctx| {
        let mut row = Vec::new();
        set_duplicate_degree(ctx, &mut row, 7, 1)
    });
    assert_eq!(operation, "catia_standard_endpoint_degree_entries");
    crate::test_support::with_service_context(|ctx| {
        let mut row = Vec::new();
        set_duplicate_degree(ctx, &mut row, 7, 1).expect("service budget");
        assert_eq!(row, [(7, 1)]);
    });
}

#[test]
fn standard_unresolved_assignment_propagates_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(12),
        "catia standard unresolved edge assignment"
    );
}

#[test]
fn standard_unresolved_marks_propagate_collection_refusal() {
    assert_eq!(
        duplicate_face_slot_operation(13),
        "catia standard unresolved edge marks"
    );
}

#[test]
fn standard_duplicate_choices_refuse_before_search_branch() {
    assert_eq!(
        duplicate_face_slot_operation(14),
        "catia_standard_duplicate_choices"
    );
}

#[test]
fn standard_duplicate_search_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    assert_eq!(
        crate::test_support::with_service_context(duplicate_face_slot_fixture)
            .expect("service resource budget"),
        Some(vec![[0, 1], [0, 1], [0, 1]])
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    assert!(matches!(
        duplicate_face_slot_fixture(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "catia_standard_duplicate_choice_scan"
    ));
}

#[test]
fn standard_duplicate_search_refuses_recursion_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    assert!(matches!(
        duplicate_face_slot_fixture(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "catia_standard_duplicate_face_search"
    ));
}

fn ambiguous_duplicate_face_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

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
        edge_classes,
        mesh_bytes,
    )
}

fn ambiguous_duplicate_face_operation(max_collection_items: u64) -> &'static str {
    standard_collection_limit_operation(max_collection_items, |ctx| {
        ambiguous_duplicate_face_fixture(ctx, Some(&[0, 1, 2, 2]), None)?;
        Ok(())
    })
}

#[test]
fn standard_duplicate_mesh_edge_faces_refuse_before_search_copy() {
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    assert!(crate::test_support::with_service_context(|ctx| {
        ambiguous_duplicate_face_fixture(ctx, Some(&[0, 1, 2, 3]), Some(&[]))
    })
    .expect("service resource budget")
    .is_none());
    let mut refused = HashSet::new();
    for cap in 0..128 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            ambiguous_duplicate_face_fixture(ctx, Some(&[0, 1, 2, 3]), Some(&[]))
        });
        if let Err(CodecError::ResourceLimit(limit)) = result {
            refused.insert(limit.operation);
        }
    }
    assert!(refused.contains("catia_standard_duplicate_mesh_edge_faces"));
}

#[test]
fn standard_duplicate_solution_values_refuse_before_copy() {
    assert_eq!(
        ambiguous_duplicate_face_operation(24),
        "catia_standard_duplicate_solution_values"
    );
}

#[test]
fn standard_duplicate_solutions_refuse_before_result_growth() {
    assert_eq!(
        ambiguous_duplicate_face_operation(26),
        "catia_standard_duplicate_solutions"
    );
}

#[test]
fn standard_duplicate_assignment_marks_propagate_collection_refusal() {
    assert_eq!(
        ambiguous_duplicate_face_operation(28),
        "catia standard duplicate assignment marks"
    );
}

#[test]
fn standard_duplicate_face_comparison_refuses_nested_face_lists() {
    use super::{duplicate_face_assignments_equivalent, EdgeBoundaryLayout, EdgeRow};
    use cadmpeg_core::CodecError;

    let rows = [
        EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        duplicate_face_assignments_equivalent(
            ctx,
            &[0, 1],
            &rows,
            &[[0, 0], [0, 0]],
            &[[0, 1], [0, 1]],
            None,
            [&[0, 0], &[0, 1]],
        )
    };
    assert!(!crate::test_support::with_service_context(run).expect("service resource budget"));
    for (cap, operation) in [
        "catia_standard_duplicate_left_faces",
        "catia_standard_duplicate_right_faces",
    ]
    .into_iter()
    .enumerate()
    {
        let result = crate::test_support::with_collection_limit(cap as u64 + 2, run);
        assert!(matches!(
            result,
            Err(CodecError::ResourceLimit(limit)) if limit.operation == operation
        ));
    }
}

#[test]
fn standard_duplicate_face_copy_refuses_before_completed_result() {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};
    use cadmpeg_core::CodecError;

    let rows = [EdgeRow {
        kind: 1,
        handles: vec![10],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        complete_duplicate_face_slots(ctx, &rows, &[[0, 1]], &[[0, 1]], 2, None, None)
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("service resource budget"),
        Some(vec![[0, 1]])
    );
    let refused = crate::test_support::with_collection_limit(0, run);
    assert!(matches!(
        refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_standard_duplicate_edge_faces"
    ));
}

#[test]
fn standard_face_edges_propagate_collection_refusal() {
    use super::{reconstruct_incidence, EdgeBoundaryLayout, EdgeRow};

    let operation = standard_collection_limit_operation(1, |ctx| {
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
fn standard_face_edge_entries_refuse_collection_limit() {
    use super::{reconstruct_incidence, EdgeBoundaryLayout, EdgeRow};

    let operation = standard_collection_limit_operation(3, |ctx| {
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
    assert_eq!(operation, "catia_standard_face_edge_entries");
}

#[test]
fn standard_incidence_mapped_collections_refuse_nested_limits() {
    use super::{reconstruct_incidence, EdgeBoundaryLayout, EdgeRow};
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let rows = || {
        vec![EdgeRow {
            kind: 0,
            handles: vec![7, 7],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }]
    };
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        reconstruct_incidence(ctx, rows(), vec![[1.0, 0.0, 0.0]], &[[0, 1]], &[[0, 0]], 2)
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("service resource budget")
        .is_some());
    let mut refused = HashSet::new();
    for cap in 0..48 {
        let result = crate::test_support::with_collection_limit(cap, run);
        if let Err(CodecError::ResourceLimit(limit)) = result {
            refused.insert(limit.operation);
        }
    }
    for operation in [
        "catia_standard_incidence_coedges",
        "catia_standard_incidence_boundaries",
        "catia_standard_incidence_faces",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
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
    let operation = standard_collection_limit_operation(4, |ctx| {
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
fn standard_boundary_inner_collections_refuse_each_limit() {
    use std::collections::HashSet;

    let edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, true)]),
    ]);
    let mut refused = HashSet::new();
    for cap in 0..24 {
        let operation = crate::test_support::with_collection_limit(cap, |ctx| {
            solve_boundary_orientation_constraints(ctx, 4, &edge_uses, false)
        });
        if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = operation {
            refused.insert(limit.operation);
        }
    }
    for operation in [
        "catia_standard_boundary_constraint_entries",
        "catia_standard_boundary_stack",
        "catia_standard_boundary_result",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn standard_orientation_refuses_before_unpaired_boundary() {
    use super::{orient_face_cycles, Boundary, CoedgeUse, FaceTopology, NonEmptyCoedges};

    let faces = || {
        vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: NonEmptyCoedges::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }]
    };
    assert!(crate::test_support::with_service_context(|ctx| {
        orient_face_cycles(ctx, &mut faces())
    })
    .expect("service resource budget")
    .is_none());
    for (cap, operation) in [
        "catia_standard_orientation_boundaries",
        "catia_standard_orientation_edge_uses",
        "catia_standard_orientation_edge_use_entries",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            standard_collection_limit_operation(cap as u64, |ctx| {
                orient_face_cycles(ctx, &mut faces())?;
                Ok(())
            }),
            operation
        );
    }
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
fn standard_completed_edge_vertices_refuse_before_absent_row() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology, NonEmptyCoedges};
    use cadmpeg_core::CodecError;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary {
                coedges: NonEmptyCoedges::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![10],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![11],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    };
    assert!(
        crate::test_support::with_service_context(|ctx| topology.edge_vertices(ctx))
            .expect("service resource budget")
            .is_none()
    );
    let refused = crate::test_support::with_collection_limit(2, |ctx| topology.edge_vertices(ctx));
    assert!(matches!(
        refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_standard_edge_vertices_complete"
    ));
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
    policy.limits.max_collection_items = 2;
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
fn standard_vertex_point_domain_entries_refuse_collection_limit() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary::new(vec![
                CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                },
                CoedgeUse {
                    edge_row: 1,
                    reversed: false,
                    start_vertex: 1,
                    end_vertex: 2,
                },
            ])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![10],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![11],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        logical_vertex_count: 3,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        topology
            .bind_vertex_points(&ctx, &[[0, 1], [1, 2]])
            .expect("service resource budget"),
        Some(vec![0, 1, 2])
    );
    assert!(topology
        .bind_vertex_points(&ctx, &[[3, 1], [1, 2]])
        .expect("service resource budget")
        .is_none());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 7;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let result = topology.bind_vertex_points(&ctx, &[[3, 1], [1, 2]]);
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia standard vertex point domain entries"));
}

#[test]
fn body_kinds_rejects_an_overflowing_face_group_sum() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let topology = StandardTopology {
        faces: Vec::new(),
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };

    assert_eq!(
        topology
            .body_kinds(&ctx, &[usize::MAX, 1])
            .expect("service resource budget"),
        None
    );
}

fn resource_limit_topology() -> StandardTopology {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology};

    let face = || FaceTopology {
        boundaries: vec![Boundary::new(vec![CoedgeUse {
            edge_row: 0,
            reversed: false,
            start_vertex: 0,
            end_vertex: 1,
        }])
        .expect("nonempty boundary")],
    };
    StandardTopology {
        faces: vec![face(), face()],
        edge_rows: (0..2)
            .map(|edge| EdgeRow {
                kind: 0,
                handles: vec![edge],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect(),
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    }
}

#[test]
fn body_group_allocations_refuse_before_missing_edge_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let topology = resource_limit_topology();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(topology
        .body_kinds(&ctx, &[2])
        .expect("service resource budget")
        .is_none());

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match topology.body_kinds(&ctx, &[2]) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(None) => {}
            Ok(Some(_)) => panic!("missing physical edge must reject the group"),
            Err(error) => panic!("unexpected body group refusal: {error}"),
        }
    }
    for operation in [
        "catia_body_group_slices",
        "catia_body_group_union",
        "catia_body_group_uses",
        "catia_body_group_components",
        "catia_body_group_seen_edges",
        "catia_body_group_paired",
        "catia_body_group_kinds",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn face_component_collections_refuse_before_group_result() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let topology = resource_limit_topology();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        topology
            .face_components(&ctx)
            .expect("service resource budget"),
        vec![vec![0, 1]]
    );

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match topology.face_components(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(groups) => assert_eq!(groups, vec![vec![0, 1]]),
            Err(error) => panic!("unexpected face component refusal: {error}"),
        }
    }
    for operation in [
        "catia_face_component_union",
        "catia_face_component_edges",
        "catia_face_component_labels",
        "catia_face_components",
        "catia_face_component_members",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn mesh_selection_reconstruction_refuses_nested_collection_limits() {
    use super::{reconstruct_mesh_selection, EdgeBoundaryLayout, EdgeRow};
    use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let rows = vec![EdgeRow {
        kind: 0,
        handles: vec![0],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let assignment_for = |edge| MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let directions = [vec![vec![false]]];
    let points = [[0.0, 0.0, 0.0]];
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert!(
        reconstruct_mesh_selection(&ctx, &rows, &points, &[assignment_for(1)], &directions)
            .expect("service resource budget")
            .is_none()
    );
    assert!(
        reconstruct_mesh_selection(&ctx, &rows, &points, &[assignment_for(0)], &directions)
            .expect("service resource budget")
            .is_some()
    );

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match reconstruct_mesh_selection(&ctx, &rows, &points, &[assignment_for(0)], &directions) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(Some(_)) => {}
            Ok(None) => panic!("single edge selection must reconstruct"),
            Err(error) => panic!("unexpected reconstruction refusal: {error}"),
        }
    }
    for operation in [
        "catia_mesh_selection_union",
        "catia_mesh_selection_corner_nodes",
        "catia_mesh_selection_corners",
        "catia_mesh_selection_coedges",
        "catia_mesh_selection_boundaries",
        "catia_mesh_selection_faces",
        "catia_mesh_selection_roots",
        "catia_mesh_selection_edge_copy",
        "catia_mesh_selection_handle_copy",
        "catia_mesh_selection_point_copy",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_cycles_rejects_duplicate_and_out_of_range_edges() {
    crate::test_support::with_service_context(|ctx| {
        assert!(incidence_cycles(ctx, &[0, 0], &[[0, 0]])
            .expect("service resource budget")
            .is_none());
        assert!(incidence_cycles(ctx, &[1], &[[0, 0]])
            .expect("service resource budget")
            .is_none());
    });
}

#[test]
fn incidence_cycles_refuse_before_invalid_edges_and_nested_growth() {
    use cadmpeg_core::CodecError;
    use std::collections::HashSet;

    let points = [[0, 1], [1, 0]];
    assert!(crate::test_support::with_service_context(|ctx| {
        incidence_cycles(ctx, &[0, 1], &points)
    })
    .expect("service resource budget")
    .is_some());
    assert!(crate::test_support::with_service_context(|ctx| {
        incidence_cycles(ctx, &[0, 0], &points)
    })
    .expect("service resource budget")
    .is_none());
    let invalid = crate::test_support::with_collection_limit(0, |ctx| {
        incidence_cycles(ctx, &[0, 0], &points)
    });
    assert!(matches!(
        invalid,
        Err(CodecError::ResourceLimit(limit))
            if limit.operation == "catia_incidence_seen_edges"
    ));

    let mut refused = HashSet::new();
    for cap in 0..20 {
        let result = crate::test_support::with_collection_limit(cap, |ctx| {
            incidence_cycles(ctx, &[0, 1], &points)
        });
        if let Err(CodecError::ResourceLimit(limit)) = result {
            refused.insert(limit.operation);
        }
    }
    for operation in [
        "catia_incidence_seen_edges",
        "catia_incidence_vertex_indices",
        "catia_incidence_vertex_rows",
        "catia_incidence_vertex_edges",
        "catia_incidence_unseen_edges",
        "catia_incidence_cycle_members",
        "catia_incidence_cycles",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_cycles_refuse_work_limit_before_unseen_scan() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let result = incidence_cycles(&ctx, &[0, 1], &[[0, 1], [1, 0]]);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "catia_incidence_unseen_scan"
    ));
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
