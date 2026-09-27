use crate::families::standard::fbb::parse_edge_tables_scoped_at;
use crate::families::standard::fbb::parse_fbb_edge_tables;
use crate::families::standard::fbb::parse_fbb_edge_tables_width;
use crate::families::standard::fbb::parse_trim_record;
use crate::families::standard::fbb::prune_edge_candidates_by_port_domains;
use crate::families::standard::fbb::prune_edge_candidates_by_port_domains_with_deferred;
use crate::families::standard::fbb::EDGE_DELIMITER;
use crate::families::standard::topology::complete_duplicate_face_slots;
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::EdgeRow;
use std::collections::HashSet;

#[test]
fn equal_endpoint_ports_produce_closed_edge_candidates() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let ports = [[10, 10], [10, 11]];
    let candidates = [vec![[0, 0], [1, 1], [2, 2]], vec![[1, 3], [2, 4]]];
    assert_eq!(
        prune_edge_candidates_by_port_domains(&ctx, &ports, &candidates)
            .expect("service resource budget"),
        Some(vec![vec![[1, 1], [2, 2]], vec![[1, 3], [2, 4]]])
    );
    assert_eq!(
        prune_edge_candidates_by_port_domains(&ctx, &[[10, 10]], &[vec![[0, 1], [0, 2]]])
            .expect("service resource budget"),
        None
    );
}

#[test]
fn endpoint_port_domains_propagate_pair_correlation_to_a_fixpoint() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let ports = [[10, 11], [11, 12], [12, 13]];
    let candidates = [vec![[0, 1], [2, 3]], vec![[1, 4], [3, 5]], vec![[4, 6]]];

    assert_eq!(
        prune_edge_candidates_by_port_domains(&ctx, &ports, &candidates)
            .expect("service resource budget"),
        Some(vec![vec![[0, 1]], vec![[1, 4]], vec![[4, 6]]])
    );
}

#[test]
fn deferred_port_rows_do_not_constrain_open_face_components() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let ports = [[10, 11], [11, 12], [12, 13]];
    let candidates = [vec![[0, 1]], vec![[1, 2]], vec![[3, 4]]];

    assert_eq!(
        prune_edge_candidates_by_port_domains(&ctx, &ports, &candidates)
            .expect("service resource budget"),
        None
    );
    assert_eq!(
        prune_edge_candidates_by_port_domains_with_deferred(
            &ctx,
            &ports,
            &candidates,
            &[false, true, true],
        )
        .expect("service resource budget"),
        Some(candidates.to_vec())
    );
}

#[test]
fn port_domain_pruning_refuses_input_sized_collections() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let ports = [[10, 11], [11, 12]];
    let candidates = [vec![[0, 1]], vec![[1, 2]]];
    let mut operations = HashSet::new();
    for deferred in [&[][..], &[false, true][..]] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("service decode context");
        assert_eq!(
            prune_edge_candidates_by_port_domains_with_deferred(
                &ctx,
                &ports,
                &candidates,
                deferred,
            )
            .expect("service resource budget"),
            Some(candidates.to_vec())
        );

        for cap in 0..=100 {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                    .expect("fixture fits input limit");
            match prune_edge_candidates_by_port_domains_with_deferred(
                &ctx,
                &ports,
                &candidates,
                deferred,
            ) {
                Err(CodecError::ResourceLimit(limit)) => {
                    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
                    operations.insert(limit.operation);
                }
                Ok(Some(_)) => {}
                Ok(None) => panic!("consistent endpoint ports must admit a candidate"),
                Err(error) => panic!("unexpected port pruning refusal: {error}"),
            }
        }
    }
    for operation in [
        "catia_port_effective_deferred",
        "catia_deferred_ports",
        "catia_port_all_points",
        "catia_port_deferred_domain",
        "catia_port_candidate_domain",
        "catia_port_domains",
        "catia_port_nodes",
        "catia_port_constrained_edges",
        "catia_port_constrained_pairs",
        "catia_port_filtered_pairs",
        "catia_port_filtered_edges",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn duplicate_face_reference_slot_is_completed_by_face_closure() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &ctx,
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[[0, 1], [1, 2], [2, 0]],
        2,
        None,
        Some(&[]),
    )
    .expect("service resource budget")
    .expect("unique face-closing slot assignment");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn duplicate_face_completion_keeps_sparse_endpoint_identities() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &ctx,
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[
            [usize::MAX, usize::MAX - 1],
            [usize::MAX - 1, 0],
            [0, usize::MAX],
        ],
        2,
        None,
        Some(&[]),
    )
    .expect("service resource budget")
    .expect("sparse endpoint identities");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn independent_duplicate_face_slots_have_one_canonical_search_order() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let rows = (0..12)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let serialized = vec![[0, 0]; rows.len()];
    let points = (0..rows.len())
        .map(|point| [point, point])
        .collect::<Vec<_>>();

    let completed = complete_duplicate_face_slots(&ctx, &rows, &serialized, &points, 2, None, None)
        .expect("service resource budget")
        .expect("independent closed edges have one face completion");
    assert_eq!(completed, vec![[0, 1]; rows.len()]);
}

