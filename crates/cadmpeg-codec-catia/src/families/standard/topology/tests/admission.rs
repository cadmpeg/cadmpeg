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
        assert_eq!(original.operation, "catia_standard_duplicate_edge_face_bounds");
        let CodecError::ResourceLimit(actual) = run(ctx).expect_err("sticky face refusal") else {
            panic!("resource refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
