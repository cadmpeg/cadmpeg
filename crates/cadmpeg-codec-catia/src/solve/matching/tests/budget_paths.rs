//! Matching admission at paths with no remaining search.

use super::super::{
    distinct_domain_matching_with_budget, repair_distinct_domain_matching_with_budget,
    retain_distinct_matching_supports, MatchingEdgeConstraint,
};

#[test]
fn completed_matching_paths_skip_unused_search_arrays() {
    // Required matching uses one domain, mate and result slot. A repair
    // seed also uses one owner slot to prove that no point is reused.
    crate::test_support::with_collection_limit(4, |ctx| {
        assert_eq!(
            distinct_domain_matching_with_budget(
                ctx,
                [&[0][..]],
                1,
                None,
                Some(MatchingEdgeConstraint::Require(0, 0))
            )
            .expect("required edge completes matching"),
            Some(vec![0])
        );
    });
    crate::test_support::with_collection_limit(3, |ctx| {
        assert_eq!(
            distinct_domain_matching_with_budget(
                ctx,
                [&[0][..]],
                128,
                None,
                Some(MatchingEdgeConstraint::Require(0, 0))
            )
            .expect("completed requirement needs no point-owner table"),
            Some(vec![0])
        );
    });
    crate::test_support::with_collection_limit(4, |ctx| {
        assert_eq!(
            repair_distinct_domain_matching_with_budget(ctx, [&[0][..]], 1, &[0], None)
                .expect("valid seed needs no repair search"),
            Some(vec![0])
        );
    });
}

#[test]
fn invalid_required_edges_skip_matching_arrays() {
    // The sole admitted collection item is the borrowed domain reference.
    for constraint in [
        MatchingEdgeConstraint::Require(0, 128),
        MatchingEdgeConstraint::Require(0, 0),
    ] {
        crate::test_support::with_collection_limit(1, |ctx| {
            assert_eq!(
                distinct_domain_matching_with_budget(ctx, [&[1][..]], 128, None, Some(constraint))
                    .expect("invalid requirement needs no matching arrays"),
                None
            );
        });
    }
}

#[test]
fn matching_support_local_slice_counts_actual_edges() {
    crate::test_support::with_service_context(|ctx| {
        let budget = ctx.work_budget(3);
        let mut domains = [vec![0]];
        assert_eq!(
            retain_distinct_matching_supports(ctx, &mut domains, 1, &[0], Some(&budget))
                .expect("one graph edge, one forward edge, one reverse edge"),
            Some(false)
        );
        assert_eq!(budget.remaining(), 0);
        assert_eq!(domains, [vec![0]]);
        assert!(ctx.resource_refusal().is_none());
    });
}
