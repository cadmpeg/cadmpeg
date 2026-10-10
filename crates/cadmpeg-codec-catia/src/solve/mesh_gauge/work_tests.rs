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
    // Two empty-assignment keys have a one-byte discriminant each. Twice
    // their sort bound admits construction but not two 128-usize key lanes.
    let slot_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<MeshEndpointRelationSelection>());
    let cap = 2 * (4 + (2 * slot_bytes + 4) * 3 * 8);
    let result = crate::test_support::with_work_limit(cap, |ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_mapped_choices_sort"));
    let short = (
        vec![],
        vec![vec![
            MeshEndpointRelationSelection::Enumerated {
                assignments: vec![],
                edge_pairs: vec![],
            };
            2
        ]],
    );
    assert_eq!(
        crate::test_support::with_work_limit(cap, |ctx| {
            map_endpoint_relation_state(ctx, &short, gauge, &[])
        })
        .expect("short assignments fit the same finite limit"),
        Some(short)
    );
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
    // Two first-node insertions pay three node passes each; the target
    // removal pays one. Three occupied-key probes compare one class key.
    let index_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<usize>());
    let group_key_bytes =
        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<super::MeshEdgeGaugeKey>());
    let group_value_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Vec<usize>>());
    let alignment = cadmpeg_core::decode::u64_from_index(
        std::mem::align_of::<super::MeshEdgeGaugeKey>()
            .max(std::mem::align_of::<Vec<usize>>())
            .max(std::mem::align_of::<usize>()),
    );
    let node_bytes = 11 * (group_key_bytes + group_value_bytes) + 16 * index_bytes + 2 * alignment;
    let class_key_bytes = 1
        + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<EdgeBoundaryLayout>())
        + cadmpeg_core::decode::u64_from_index(std::mem::size_of::<MeshEdgeGeometry>())
        + 5 * index_bytes;
    // Four copied row indices and 64 fixed producer/projection steps bound
    // class setup. Each long column visits one choice, two rows, its empty
    // pair producer, and both keys' Option entries: six visits per column.
    let setup = 7 * node_bytes + 3 * class_key_bytes + 4 * index_bytes + 64;
    let slot_bytes = cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
        Vec<Option<[usize; 2]>>,
        usize,
    )>());
    // Each signature grows through capacities 4, 8, 16, 32, 64, 128,
    // then 256. Growth moves the old capacity in bytes before allocation.
    let relocated_slots = 4 + 8 + 16 + 32 + 64 + 128;
    let relocation = 2
        * relocated_slots
        * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<Option<[usize; 2]>>());
    // The common cap admits setup, long-column visits and relocation plus
    // the short two-key sort, but not the long keys' comparison bound.
    let cap = setup + 6 * 128 + relocation + 6 + (2 * slot_bytes + 4 * (1 + index_bytes)) * 3 * 8;
    let result = crate::test_support::with_work_limit(cap, |ctx| {
        relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
    });
    assert!(
        matches!(&result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_ordered_rows_sort"),
        "{result:?}"
    );
    let short = (vec![None; 2], Vec::new());
    assert_eq!(
        crate::test_support::with_work_limit(cap, |ctx| {
            relation_row_gauge_mapping(ctx, &short, gauge, &[0, 1])
        })
        .expect("short signatures fit the same finite limit"),
        Some(vec![0, 1])
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
        })
        .expect("service work allowance"),
        Some(vec![0, 1])
    );
}

#[test]
fn relation_rows_without_pair_columns_skip_signature_slots() {
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
    // Two identity rows, four normalized pairs, six grouped key/member
    // entries, five class-copy entries, and four signature entries.
    const BASE_ITEMS: u64 = 2 + 4 + 6 + 5 + 4;
    for choices in [0, 128] {
        let state = (
            vec![None; 2],
            vec![vec![MeshEndpointRelationSelection::Deferred; choices]],
        );
        let items = BASE_ITEMS + 2 * cadmpeg_core::decode::u64_from_index(choices);
        assert_eq!(
            crate::test_support::with_collection_limit(items, |ctx| {
                relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
            })
            .expect("only row signatures need collection slots"),
            Some(vec![0, 1])
        );
    }
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
        .map(|(gauge, _storage)| gauge)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_gauge_permutation_limit"));
}

#[test]
fn coordinate_permutation_search_refuses_caller_work_before_enumeration() {
    let result = crate::test_support::with_work_refusal("catia_gauge_permutation_search", |ctx| {
        super::enumerate_coordinate_permutations(ctx, &[0, 1], 2)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_permutation_search"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::enumerate_coordinate_permutations(ctx, &[0, 1], 2)
        })
        .expect("two permutations fit service work"),
        vec![vec![0, 1], vec![1, 0]],
    );
    let output = crate::test_support::with_service_context(|ctx| {
        super::enumerate_coordinate_permutations(ctx, &[0, 2, 5], 6)
    })
    .expect("six permutations fit service work");
    assert_eq!(
        output,
        vec![
            vec![0, 2, 5],
            vec![0, 5, 2],
            vec![2, 0, 5],
            vec![2, 5, 0],
            vec![5, 0, 2],
            vec![5, 2, 0],
        ]
    );
}

