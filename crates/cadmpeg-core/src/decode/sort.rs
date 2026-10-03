// SPDX-License-Identifier: Apache-2.0
//! Work admission and borrowed or copied keys for sorting.

use std::cmp::Ordering;
use std::marker::PhantomData;

use super::cost::DecodeCost;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

pub(super) trait SortProjection<T> {
    type Key<'value>: Copy + DecodeCost where Self: 'value, T: 'value;
    fn project<'value>(&'value self, value: &'value T) -> Self::Key<'value>;
}

pub(super) struct BorrowedKey<K: ?Sized, F> {
    pub(super) key: F,
    pub(super) marker: PhantomData<*const K>,
}

impl<T, K: DecodeCost + ?Sized, F: Fn(&T) -> &K> SortProjection<T> for BorrowedKey<K, F> {
    type Key<'value> = &'value K where Self: 'value, T: 'value;
    fn project<'value>(&'value self, value: &'value T) -> Self::Key<'value> {
        (self.key)(value)
    }
}

pub(super) struct CopiedKey<K, F> {
    pub(super) key: F,
    pub(super) marker: PhantomData<K>,
}

impl<T, K: Copy + DecodeCost, F: Fn(&T) -> K> SortProjection<T> for CopiedKey<K, F> {
    type Key<'value> = K where Self: 'value, T: 'value;
    fn project<'value>(&'value self, value: &'value T) -> Self::Key<'value> {
        (self.key)(value)
    }
}

impl DecodeContext<'_> {
    pub(super) fn admit_sort<T, P: SortProjection<T>>(
        &self,
        values: &[T],
        projection: &P,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let count = u64_from_index(values.len());
        self.charge_work(count, operation)?;
        let mut maximum_key_bytes = 0_u64;
        for value in self.admit_iter(values, operation)? {
            maximum_key_bytes = maximum_key_bytes.max(projection.project(value).decode_cost(self, operation)?);
        }
        let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
        let key_bytes = self.cost_product(count, maximum_key_bytes, operation)?;
        let work = count
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .and_then(|storage| storage.checked_add(key_bytes.checked_mul(2)?))
            .and_then(|work| work.checked_mul(levels))
            .and_then(|work| work.checked_mul(8))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.charge_work(work, operation)
    }

    /// Sorts borrowed keys in place without allocating scratch.
    /// Each value must project the same key and comparison cost throughout the sort.
    pub fn sort_unstable_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> &K,
        mut compare: impl FnMut(&K, &K) -> Ordering,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let projection = BorrowedKey { key, marker: PhantomData };
        self.sort_unstable_projected(values, &projection, |left, right| compare((projection.key)(left), (projection.key)(right)), operation)
    }

    /// Sorts copied keys, including scalar getter results and borrowed source identities.
    /// Each value must project the same key and comparison cost throughout the sort.
    pub fn sort_unstable_by_key<T, K: Copy + DecodeCost>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> K,
        mut compare: impl FnMut(&K, &K) -> Ordering,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let projection = CopiedKey { key, marker: PhantomData };
        self.sort_unstable_projected(values, &projection, |left, right| compare(&(projection.key)(left), &(projection.key)(right)), operation)
    }
    fn sort_unstable_projected<T, P: SortProjection<T>>(&self, values: &mut [T], projection: &P, compare: impl FnMut(&T, &T) -> Ordering, operation: &'static str) -> Result<(), CodecError> {
        self.admit_sort(values, projection, operation)?;
        values.sort_unstable_by(compare);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeContext;
    use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn charged_unstable_sort_work_refusal_preserves_input() {
        let mut values = [3u64, 1, 2];
        let expected = values;
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Six measuring visits plus three levels of moved bytes and both eight-byte keys per comparison.
        policy.limits.max_work_units = 6 + (3 * 8 + 2 * 3 * 8) * 3 * 8 - 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        assert!(matches!(
            ctx.sort_unstable_by(&mut values,
            |value| value,
            Ord::cmp, "test unstable sort"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits
        ));
        assert_eq!(values, expected);
    }

    #[test]
    fn charged_unstable_sort_orders_values_without_scratch() {
        let mut values = [3u64, 1, 2, 1];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
        ctx.sort_unstable_by(&mut values,
            |value| value,
            Ord::cmp, "test unstable sort")
            .expect("sort is admitted");
        assert_eq!(values, [1, 1, 2, 3]);
    }

    #[test]
    fn variable_sort_keys_admit_the_largest_operand_for_every_comparison() {
        let arena = DecodeArena::new();
        // Six measuring visits plus three levels of slot moves and two maximum keys per comparison.
        let total = 6 + (3 * u64::try_from(std::mem::size_of::<&str>()).unwrap() + 2 * 3 * 8) * 3 * 8;
        for stable in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = total - 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut values = ["bbbbbbbb", "a", "c"];
            let comparisons = std::cell::Cell::new(0);
            let compare = |left: &&str, right: &&str| {
                comparisons.set(comparisons.get() + 1);
                left.cmp(right)
            };
            let result = if stable {
                ctx.stable_sort_by(&mut values, |value| value, compare, "variable keys")
            } else {
                ctx.sort_unstable_by(&mut values, |value| value, compare, "variable keys")
            };
            let CodecError::ResourceLimit(first) = result.unwrap_err() else { panic!("resource refusal") };
            assert_eq!(first.used, 6);
            assert_eq!(first.additional, total - 6);
            assert_eq!(comparisons.get(), 0);
            assert_eq!(values, ["bbbbbbbb", "a", "c"]);
            assert_eq!(ctx.resource_refusal(), Some(first));
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = total;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut values = ["bbbbbbbb", "a", "c"];
        ctx.sort_unstable_by(&mut values, |value| value, Ord::cmp, "variable keys").unwrap();
        assert_eq!(values, ["a", "bbbbbbbb", "c"]);
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").unwrap_err() else { panic!("resource refusal") };
        assert_eq!(limit.used, total);
    }

    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    struct OverflowKey;
    impl crate::decode::cost::DecodeCost for OverflowKey {
        fn decode_cost(&self, _ctx: &DecodeContext<'_>, _operation: &'static str) -> Result<u64, CodecError> {
            Ok(u64::MAX)
        }
    }

    #[test]
    fn charged_unstable_sort_key_byte_overflow_preserves_input() {
        let mut values = [2u8, 1];
        let expected = values;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("empty root is admitted");
        assert!(matches!(
            ctx.sort_unstable_by(&mut values,
            |_| &OverflowKey,
            Ord::cmp, "test unstable sort"),
            Err(CodecError::ResourceLimit(_))
        ));
        assert_eq!(values, expected);
    }
}
