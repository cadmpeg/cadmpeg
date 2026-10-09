use super::{
    distinct_domain_matching_with_budget, repair_distinct_domain_matching_with_budget,
    retain_distinct_matching_supports, DecodeArena, DecodeContext, DecodePolicy,
    MatchingEdgeConstraint,
};
use cadmpeg_core::CodecError;

fn with_no_storage(run: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root needs no storage");
    run(&ctx);
}

#[test]
fn empty_distinct_matching_preserves_required_and_excluded_edges_without_point_storage() {
    with_no_storage(|ctx| {
        for point_count in [0, 4096, usize::MAX] {
            for constraint in [None, Some(MatchingEdgeConstraint::Exclude(usize::MAX, usize::MAX))] {
                assert_eq!(
                    distinct_domain_matching_with_budget(ctx, [], point_count, None, constraint)
                        .expect("empty matching needs no point arrays"),
                    Some(Vec::new())
                );
            }
            for constraint in [
                MatchingEdgeConstraint::Require(0, 0),
                MatchingEdgeConstraint::Require(usize::MAX, usize::MAX),
            ] {
                assert_eq!(
                    distinct_domain_matching_with_budget(ctx, [], point_count, None, Some(constraint))
                        .expect("required domain does not exist"),
                    None
                );
            }
        }
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn empty_matching_repair_preserves_mapping_shape_without_point_storage() {
    with_no_storage(|ctx| {
        for point_count in [0, 4096, usize::MAX] {
            assert_eq!(
                repair_distinct_domain_matching_with_budget(ctx, [], point_count, &[], None)
                    .expect("empty repair needs no point arrays"),
                Some(Vec::new())
            );
            assert_eq!(
                repair_distinct_domain_matching_with_budget(ctx, [], point_count, &[0], None)
                    .expect("extra mapping entry is invalid"),
                None
            );
        }
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn empty_matching_supports_preserve_mapping_validation_without_point_storage() {
    with_no_storage(|ctx| {
        for point_count in [0, 4096, usize::MAX] {
            assert_eq!(
                retain_distinct_matching_supports(ctx, &mut [], point_count, &[], None)
                    .expect("empty support graph needs no point arrays"),
                Some(false)
            );
            assert_eq!(
                retain_distinct_matching_supports(ctx, &mut [], point_count, &[0], None)
                    .expect("extra mapping entry is invalid"),
                None
            );
        }
        let mut domains = [Vec::new()];
        assert_eq!(
            retain_distinct_matching_supports(ctx, &mut domains, 1, &[1], None)
                .expect("invalid point is rejected before graph storage"),
            None
        );
        assert_eq!(domains, [Vec::<usize>::new()]);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn empty_matching_paths_preserve_the_original_caller_refusal() {
    crate::test_support::with_work_limit(0, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "test original refusal") else {
            panic!("original refusal")
        };
        let results = [
            distinct_domain_matching_with_budget(ctx, [], usize::MAX, None, None).map(|_| ()),
            repair_distinct_domain_matching_with_budget(ctx, [], usize::MAX, &[], None).map(|_| ()),
            retain_distinct_matching_supports(ctx, &mut [], usize::MAX, &[], None).map(|_| ()),
        ];
        for result in results {
            assert!(matches!(result, Err(CodecError::ResourceLimit(next)) if next == original));
        }
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn completed_matching_refuses_retention_at_the_actual_return_boundary() {
    crate::test_support::with_retained_limit(0, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            distinct_domain_matching_with_budget(ctx, [&[0][..]], 1, None, None)
        else {
            panic!("owned completed matching cannot be retained")
        };
        assert_eq!(original.operation, "catia_match_completed");
        assert_eq!(original.dimension, cadmpeg_core::decode::ResourceDimension::RetainedBytes);
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(matches!(
            distinct_domain_matching_with_budget(ctx, [], usize::MAX, None, None),
            Err(CodecError::ResourceLimit(next)) if next == original
        ));
    });
}
