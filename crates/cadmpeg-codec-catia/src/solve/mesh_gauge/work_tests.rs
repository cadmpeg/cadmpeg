use super::{map_endpoint_relation_state, relation_row_gauge_mapping, EdgeBoundaryLayout,
    EdgeRow, MeshCandidateGauge, MeshEdgeGeometry, MeshEndpointRelationSelection};
use cadmpeg_core::CodecError;
use cadmpeg_core::decode::ResourceDimension;

#[test]
fn relation_choice_sort_refuses_assignment_bytes() {
    let state = (vec![], vec![vec![MeshEndpointRelationSelection::Enumerated {
        assignments: vec![0; 128], edge_pairs: vec![],
    }; 2]]);
    let gauge = MeshCandidateGauge {
        edge_rows: &[], edge_faces: &[], edge_geometry: &[], edge_candidates: &[],
        edge_identity_evidence: &[], coordinate_gauge: None,
    };
    let result = crate::test_support::with_work_limit(20_000, |ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_mapped_choices_sort"));
    assert_eq!(crate::test_support::with_service_context(|ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[])
    }).expect("service work allowance"), Some(state));
}

#[test]
fn relation_row_sort_refuses_endpoint_signature_bytes() {
    let rows = std::array::from_fn::<_, 2, _>(|_| EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
        .expect("nonempty handles"));
    let faces = [[0, 1]; 2];
    let geometry = [MeshEdgeGeometry::Line; 2];
    let candidates = [vec![[0, 1]], vec![[0, 1]]];
    let evidence = [false; 2];
    let gauge = MeshCandidateGauge {
        edge_rows: &rows, edge_faces: &faces, edge_geometry: &geometry,
        edge_candidates: &candidates, edge_identity_evidence: &evidence, coordinate_gauge: None,
    };
    let state = (vec![None; 2], vec![vec![MeshEndpointRelationSelection::Deferred; 128]]);
    let result = crate::test_support::with_work_limit(30_000, |ctx| {
        relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_ordered_rows_sort"));
    assert_eq!(crate::test_support::with_service_context(|ctx| {
        relation_row_gauge_mapping(ctx, &state, gauge, &[0, 1])
    }).expect("service work allowance"), Some(vec![0, 1]));
}

#[test]
fn coordinate_gauge_refuses_unsearched_eight_point_class() {
    let rows = [EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
        .expect("nonempty handles")];
    let candidates = [(0..8).flat_map(|left| (left + 1..8).map(move |right| [left, right]))
        .collect::<Vec<_>>()];
    let result = crate::test_support::with_service_context(|ctx| {
        super::build_mesh_coordinate_gauge(ctx, 8, &rows, &[[0, 1]], &[MeshEdgeGeometry::Line],
            &candidates, &[false])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_gauge_permutation_limit"));
}

#[test]
fn coordinate_permutation_search_refuses_caller_work_before_enumeration() {
    let result = crate::test_support::with_work_limit(0, |ctx| {
        super::enumerate_coordinate_permutations(ctx, &[0, 1], 0, &mut vec![],
            &mut [false; 2], &mut vec![])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_permutation_search"));
    let mut output = vec![];
    crate::test_support::with_service_context(|ctx| {
        super::enumerate_coordinate_permutations(ctx, &[0, 1], 0, &mut vec![],
            &mut [false; 2], &mut output)
    }).expect("two permutations fit service work");
    assert_eq!(output, vec![vec![0, 1], vec![1, 0]]);
}

#[test]
fn gauge_signature_lookup_refuses_repeated_long_equal_keys() {
    let signatures = vec![vec![0usize; 128]; 2];
    let result = crate::test_support::with_work_limit(1_000, |ctx| {
        super::intern_gauge_signatures(ctx, signatures.clone(), |key| std::mem::size_of_val(key.as_slice()))
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_gauge_signature_compare"));
    assert_eq!(crate::test_support::with_service_context(|ctx| {
        super::intern_gauge_signatures(ctx, signatures, |key| std::mem::size_of_val(key.as_slice()))
    }).expect("service comparison work"), vec![0, 0]);
}

fn observed_work_refusals<T>(run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, CodecError>)
    -> std::collections::HashSet<&'static str> {
    let mut operations = std::collections::HashSet::new();
    let mut cap = 0;
    for _ in 0..1024 {
        match crate::test_support::with_work_limit(cap, &run) {
            Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits => {
                operations.insert(limit.operation);
                cap = limit.used.checked_add(limit.additional).expect("finite test work");
            }
            Ok(_) => return operations,
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    panic!("fixture did not complete its admitted work");
}

#[test]
fn coordinate_gauge_membership_scans_refuse_before_search() {
    let rows = [EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun)
        .expect("nonempty handles")];
    let operations = observed_work_refusals(|ctx| {
        super::build_mesh_coordinate_gauge(ctx, 2, &rows, &[[0, 1]], &[MeshEdgeGeometry::Line],
            &[vec![[0, 1]]], &[false])
    });
    for operation in ["catia_gauge_group_point_scan", "catia_gauge_affected_edge_scan", "catia_gauge_affected_point_scan"] {
        assert!(operations.contains(operation), "missing work refusal for {operation}");
    }
}

#[test]
fn mapped_pair_scan_refuses_before_duplicate_becomes_none() {
    let state = (vec![None], vec![vec![MeshEndpointRelationSelection::Enumerated {
        assignments: vec![], edge_pairs: vec![(0, [0, 1]), (0, [0, 1])],
    }]]);
    let gauge = MeshCandidateGauge {
        edge_rows: &[], edge_faces: &[], edge_geometry: &[], edge_candidates: &[],
        edge_identity_evidence: &[], coordinate_gauge: None,
    };
    let result = crate::test_support::with_work_limit(386, |ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[0, 1])
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "catia_relation_mapped_pair_scan"));
    assert!(crate::test_support::with_service_context(|ctx| {
        map_endpoint_relation_state(ctx, &state, gauge, &[0, 1])
    }).expect("service work").is_none());
}
