// SPDX-License-Identifier: Apache-2.0

use crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext;
use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};

fn probe_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_collection_items = u64::MAX;
    policy.limits.max_materialized_bytes = u64::MAX;
    policy
}

fn selector(edges: Vec<i64>) -> DesignEdgeRecipeSelectorContext {
    DesignEdgeRecipeSelectorContext {
        selector: 0,
        clauses: Vec::new(),
        incidence_matching_edge_slots: edges,
        boundary_count_matching_edge_slots: Vec::new(),
    }
}

#[test]
fn edge_recipe_reference_intersection_admits_candidates_and_membership() {
    for operation in ["scan F3D reference intersection candidates", "scan F3D reference intersection sets", "find F3D reference intersection edge"] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(super::super::super::resolved_edge_candidate_intersection(ctx, &[], [&[17, 18][..], &[17, 19][..]]).unwrap(), Some(17));
        });
        let error = crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0, |ctx| super::super::super::resolved_edge_candidate_intersection(ctx, &[], [&[17, 18][..], &[17, 19][..]]));
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn edge_recipe_selector_intersection_admits_membership() {
    for operation in ["scan F3D selector intersection candidates", "scan F3D corroborating selectors", "find F3D corroborating selector edge", "find F3D shared selector edge"] {
        let selectors = [selector(vec![17, 18]), selector(vec![17, 19])];
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(super::super::super::resolved_edge_candidate_intersection(ctx, &selectors, [&[17][..]]).unwrap(), Some(17));
        });
        let error = crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0, |ctx| super::super::super::resolved_edge_candidate_intersection(ctx, &selectors, [&[17][..]]));
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn edge_recipe_incidence_intersection_admits_set_equality_and_membership() {
    for operation in ["compare F3D reference edge sets", "find F3D incidence reference edge", "scan F3D incidence edge candidates"] {
        let selectors = [selector(vec![11])];
        let sets = [&[10, 11][..], &[11, 10, 10][..], &[11, 12][..]];
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(super::super::super::unique_incidence_edge_shared_by_reference_faces(ctx, &selectors, sets).unwrap(), Some(11));
        });
        let error = crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0, |ctx| super::super::super::unique_incidence_edge_shared_by_reference_faces(ctx, &selectors, sets));
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}



#[test]
fn edge_recipe_disjoint_intersection_charges_only_reached_searches() {
    crate::test_support::with_decode_context(|ctx| {
        assert!(matches!(super::super::super::unique_edge_set_intersection(&[17, 18], &std::iter::once(&[19][..]), ctx).unwrap(), super::super::super::EdgeSetIntersection::Disjoint));
    });
    let operation = "scan F3D reference intersection candidates";
    let error = crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 2, |ctx| super::super::super::unique_edge_set_intersection(&[17, 18], &std::iter::once(&[19][..]), ctx));
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
}

#[test]
fn edge_recipe_triplet_intersection_admits_overlap_and_corroboration() {
    use crate::records::topology::edge_recipe::{
        DesignEdgeRecipeSelectorClause, DesignTopologyIncident, DesignTopologyIncidentSide,
        DesignTopologyRecipeEntry, DesignTopologyRecipeTriplet,
    };
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(3).unwrap(),
        middle: 2,
        incident: Some(DesignTopologyIncident {
            ordinal: 1,
            side: DesignTopologyIncidentSide::Preceding,
        }),
    };
    let clause = DesignEdgeRecipeSelectorClause {
        entry: DesignTopologyRecipeEntry {
            selector: 0,
            boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
            topology_triplets: [triplet, triplet],
        },
        triplet_edge_slots: [vec![17, 18], vec![17]],
    };
    let mut common = selector(Vec::new());
    common.clauses = vec![Some(clause), None];
    let selectors = [common.clone(), common];
    for operation in [
        "scan F3D triplet selector clauses",
        "find F3D triplet pair overlap",
        "scan F3D triplet edge candidates",
        "find F3D triplet second edge",
        "find F3D corroborating triplet edge",
        "find F3D shared triplet edge",
    ] {
        crate::test_support::with_decode_policy(&probe_policy(), |ctx| {
            assert_eq!(
                super::super::super::resolved_edge_candidate_intersection(
                    ctx,
                    &selectors,
                    [&[17, 18][..]]
                )
                .unwrap(),
                Some(17)
            );

        });
        let error = crate::test_support::resource_refusal_at(ResourceDimension::WorkUnits, operation, 0, |ctx| super::super::super::resolved_edge_candidate_intersection(ctx, &selectors, [&[17, 18][..]]));
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn edge_recipe_rejected_assignment_candidates_release_scratch() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4 * u64::try_from(std::mem::size_of::<i64>()).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..2 {
            assert_eq!(
                super::super::super::edge_assignment_candidates(
                    &[selector(vec![17])],
                    [&[18][..]].into_iter().map(Some),
                    ctx
                )
                .unwrap(),
                None
            );
        }
        assert!(
            matches!(super::super::super::edge_assignment_candidates(&[selector(vec![17])], [&[17][..]].into_iter().map(Some), ctx), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    });
}


