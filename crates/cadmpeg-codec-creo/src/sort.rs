// SPDX-License-Identifier: Apache-2.0
//! Stable ordering with caller-admitted temporary storage.

use std::cmp::Ordering;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(crate) fn stable_sort_by_key<T, K: Ord>(
    ctx: &DecodeContext<'_>,
    values: &mut [T],
    mut key: impl FnMut(&T) -> K,
    operation: &'static str,
) -> Result<(), CodecError> {
    stable_sort_by(
        ctx,
        values,
        |left, right| key(left).cmp(&key(right)),
        operation,
    )
}

pub(crate) fn stable_sort_by<T>(
    ctx: &DecodeContext<'_>,
    values: &mut [T],
    mut compare: impl FnMut(&T, &T) -> Ordering,
    operation: &'static str,
) -> Result<(), CodecError> {
    if values.len() <= 20 {
        for index in 1..values.len() {
            let mut cursor = index;
            while cursor > 0 {
                ctx.charge_work(1, "creo stable sort work")?;
                if compare(&values[cursor - 1], &values[cursor]) != Ordering::Greater {
                    break;
                }
                values.swap(cursor - 1, cursor);
                cursor -= 1;
            }
        }
        return Ok(());
    }
    let count = values.len();
    let bytes = count
        .checked_mul(2 * std::mem::size_of::<usize>())
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let _reservation = ctx.reserve_scoped(u64_from_index(bytes), operation)?;
    let mut order = ctx.alloc_filled(count, 0usize, operation)?;
    for (index, slot) in order.iter_mut().enumerate() {
        *slot = index;
    }
    let mut destinations = ctx.alloc_filled(count, 0usize, operation)?;
    let levels = u64::from(usize::BITS - count.leading_zeros());
    let work = u64_from_index(count)
        .checked_mul(levels)
        .ok_or_else(|| ctx.refuse_codec_limit("creo stable sort work", u64::MAX, u64::MAX))?;
    ctx.charge_work(work, "creo stable sort work")?;
    order.sort_unstable_by(|left, right| {
        compare(&values[*left], &values[*right]).then(left.cmp(right))
    });
    for (destination, source) in order.into_iter().enumerate() {
        destinations[source] = destination;
    }
    for index in 0..count {
        while destinations[index] != index {
            let next = destinations[index];
            values.swap(index, next);
            destinations.swap(index, next);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{stable_sort_by, stable_sort_by_key};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    #[test]
    fn admitted_stable_sort_preserves_equal_key_order_and_permutations() {
        for count in 0..128 {
            let original: Vec<_> = (0..count)
                .map(|index| ((index * 37 + 11) % 7, index))
                .collect();
            let mut expected = original.clone();
            expected.sort_by_key(|row| row.0);
            let mut actual = original.clone();
            crate::decode::with_test_decode_ctx(|ctx| {
                stable_sort_by_key(ctx, &mut actual, |row| row.0, "sort test")
            })
            .expect("service ordering admitted");
            assert_eq!(actual, expected, "count {count}");
            let mut descending = original;
            crate::decode::with_test_decode_ctx(|ctx| {
                stable_sort_by(
                    ctx,
                    &mut descending,
                    |left, right| right.0.cmp(&left.0),
                    "sort test",
                )
            })
            .expect("service comparison ordering admitted");
            expected.sort_by_key(|right| std::cmp::Reverse(right.0));
            assert_eq!(descending, expected, "descending count {count}");
        }
    }

    #[test]
    fn admitted_stable_sort_refuses_each_scratch_boundary_at_need_minus_one() {
        let bytes = u64::try_from(21 * 2 * std::mem::size_of::<usize>()).expect("scratch bytes");
        for (dimension, limit) in [
            (ResourceDimension::MaterializedBytes, bytes - 1),
            (ResourceDimension::CollectionItems, 20),
            (ResourceDimension::CollectionItems, 41),
            (ResourceDimension::WorkUnits, 104),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = limit;
                }
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
                _ => panic!("test dimension"),
            }
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let mut values: Vec<_> = (0..21).rev().collect();
            let before = values.clone();
            let error = stable_sort_by_key(&ctx, &mut values, |value| *value, "sort scratch")
                .expect_err("need minus one refuses");
            let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
                panic!("sort resource refusal");
            };
            assert_eq!(resource.dimension, dimension);
            assert_eq!(resource.limit, limit);
            assert_eq!(resource.used + resource.additional, limit + 1);
            assert_eq!(
                resource.operation,
                if dimension == ResourceDimension::WorkUnits {
                    "creo stable sort work"
                } else {
                    "sort scratch"
                }
            );
            assert_eq!(values, before, "refusal precedes sorting");
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_collection_items = 42;
        policy.limits.max_work_units = 105;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut values: Vec<_> = (0..21).rev().collect();
        stable_sort_by_key(&ctx, &mut values, |value| *value, "sort scratch")
            .expect("exact need admits");
        assert_eq!(values, (0..21).collect::<Vec<_>>());
    }

    #[test]
    fn admitted_stable_sort_refuses_small_slice_comparison_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut values = [2, 1];
        let error = stable_sort_by_key(&ctx, &mut values, |value| *value, "sort scratch")
            .expect_err("one comparison refused");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::WorkUnits && resource.operation == "creo stable sort work"
                && resource.used == 0 && resource.additional == 1)
        );
        assert_eq!(values, [2, 1]);
        crate::decode::with_test_decode_ctx(|ctx| {
            stable_sort_by_key(ctx, &mut values, |value| *value, "sort scratch")
        })
        .expect("service comparison admitted");
        assert_eq!(values, [1, 2]);
    }
}
