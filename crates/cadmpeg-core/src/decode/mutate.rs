// SPDX-License-Identifier: Apache-2.0
//! Charged slice copies and stable in-place vector compaction.
use super::cost::DecodeCost;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet};

impl DecodeContext<'_> {
    /// Inserts one value after admitting the suffix shift and slot growth.
    /// An index past the end is malformed; an earlier refusal is returned first.
    pub fn insert_vec<T>(
        &self,
        values: &mut Vec<T>,
        index: usize,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(0, operation)?;
        if index > values.len() {
            return Err(CodecError::malformed(
                "vector insertion index exceeds length",
            ));
        }
        self.reserve_vec(values, 1, operation)?;
        self.admit_moves(&values[index..], 1, operation)?;
        values.insert(index, value);
        Ok(())
    }

    /// Converts vector storage to a boxed slice after admitting a possible shrink copy.
    /// The caller admits the vector's existing storage.
    pub fn into_boxed_slice<T>(
        &self,
        values: Vec<T>,
        operation: &'static str,
    ) -> Result<Box<[T]>, CodecError> {
        if std::mem::size_of::<T>() == 0 {
            self.reserve_scoped(0, operation)?;
            return Ok(values.into_boxed_slice());
        }
        if values.capacity() == values.len() {
            self.reserve_scoped(0, operation)?;
            return Ok(values.into_boxed_slice());
        }
        let bytes = self.cost_product(
            u64_from_index(values.len()),
            u64_from_index(std::mem::size_of::<T>()),
            operation,
        )?;
        let _storage = self.reserve_scoped(bytes, operation)?;
        self.admit_moves(&values, 1, operation)?;
        Ok(values.into_boxed_slice())
    }

    /// Admits slot visits and moves of the complete inline representation.
    pub(super) fn admit_moves<T>(
        &self,
        values: &[T],
        moves: u64,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let count = u64_from_index(values.len());
        let bytes =
            self.cost_product(count, u64_from_index(std::mem::size_of::<T>()), operation)?;
        let bytes = self.cost_product(bytes, moves, operation)?;
        self.charge_work(self.cost_sum(count, bytes, operation)?, operation)
    }

    /// Copies equal-length slices after admitting all inline source bytes.
    pub fn copy_into<T: Copy>(
        &self,
        target: &mut [T],
        source: &[T],
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if target.len() != source.len() {
            return Err(CodecError::malformed("slice copy lengths differ"));
        }
        self.admit_moves(source, 1, operation)?;
        target.copy_from_slice(source);
        Ok(())
    }

    /// Fills inline Copy values after admitting each slot and its written bytes.
    pub fn fill<T: Copy>(
        &self,
        values: &mut [T],
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(values, 1, operation)?;
        values.fill(value);
        Ok(())
    }

    /// Replaces each value through a fallible factory that admits child construction.
    /// A refusal preserves later values; completed replacements remain installed.
    pub fn fill_with<T>(
        &self,
        values: &mut [T],
        mut make: impl FnMut() -> Result<T, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(values, 1, operation)?;
        for value in values {
            self.charge_work(1, operation)?;
            *value = make()?;
        }
        Ok(())
    }

    /// Appends Copy elements after admitting the source copy and target growth.
    pub fn extend_from_slice<T: Copy>(
        &self,
        values: &mut Vec<T>,
        source: &[T],
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(source, 1, operation)?;
        self.reserve_vec(values, source.len(), operation)?;
        values.extend_from_slice(source);
        Ok(())
    }

    /// Copies a valid original range into the same vector without child clones.
    pub fn extend_from_within<T: Copy>(
        &self,
        values: &mut Vec<T>,
        source: std::ops::Range<usize>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let Some(slice) = values.get(source.clone()) else {
            return Err(CodecError::malformed("vector copy range exceeds length"));
        };
        self.admit_moves(slice, 1, operation)?;
        let count = slice.len();
        self.reserve_vec(values, count, operation)?;
        for offset in 0..count {
            self.charge_work(1, operation)?;
            let index = source
                .start
                .checked_add(offset)
                .ok_or_else(|| self.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
            let value = values[index];
            self.reserve_capacity(values, 1, operation)?;
            values.push(value);
        }
        Ok(())
    }

    /// Resizes through a fallible factory; existing values move without cloning children.
    /// The factory admits child construction. A refusal can leave a shorter growth.
    pub fn resize_with<T>(
        &self,
        values: &mut Vec<T>,
        length: usize,
        mut make: impl FnMut() -> Result<T, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if length <= values.len() {
            return self.truncate_vec(values, length, operation);
        }
        let count = length - values.len();
        self.reserve_vec(values, count, operation)?;
        for _index in 0..count {
            self.charge_work(1, operation)?;
            values.push(make()?);
        }
        Ok(())
    }

    /// Resizes Copy values through the one fallible growth implementation.
    pub fn resize_vec<T: Copy>(
        &self,
        values: &mut Vec<T>,
        length: usize,
        value: T,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.resize_with(values, length, || Ok(value), operation)
    }

    /// Moves a suffix into admitted storage and preserves both subsequence orders.
    /// A refusal during movement can leave a partially split vector.
    pub fn split_off_vec<T>(
        &self,
        values: &mut Vec<T>,
        at: usize,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let Some(suffix) = values.get(at..) else {
            return Err(CodecError::malformed("vector split exceeds length"));
        };
        self.admit_moves(suffix, 3, operation)?;
        let count = suffix.len();
        let mut output = self.collection_vec(count, operation)?;
        for _index in 0..count {
            self.charge_work(1, operation)?;
            let Some(value) = values.pop() else {
                return Err(CodecError::malformed("vector split suffix disappeared"));
            };
            self.reserve_capacity(&mut output, 1, operation)?;
            output.push(value);
        }
        self.reverse(&mut output, operation)?;
        Ok(output)
    }

    fn split_vec_range<T>(
        &self,
        values: &mut Vec<T>,
        range: std::ops::Range<usize>,
        operation: &'static str,
    ) -> Result<(Vec<T>, Vec<T>), CodecError> {
        if range.start > range.end || range.end > values.len() {
            return Err(CodecError::malformed("vector removal range exceeds length"));
        }
        let tail = self.split_off_vec(values, range.end, operation)?;
        let removed = self.split_off_vec(values, range.start, operation)?;
        Ok((removed, tail))
    }

    /// Eagerly drains a range into retained storage through admitted suffix moves.
    /// A refusal can leave a partially drained vector.
    pub fn drain_vec<T>(
        &self,
        values: &mut Vec<T>,
        range: std::ops::Range<usize>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let (removed, mut tail) = self.split_vec_range(values, range, operation)?;
        self.append_vec(values, &mut tail, operation)?;
        Ok(removed)
    }

    /// Replaces a range with owned values and returns its removed values in order.
    /// Replacement children already exist. A refusal can leave a partial replacement.
    pub fn splice_vec<T>(
        &self,
        values: &mut Vec<T>,
        range: std::ops::Range<usize>,
        mut replacement: Vec<T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        let (removed, mut tail) = self.split_vec_range(values, range, operation)?;
        self.append_vec(values, &mut replacement, operation)?;
        self.append_vec(values, &mut tail, operation)?;
        Ok(removed)
    }

    /// Shrinks owned vector capacity through the one admitted boxed-slice conversion.
    pub fn shrink_vec<T>(
        &self,
        values: Vec<T>,
        operation: &'static str,
    ) -> Result<Vec<T>, CodecError> {
        Ok(self.into_boxed_slice(values, operation)?.into_vec())
    }

    /// Reverses slot order after admitting the inline values to move.
    pub fn reverse<T>(&self, values: &mut [T], operation: &'static str) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        values.reverse();
        Ok(())
    }

    /// Rotates slots left after admitting three inline moves per slot.
    pub fn rotate_left<T>(
        &self,
        values: &mut [T],
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        if count > values.len() {
            return Err(CodecError::malformed("slice rotation exceeds length"));
        }
        self.admit_moves(values, 3, operation)?;
        values.rotate_left(count);
        Ok(())
    }

    /// Rotates slots right through the admitted left rotation.
    pub fn rotate_right<T>(
        &self,
        values: &mut [T],
        count: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let Some(left) = values.len().checked_sub(count) else {
            return Err(CodecError::malformed("slice rotation exceeds length"));
        };
        self.rotate_left(values, left, operation)
    }

    /// Copies an overlapping range after admitting the complete inline slice.
    pub fn copy_within<T: Copy>(
        &self,
        values: &mut [T],
        source: std::ops::Range<usize>,
        target: usize,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        let Some(count) = source.end.checked_sub(source.start) else {
            return Err(CodecError::malformed("slice copy range is reversed"));
        };
        if source.end > values.len() || target > values.len() || count > values.len() - target {
            return Err(CodecError::malformed("slice copy range exceeds length"));
        }
        self.admit_moves(values, 1, operation)?;
        values.copy_within(source, target);
        Ok(())
    }

    /// Drops a vector suffix. Releasing values is paid by the charges that
    /// admitted them, so truncation charges nothing.
    pub fn truncate_vec<T>(
        &self,
        values: &mut Vec<T>,
        length: usize,
        _operation: &'static str,
    ) -> Result<(), CodecError> {
        values.truncate(length);
        Ok(())
    }

    /// Drops all vector values through the admitted suffix removal.
    pub fn clear_vec<T>(
        &self,
        values: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.truncate_vec(values, 0, operation)
    }

    /// Keeps selected values in order. The predicate admits child work.
    /// On callback refusal, the vector keeps every value and may change their order.
    pub fn retain_vec<T>(
        &self,
        values: &mut Vec<T>,
        mut keep: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.retain_mut(values, |value| keep(value), operation)
    }

    /// Keeps selected values while allowing the predicate to edit each value.
    /// The predicate admits child work. A refusal keeps all values and completed edits.
    pub fn retain_mut<T>(
        &self,
        values: &mut Vec<T>,
        mut keep: impl FnMut(&mut T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        let mut write = 0;
        for read in 0..values.len() {
            self.charge_work(1, operation)?;
            if keep(&mut values[read])? {
                values.swap(write, read);
                write += 1;
            }
        }
        self.truncate_vec(values, write, operation)
    }

    /// Keeps selected B-tree map entries in key order. Every entry visit is
    /// admitted first; each removal admits its rebalancing before the entry is
    /// removed. The predicate admits child work. On refusal the remaining
    /// entries are kept and the original refusal is returned.
    pub fn retain_btree_map<K: Ord, V>(
        &self,
        values: &mut BTreeMap<K, V>,
        mut keep: impl FnMut(&K, &mut V) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        let len = values.len();
        let mut refusal = None;
        values.retain(|key, value| {
            if refusal.is_some() {
                return true;
            }
            match keep(key, value)
                .and_then(|kept| self.admit_tree_removal::<K, V>(kept, len, operation))
            {
                Ok(kept) => kept,
                Err(error) => {
                    refusal = Some(error);
                    true
                }
            }
        });
        refusal.map_or(Ok(()), Err)
    }

    /// Keeps selected B-tree set values in order, admitting visits and each
    /// removal as `retain_btree_map` does.
    pub fn retain_btree_set<T: Ord>(
        &self,
        values: &mut BTreeSet<T>,
        mut keep: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.charge_work(u64_from_index(values.len()), operation)?;
        let len = values.len();
        let mut refusal = None;
        values.retain(|value| {
            if refusal.is_some() {
                return true;
            }
            match keep(value)
                .and_then(|kept| self.admit_tree_removal::<T, ()>(kept, len, operation))
            {
                Ok(kept) => kept,
                Err(error) => {
                    refusal = Some(error);
                    true
                }
            }
        });
        refusal.map_or(Ok(()), Err)
    }

    fn admit_tree_removal<K, V>(
        &self,
        kept: bool,
        len: usize,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if !kept {
            self.admit_tree_removal_work::<K, V>(len, operation)?;
        }
        Ok(kept)
    }

    /// Removes adjacent equal values and keeps the first value in each run.
    pub fn dedup_vec<T: DecodeCost + PartialEq>(
        &self,
        values: &mut Vec<T>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.dedup_by(
            values,
            |left, right| self.equal(left, right, operation),
            operation,
        )
    }

    /// Compacts adjacent matches. The predicate admits child work.
    /// On callback refusal, the vector keeps every value and may change their order.
    pub fn dedup_by<T>(
        &self,
        values: &mut Vec<T>,
        mut same: impl FnMut(&T, &T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        if values.is_empty() {
            return Ok(());
        }
        let mut write = 1;
        for read in 1..values.len() {
            self.charge_work(1, operation)?;
            if !same(&values[write - 1], &values[read])? {
                values.swap(write, read);
                write += 1;
            }
        }
        self.truncate_vec(values, write, operation)
    }

    /// Compacts adjacent projected keys through complete-key comparison.
    /// The extractor admits construction of each projected key.
    pub fn dedup_by_key<T, K: DecodeCost + PartialEq>(
        &self,
        values: &mut Vec<T>,
        mut key: impl FnMut(&T) -> Result<K, CodecError>,
        operation: &'static str,
    ) -> Result<(), CodecError> {
        self.dedup_by(
            values,
            |left, right| self.equal(&key(left)?, &key(right)?, operation),
            operation,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;

    #[test]
    fn charged_vector_edits_preserve_copy_split_drain_and_splice_order() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        let mut values = vec![0_u8, 1, 2, 3];
        ctx.fill(&mut values[..2], 9, "fill")
            .expect("test operation succeeds");
        assert_eq!(values, [9, 9, 2, 3]);
        ctx.extend_from_slice(&mut values, &[4, 5], "extend")
            .expect("test operation succeeds");
        ctx.extend_from_within(&mut values, 2..4, "within")
            .expect("test operation succeeds");
        assert_eq!(values, [9, 9, 2, 3, 4, 5, 2, 3]);
        let tail = ctx
            .split_off_vec(&mut values, 6, "split")
            .expect("test operation succeeds");
        assert_eq!(tail, [2, 3]);
        assert_eq!(values, [9, 9, 2, 3, 4, 5]);
        assert_eq!(
            ctx.drain_vec(&mut values, 1..3, "drain")
                .expect("test operation succeeds"),
            [9, 2]
        );
        assert_eq!(values, [9, 3, 4, 5]);
        assert_eq!(
            ctx.splice_vec(&mut values, 1..3, vec![7, 8, 6], "splice")
                .expect("test operation succeeds"),
            [3, 4]
        );
        assert_eq!(values, [9, 7, 8, 6, 5]);
        ctx.resize_vec(&mut values, 7, 1, "grow")
            .expect("test operation succeeds");
        assert_eq!(values, [9, 7, 8, 6, 5, 1, 1]);
        ctx.resize_vec(&mut values, 2, 0, "truncate")
            .expect("test operation succeeds");
        assert_eq!(values, [9, 7]);
        let values = ctx
            .shrink_vec(values, "shrink")
            .expect("test operation succeeds");
        assert_eq!(values.capacity(), values.len());
        assert_eq!(values, [9, 7]);
    }

    #[test]
    fn charged_mutable_retention_and_factory_fill_keep_completed_edits() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation succeeds");
        let mut values = vec![1_u32, 2, 3, 4];
        ctx.retain_mut(
            &mut values,
            |value| {
                *value += 10;
                Ok(*value % 2 == 0)
            },
            "retain edits",
        )
        .expect("test operation succeeds");
        assert_eq!(values, [12, 14]);
        let mut next = 0;
        ctx.fill_with(
            &mut values,
            || {
                next += 1;
                Ok(next)
            },
            "factory fill",
        )
        .expect("test operation succeeds");
        assert_eq!(values, [1, 2]);
        let CodecError::ResourceLimit(first) = ctx.refuse_codec_limit("child refusal", 1, 0) else {
            panic!("resource refusal")
        };
        let mut calls = 0;
        let error = ctx
            .fill_with(
                &mut values,
                || {
                    calls += 1;
                    Err(CodecError::ResourceLimit(first))
                },
                "fused fill",
            )
            .expect_err("operation refuses");
        let CodecError::ResourceLimit(repeated) = error else {
            panic!("resource refusal")
        };
        assert_eq!(repeated, first);
        assert_eq!(calls, 0);
        assert_eq!(values, [1, 2]);
    }

    #[test]
    fn vector_edit_refusal_precedes_factories_and_suffix_movement() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        let mut values = vec![1_u8, 2, 3];
        let mut calls = 0;
        let CodecError::ResourceLimit(first) = ctx
            .fill_with(
                &mut values,
                || {
                    calls += 1;
                    Ok(9)
                },
                "fill refusal",
            )
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(values, [1, 2, 3]);
        assert_eq!(calls, 0);
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(repeated) = ctx
            .split_off_vec(&mut values, 1, "later split")
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        assert_eq!(values, [1, 2, 3]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        assert!(matches!(
            ctx.split_off_vec(&mut values, 1, "split storage"),
            Err(CodecError::ResourceLimit(_))
        ));
        assert_eq!(values, [1, 2, 3]);
        assert!(ctx
            .extend_from_within(&mut values, 3..4, "invalid range")
            .is_err());
        assert_eq!(values, [1, 2, 3]);
    }

    #[test]
    fn vector_growth_propagates_the_factory_resource_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Four initial String slots; no retained bytes remain for the factory's text.
        policy.limits.max_retained_bytes =
            4 * u64::try_from(std::mem::size_of::<String>()).expect("test operation succeeds");
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
        let mut values = Vec::<String>::new();
        let mut calls = 0;
        let CodecError::ResourceLimit(first) = ctx
            .resize_with(
                &mut values,
                1,
                || {
                    calls += 1;
                    ctx.copy_retained_text("child", "factory text")
                },
                "growth",
            )
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.operation, "factory text");
        assert_eq!(first.used, policy.limits.max_retained_bytes);
        assert_eq!(first.additional, 5);
        assert_eq!(calls, 1);
        assert!(values.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(repeated) = ctx
            .resize_with(&mut values, 1, || Ok(String::new()), "later growth")
            .expect_err("operation refuses")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
    }

    #[test]
    fn vector_boxing_reuses_exact_capacity_without_work_or_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let values = vec![1_u8, 2, 3];
        assert_eq!(values.len(), values.capacity());
        let pointer = values.as_ptr();
        let boxed = ctx.into_boxed_slice(values, "box").expect("same storage");
        assert_eq!(boxed.as_ptr(), pointer);
        assert_eq!(&*boxed, &[1, 2, 3]);
        let zero_sized = ctx
            .into_boxed_slice(vec![(); 1024], "zero-sized box")
            .expect("metadata only");
        assert_eq!(zero_sized.len(), 1024);
    }

    #[test]
    fn vector_boxing_admits_shrink_storage_and_moves_before_conversion() {
        let arena = DecodeArena::new();
        for (work, storage) in [(5, 3), (6, 2)] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_materialized_bytes = storage;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let mut values = Vec::with_capacity(8);
            values.extend_from_slice(&[1_u8, 2, 3]);
            let CodecError::ResourceLimit(first) =
                ctx.into_boxed_slice(values, "box").expect_err("refusal")
            else {
                panic!("refusal")
            };
            let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
            else {
                panic!("refusal")
            };
            assert_eq!(first, second);
        }
        let mut policy = DecodePolicy::service();
        // Three slot visits and three moved bytes; three temporary allocation bytes.
        policy.limits.max_work_units = 6;
        policy.limits.max_materialized_bytes = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = Vec::with_capacity(8);
        values.extend_from_slice(&[1_u8, 2, 3]);
        assert_eq!(
            &*ctx.into_boxed_slice(values, "box").expect("admission"),
            &[1, 2, 3]
        );
        ctx.reserve_scoped(3, "released shrink storage")
            .expect("released");
    }

    #[test]
    fn btree_retention_refuses_before_predicate_and_propagates_child_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = std::collections::BTreeMap::from([(1_u8, 2_u8), (3, 4)]);
        let called = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(first) = ctx
            .retain_btree_map(
                &mut values,
                |_, _| {
                    called.set(true);
                    Ok(false)
                },
                "retain",
            )
            .expect_err("two visits exceed one unit")
        else {
            panic!("refusal")
        };
        assert!(!called.get());
        assert_eq!(values.len(), 2);
        let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut calls = 0;
        let CodecError::ResourceLimit(child) = ctx
            .retain_btree_map(
                &mut values,
                |_, _| {
                    calls += 1;
                    Err(ctx.refuse_codec_limit("child", 0, 1))
                },
                "retain",
            )
            .expect_err("child refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(calls, 1);
        assert_eq!(child.operation, "child");
        assert_eq!(
            values,
            std::collections::BTreeMap::from([(1_u8, 2_u8), (3, 4)])
        );
    }

    #[test]
    fn btree_retention_charges_visits_and_each_removal() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = std::collections::BTreeMap::from([(1_u8, 2_u8), (3, 4)]);
        ctx.retain_btree_map(
            &mut values,
            |key, value| {
                *value += 1;
                Ok(*key == 3)
            },
            "retain",
        )
        .expect("admission");
        assert_eq!(values, std::collections::BTreeMap::from([(3_u8, 5_u8)]));
        let mut set = std::collections::BTreeSet::from([1_u8, 3]);
        ctx.retain_btree_set(&mut set, |value| Ok(*value == 1), "retain set")
            .expect("admission");
        assert_eq!(set, std::collections::BTreeSet::from([1_u8]));
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("refusal")
        };
        // Two visits per tree, and one removal per tree shifting its single leaf.
        let node_bytes = |slot: usize| {
            crate::decode::u64_from_index(
                11 * slot + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<usize>(),
            )
        };
        assert_eq!(limit.used, 2 + 2 + node_bytes(2) + node_bytes(1));
    }

    #[test]
    fn slice_moves_preserve_copy_and_reverse_results() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut target = [0_u16; 3];
        ctx.copy_into(&mut target, &[1, 2, 3], "copy")
            .expect("admission");
        ctx.reverse(&mut target, "reverse").expect("admission");
        assert_eq!(target, [3, 2, 1]);
        assert!(matches!(
            ctx.copy_into(&mut target, &[1], "mismatch"),
            Err(CodecError::Malformed(_))
        ));
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("refusal")
        };
        // Two three-slot visits and twelve two-byte moves: one copy and three per swap.
        assert_eq!(limit.used, 30);
    }

    #[test]
    fn slice_rotation_and_overlapping_copy_admit_inline_moves() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three four-slot visits, two rotations of three moves, and one copy of two-byte values.
        policy.limits.max_work_units = 68;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = [1_u16, 2, 3, 4];
        ctx.rotate_left(&mut values, 1, "left").expect("admission");
        assert_eq!(values, [2, 3, 4, 1]);
        ctx.rotate_right(&mut values, 1, "right")
            .expect("admission");
        assert_eq!(values, [1, 2, 3, 4]);
        ctx.copy_within(&mut values, 0..3, 1, "overlap")
            .expect("admission");
        assert_eq!(values, [1, 1, 2, 3]);
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(1, "probe").expect_err("exact total")
        else {
            panic!("refusal")
        };
        assert_eq!(limit.used, 68);
    }

    #[test]
    fn slice_rotation_and_copy_refuse_before_mutation() {
        let arena = DecodeArena::new();
        for rotation in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = if rotation { 27 } else { 11 };
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let mut values = [1_u16, 2, 3, 4];
            let result = if rotation {
                ctx.rotate_right(&mut values, 1, "rotate")
            } else {
                ctx.copy_within(&mut values, 0..3, 1, "copy")
            };
            let CodecError::ResourceLimit(first) = result.expect_err("refusal") else {
                panic!("refusal")
            };
            assert_eq!(values, [1, 2, 3, 4]);
            let CodecError::ResourceLimit(second) = ctx.charge_work(0, "later").expect_err("fused")
            else {
                panic!("refusal")
            };
            assert_eq!(first, second);
        }
    }

    #[test]
    fn slice_rotation_and_copy_validate_ranges_and_empty_slices() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = [1_u8, 2, 3];
        assert!(matches!(
            ctx.rotate_left(&mut values, 4, "left"),
            Err(CodecError::Malformed(_))
        ));
        assert!(matches!(
            ctx.rotate_right(&mut values, 4, "right"),
            Err(CodecError::Malformed(_))
        ));
        for (source, target) in [
            (std::ops::Range { start: 2, end: 1 }, 0),
            (0..4, 0),
            (0..1, 4),
            (0..3, 1),
            (usize::MAX..usize::MAX, 0),
        ] {
            assert!(matches!(
                ctx.copy_within(&mut values, source, target, "copy"),
                Err(CodecError::Malformed(_))
            ));
            assert_eq!(values, [1, 2, 3]);
        }
        ctx.rotate_left(&mut values, 0, "zero").expect("admission");
        ctx.rotate_right(&mut values, 3, "full").expect("admission");
        assert_eq!(values, [1, 2, 3]);
        let mut empty: [u8; 0] = [];
        ctx.rotate_left(&mut empty, 0, "empty").expect("admission");
        ctx.rotate_right(&mut empty, 0, "empty").expect("admission");
        ctx.copy_within(&mut empty, 0..0, 0, "empty")
            .expect("admission");
        let mut owned = [String::from("first"), String::from("second")];
        ctx.rotate_right(&mut owned, 1, "owned slots")
            .expect("admission");
        assert_eq!(owned, ["second", "first"]);
    }

    #[test]
    fn vector_compaction_keeps_survivor_order_and_first_run_values() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = vec![1_u8, 1, 2, 1, 1, 3];
        ctx.dedup_vec(&mut values, "dedup").expect("admission");
        assert_eq!(values, [1, 2, 1, 3]);
        ctx.retain_vec(&mut values, |value| Ok(*value != 1), "retain")
            .expect("admission");
        assert_eq!(values, [2, 3]);
        ctx.clear_vec(&mut values, "clear").expect("admission");
        assert!(values.is_empty());
        let mut records = vec![(1_u8, 0_u8), (1, 1), (2, 2), (2, 3)];
        ctx.dedup_by_key(&mut records, |value| Ok(value.0), "keys")
            .expect("admission");
        assert_eq!(records, [(1, 0), (2, 2)]);
        ctx.truncate_vec(&mut records, 10, "past end")
            .expect("no removal");
        assert_eq!(records, [(1, 0), (2, 2)]);
    }

    #[test]
    fn mutation_refusal_precedes_changes_and_propagates_original_error() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = vec![1_u8, 2, 3];
        let called = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(first) = ctx
            .retain_vec(
                &mut values,
                |_| {
                    called.set(true);
                    Ok(false)
                },
                "retain",
            )
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert!(!called.get());
        assert_eq!(values, [1, 2, 3]);
        let CodecError::ResourceLimit(repeated) =
            ctx.reverse(&mut values, "reverse").expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(first, repeated);
        assert_eq!(values, [1, 2, 3]);
    }

    #[test]
    fn callback_refusal_keeps_all_vector_values() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = vec![1_u8, 2, 3];
        let CodecError::ResourceLimit(child) = ctx
            .retain_vec(
                &mut values,
                |value| {
                    if *value == 3 {
                        Err(ctx.refuse_codec_limit("child", 0, 1))
                    } else {
                        Ok(*value == 2)
                    }
                },
                "retain",
            )
            .expect_err("child refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(child.operation, "child");
        values.sort_unstable();
        assert_eq!(values, [1, 2, 3]);
    }
    #[test]
    fn indexed_insertion_preserves_order_and_charges_suffix_before_mutation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 6;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = Vec::with_capacity(4);
        values.extend([1_u8, 3, 4]);
        // Shifting the two-byte suffix visits and moves two slots.
        ctx.insert_vec(&mut values, 1, 2, "insert")
            .expect("suffix moves");
        assert_eq!(values, [1, 2, 3, 4]);
        let CodecError::ResourceLimit(first) = ctx
            .insert_vec(&mut values, 0, 0, "next insertion")
            .expect_err("growth of the full vector exceeds the remaining work")
        else {
            panic!("resource refusal")
        };
        // Reallocating the full four-slot vector copies four slots first.
        assert_eq!(first.used, 4);
        assert_eq!(first.additional, 4);
        assert_eq!(values, [1, 2, 3, 4]);
        let CodecError::ResourceLimit(repeated) = ctx
            .insert_vec(&mut values, 4, 5, "later")
            .expect_err("sticky refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        let CodecError::ResourceLimit(invalid) = ctx
            .insert_vec(&mut values, 5, 5, "invalid after refusal")
            .expect_err("sticky refusal precedes invalid index")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, invalid);
    }

    #[test]
    fn indexed_insertion_validates_index_and_admits_scoped_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 8;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (mut values, mut storage) = ctx.temporary_vec::<u8>(0, "temporary").expect("empty");
        assert!(matches!(
            storage.with_storage(|| ctx.insert_vec(&mut values, 1, 1, "invalid")),
            Err(CodecError::Malformed(_))
        ));
        assert!(values.is_empty());
        storage
            .with_storage(|| ctx.insert_vec(&mut values, 0, 1, "insert"))
            .expect("scoped growth");
        storage
            .with_storage(|| ctx.insert_vec(&mut values, 1, 2, "end"))
            .expect("end insertion");
        assert_eq!(values, [1, 2]);
        drop((values, storage));
        ctx.reserve_scoped(8, "released vector").expect("released");
    }

    #[test]
    fn indexed_insertion_refuses_growth_before_mutation() {
        for scoped in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            if scoped {
                policy.limits.max_materialized_bytes = 0;
            } else {
                policy.limits.max_retained_bytes = 0;
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let mut values = Vec::<u8>::new();
            let mut storage = ctx.reserve_scoped(0, "scope").expect("scope");
            let error = if scoped {
                storage.with_storage(|| ctx.insert_vec(&mut values, 0, 1, "insert"))
            } else {
                ctx.insert_vec(&mut values, 0, 1, "insert")
            };
            assert!(matches!(error, Err(CodecError::ResourceLimit(_))));
            assert!(values.is_empty());
            assert_eq!(values.capacity(), 0);
        }
    }
}
