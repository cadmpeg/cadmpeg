// SPDX-License-Identifier: Apache-2.0

use super::DecodeContext;
use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn already_ordered_values_need_linear_work_and_no_sort_scratch() {
    let mut values = (0..100).map(|key| (key, [0u8; 1_024])).collect::<Vec<_>>();
    for stable in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 400_000;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root admission");
        if stable {
            ctx.stable_sort_by(
                &mut values,
                |left, right| left.0.cmp(&right.0),
                |_| 0,
                "ordered sort",
            )
        } else {
            ctx.sort_unstable_by(
                &mut values,
                |left, right| left.0.cmp(&right.0),
                |_| 0,
                "ordered sort",
            )
        }
        .expect("ordered arena needs only linear work");
        assert!(values
            .iter()
            .enumerate()
            .all(|(index, value)| index == value.0));
        assert!(ctx.finish_session().is_ok());
    }
}

#[test]
fn charged_stable_sort_preserves_duplicate_order_and_permutation_cycles() {
    for count in 0..96 {
        let mut values: Vec<_> = (0..count)
            .map(|index| ((count - index) % 7, index))
            .collect();
        let mut expected = values.clone();
        expected.sort_by_key(|value| value.0);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("admitted test operation");
        ctx.stable_sort_by(
            &mut values,
            |left, right| left.0.cmp(&right.0),
            |_| 0,
            "test stable sort",
        )
        .expect("admitted test operation");
        assert_eq!(values, expected);
    }
}

#[test]
fn charged_stable_sort_scoped_refusal_preserves_input() {
    let mut values: Vec<_> = (0..32).rev().collect();
    let expected = values.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes =
        2 * 32 * u64::try_from(std::mem::size_of::<usize>()).expect("admitted test operation") - 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert!(
        matches!(ctx.stable_sort_by(&mut values, Ord::cmp, |_| 0, "test stable sort"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert_eq!(values, expected);
    policy.limits.max_materialized_bytes += 1;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    ctx.stable_sort_by(&mut values, Ord::cmp, |_| 0, "test stable sort")
        .expect("admitted test operation");
    assert_eq!(values, (0..32).collect::<Vec<_>>());
}

#[test]
fn charged_stable_sort_work_refusal_preserves_input() {
    let mut values = vec![("long compared key", 1), ("different key", 2)];
    let expected = values.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert!(
        matches!(ctx.stable_sort_by(&mut values, |left, right| left.0.cmp(right.0), |value| value.0.len(), "test stable sort"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
    );
    assert_eq!(values, expected);
}

#[test]
fn small_sort_work_admission_is_independent_of_input_order() {
    let need = 2 + 2 * 8 * 3 * 8;
    for stable in [false, true] {
        for input in [[1u64, 2], [2u64, 1]] {
            for admitted in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = 0;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_work_units = if admitted { need } else { need - 1 };
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let mut values = input;
                let result = if stable {
                    ctx.stable_sort_by(&mut values, Ord::cmp, |_| 0, "small sort")
                } else {
                    ctx.sort_unstable_by(&mut values, Ord::cmp, |_| 0, "small sort")
                };
                if admitted {
                    result.expect("bounded work admits either input order without scratch");
                    assert_eq!(values, [1, 2]);
                } else {
                    let CodecError::ResourceLimit(limit) =
                        result.expect_err("one work unit below the bound refuses either order")
                    else {
                        panic!("work refusal")
                    };
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(ctx.resource_refusal(), Some(limit));
                    assert_eq!(values, input);
                }
            }
        }
    }
}
