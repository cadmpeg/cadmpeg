// SPDX-License-Identifier: Apache-2.0
use super::super::pcurve_endpoint_is_ambiguous;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

#[test]
fn vertex_ambiguity_visits_only_present_comparisons_until_first_disagreement() {
    let same = [1.0, 2.0, 3.0];
    let different = [2.0, 2.0, 3.0];
    let mut first = vec![same, different];
    first.extend(std::iter::repeat_n(same, 128));
    let mut second = vec![same, same, different];
    second.extend(std::iter::repeat_n(same, 128));
    for (points, visits, expected) in [
        (Vec::new(), 0, false),
        (vec![same], 0, false),
        (vec![same, same], 1, false),
        (vec![same, same, same], 2, false),
        (first, 1, true),
        (second, 2, true),
    ] {
        crate::test_support::assert_refusal_order(
            ResourceDimension::WorkUnits,
            &vec![
                "creo pcurve endpoint ambiguity search";
                usize::try_from(visits).expect("visit count")
            ],
            |cap| {
                let arena = DecodeArena::new();
                let policy = visit_policy(cap);
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let result = pcurve_endpoint_is_ambiguous(&ctx, &points);
                let original = if let Ok(actual) = result {
                    assert_eq!(cap, visits);
                    assert_eq!(actual, expected);
                    let original = ctx
                        .charge_work_limit(1, "after vertex ambiguity")
                        .expect_err("exact visits");
                    assert_eq!(
                        (original.dimension, original.used, original.additional),
                        (ResourceDimension::WorkUnits, visits, 1)
                    );
                    original
                } else {
                    let original = ctx.resource_refusal().expect("comparison refusal");
                    assert!(
                        matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                    assert_eq!(
                        (
                            original.dimension,
                            original.limit,
                            original.used,
                            original.additional,
                            original.operation
                        ),
                        (
                            ResourceDimension::WorkUnits,
                            cap,
                            cap,
                            1,
                            "creo pcurve endpoint ambiguity search"
                        )
                    );
                    original
                };
                if visits != 0 {
                    assert!(
                        matches!(pcurve_endpoint_is_ambiguous(&ctx, &points), Err(CodecError::ResourceLimit(actual)) if actual == original)
                    );
                }
                assert_eq!(ctx.resource_refusal(), Some(original));
                if cap == visits {
                    Ok(())
                } else {
                    Err(CodecError::ResourceLimit(original))
                }
            },
        );
    }
}