#[test]
fn gauge_signature_lookup_refuses_repeated_long_equal_keys() {
    let signatures = vec![vec![0usize; 128]; 2];
    let result = crate::test_support::with_work_refusal("catia_gauge_signature_keys", |ctx| {
        super::intern_gauge_signatures(ctx, signatures.clone())
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_signature_keys"));
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            super::intern_gauge_signatures(ctx, signatures)
        })
        .expect("service comparison work"),
        (vec![0, 0], 1)
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
        .map(|(gauge, _storage)| gauge)
    });
    for operation in [
        "catia_gauge_group_point_scan",
        "catia_gauge_affected_edge_scan",
        "catia_gauge_original_rows",
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
    let result = crate::test_support::with_work_refusal("catia_relation_mapped_pair_scan", |ctx| {
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
            .map(|(gauge, _storage)| gauge)
        });
        for operation in [
            "catia_gauge_refinement_rounds",
            "catia_gauge_automorphism_rows_compare",
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
    let result =
        crate::test_support::with_work_refusal("catia_gauge_partial_pair_compare", |ctx| {
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
        let storage = ctx.reserve_scoped(0, "test topology storage")?;
        super::canonicalize_mesh_coordinate_gauges(ctx, topology.clone(), storage, gauge)
            .map(|value| value.map(|(topology, _storage)| topology))
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
fn gauge_factorial_refuses_only_after_the_visited_factors() {
    let error =
        crate::test_support::with_work_limit(2, |ctx| super::bounded_factorial(ctx, 10_000, 3))
            .expect_err("factors two and three exceed the permutation cap");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "catia_gauge_permutation_limit"
            && limit.used == 3 && limit.additional == 3));
}

#[test]
fn empty_coordinate_gauge_does_no_refinement_work() {
    crate::test_support::with_work_limit(0, |ctx| {
        let (gauge, _storage) = super::build_mesh_coordinate_gauge(ctx, 0, &[], &[], &[], &[], &[])
            .expect("empty gauge has no visited input");
        assert!(gauge.components.is_empty());
    });
}

#[test]
fn coordinate_gauge_storage_is_scoped_and_released_between_calls() {
    let rows = [
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("nonempty row"),
    ];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let (gauge, _storage) = super::build_mesh_coordinate_gauge(
            ctx,
            2,
            &rows,
            &[[0, 1]],
            &[MeshEdgeGeometry::Line],
            &[vec![[0, 1]]],
            &[false],
        )?;
        assert_eq!(gauge.components, vec![vec![vec![0, 1], vec![1, 0]]]);
        Ok::<_, CodecError>(())
    };
    crate::test_support::with_retained_limit(0, run).expect("gauge is solver scratch");
    crate::test_support::with_materialized_limit(32_768, |ctx| {
        for _ in 0..128 {
            run(ctx).expect("only one gauge and its scratch are live");
        }
    });
}

#[test]
fn gauge_equivalence_keeps_no_retained_drafts() {
    let topology = comparison_topology();
    let candidate = (topology, vec![0, 1]);
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(
            super::mesh_candidates_equivalent_with_context(ctx, &candidate, &candidate, None)
                .expect("boolean comparison uses scoped drafts")
        );
    });
}

#[test]
fn gauge_coedge_queries_stop_before_unvisited_faces() {
    let topology = comparison_topology();
    crate::test_support::with_work_limit(3, |ctx| {
        let mut visits = 0;
        assert!(
            !super::visit_coedges(ctx, &topology, "test coedge visit", |_, _, _, _| {
                visits += 1;
                Ok(false)
            })
            .expect("one face, one boundary, one coedge")
        );
        assert_eq!(visits, 1);
    });
    crate::test_support::with_work_limit(3, |ctx| {
        let mut topology = topology.clone();
        assert!(
            !super::rewrite_coedges(ctx, &mut topology, "test coedge rewrite", |_| false)
                .expect("one face, one boundary, one coedge")
        );
    });
}

#[test]
fn least_rotation_refuses_an_overflowing_bound_before_key_visits() {
    crate::test_support::with_service_context(|ctx| {
        let visited = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(limit) = super::least_rotation(
            ctx,
            usize::MAX,
            |_| {
                visited.set(true);
                0_usize
            },
            "test least rotation bound",
        )
        .expect_err("three times the cycle length exceeds the index range") else {
            panic!("resource refusal");
        };
        assert_eq!(
            limit.dimension,
            ResourceDimension::Codec("test least rotation bound")
        );
        assert_eq!(limit.operation, "test least rotation bound");
        assert_eq!(
            limit.used,
            cadmpeg_core::decode::u64_from_index(usize::MAX / 3)
        );
        assert_eq!(
            limit.additional,
            cadmpeg_core::decode::u64_from_index(usize::MAX - usize::MAX / 3)
        );
        assert!(!visited.get());
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn mesh_gauge_fixed_pair_normalization_uses_only_collection_steps() {
    // Two producer values and the terminal step; allocation uses its own budget.
    const WORK: u64 = 3;
    crate::test_support::with_work_limit(WORK, |ctx| {
        assert_eq!(
            super::normalized_endpoint_options(ctx, &[[3, 2], [1, 0]])
                .expect("fixed-key comparison and swap"),
            vec![[0, 1], [2, 3]]
        );
    });
    crate::test_support::with_work_limit(WORK, |ctx| {
        assert_eq!(
            super::normalized_endpoint_options(ctx, &[[3, 2], [2, 3]])
                .expect("fixed-key comparison and duplicate removal"),
            vec![[2, 3]]
        );
    });
    crate::test_support::with_work_limit(WORK, |ctx| {
        assert_eq!(
            super::mapped_normalized_endpoint_options(ctx, &[[0, 1], [1, 0]], &[3, 2])
                .expect("fixed mapped pairs"),
            Some(vec![[2, 3]])
        );
    });
}
