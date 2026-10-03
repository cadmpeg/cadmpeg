// SPDX-License-Identifier: Apache-2.0
//! Fallible linear search and byte comparison under the caller's work budget.

use super::iter_source::IterSource;
use super::{u64_from_index, DecodeContext};
use crate::CodecError;

/// An iterator whose upper visit bound was charged before construction.
/// Its private source cannot be cloned or extracted for unpaid replay.
#[derive(Debug)]
pub struct AdmittedIter<I> {
    source: I,
}

impl<I: Iterator> Iterator for AdmittedIter<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.source.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.source.size_hint()
    }
}

impl<I: DoubleEndedIterator> DoubleEndedIterator for AdmittedIter<I> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.source.next_back()
    }
}

impl<I: ExactSizeIterator> ExactSizeIterator for AdmittedIter<I> {}

impl<'a, T> AdmittedIter<std::slice::Iter<'a, T>> {
    /// Consumes admission for overlapping windows; each window has child work.
    pub fn windows(self, size: std::num::NonZeroUsize) -> AdmittedIter<std::slice::Windows<'a, T>> {
        AdmittedIter { source: self.source.as_slice().windows(size.get()) }
    }

    /// Consumes admission for non-overlapping chunks.
    pub fn chunks(self, size: std::num::NonZeroUsize) -> AdmittedIter<std::slice::Chunks<'a, T>> {
        AdmittedIter { source: self.source.as_slice().chunks(size.get()) }
    }
}

impl DecodeContext<'_> {
    /// Admits the collection traversal before visiting any element. Iterator adapters
    /// run on the admitted result; callbacks admit their own child work.
    pub fn admit_iter<'values, S: IterSource + ?Sized>(
        &self,
        values: &'values S,
        operation: &'static str,
    ) -> Result<AdmittedIter<S::Iter<'values>>, CodecError> {
        self.charge_work(u64_from_index(values.visit_bound()), operation)?;
        Ok(AdmittedIter { source: values.source_iter() })
    }

    /// Returns the first matching position. Each visited slot is admitted before
    /// the predicate runs. The predicate admits its own input-sized child work.
    pub fn position_by<T>(
        &self,
        values: &[T],
        mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        for (index, value) in values.iter().enumerate() {
            self.charge_work(1, operation)?;
            if predicate(value)? {
                return Ok(Some(index));
            }
        }
        Ok(None)
    }

    /// Compares equal-length byte slices after admitting the complete scan.
    /// Unequal lengths need no input-sized comparison.
    pub fn equal_bytes(
        &self,
        left: &[u8],
        right: &[u8],
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if left.len() != right.len() {
            return Ok(false);
        }
        self.charge_work(u64_from_index(left.len()), operation)?;
        Ok(left == right)
    }
}

