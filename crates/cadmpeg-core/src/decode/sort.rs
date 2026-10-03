// SPDX-License-Identifier: Apache-2.0
//! Work admission and borrowed or copied keys for sorting.

use std::cmp::Ordering;
use std::marker::PhantomData;

use super::cost::DecodeCost;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

pub(super) trait SortProjection<T> {
    type Key<'value>: DecodeCost where Self: 'value, T: 'value;
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
        let mut bytes = 0_u64;
        for value in self.admit_iter(values, operation)? {
            bytes = self.cost_sum(bytes, projection.project(value).decode_cost(self, operation)?, operation)?;
        }
        let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
        let work = count
            .checked_mul(u64_from_index(std::mem::size_of::<T>()))
            .and_then(|storage| storage.checked_add(bytes.checked_mul(2)?))
            .and_then(|work| work.checked_mul(levels))
            .and_then(|work| work.checked_mul(8))
            .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        self.charge_work(work, operation)
    }

    /// Sorts borrowed keys in place without allocating scratch.
    pub fn sort_unstable_by<T, K: DecodeCost + ?Sized>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> &K,
        mut compare: impl FnMut(&K, &K) -> Ordering,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let projection = BorrowedKey { key, marker: PhantomData };
        self.admit_sort(values, &projection, operation)?;
        Self::sort_unstable_admitted(values, |left, right| compare((projection.key)(left), (projection.key)(right)));
        Ok(())
    }

    /// Sorts copied keys, including scalar getter results and borrowed source identities.
    pub fn sort_unstable_by_key<T, K: Copy + DecodeCost>(
        &self,
        values: &mut [T],
        key: impl Fn(&T) -> K,
        mut compare: impl FnMut(&K, &K) -> Ordering,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let projection = CopiedKey { key, marker: PhantomData };
        self.admit_sort(values, &projection, operation)?;
        Self::sort_unstable_admitted(values, |left, right| compare(&(projection.key)(left), &(projection.key)(right)));
        Ok(())
    }
    fn sort_unstable_admitted<T>(values: &mut [T], compare: impl FnMut(&T, &T) -> Ordering) {
        values.sort_unstable_by(compare);
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
