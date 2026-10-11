// SPDX-License-Identifier: Apache-2.0
//! Deferred curve completion in FIFO retry order, without expanding retries.

use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

// A key has at most usize::MAX links. Prefix comparisons use fixed-width
// binary lifting, rather than walking an input-sized common prefix.
const KEY_LEVELS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyId(usize);

struct Key {
    group: usize,
    ordinal: usize,
    tail: Option<KeyId>,
    blocks: [usize; KEY_LEVELS],
    skips: [Option<KeyId>; KEY_LEVELS],
}

#[derive(Clone, Copy)]
struct AttemptBlock {
    group: usize,
    before: KeyId,
}

struct Node {
    initial: KeyId,
    remaining: usize,
    attempts: Vec<AttemptBlock>,
}

#[derive(Clone, Copy)]
struct Ready {
    node: u64,
    key: KeyId,
}

struct QueryFrame {
    group: usize,
    fallback: Option<usize>,
}

/// A producer's wake list is the preceding retry history restricted to its
/// dependents. Each dependent stores one prefix reference per producer, not
/// one entry per retry. Its first occurrence after the final producer selects
/// its successful FIFO position. Partial retries never parse the record again.
pub(super) struct DeferredCurves {
    initial: VecDeque<u64>,
    ordinal: usize,
    group: usize,
    current: Option<KeyId>,
    waiting: HashMap<u64, Vec<u64>>,
    nodes: HashMap<u64, Node>,
    ready: Vec<Ready>,
    keys: Vec<Key>,
    key_index: HashMap<(usize, usize), KeyId>,
    block_index: HashMap<(usize, usize, usize), usize>,
}

impl DeferredCurves {
    pub(super) fn has_next(&self) -> bool {
        !self.initial.is_empty() || !self.ready.is_empty()
    }

    pub(super) fn new(initial: Vec<u64>) -> Self {
        Self {
            initial: VecDeque::from(initial),
            ordinal: 0,
            group: 0,
            current: None,
            waiting: HashMap::new(),
            nodes: HashMap::new(),
            ready: Vec::new(),
            keys: Vec::new(),
            key_index: HashMap::new(),
            block_index: HashMap::new(),
        }
    }

    pub(super) fn next(&mut self, ctx: &DecodeContext<'_>) -> Result<Option<u64>, CodecError> {
        if let Some(node) = self.initial.pop_front() {
            let key = self.make_key(0, self.ordinal, None, ctx)?;
            self.ordinal += 1;
            self.current = Some(key);
            return Ok(Some(node));
        }
        let Some(last) = self.ready.pop() else {
            return Ok(None);
        };
        let first = if self.ready.is_empty() {
            last
        } else {
            let first = self.ready[0];
            self.ready[0] = last;
            let mut index = 0;
            loop {
                ctx.charge_work(1, "step deferred ready heap descent")?;
                let left = index * 2 + 1;
                if left >= self.ready.len() {
                    break;
                }
                let right = left + 1;
                let child = if right < self.ready.len()
                    && self.compare(self.ready[right].key, self.ready[left].key)? == Ordering::Less
                {
                    right
                } else {
                    left
                };
                if self.compare(self.ready[index].key, self.ready[child].key)? != Ordering::Greater {
                    break;
                }
                self.ready.swap(index, child);
                index = child;
            }
            first
        };
        self.current = Some(first.key);
        Ok(Some(first.node))
    }

    pub(super) fn register(
        &mut self,
        ctx: &DecodeContext<'_>,
        dependency: u64,
        dependent: u64,
        group_operation: &'static str,
        member_operation: &'static str,
    ) -> Result<(), CodecError> {
        let initial = self.current.ok_or_else(invalid_state)?;
        if let Some(node) = ctx.get_mut_hash_map(
            &mut self.nodes,
            &dependent,
            "step deferred curve node lookup",
        )? {
            node.remaining += 1;
        } else {
            ctx.insert_hash_map(
                &mut self.nodes,
                dependent,
                Node {
                    initial,
                    remaining: 1,
                    attempts: Vec::new(),
                },
                "step deferred curve nodes",
            )?;
        }
        ctx.push_hash_group(
            &mut self.waiting,
            dependency,
            dependent,
            group_operation,
            member_operation,
        )
    }

