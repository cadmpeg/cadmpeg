// SPDX-License-Identifier: Apache-2.0
//! A scoped max-heap with admission before storage, comparisons and moves.

use cadmpeg_core::decode::{DecodeContext, ResourceLimit, ScopedReservation};

pub(super) struct PriorityQueue<'ctx, 'arena, T> {
    values: Vec<T>,
    context: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena, T: Ord> PriorityQueue<'ctx, 'arena, T> {
    pub(super) fn new(context: &'ctx DecodeContext<'arena>) -> Result<Self, ResourceLimit> {
        let storage = context.reserve_scoped_limit(0, "IR priority queue")?;
        Ok(Self {
            values: Vec::new(),
            context,
            storage,
        })
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), ResourceLimit> {
        self.context.reserve_scoped_vec_limit(
            &mut self.storage,
            &mut self.values,
            1,
            "IR priority queue",
        )?;
        self.context
            .charge_work_limit(1, "IR priority queue append")?;
        self.values.push(value);
        let mut index = self.values.len() - 1;
        while index > 0 {
            let parent = (index - 1) / 2;
            self.context
                .charge_work_limit(1, "IR priority queue comparison")?;
            if self.values[index] <= self.values[parent] {
                break;
            }
            self.context
                .charge_work_limit(2, "IR priority queue swap")?;
            self.values.swap(index, parent);
            index = parent;
        }
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Result<Option<T>, ResourceLimit> {
        if self.values.is_empty() {
            self.context
                .charge_work_limit(0, "IR priority queue empty")?;
            return Ok(None);
        }
        self.context
            .charge_work_limit(1, "IR priority queue remove")?;
        let Some(last) = self.values.pop() else {
            return Ok(None);
        };
        if self.values.is_empty() {
            return Ok(Some(last));
        }
        self.context
            .charge_work_limit(2, "IR priority queue root replacement")?;
        let result = std::mem::replace(&mut self.values[0], last);
        self.restore_root()?;
        Ok(Some(result))
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    pub(super) fn peek(&self) -> Result<Option<&T>, ResourceLimit> {
        self.context
            .charge_work_limit(u64::from(!self.values.is_empty()), "IR priority queue peek")?;
        Ok(self.values.first())
    }

    pub(super) fn replace_max(&mut self, value: T) -> Result<Option<T>, ResourceLimit> {
        if self.values.is_empty() {
            self.push(value)?;
            return Ok(None);
        }
        self.context
            .charge_work_limit(2, "IR priority queue root replacement")?;
        let result = std::mem::replace(&mut self.values[0], value);
        self.restore_root()?;
        Ok(Some(result))
    }

    fn restore_root(&mut self) -> Result<(), ResourceLimit> {
        let mut index = 0usize;
        while let Some(left) = index.checked_mul(2).and_then(|index| index.checked_add(1)) {
            if left >= self.values.len() {
                break;
            }
            let right = left + 1;
            let child = if right < self.values.len() {
                self.context
                    .charge_work_limit(1, "IR priority queue comparison")?;
                if self.values[right] > self.values[left] {
                    right
                } else {
                    left
                }
            } else {
                left
            };
            self.context
                .charge_work_limit(1, "IR priority queue comparison")?;
            if self.values[index] >= self.values[child] {
                break;
            }
            self.context
                .charge_work_limit(2, "IR priority queue swap")?;
            self.values.swap(index, child);
            index = child;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PriorityQueue;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn priority_queue_preserves_admission_and_max_heap_order() {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::CollectionItems,
            ResourceDimension::WorkUnits,
        ] {
            let operation = if dimension == ResourceDimension::WorkUnits {
                "IR priority queue append"
            } else {
                "IR priority queue"
            };
            cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
                let mut policy = DecodePolicy::service();
                match dimension {
                    ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                    ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                    ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                    _ => unreachable!("tested queue dimensions"),
                }
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
                let result = queue.push(3_u32).map_err(CodecError::from);
                let Err(CodecError::ResourceLimit(limit)) = &result else {
                    panic!("queue admission");
                };
                assert_eq!(limit.dimension, dimension);
                drop(queue);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                result
            });
        }
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 256;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
        assert_eq!(queue.pop().expect("empty pop"), None);
        for value in [4_u32, 7, 2, 7, 1] {
            queue.push(value).expect("insert");
        }
        for expected in [7, 7, 4, 2, 1] {
            assert_eq!(queue.pop().expect("remove"), Some(expected));
        }
        assert_eq!(queue.pop().expect("empty pop"), None);
        let bytes = u64::try_from(queue.values.capacity() * std::mem::size_of::<u32>())
            .expect("test bytes fit");
        let spare = ctx
            .reserve_scoped_limit(256 - bytes, "test live priority queue")
            .expect("capacity remains reserved after removals");
        drop(spare);
        drop(queue);
        let reuse = ctx
            .reserve_scoped_limit(256, "test priority queue released")
            .expect("all bytes reusable");
        drop(reuse);
        ctx.finish_session().expect("no retained queue storage");
    }

    #[test]
    fn priority_queue_returns_comparison_and_removal_refusals_unchanged() {
        for operation in ["IR priority queue comparison", "IR priority queue swap", "IR priority queue remove", "IR priority queue root replacement"] {
            cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits, operation, |cap| {
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let arena = DecodeArena::new();
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                    let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
                    let result = (|| -> Result<(), cadmpeg_core::decode::ResourceLimit> {
                        queue.push(1_u32)?;
                        queue.push(2)?;
                        if matches!(operation, "IR priority queue remove" | "IR priority queue root replacement") {
                            let _ = queue.pop()?;
                        }
                        Ok(())
                    })().map_err(CodecError::from);
                    let Err(CodecError::ResourceLimit(limit)) = &result else {
                        panic!("the named queue boundary must refuse");
                    };
                    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(limit.operation, operation);
                    drop(queue);
                    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                    result
                },
            );
        }
    }

    #[test]
    fn priority_queue_replacement_reuses_slots_and_preserves_order() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        policy.limits.max_materialized_bytes = 64;
        policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
        assert_eq!(
            queue
                .replace_max(1_u32)
                .expect("empty replacement inserts once"),
            None
        );
        queue.push(4).expect("second and final slot");
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek().expect("root read"), Some(&4));
        assert_eq!(queue.replace_max(7).expect("reuse root"), Some(4));
        assert_eq!(queue.replace_max(3).expect("reuse root again"), Some(7));
        assert_eq!(
            queue.replace_max(0).expect("promote the remaining child"),
            Some(3)
        );
        assert_eq!(queue.pop().expect("remove first"), Some(1));
        assert_eq!(queue.pop().expect("remove second"), Some(0));
        assert_eq!(queue.peek().expect("empty root"), None);
        drop(queue);
        ctx.finish_session()
            .expect("replacements allocate no additional slots");
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, "IR priority queue root replacement", |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
                queue.push(1_u32).expect("one append unit");
                let result = queue.replace_max(2).map_err(CodecError::from);
                let Err(CodecError::ResourceLimit(limit)) = &result else {
                    panic!("two moved rows need admission");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "IR priority queue root replacement");
                drop(queue);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                result
            },
        );
    }

    #[test]
    fn priority_queue_peek_preserves_work_refusal() {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits, "IR priority queue peek", |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let arena = DecodeArena::new();
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
                queue.push(1_u32).expect("one append unit");
                let result = queue.peek().map(|value| value.copied()).map_err(CodecError::from);
                let Err(CodecError::ResourceLimit(limit)) = &result else {
                    panic!("root read needs work");
                };
                assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
                assert_eq!(limit.operation, "IR priority queue peek");
                drop(queue);
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                result
            },
        );
    }

    #[test]
    fn priority_queue_charges_only_comparisons_and_moves_it_performs() {
        let mut policy = DecodePolicy::service();
        // Four appends, four removals, three root replacements (two units each),
        // seven comparisons and four swaps (two units each): 29 units.
        policy.limits.max_work_units = 29;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut queue = PriorityQueue::new(&ctx).expect("empty queue");
        for value in [1_u32, 3, 2, 4] {
            queue.push(value).expect("append and actual sift work");
        }
        for expected in [4_u32, 3, 2, 1] {
            assert_eq!(queue.pop().expect("actual sift and removal work"), Some(expected));
        }
        assert_eq!(queue.pop().expect("empty pop makes no comparison"), None);
        let limit = ctx.charge_work_limit(1, "test next queue operation").expect_err("exact queue work");
        assert_eq!((limit.used, limit.additional), (29, 1));
        drop(queue);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
    }
}
