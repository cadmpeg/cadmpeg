// SPDX-License-Identifier: Apache-2.0
use super::{convergent_face_support, support};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn convergent_work_refusal(operation: &'static str) {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| convergent_face_support(ctx, &[10, 11], &contexts),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
}

#[test]
fn convergent_context_source_refuses_work_limit() {
    convergent_work_refusal("f3d convergent context source");
}

#[test]
fn convergent_support_source_refuses_work_limit() {
    convergent_work_refusal("f3d convergent support source");
}

#[test]
fn convergent_candidate_source_refuses_work_limit() {
    convergent_work_refusal("f3d convergent candidate source");
}

#[test]
fn convergent_support_equality_refuses_work_limit() {
    convergent_work_refusal("f3d convergent support equality");
}

#[test]
fn convergent_active_coverage_refuses_work_limit() {
    convergent_work_refusal("f3d convergent active coverage");
}

#[test]
fn convergent_accepted_support_refuses_retained_limit() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        "f3d convergent accepted support faces",
        0,
        |ctx| convergent_face_support(ctx, &[10, 11], &contexts),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "f3d convergent accepted support faces"));
}

#[test]
fn convergent_support_conflict_copies_no_output_and_releases_scratch() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            convergent_face_support(ctx, &[10, 11], &contexts).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test convergent scratch release",
            )
            .unwrap(),
        );
    });
}

#[test]
fn convergent_incomplete_coverage_copies_no_output_and_releases_scratch() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            convergent_face_support(ctx, &[10, 11, 12], &contexts).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test convergent coverage scratch release",
            )
            .unwrap(),
        );
    });
}

#[test]
fn convergent_context_conflict_stops_before_original_tail() {
    let mut contexts = vec![support(10, &[(100, 4)]), support(11, &[(101, 4)])];
    contexts.extend((0..10_000).map(|_| support(12, &[(100, 4)])));
    let mut policy = DecodePolicy::service();
    // Two one-slot support vectors need fewer than 1024 units including equality and sort.
    policy.limits.max_work_units = 1024;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            convergent_face_support(ctx, &[10, 11], &contexts).unwrap(),
            None
        );
    });
}

#[test]
fn convergent_empty_active_faces_do_no_source_work() {
    let contexts = [support(10, &[(100, 4)])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(convergent_face_support(ctx, &[], &contexts).unwrap(), None);
    });
}

#[test]
fn convergent_support_preserves_duplicate_and_sorted_set_rules() {
    let mut contexts = [
        support(11, &[(101, 4), (100, 4), (101, 4)]),
        support(10, &[(100, 4), (101, 4)]),
    ];
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            convergent_face_support(ctx, &[10, 11], &contexts).unwrap(),
            Some(vec![100, 101])
        );
    });
    contexts[1].active_face_slot = 11;
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(
            convergent_face_support(ctx, &[11], &contexts).unwrap(),
            Some(vec![100, 101])
        );
    });
}

#[test]
fn effective_face_queries_refuse_each_original_work_operation() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    let candidates = [super::face(10), super::face(11)];
    for operation in [
        "f3d effective candidate source",
        "f3d stable face slot suffix",
        "f3d effective active source",
        "f3d effective active containment",
        "f3d effective candidate membership",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| super::effective_historical_face_slots(ctx, &candidates, &contexts).map(|_| ()),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}

#[test]
fn effective_face_mismatch_copies_no_output_and_releases_scratch() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(100, 4)])];
    let candidates = [super::face(10)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::effective_historical_face_slots(ctx, &candidates, &contexts)
                .unwrap()
                .map(|faces| faces.0),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test effective scratch release",
            )
            .unwrap(),
        );
    });
}

#[test]
fn effective_long_slot_refuses_numeric_parse_work() {
    let face =
        cadmpeg_ir::ids::FaceId::mint(format!("f3d:brep:entity#{}", "0".repeat(1024))).unwrap();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d stable face slot value",
        0,
        |ctx| {
            super::effective_historical_face_slots(ctx, std::slice::from_ref(&face), &[])
                .map(|_| ())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "f3d stable face slot value"));
}

#[test]
fn stable_face_sources_and_index_membership_refuse_work() {
    let contexts = [support(10, &[(10, 4)]), support(11, &[(11, 4)])];
    for operation in [
        "f3d stable bounded active source",
        "f3d stable bounded context source",
        "f3d stable bounded active membership",
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| super::stable_face_support_set(ctx, &[10, 11], &contexts),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation));
    }
}

#[test]
fn stable_face_conflict_copies_no_output_and_releases_indexes() {
    let contexts = [support(10, &[(10, 4)]), support(11, &[(12, 4)])];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            super::stable_face_support_set(ctx, &[10, 11], &contexts).unwrap(),
            None
        );
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test stable scratch release",
            )
            .unwrap(),
        );
    });
}

#[test]
fn effective_faces_remain_scoped_through_convergence_rejection() {
    let contexts = [support(10, &[(100, 4)]), support(11, &[(101, 4)])];
    let candidates = [super::face(10), super::face(11)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let faces = super::effective_historical_face_slots(ctx, &candidates, &contexts)
            .unwrap()
            .unwrap();
        assert_eq!(faces.0, vec![10, 11]);
        assert_eq!(
            convergent_face_support(ctx, &faces.0, &contexts).unwrap(),
            None
        );
        drop(faces);
        drop(
            ctx.reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test effective convergence release",
            )
            .unwrap(),
        );
    });
}

#[test]
fn convergent_coverage_length_mismatch_skips_active_value_comparisons() {
    let contexts = [support(10, &[(100, 4)])];
    let active: Vec<i64> = (0..10_000).collect();
    let mut policy = DecodePolicy::service();
    // One context and its one-slot vectors fit. Active values need no visits on a length mismatch.
    policy.limits.max_work_units = 1024;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            convergent_face_support(ctx, &active, &contexts).unwrap(),
            None
        );
    });
}
