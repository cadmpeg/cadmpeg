// SPDX-License-Identifier: Apache-2.0
//! Work admission for binary heap sifts and slot movement.
use std::collections::BinaryHeap;

use super::cost::DecodeCost;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

impl DecodeContext<'_> {
    // A downward sift and its upward correction use at most two comparisons
    // and four inline moves per level. Every comparison reads two operands.
    pub(super) fn admit_heap<T: Ord + DecodeCost>(&self, values: &BinaryHeap<T>, incoming: Option<&T>, operation: &'static str) -> Result<(), CodecError> {
        let mut maximum = 0_u64;
        for value in self.admit_iter(values.as_slice(), operation)? {
            maximum = maximum.max(value.decode_cost(self, operation)?);
        }
        let mut count = u64_from_index(values.len());
        if let Some(value) = incoming {
            self.charge_work(1, operation)?;
            maximum = maximum.max(value.decode_cost(self, operation)?);
            count = self.cost_sum(count, 1, operation)?;
        }
        if count == 0 { return self.charge_work(0, operation); }
        let levels = u64::from(u64::BITS - count.leading_zeros()) + 1;
        let operand_and_moves = self.cost_sum(maximum, u64_from_index(std::mem::size_of::<T>()), operation)?;
        let work = self.cost_product(self.cost_product(operand_and_moves, 4, operation)?, levels, operation)?;
        self.charge_work(work, operation)
    }

    /// Removes the greatest value after admitting comparisons and inline moves.
    /// Ordering and comparison cost must remain stable during the operation.
    pub fn pop_heap<T: Ord + DecodeCost>(&self, values: &mut BinaryHeap<T>, operation: &'static str) -> Result<Option<T>, CodecError> {
        self.admit_heap(values, None, operation)?;
        Ok(values.pop())
    }

    /// Inserts a value after admitting comparisons, inline moves and capacity growth.
    /// Ordering and comparison cost must remain stable during the operation.
    pub fn push_heap<T: Ord + DecodeCost>(&self, values: &mut BinaryHeap<T>, value: T, operation: &'static str) -> Result<(), CodecError> {
        self.reserve_heap(values, 1, operation)?;
        self.admit_heap(values, Some(&value), operation)?;
        values.push(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{BinaryHeap, CodecError, DecodeContext};
    use crate::decode::{DecodeArena, DecodePolicy, ResourceDimension};

    #[test]
    fn heap_sifts_charge_maximum_operand_and_inline_moves_before_mutation() {
        let arena = DecodeArena::new();
        // Three measuring visits plus three levels of four maximum keys and four inline moves.
        let total = 3 + 3 * 4 * (8 + u64::try_from(std::mem::size_of::<String>()).unwrap());
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = total - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut heap = BinaryHeap::from(["zzzzzzzz".to_owned(), "b".to_owned(), "a".to_owned()]);
        let old = heap.clone();
        let CodecError::ResourceLimit(first) = ctx.pop_heap(&mut heap, "heap pop").unwrap_err() else { panic!("resource refusal") };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(heap.as_slice(), old.as_slice());
        let CodecError::ResourceLimit(repeated) = ctx.push_heap(&mut heap, "new".to_owned(), "heap push").unwrap_err() else { panic!("resource refusal") };
        assert_eq!(first, repeated);
        assert_eq!(heap.as_slice(), old.as_slice());

        policy.limits.max_work_units = total;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert_eq!(ctx.pop_heap(&mut heap, "heap pop").unwrap().as_deref(), Some("zzzzzzzz"));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").unwrap_err() else { panic!("resource refusal") };
        assert_eq!(limit.used, total);
    }

    #[test]
    fn heap_push_admits_storage_and_preserves_heap_order() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut heap = BinaryHeap::new();
        for value in [3_u8, 1, 4, 2] { ctx.push_heap(&mut heap, value, "heap insert").unwrap(); }
        for value in [4_u8, 3, 2, 1] { assert_eq!(ctx.pop_heap(&mut heap, "heap remove").unwrap(), Some(value)); }
        assert_eq!(ctx.pop_heap(&mut heap, "empty heap").unwrap(), None);
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut heap = BinaryHeap::new();
        assert!(matches!(ctx.push_heap(&mut heap, 1_u8, "heap storage"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
        assert!(heap.is_empty());
    }
}
