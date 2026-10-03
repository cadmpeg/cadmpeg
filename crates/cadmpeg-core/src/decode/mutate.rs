// SPDX-License-Identifier: Apache-2.0
//! Charged slice copies and stable in-place vector compaction.
use super::cost::DecodeCost;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

impl DecodeContext<'_> {
    /// Admits slot visits and moves of the complete inline representation.
    pub(super) fn admit_moves<T>(&self, values: &[T], moves: u64, operation: &'static str) -> Result<(), CodecError> {
        let count = u64_from_index(values.len());
        let bytes = self.cost_product(count, u64_from_index(std::mem::size_of::<T>()), operation)?;
        let bytes = self.cost_product(bytes, moves, operation)?;
        self.charge_work(self.cost_sum(count, bytes, operation)?, operation)
    }

    /// Copies equal-length slices after admitting all inline source bytes.
    pub fn copy_into<T: Copy>(&self, target: &mut [T], source: &[T], operation: &'static str) -> Result<(), CodecError> {
        if target.len() != source.len() {
            return Err(CodecError::malformed("slice copy lengths differ"));
        }
        self.admit_moves(source, 1, operation)?;
        target.copy_from_slice(source);
        Ok(())
    }

    /// Reverses slot order after admitting the inline values to move.
    pub fn reverse<T>(&self, values: &mut [T], operation: &'static str) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        values.reverse();
        Ok(())
    }

    /// Drops a vector suffix after admitting its slots and complete child bytes.
    pub fn truncate_vec<T: DecodeCost>(&self, values: &mut Vec<T>, length: usize, operation: &'static str) -> Result<(), CodecError> {
        if let Some(removed) = values.get(length..) {
            self.charge_work(u64_from_index(removed.len()), operation)?;
            self.charge_key(removed, 1, operation)?;
            values.truncate(length);
        }
        Ok(())
    }

    /// Drops all vector values through the admitted suffix removal.
    pub fn clear_vec<T: DecodeCost>(&self, values: &mut Vec<T>, operation: &'static str) -> Result<(), CodecError> {
        self.truncate_vec(values, 0, operation)
    }

    /// Keeps selected values in order. The predicate admits child work.
    /// On callback refusal, the vector keeps every value and may change their order.
    pub fn retain_vec<T: DecodeCost>(&self, values: &mut Vec<T>, mut keep: impl FnMut(&T) -> Result<bool, CodecError>, operation: &'static str) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        let mut write = 0;
        for read in 0..values.len() {
            self.charge_work(1, operation)?;
            if keep(&values[read])? {
                values.swap(write, read);
                write += 1;
            }
        }
        self.truncate_vec(values, write, operation)
    }

    /// Removes adjacent equal values and keeps the first value in each run.
    pub fn dedup_vec<T: DecodeCost + PartialEq>(&self, values: &mut Vec<T>, operation: &'static str) -> Result<(), CodecError> {
        self.dedup_by(values, |left, right| self.equal(left, right, operation), operation)
    }

    /// Compacts adjacent matches. The predicate admits child work.
    /// On callback refusal, the vector keeps every value and may change their order.
    pub fn dedup_by<T: DecodeCost>(&self, values: &mut Vec<T>, mut same: impl FnMut(&T, &T) -> Result<bool, CodecError>, operation: &'static str) -> Result<(), CodecError> {
        self.admit_moves(values, 3, operation)?;
        if values.is_empty() { return Ok(()); }
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
    pub fn dedup_by_key<T: DecodeCost, K: DecodeCost + PartialEq>(&self, values: &mut Vec<T>, mut key: impl FnMut(&T) -> Result<K, CodecError>, operation: &'static str) -> Result<(), CodecError> {
        self.dedup_by(values, |left, right| self.equal(&key(left)?, &key(right)?, operation), operation)
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::CodecError;

    #[test]
    fn slice_moves_preserve_copy_and_reverse_results() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut target = [0_u16; 3];
        ctx.copy_into(&mut target, &[1, 2, 3], "copy").expect("admission");
        ctx.reverse(&mut target, "reverse").expect("admission");
        assert_eq!(target, [3, 2, 1]);
        assert!(matches!(ctx.copy_into(&mut target, &[1], "mismatch"), Err(CodecError::Malformed(_))));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else { panic!("refusal") };
        // Two three-slot visits and twelve two-byte moves: one copy and three per swap.
        assert_eq!(limit.used, 30);
    }

    #[test]
    fn vector_compaction_keeps_survivor_order_and_first_run_values() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = vec![1_u8, 1, 2, 1, 1, 3];
        ctx.dedup_vec(&mut values, "dedup").expect("admission");
        assert_eq!(values, [1, 2, 1, 3]);
        ctx.retain_vec(&mut values, |value| Ok(*value != 1), "retain").expect("admission");
        assert_eq!(values, [2, 3]);
        ctx.clear_vec(&mut values, "clear").expect("admission");
        assert!(values.is_empty());
        let mut records = vec![(1_u8, 0_u8), (1, 1), (2, 2), (2, 3)];
        ctx.dedup_by_key(&mut records, |value| Ok(value.0), "keys").expect("admission");
        assert_eq!(records, [(1, 0), (2, 2)]);
        ctx.truncate_vec(&mut records, 10, "past end").expect("no removal");
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
        let CodecError::ResourceLimit(first) = ctx.retain_vec(&mut values, |_| { called.set(true); Ok(false) }, "retain").expect_err("refusal") else { panic!("refusal") };
        assert!(!called.get());
        assert_eq!(values, [1, 2, 3]);
        let CodecError::ResourceLimit(repeated) = ctx.reverse(&mut values, "reverse").expect_err("refusal") else { panic!("refusal") };
        assert_eq!(first, repeated);
        assert_eq!(values, [1, 2, 3]);
    }

    #[test]
    fn callback_refusal_keeps_all_vector_values() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = vec![1_u8, 2, 3];
        let CodecError::ResourceLimit(child) = ctx.retain_vec(&mut values, |value| {
            if *value == 3 { Err(ctx.refuse_codec_limit("child", 0, 1)) } else { Ok(*value == 2) }
        }, "retain").expect_err("child refusal") else { panic!("refusal") };
        assert_eq!(child.operation, "child");
        values.sort_unstable();
        assert_eq!(values, [1, 2, 3]);
    }
}
