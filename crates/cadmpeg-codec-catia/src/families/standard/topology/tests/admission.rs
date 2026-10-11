use super::super::{
    complete_duplicate_face_slots, reconstruct_incidence_with_edge_classes_and_mesh,
    EdgeBoundaryLayout, EdgeRow, StandardIncidenceEvidence,
};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

fn rows() -> Vec<EdgeRow> {
    (0..4)
        .map(|handle| {
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("complete row")
        })
        .collect()
}

#[test]
fn duplicate_class_count_rejects_before_copy_at_reconstruction_boundary() {
    for classes in [&[][..], &[0, 1, 2][..], &[0, 1, 2, 2, 3][..]] {
        let run = |ctx: &DecodeContext<'_>| {
            reconstruct_incidence_with_edge_classes_and_mesh(
                ctx,
                rows(),
                vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                &[[0, 1], [0, 1], [2, 2], [2, 2]],
                &[[0, 1], [1, 2], [2, 0], [0, 2]],
                3,
                StandardIncidenceEvidence {
                    edge_classes: Some(classes),
                    mesh_bytes: None,
                },
            )
        };
        assert!(crate::test_support::with_service_context(run)
            .expect("invalid evidence is structural")
            .is_none());
        assert!(crate::test_support::with_collection_limit(0, run)
            .expect("reject before copying face slots")
            .is_none());
    }
}

#[test]
fn duplicate_class_count_preserves_prior_face_scan_and_sticky_refusal() {
    let rows = rows();
    let run = |ctx: &DecodeContext<'_>| {
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &[[0, 1], [0, 1], [2, 2], [2, 2]],
            &[[0, 1], [1, 2], [2, 0], [0, 2]],
            3,
            Some(&[]),
            None,
        )
    };
    crate::test_support::with_work_limit(0, |ctx| {
        let CodecError::ResourceLimit(original) = run(ctx).expect_err("face scan precedes classes")
        else {
            panic!("resource refusal");
        };
        assert_eq!(
            original.operation,
            "catia_standard_duplicate_edge_face_bounds"
        );
        let CodecError::ResourceLimit(actual) = run(ctx).expect_err("sticky face refusal") else {
            panic!("resource refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn duplicate_degree_rollback_reuses_inactive_node_storage() {
    use super::super::{
        duplicate_degree, duplicate_face_admits_edge, restore_duplicate_degree,
        set_duplicate_degree,
    };
    use std::collections::BTreeMap;
    // Two node bounds cover six live slots; 64 alternatives must not grow them again.
    crate::test_support::with_materialized_limit(1024, |ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "degree rollback test")
            .expect("scope");
        let mut row = BTreeMap::new();
        storage
            .with_storage(|| {
                for point in 0..5 {
                    set_duplicate_degree(ctx, &mut row, point, 1)?;
                }
                for _ in 0..64 {
                    let before = duplicate_degree(ctx, &row, 5)?;
                    assert_eq!(before, None);
                    assert!(duplicate_face_admits_edge(ctx, &row, [0, 5])?);
                    set_duplicate_degree(ctx, &mut row, 5, 1)?;
                    assert_eq!(duplicate_degree(ctx, &row, 5)?, Some(1));
                    restore_duplicate_degree(ctx, &mut row, 5, before)?;
                    assert_eq!(duplicate_degree(ctx, &row, 5)?, None);
                    assert_eq!(row.len(), 6);
                    assert_eq!(row[&5], 0);
                }
                Ok::<_, cadmpeg_core::CodecError>(())
            })
            .expect("bounded degree slots");
        assert_eq!(row.len(), 6);
        assert!(row.iter().take(5).all(|(_, degree)| *degree == 1));
    });
}

#[test]
fn inactive_degree_slots_preserve_duplicate_face_ambiguity() {
    let rows: Vec<_> = [[0, 1], [1, 2], [2, 0]]
        .into_iter()
        .map(|ends| {
            EdgeRow::new(1, ends.to_vec(), EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("edge row")
        })
        .collect();
    let edge_faces = [[0, 0]; 3];
    let endpoints = [[0, 1], [1, 2], [2, 0]];
    crate::test_support::with_service_context(|ctx| {
        let unique =
            complete_duplicate_face_slots(ctx, &rows, &edge_faces, &endpoints, 2, None, None)
                .expect("search")
                .expect("one second face");
        assert_eq!(unique, vec![[0, 1]; 3]);
        let ambiguous =
            complete_duplicate_face_slots(ctx, &rows, &edge_faces, &endpoints, 3, None, None)
                .expect("search");
        assert!(
            ambiguous.is_none(),
            "rollback slots must not prevent closure on another face"
        );
    });
}