    pub(super) fn complete(
        &mut self,
        producer: u64,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let before = self.current.ok_or_else(invalid_state)?;
        self.group += 1;
        let Some(dependents) =
            ctx.get_hash_map(&self.waiting, &producer, "step deferred producer lookup")?
        else {
            return Ok(());
        };
        let count = dependents.len();
        // Keep the wake-list backing with its stage reservation after use.
        for index in 0..count {
            ctx.charge_work(1, "step deferred dependent traversal")?;
            let dependent = *ctx
                .get_hash_map(&self.waiting, &producer, "step deferred producer lookup")?
                .and_then(|values| values.get(index))
                .ok_or_else(invalid_state)?;
            let node = ctx
                .get_mut_hash_map(
                    &mut self.nodes,
                    &dependent,
                    "step deferred curve node lookup",
                )?
                .ok_or_else(invalid_state)?;
            // Repeated edges produce consecutive equal occurrences. Removing
            // those repetitions preserves every filtered wake list's order.
            if node.attempts.last().is_none_or(|last| last.group != self.group) {
                ctx.push_vec(
                    &mut node.attempts,
                    AttemptBlock {
                        group: self.group,
                        before,
                    },
                    "step deferred curve history",
                )?;
            }
            node.remaining -= 1;
            if node.remaining == 0 {
                let key = self.first_after(dependent, before, ctx)?;
                ctx.push_vec(
                    &mut self.ready,
                    Ready { node: dependent, key },
                    "step_deferred_curve_queue",
                )?;
                let mut index = self.ready.len() - 1;
                while index > 0 {
                    ctx.charge_work(1, "step deferred ready heap ascent")?;
                    let parent = (index - 1) / 2;
                    if self.compare(self.ready[index].key, self.ready[parent].key)? != Ordering::Less {
                        break;
                    }
                    self.ready.swap(index, parent);
                    index = parent;
                }
            }
        }
        Ok(())
    }

    fn node(&self, node: u64, ctx: &DecodeContext<'_>) -> Result<&Node, CodecError> {
        ctx.get_hash_map(&self.nodes, &node, "step deferred curve node lookup")?
            .ok_or_else(invalid_state)
    }

    fn key(&self, key: KeyId) -> Result<&Key, CodecError> {
        self.keys.get(key.0).ok_or_else(invalid_state)
    }

    fn block_id(
        &mut self,
        block: (usize, usize, usize),
        ctx: &DecodeContext<'_>,
    ) -> Result<usize, CodecError> {
        if let Some(id) = ctx.get_hash_map(
            &self.block_index,
            &block,
            "step deferred key prefix lookup",
        )? {
            return Ok(*id);
        }
        let id = self.block_index.len() + 1;
        ctx.insert_hash_map(
            &mut self.block_index,
            block,
            id,
            "step deferred key prefixes",
        )?;
        Ok(id)
    }

    fn make_key(
        &mut self,
        group: usize,
        ordinal: usize,
        tail: Option<KeyId>,
        ctx: &DecodeContext<'_>,
    ) -> Result<KeyId, CodecError> {
        let index_key = (group, tail.map_or(ordinal, |key| key.0));
        if let Some(id) = ctx.get_hash_map(
            &self.key_index,
            &index_key,
            "step deferred event key lookup",
        )? {
            return Ok(*id);
        }
        let mut key = Key {
            group,
            ordinal,
            tail,
            blocks: [0; KEY_LEVELS],
            skips: [None; KEY_LEVELS],
        };
        key.blocks[0] = self.block_id((0, group, ordinal), ctx)?;
        key.skips[0] = tail;
        for level in 1..KEY_LEVELS {
            let Some(right) = key.skips[level - 1] else {
                break;
            };
            let right = self.key(right)?;
            let right_block = right.blocks[level - 1];
            if right_block == 0 {
                break;
            }
            let skip = right.skips[level - 1];
            key.blocks[level] =
                self.block_id((level, key.blocks[level - 1], right_block), ctx)?;
            key.skips[level] = skip;
        }
        let id = KeyId(self.keys.len());
        ctx.push_vec(&mut self.keys, key, "step deferred event keys")?;
        ctx.insert_hash_map(
            &mut self.key_index,
            index_key,
            id,
            "step deferred event key index",
        )?;
        Ok(id)
    }

