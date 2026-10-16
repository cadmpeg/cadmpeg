use super::{EdgeBoundaryLayout, EdgeRow, MeshEdgeGeometry};
use crate::solve::mesh_gauge::map_endpoint_relation_state;
use crate::solve::mesh_gauge::relation_row_gauge_mapping;
use crate::solve::mesh_gauge::MeshCandidateGauge;
use crate::solve::mesh_quotient::MeshEndpointRelationSelection;

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
    .expect("group the gauge signatures");
    assert_eq!(colors, (0..1024).collect::<Vec<_>>());
}

#[test]
fn edgeless_relation_state_maps_unchanged() {
    let state = (
        vec![],
        vec![vec![
            MeshEndpointRelationSelection::Enumerated {
                assignments: vec![0; 128],
                edge_pairs: vec![],
            };
            2
        ]],
    );
    let gauge = MeshCandidateGauge {
        edge_rows: &[],
        edge_faces: &[],
        edge_geometry: &[],
        edge_candidates: &[],
        edge_identity_evidence: &[],
        coordinate_gauge: None,
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            map_endpoint_relation_state(ctx, &state, gauge, &[])
        })
        .expect("map the endpoint relation state"),
        Some(state)
    );
}

#[test]
fn identical_edge_rows_keep_the_identity_row_mapping() {
    let rows = std::array::from_fn::<_, 2, _>(|_| {
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("nonempty handles")
    });
    let faces = [[0, 1]; 2];
    let geometry = [MeshEdgeGeometry::Line; 2];
    let candidates = [vec![[0, 1]], vec![[0, 1]]];
    let evidence = [false; 2];
    let gauge = MeshCandidateGauge {
        edge_rows: &rows,
        edge_faces: &faces,
        edge_geometry: &geometry,
        edge_candidates: &candidates,
        edge_identity_evidence: &evidence,
        coordinate_gauge: None,
    };
    let state = (
        vec![None; 2],
        vec![vec![MeshEndpointRelationSelection::Deferred; 128]],
    );
    assert_eq!(
        relation_row_gauge_mapping(&state, gauge, &[0, 1])
            .expect("map the relation rows to the gauge"),
        Some(vec![0, 1])
    );
}

#[test]
fn coordinate_permutations_of_two_points_list_both_orders() {
    let mut output = vec![];
    crate::test_support::with_service_context(|ctx| {
        super::enumerate_coordinate_permutations(
            ctx,
            &[0, 1],
            0,
            &mut vec![],
            &mut [false; 2],
            &mut output,
        )
    })
    .expect("two permutations fit service work");
    assert_eq!(output, vec![vec![0, 1], vec![1, 0]]);
}

#[test]
fn equal_gauge_signatures_share_one_color() {
    let signatures = vec![vec![0usize; 128]; 2];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::intern_gauge_signatures(ctx, signatures)
        })
        .expect("group the gauge signatures"),
        vec![0, 0]
    );
}

#[test]
fn relation_selection_naming_an_edge_twice_does_not_map() {
    let state = (
        vec![None],
        vec![vec![MeshEndpointRelationSelection::Enumerated {
            assignments: vec![],
            edge_pairs: vec![(0, [0, 1]), (0, [0, 1])],
        }]],
    );
    let gauge = MeshCandidateGauge {
        edge_rows: &[],
        edge_faces: &[],
        edge_geometry: &[],
        edge_candidates: &[],
        edge_identity_evidence: &[],
        coordinate_gauge: None,
    };
    assert!(crate::test_support::with_service_context(|ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[0, 1])
    })
    .expect("map the endpoint relation state")
    .is_none());
}

#[test]
fn unassigned_endpoint_pairs_canonicalize_unchanged() {
    let coordinate = super::MeshCoordinateGauge {
        components: vec![vec![vec![0, 1], vec![1, 0]]],
    };
    let gauge = MeshCandidateGauge {
        edge_rows: &[],
        edge_faces: &[],
        edge_geometry: &[],
        edge_candidates: &[],
        edge_identity_evidence: &[],
        coordinate_gauge: Some(&coordinate),
    };
    let pairs = vec![None; 128];
    assert_eq!(
        super::canonicalize_partial_endpoint_pair_gauge(&pairs, gauge),
        Some(pairs)
    );
}

fn comparison_topology() -> crate::families::standard::topology::StandardTopologyDraft {
    use crate::families::standard::topology::{
        BoundaryDraft, CoedgeUse, FaceTopologyDraft, StandardTopologyDraft,
    };
    StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft::new(vec![
                CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                },
                CoedgeUse {
                    edge_row: 0,
                    reversed: true,
                    start_vertex: 1,
                    end_vertex: 0,
                },
            ])
            .expect("nonempty cycle")],
        }],
        edge_rows: vec![
            EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("nonempty handles"),
        ],
        vertex_points: vec![[0.0, 0.0, 0.0]; 2],
        logical_vertex_count: 2,
    }
}

#[test]
fn mesh_candidate_without_gauge_is_equivalent_to_itself() {
    let candidate = (comparison_topology(), vec![0, 1]);
    assert!(crate::test_support::with_service_context(|ctx| {
        super::mesh_candidates_equivalent(ctx, &candidate, &candidate, None)
    })
    .expect("check that the mesh candidates are equivalent"));
}