#[test]
fn counted_edge_arities_are_bounded_by_remaining_bytes() {
    let oversized_row = [0x01, 0x01, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff];
    assert!(
        crate::test_support::with_service_context(|ctx| parse_edge_tables_scoped_at(
            ctx,
            &oversized_row,
            0
        ))
        .expect("service resource budget")
        .is_none()
    );
    assert!(
        crate::test_support::with_service_context(|ctx| parse_fbb_edge_tables_width(
            ctx,
            &oversized_row,
            0,
            3
        ))
        .expect("service resource budget")
        .is_none()
    );
}

#[test]
fn fbb_edge_width_requires_a_complete_counted_vertex_table() {
    let mut bytes = Vec::new();
    for kind in [1, 2] {
        bytes.extend_from_slice(&[0x01, kind, 0x01, 0x02, 0x02]);
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        bytes.extend_from_slice(&EDGE_DELIMITER);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x01]);

    assert!(
        crate::test_support::with_service_context(|ctx| parse_fbb_edge_tables_width(
            ctx, &bytes, 0, 3
        ))
        .expect("service resource budget")
        .is_some()
    );
    assert!(
        crate::test_support::with_service_context(|ctx| parse_fbb_edge_tables(ctx, &bytes, 0))
            .expect("service resource budget")
            .is_none()
    );
}

#[test]
fn standard_edge_tables_use_the_fixed_u16_width_when_rows_are_empty() {
    let mut bytes = vec![
        0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2, 0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2,
        0xd2,
    ];
    for kind in [1, 2] {
        bytes.extend_from_slice(&[0x01, kind, 0x00]);
        bytes.extend_from_slice(&EDGE_DELIMITER);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x03]);
    for xyz in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let points = crate::test_support::with_service_context(|ctx| {
        crate::families::standard::fbb::standard_vertex_points(ctx, &bytes)
    })
    .expect("service resource budget")
    .expect("standard vertex table");
    assert_eq!(points.len(), 3);
}

#[test]
fn trim_primitive_counts_are_bounded_by_remaining_bytes() {
    let oversized_primitives = [
        0x01, 0x46, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00,
        0x00,
    ];
    assert!(
        crate::test_support::with_service_context(|ctx| parse_trim_record(
            ctx,
            &oversized_primitives,
            0,
            2
        ))
        .expect("service resource budget")
        .is_none()
    );
}

#[test]
fn duplicate_face_completion_rejects_out_of_range_faces() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let rows = vec![EdgeRow {
        kind: 0,
        handles: vec![0, 1],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    assert!(
        complete_duplicate_face_slots(&ctx, &rows, &[[0, 2]], &[[0, 1]], 2, None, None,)
            .expect("service resource budget")
            .is_none()
    );
}

#[test]
fn equivalent_edge_rows_share_one_incidence_assignment_gauge() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("service decode context");
    let rows = vec![
        EdgeRow {
            kind: 0,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![2, 3],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![4, 5],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let faces = complete_duplicate_face_slots(
        &ctx,
        &rows,
        &[[0, 1], [0, 1], [2, 2], [2, 2]],
        &[[0, 1], [1, 2], [2, 0], [0, 2]],
        3,
        Some(&[0, 1, 2, 2]),
        None,
    )
    .expect("service resource budget")
    .expect("one assignment modulo equivalent edge rows");

    let mut assigned = [faces[2][1], faces[3][1]];
    assigned.sort_unstable();
    assert_eq!(assigned, [0, 1]);
}

#[test]
fn native_edge_carrier_binding_requires_equal_object_identity() {
    use crate::families::standard::decode::standard_native_support_edge_ids;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};

    let supports = [
        StandardCurveSupport {
            pos: 0,
            tag: 70,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        },
        StandardCurveSupport {
            pos: 1,
            tag: 900,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        },
    ];
    let native_support_ids = HashSet::from([70, 71]);
    assert_eq!(
        standard_native_support_edge_ids(&supports, &native_support_ids),
        vec![Some(70), None]
    );

    assert_eq!(
        standard_native_support_edge_ids(&supports[1..], &HashSet::from([900])),
        vec![Some(900)]
    );
}
