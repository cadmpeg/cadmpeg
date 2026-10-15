use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopologyDraft};
use std::collections::HashMap;

fn duplicate_face_slot_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    let rows = (0..3)
        .map(|handle| {
            assert!(
                EdgeRow::new(0, vec![handle], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
            );
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
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

#[test]
fn body_kinds_rejects_an_overflowing_face_group_sum() {
    let topology = StandardTopologyDraft {
        faces: Vec::new(),
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };

    assert_eq!(
        topology
            .body_kinds(&[usize::MAX, 1])
            .expect("service resource budget"),
        None
    );
}

#[test]
fn incidence_cycles_rejects_duplicate_and_out_of_range_edges() {
    assert!(incidence_cycles(&[0, 0], &[[0, 0]])
        .expect("service resource budget")
        .is_none());
    assert!(incidence_cycles(&[1], &[[0, 0]])
        .expect("service resource budget")
        .is_none());
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

#[test]
fn edge_row_admission_couples_table_kind_and_boundary_shape() {
    use super::{EdgeBoundaryLayout, EdgeRow};
    for kind in [0, 3, 0xff] {
        assert!(EdgeRow::new(kind, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
    }
    for handles in [Vec::new(), vec![0]] {
        assert!(EdgeRow::new(1, handles, EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
    }
    assert!(EdgeRow::new(
        1,
        vec![0, 1],
        EdgeBoundaryLayout::InteriorWithFlankingCorners
    )
    .is_none());
    for kind in [1, 2] {
        let row = EdgeRow::new(
            kind,
            vec![0, 1, 2],
            EdgeBoundaryLayout::InteriorWithFlankingCorners,
        )
        .expect("interior handle");
        assert_eq!(row.kind(), kind);
        assert_eq!(row.boundary_pattern(), Some(&[1][..]));
    }
    let mut complete =
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("complete run");
    assert!(!complete.select_flanking_corners());
    assert_eq!(
        complete.boundary_layout(),
        EdgeBoundaryLayout::CompleteBoundaryRun
    );
}

#[test]
fn duplicate_face_sort_assigns_free_face_slots() {
    assert_eq!(
        crate::test_support::with_service_context(duplicate_face_slot_fixture)
            .expect("service topology admission"),
        Some(vec![[0, 1], [0, 1], [0, 1]])
    );
}