    fn compare(&self, mut left: KeyId, mut right: KeyId) -> Result<Ordering, CodecError> {
        if left == right {
            return Ok(Ordering::Equal);
        }
        for level in (0..KEY_LEVELS).rev() {
            let a = self.key(left)?;
            let b = self.key(right)?;
            if a.blocks[level] != 0 && a.blocks[level] == b.blocks[level] {
                match (a.skips[level], b.skips[level]) {
                    (Some(a), Some(b)) => {
                        left = a;
                        right = b;
                    }
                    (None, None) => return Ok(Ordering::Equal),
                    (None, Some(_)) => return Ok(Ordering::Less),
                    (Some(_), None) => return Ok(Ordering::Greater),
                }
            }
        }
        let a = self.key(left)?;
        let b = self.key(right)?;
        Ok((a.group, a.ordinal).cmp(&(b.group, b.ordinal)))
    }

    fn first_after(
        &mut self,
        node: u64,
        after: KeyId,
        ctx: &DecodeContext<'_>,
    ) -> Result<KeyId, CodecError> {
        let initial = self.node(node, ctx)?.initial;
        let mut count = self.node(node, ctx)?.attempts.len();
        let mut lower = Some(after);
        let mut upper = None;
        let mut storage = ctx.reserve_scoped(0, "step deferred history query storage")?;
        let mut frames = Vec::new();
        let mut result;
        loop {
            ctx.charge_work(1, "step deferred history query")?;
            if let Some(high) = upper {
                if self.compare(initial, high)? != Ordering::Less {
                    result = None;
                    break;
                }
            }
            let Some(low) = lower else {
                result = Some(initial);
                break;
            };
            if self.compare(initial, low)? == Ordering::Greater {
                result = Some(initial);
                break;
            }
            if let Some(high) = upper {
                if self.compare(low, high)? != Ordering::Less {
                    result = None;
                    break;
                }
            }
            let low_group = self.key(low)?.group;
            let high_group = upper.map(|key| self.key(key).map(|key| key.group)).transpose()?;
            let mut start = 0;
            let mut end = count;
            while start < end {
                ctx.charge_work(1, "step deferred history block lookup")?;
                let mid = start + (end - start) / 2;
                if self.node(node, ctx)?.attempts[mid].group < low_group {
                    start = mid + 1;
                } else {
                    end = mid;
                }
            }
            let block = self.node(node, ctx)?.attempts.get(start).copied().filter(|_| start < count);
            let Some(block) = block.filter(|block| high_group.is_none_or(|high| block.group <= high)) else {
                result = None;
                break;
            };
            let child_upper = self.child_upper(block, upper)?;
            if block.group > low_group {
                result = if self.compare(initial, child_upper)? == Ordering::Less {
                    Some(self.make_key(block.group, 0, Some(initial), ctx)?)
                } else {
                    None
                };
                break;
            }
            let fallback = self.node(node, ctx)?.attempts.get(start + 1).copied()
                .filter(|_| start + 1 < count)
                .filter(|block| high_group.is_none_or(|high| block.group <= high));
            let fallback = if let Some(block) = fallback {
                (self.compare(initial, self.child_upper(block, upper)?)? == Ordering::Less)
                    .then_some(block.group)
            } else {
                None
            };
            storage.with_storage(|| ctx.push_vec(
                &mut frames,
                QueryFrame { group: block.group, fallback },
                "step deferred history query frames",
            ))?;
            count = start;
            lower = self.key(low)?.tail;
            upper = Some(child_upper);
        }
        while let Some(frame) = frames.pop() {
            ctx.charge_work(1, "step deferred history query unwind")?;
            result = if let Some(tail) = result {
                Some(self.make_key(frame.group, 0, Some(tail), ctx)?)
            } else if let Some(group) = frame.fallback {
                Some(self.make_key(group, 0, Some(initial), ctx)?)
            } else {
                None
            };
        }
        result.ok_or_else(invalid_state)
    }

    fn child_upper(&self, block: AttemptBlock, upper: Option<KeyId>) -> Result<KeyId, CodecError> {
        let Some(upper) = upper else {
            return Ok(block.before);
        };
        let key = self.key(upper)?;
        if key.group != block.group {
            return Ok(block.before);
        }
        let upper = key.tail.ok_or_else(invalid_state)?;
        Ok(if self.compare(upper, block.before)? == Ordering::Less {
            upper
        } else {
            block.before
        })
    }
}

fn invalid_state() -> CodecError {
    CodecError::malformed("STEP deferred curve history is inconsistent")
}

#[cfg(test)]
mod tests;