#[cfg(test)]
mod tests {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    #[test]
    fn iteration_charges_before_first_element() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("context");
        let visited = std::cell::Cell::new(0);
        let values = [1, 2, 3];
        let source = ctx.admit_iter(&values, "iteration").expect("admission");
        let mut admitted = source.inspect(|_| visited.set(visited.get() + 1));
        assert_eq!(visited.get(), 0);
        assert_eq!(admitted.next(), Some(&1));
        assert_eq!(visited.get(), 1);
        assert!(admitted.any(|value| *value == 3));
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else {
            panic!("resource refusal");
        };
        // Three source slots count once, before iteration starts.
        assert_eq!(limit.used, 3);
    }

    #[test]
    fn iteration_refusal_preserves_error_and_visits_nothing() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let visited = std::cell::Cell::new(0);
        let result = ctx.admit_iter(&[1, 2, 3], "iteration");
        let error = result.map(|source| source.inspect(|_| visited.set(visited.get() + 1)))
            .expect_err("refusal");
        assert_eq!(visited.get(), 0);
        let CodecError::ResourceLimit(limit) = error else { panic!("resource refusal") };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!(limit.operation, "iteration");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    }

    #[test]
    fn iteration_sources_charge_text_bytes_and_hash_capacity() {
        use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("context");
        let vector = vec![1, 2];
        let boxed = vec![1, 2].into_boxed_slice();
        let queue = VecDeque::from([1, 2]);
        let tree = BTreeMap::from([(1, 2)]);
        let ordered = BTreeSet::from([1, 2]);
        let mut map = HashMap::with_capacity(100);
        map.insert(1, 2);
        let mut set = HashSet::with_capacity(100);
        set.insert(1);
        assert_eq!(ctx.admit_iter(&vector, "vector").expect("vector").count(), 2);
        assert_eq!(ctx.admit_iter(&boxed, "box").expect("box").count(), 2);
        assert_eq!(ctx.admit_iter(&queue, "queue").expect("queue").count(), 2);
        assert_eq!(ctx.admit_iter(&tree, "tree").expect("tree").count(), 1);
        assert_eq!(ctx.admit_iter(&ordered, "set").expect("set").count(), 2);
        assert_eq!(ctx.admit_iter("é", "text").expect("text").count(), 1);
        assert_eq!(ctx.admit_iter("é".as_bytes(), "bytes").expect("bytes").count(), 2);
        assert_eq!(ctx.admit_iter(&map, "map").expect("map").count(), 1);
        assert_eq!(ctx.admit_iter(&set, "set").expect("set").count(), 1);
        let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else {
            panic!("resource refusal");
        };
        // Nine collection slots, four text bytes, and both hash capacity scans.
        assert_eq!(limit.used, 13 + super::u64_from_index(map.capacity() + set.capacity()));
    }

    #[test]
    fn iteration_slice_adapters_consume_admission() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("context");
        let size = std::num::NonZeroUsize::new(2).expect("nonzero");
        let values = [1, 2, 3];
        let windows: Vec<_> = ctx.admit_iter(&values, "windows").expect("admission")
            .windows(size).collect();
        assert_eq!(windows, [&[1, 2][..], &[2, 3][..]]);
        let chunks: Vec<_> = ctx.admit_iter(&values, "chunks").expect("admission")
            .chunks(size).rev().collect();
        assert_eq!(chunks, [&[3][..], &[1, 2][..]]);
    }

    #[test]
    fn scan_search_admits_before_predicate_and_preserves_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation is admitted");
        let mut visited = 0;
        let error = ctx
            .position_by(
                &[1, 2],
                |_| {
                    visited += 1;
                    Ok(false)
                },
                "search",
            )
            .expect_err("test operation must refuse");
        assert_eq!(visited, 1);
        let CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_search_stops_at_match_and_propagates_child_error() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation is admitted");
        assert_eq!(
            ctx.position_by(&[1, 2, 3], |value| Ok(*value == 2), "search")
                .expect("test operation is admitted"),
            Some(1)
        );
        assert_eq!(
            ctx.position_by(&[1], |_| Ok(false), "search")
                .expect("test operation is admitted"),
            None
        );
        let error = ctx
            .position_by(
                &[1],
                |_| Err(ctx.refuse_codec_limit("child", 0, 1)),
                "search",
            )
            .expect_err("test operation must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("child refusal");
        };
        assert_eq!(limit.operation, "child");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_byte_equality_admits_before_comparison() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("test operation is admitted");
        assert!(!ctx
            .equal_bytes(b"a", b"ab", "compare")
            .expect("test operation is admitted"));
        let error = ctx
            .equal_bytes(b"ab", b"ab", "compare")
            .expect_err("test operation must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("work refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }

    #[test]
    fn scan_byte_equality_preserves_exact_results() {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("test operation is admitted");
        assert!(ctx
            .equal_bytes(b"ab", b"ab", "compare")
            .expect("test operation is admitted"));
        assert!(!ctx
            .equal_bytes(b"ab", b"ac", "compare")
            .expect("test operation is admitted"));
        assert!(ctx
            .equal_bytes(b"", b"", "compare")
            .expect("test operation is admitted"));
    }
}
