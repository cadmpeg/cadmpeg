// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

fn assignment_refusal(dimension: ResourceDimension, operation: &str) {
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        super::super::bipartite_assignment(&[vec![17]], None, ctx)
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn edge_assignment_visits_refuse_materialized_limit() {
    assignment_refusal(
        ResourceDimension::MaterializedBytes,
        "f3d edge assignment visits",
    );
}

#[test]
fn edge_assignment_members_refuse_materialized_limit() {
    assignment_refusal(
        ResourceDimension::MaterializedBytes,
        "f3d edge assignment members",
    );
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            super::super::bipartite_assignment(&[vec![17]], None, ctx).unwrap(),
            Some(vec![17])
        );
    });
}

#[test]
fn edge_assignment_search_refuses_recursion_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "f3d edge assignment search",
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            crate::test_support::with_decode_policy(&policy, |ctx| {
                super::super::bipartite_assignment(&[vec![17]], None, ctx)
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RecursionDepth && limit.operation == "f3d edge assignment search"));
}

#[test]
fn edge_assignment_candidate_refuses_work_limit() {
    assignment_refusal(
        ResourceDimension::WorkUnits,
        "f3d edge assignment candidate",
    );
}
