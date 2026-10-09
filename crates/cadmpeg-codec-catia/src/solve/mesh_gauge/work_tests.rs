use super::{
    map_endpoint_relation_state, relation_row_gauge_mapping, EdgeBoundaryLayout, EdgeRow,
    MeshCandidateGauge, MeshEdgeGeometry, MeshEndpointRelationSelection,
};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

#[test]
fn relation_choice_sort_refuses_assignment_bytes() {
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
    let result = crate::test_support::with_work_limit(20_000, |ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_mapped_choices_sort"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            map_endpoint_relation_state(ctx, &state, gauge, &[])
        })
        .expect("service work allowance"),
        Some(state)
    );
}

#[test]
fn relation_row_sort_refuses_endpoint_signature_bytes() {
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
    let result = crate::test_support::with_work_limit(30_000, |ctx| {
        relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_ordered_rows_sort"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
        })
        .expect("service work allowance"),
        Some(vec![0, 1])
    );
}

#[test]
fn coordinate_gauge_refuses_unsearched_eight_point_class() {
    let rows = [
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("nonempty handles"),
    ];
    let candidates = [(0..8)
        .flat_map(|left| (left + 1..8).map(move |right| [left, right]))
        .collect::<Vec<_>>()];
    let result = crate::test_support::with_service_context(|ctx| {
        super::build_mesh_coordinate_gauge(
            ctx,
            8,
            &rows,
            &[[0, 1]],
            &[MeshEdgeGeometry::Line],
            &candidates,
            &[false],
        )
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_gauge_permutation_limit"));
}

#[test]
fn coordinate_permutation_search_refuses_caller_work_before_enumeration() {
    let result = crate::test_support::with_work_limit(0, |ctx| {
        super::enumerate_coordinate_permutations(
            ctx,
            &[0, 1],
            0,
            &mut vec![],
            &mut [false; 2],
            &mut vec![],
        )
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_permutation_search"));
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
fn gauge_signature_lookup_refuses_repeated_long_equal_keys() {
    let signatures = vec![vec![0usize; 128]; 2];
    let result = crate::test_support::with_work_limit(1_000, |ctx| {
        super::intern_gauge_signatures(ctx, signatures.clone(), |key| {
            std::mem::size_of_val(key.as_slice())
        })
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_signature_compare"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::intern_gauge_signatures(ctx, signatures, |key| {
                std::mem::size_of_val(key.as_slice())
            })
        })
        .expect("service comparison work"),
        vec![0, 0]
    );
}

#[test]
fn singleton_coordinate_classes_keep_one_identity_without_products() {
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
    let gauge = crate::test_support::with_collection_limit(200_000, |ctx| {
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

fn observed_work_refusals<T>(
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>,
) -> std::collections::HashSet<&'static str> {
    let mut operations = std::collections::HashSet::new();
    let mut cap = 0;
    for _ in 0..1024 {
        match crate::test_support::with_work_limit(cap, &run) {
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits =>
            {
                operations.insert(limit.operation);
                cap = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("finite test work");
            }
            Ok(_) => return operations,
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    panic!("fixture did not complete its admitted work");
}

#[test]
fn coordinate_gauge_membership_scans_refuse_before_search() {
    let rows = [
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("nonempty handles"),
    ];
    let operations = observed_work_refusals(|ctx| {
        super::build_mesh_coordinate_gauge(
            ctx,
            2,
            &rows,
            &[[0, 1]],
            &[MeshEdgeGeometry::Line],
            &[vec![[0, 1]]],
            &[false],
        )
    });
    for operation in [
        "catia_gauge_group_point_scan",
        "catia_gauge_affected_edge_scan",
        "catia_gauge_affected_point_scan",
    ] {
        assert!(
            operations.contains(operation),
            "missing work refusal for {operation}"
        );
    }
}

#[test]
fn mapped_pair_scan_refuses_before_duplicate_becomes_none() {
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
    // One fill unit for the assigned row, then one pair sort of two items
    // (2 + 16 * 3 * 8) for the first mapped pair, precede the second pair's scan.
    let result = crate::test_support::with_work_limit(1 + 2 + 16 * 3 * 8, |ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[0, 1])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_mapped_pair_scan"));
    assert!(crate::test_support::with_service_context(|ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[0, 1])
    })
    .expect("service work")
    .is_none());
}

#[test]
fn coordinate_refinement_and_automorphism_comparisons_refuse_key_bytes() {
    let rows = [
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("nonempty handles"),
    ];
    for evidence in [false, true] {
        let operations = observed_work_refusals(|ctx| {
            super::build_mesh_coordinate_gauge(
                ctx,
                2,
                &rows,
                &[[0, 1]],
                &[MeshEdgeGeometry::Line],
                &[vec![[0, 1]]],
                &[evidence],
            )
        });
        for operation in [
            "catia_gauge_refinement_compare",
            "catia_gauge_automorphism_rows_compare",
            "catia_gauge_permutation_dedup_compare",
        ] {
            assert!(
                operations.contains(operation),
                "missing work refusal for {operation}"
            );
        }
        if evidence {
            assert!(operations.contains("catia_gauge_identity_row_compare"));
        }
    }
}

#[test]
fn partial_endpoint_candidate_comparison_refuses_long_equal_keys() {
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
    let result = crate::test_support::with_work_limit(1_000, |ctx| {
        super::canonicalize_partial_endpoint_pair_gauge(ctx, &pairs, gauge)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_gauge_partial_pair_compare"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::canonicalize_partial_endpoint_pair_gauge(ctx, &pairs, gauge)
        })
        .expect("service comparison work"),
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
fn coordinate_topology_candidate_comparison_refuses_before_selection() {
    let topology = comparison_topology();
    let coordinate = super::MeshCoordinateGauge {
        components: vec![vec![vec![0, 1], vec![1, 0]]],
    };
    let candidates = [vec![[0, 1]]];
    let gauge = MeshCandidateGauge {
        edge_rows: &topology.edge_rows,
        edge_faces: &[],
        edge_geometry: &[MeshEdgeGeometry::Line],
        edge_candidates: &candidates,
        edge_identity_evidence: &[false],
        coordinate_gauge: Some(&coordinate),
    };
    let operations = observed_work_refusals(|ctx| {
        super::canonicalize_mesh_coordinate_gauges(ctx, topology.clone(), gauge)
    });
    assert!(operations.contains("catia_gauge_coordinate_topology_compare"));
}

#[test]
fn endpoint_relation_candidate_comparison_refuses_before_selection() {
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
    let domains = vec![vec![super::MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Deferred,
    }]];
    let operations = observed_work_refusals(|ctx| {
        super::canonicalize_endpoint_relation_state(ctx, &domains, &[None], gauge)
    });
    assert!(operations.contains("catia_relation_candidate_compare"));
}

#[test]
fn candidate_equivalence_refuses_each_variable_length_comparison() {
    let candidate = (comparison_topology(), vec![0, 1]);
    let operations = observed_work_refusals(|ctx| {
        super::mesh_candidates_equivalent_with_context(ctx, &candidate, &candidate, None)
    });
    for operation in [
        "catia_gauge_candidate_point_compare",
        "catia_gauge_candidate_topology_compare",
        "catia_gauge_candidate_assignment_compare",
    ] {
        assert!(
            operations.contains(operation),
            "missing work refusal for {operation}"
        );
    }
    assert!(crate::test_support::with_service_context(|ctx| {
        super::mesh_candidates_equivalent_with_context(ctx, &candidate, &candidate, None)
    })
    .expect("service comparison work"));
}

#[test]
fn signature_colors_preserve_first_occurrence_order() {
    let colors = crate::test_support::with_service_context(|ctx| {
        super::intern_gauge_signatures(ctx, [5, 3, 5, 8, 3], |_| 0)
    })
    .expect("signature admission");
    assert_eq!(colors, [0, 1, 0, 2, 1]);
}

#[test]
fn signature_interning_batches_large_key_comparisons() {
    let signatures = (0..1024).rev().map(|value| (value, vec![value; 32]));
    let colors = crate::test_support::with_work_limit(256_000_000, |ctx| {
        super::intern_gauge_signatures(ctx, signatures, |key| {
            key.1.len() * std::mem::size_of::<usize>()
        })
    })
    .expect("batch sorting fits a logarithmic comparison allowance");
    assert_eq!(colors, (0..1024).collect::<Vec<_>>());
}
