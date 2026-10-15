use super::{EdgeBoundaryLayout, EdgeRow, MeshEdgeGeometry};

#[test]
fn coordinate_gauge_keeps_identity_for_unsearched_eight_point_class() {
    let rows = [
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("nonempty handles"),
    ];
    let candidates = [(0..8)
        .flat_map(|left| (left + 1..8).map(move |right| [left, right]))
        .collect::<Vec<_>>()];
    let (result, refusal) = crate::test_support::with_service_context(|ctx| {
        let result = super::build_mesh_coordinate_gauge(
            ctx,
            8,
            &rows,
            &[[0, 1]],
            &[MeshEdgeGeometry::Line],
            &candidates,
            &[false],
        );
        (result, ctx.resource_refusal())
    });
    // 8! orders exceed the permutation limit: the class is not searched and
    // its component keeps the identity order only.
    let gauge = result.expect("the permutation limit skips the class search");
    assert_eq!(refusal, None);
    assert!(!gauge.components.is_empty());
    for permutations in &gauge.components {
        assert_eq!(permutations, &vec![(0..8).collect::<Vec<_>>()]);
    }
}

#[test]
fn singleton_coordinate_classes_keep_one_identity() {
    const EDGE_COUNT: usize = 512;
    let rows = (1..=EDGE_COUNT)
        .map(|point| {
            EdgeRow::new(
                1,
                vec![0, u32::try_from(point).expect("fixture point fits")],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("nonempty handles")
        })
        .collect::<Vec<_>>();
    let geometry = (1..=EDGE_COUNT)
        .map(|radius| MeshEdgeGeometry::Circle {
            center: [0; 3],
            radius: u64::try_from(radius).expect("fixture radius fits"),
        })
        .collect::<Vec<_>>();
    let candidates = (1..=EDGE_COUNT)
        .map(|point| vec![[0, point]])
        .collect::<Vec<_>>();
    let gauge = crate::test_support::with_service_context(|ctx| {
        super::build_mesh_coordinate_gauge(
            ctx,
            EDGE_COUNT + 1,
            &rows,
            &[[0, 1]; EDGE_COUNT],
            &geometry,
            &candidates,
            &[false; EDGE_COUNT],
        )
    })
    .expect("unique incident carriers need one identity permutation");
    assert_eq!(
        gauge.components,
        vec![vec![(0..=EDGE_COUNT).collect::<Vec<_>>()]]
    );
}

#[test]
fn signature_colors_preserve_first_occurrence_order() {
    let colors = crate::test_support::with_service_context(|ctx| {
        super::intern_gauge_signatures(ctx, [5, 3, 5, 8, 3])
    })
    .expect("signature admission");
    assert_eq!(colors, [0, 1, 0, 2, 1]);
}

#[test]
fn signature_interning_orders_large_keys() {
    let signatures = (0..1024).rev().map(|value| (value, vec![value; 32]));
    let colors = crate::test_support::with_service_context(|ctx| {
        super::intern_gauge_signatures(ctx, signatures)
    })
    .expect("service decode");
    assert_eq!(colors, (0..1024).collect::<Vec<_>>());
}
