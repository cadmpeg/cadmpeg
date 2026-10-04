// SPDX-License-Identifier: Apache-2.0
//! Fallible linear search and byte comparison under the caller's work budget.

use super::iter_source::{IterSource, VisitBoundError};
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
        AdmittedIter {
            source: self.source.as_slice().windows(size.get()),
        }
    }

    /// Consumes admission for non-overlapping chunks.
    pub fn chunks(self, size: std::num::NonZeroUsize) -> AdmittedIter<std::slice::Chunks<'a, T>> {
        AdmittedIter {
            source: self.source.as_slice().chunks(size.get()),
        }
    }
}

impl<'text> AdmittedIter<std::str::Chars<'text>> {
    /// Consumes byte admission for the remaining UTF-16 units.
    /// A UTF-16 unit count does not exceed the UTF-8 byte count.
    pub fn encode_utf16(self) -> AdmittedIter<std::str::EncodeUtf16<'text>> {
        AdmittedIter {
            source: self.source.as_str().encode_utf16(),
        }
    }
}

impl DecodeContext<'_> {
    /// Finds a non-empty byte pattern after admitting both input extents.
    /// An empty pattern has no matches.
    pub fn find_bytes(
        &self,
        haystack: &[u8],
        needle: &[u8],
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        if needle.is_empty() {
            self.charge_work(0, operation)?;
            return Ok(None);
        }
        let work = self.cost_sum(
            u64_from_index(haystack.len()),
            u64_from_index(needle.len()),
            operation,
        )?;
        self.charge_work(work, operation)?;
        Ok(memchr::memmem::find(haystack, needle))
    }

    /// Finds the last non-empty byte pattern after admitting both input extents.
    pub fn rfind_bytes(
        &self,
        haystack: &[u8],
        needle: &[u8],
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        if needle.is_empty() {
            self.charge_work(0, operation)?;
            return Ok(None);
        }
        let work = self.cost_sum(
            u64_from_index(haystack.len()),
            u64_from_index(needle.len()),
            operation,
        )?;
        self.charge_work(work, operation)?;
        Ok(memchr::memmem::rfind(haystack, needle))
    }

    /// Finds an absolute offset within a bounded byte range.
    /// An invalid range or empty pattern has no matches.
    pub fn find_bytes_in(
        &self,
        haystack: &[u8],
        needle: &[u8],
        start: usize,
        end: usize,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        self.charge_work(0, operation)?;
        let Some(window) = haystack.get(start..end) else {
            return Ok(None);
        };
        Ok(self
            .find_bytes(window, needle, operation)?
            .map(|offset| start + offset))
    }

    /// Finds an absolute offset at or after the supplied starting position.
    pub fn find_bytes_from(
        &self,
        haystack: &[u8],
        needle: &[u8],
        start: usize,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        self.find_bytes_in(haystack, needle, start, haystack.len(), operation)
    }

    /// Tests byte-pattern membership through the admitted forward search.
    pub fn contains_bytes(
        &self,
        haystack: &[u8],
        needle: &[u8],
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        Ok(self.find_bytes(haystack, needle, operation)?.is_some())
    }

    /// Admits needle construction and all non-overlapping byte-pattern searches.
    /// The borrowed searcher uses constant storage and cannot replay its admission.
    pub fn find_bytes_iter<'bytes>(
        &self,
        haystack: &'bytes [u8],
        needle: &'bytes [u8],
        operation: &'static str,
    ) -> Result<AdmittedIter<impl Iterator<Item = usize> + std::fmt::Debug + 'bytes>, CodecError>
    {
        let search = if needle.is_empty() {
            self.charge_work(0, operation)?;
            None
        } else {
            let work = self.cost_sum(
                u64_from_index(haystack.len()),
                u64_from_index(needle.len()),
                operation,
            )?;
            self.charge_work(work, operation)?;
            Some(memchr::memmem::find_iter(haystack, needle))
        };
        Ok(AdmittedIter {
            source: search.into_iter().flatten(),
        })
    }

    /// Validates borrowed UTF-8 after admitting every input byte.
    /// The inner result preserves the standard validation error.
    pub fn validate_utf8<'bytes>(
        &self,
        bytes: &'bytes [u8],
        operation: &'static str,
    ) -> Result<Result<&'bytes str, std::str::Utf8Error>, CodecError> {
        self.charge_work(u64_from_index(bytes.len()), operation)?;
        Ok(std::str::from_utf8(bytes))
    }

    /// Compares equal-length text with ASCII case folding after admission.
    /// Unequal lengths require no scan.
    pub fn eq_ignore_ascii_case(
        &self,
        left: &str,
        right: &str,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        if left.len() != right.len() {
            return Ok(false);
        }
        self.charge_work(u64_from_index(left.len()), operation)?;
        Ok(left.eq_ignore_ascii_case(right))
    }

    /// Admits the source traversal before visiting any element. Iterator adapters
    /// run on the admitted result; callbacks admit their own child work.
    pub fn admit_iter<S: IterSource>(
        &self,
        values: S,
        operation: &'static str,
    ) -> Result<AdmittedIter<S::Iter>, super::ResourceLimit> {
        let bound = match values.visit_bound() {
            Ok(bound) => bound,
            Err(VisitBoundError::ExceedsU64) => {
                return Err(self.budget.work_bound_overflow_limit(operation));
            }
        };
        self.charge_work_limit(bound, operation)?;
        Ok(AdmittedIter {
            source: values.source_iter(),
        })
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

    /// Tests for a match, charging only slots visited by position search.
    pub fn any_by<T>(
        &self,
        values: &[T],
        predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        Ok(self.position_by(values, predicate, operation)?.is_some())
    }

    /// Tests every value until a predicate fails.
    pub fn all_by<T>(
        &self,
        values: &[T],
        mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<bool, CodecError> {
        Ok(self
            .position_by(
                values,
                |value| predicate(value).map(|matches| !matches),
                operation,
            )?
            .is_none())
    }

    /// Borrows the first matching value without retaining new storage.
    pub fn find_by<'values, T>(
        &self,
        values: &'values [T],
        predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        Ok(self
            .position_by(values, predicate, operation)?
            .and_then(|index| values.get(index)))
    }

    /// Maps values until a result is present; callbacks admit child construction.
    pub fn find_map<T, U>(
        &self,
        values: &[T],
        mut map: impl FnMut(&T) -> Result<Option<U>, CodecError>,
        operation: &'static str,
    ) -> Result<Option<U>, CodecError> {
        let mut found = None;
        let _position = self.position_by(
            values,
            |value| {
                found = map(value)?;
                Ok(found.is_some())
            },
            operation,
        )?;
        Ok(found)
    }

    /// Counts a sealed source after admitting its complete traversal.
    pub fn count<S: IterSource>(
        &self,
        values: S,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        Ok(self.admit_iter(values, operation)?.count())
    }

    /// Folds all slice slots, charging each visit before the fallible callback.
    pub fn fold<'values, T, A>(
        &self,
        values: &'values [T],
        initial: A,
        mut fold: impl FnMut(A, &'values T) -> Result<A, CodecError>,
        operation: &'static str,
    ) -> Result<A, CodecError> {
        let mut result = initial;
        for value in values {
            self.charge_work(1, operation)?;
            result = fold(result, value)?;
        }
        Ok(result)
    }

    /// Sums fallibly mapped values through the same admitted fold.
    /// The callback owns conversion and checked addition for its result type.
    pub fn sum<T, A: super::text::TextScalar + Default>(
        &self,
        values: &[T],
        add: impl FnMut(A, &T) -> Result<A, CodecError>,
        operation: &'static str,
    ) -> Result<A, CodecError> {
        self.fold(values, A::default(), add, operation)
    }

    /// Selects the first least value; comparisons admit their own child work.
    pub fn min_by<'values, T>(
        &self,
        values: &'values [T],
        mut compare: impl FnMut(&T, &T) -> Result<std::cmp::Ordering, CodecError>,
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        self.fold(
            values,
            None,
            |minimum, value| match minimum {
                Some(previous) if compare(previous, value)? != std::cmp::Ordering::Greater => {
                    Ok(minimum)
                }
                _ => Ok(Some(value)),
            },
            operation,
        )
    }

    /// Selects the last greatest value; comparisons admit their own child work.
    pub fn max_by<'values, T>(
        &self,
        values: &'values [T],
        mut compare: impl FnMut(&T, &T) -> Result<std::cmp::Ordering, CodecError>,
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        self.fold(
            values,
            None,
            |maximum, value| match maximum {
                Some(previous) if compare(previous, value)? == std::cmp::Ordering::Greater => {
                    Ok(maximum)
                }
                _ => Ok(Some(value)),
            },
            operation,
        )
    }

    /// Selects the least key through fallible extraction and comparison.
    pub fn min_by_key<'values, T, K>(
        &self,
        values: &'values [T],
        mut key: impl FnMut(&T) -> Result<K, CodecError>,
        mut compare: impl FnMut(&K, &K) -> Result<std::cmp::Ordering, CodecError>,
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        let selected = self.fold(
            values,
            None,
            |selected: Option<(&T, K)>, value| {
                let current = key(value)?;
                match selected {
                    Some((previous, previous_key))
                        if compare(&previous_key, &current)? != std::cmp::Ordering::Greater =>
                    {
                        Ok(Some((previous, previous_key)))
                    }
                    _ => Ok(Some((value, current))),
                }
            },
            operation,
        )?;
        Ok(selected.map(|(value, _)| value))
    }

    /// Selects the greatest key through fallible extraction and comparison.
    pub fn max_by_key<'values, T, K>(
        &self,
        values: &'values [T],
        mut key: impl FnMut(&T) -> Result<K, CodecError>,
        mut compare: impl FnMut(&K, &K) -> Result<std::cmp::Ordering, CodecError>,
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        let selected = self.fold(
            values,
            None,
            |selected: Option<(&T, K)>, value| {
                let current = key(value)?;
                match selected {
                    Some((previous, previous_key))
                        if compare(&previous_key, &current)? == std::cmp::Ordering::Greater =>
                    {
                        Ok(Some((previous, previous_key)))
                    }
                    _ => Ok(Some((value, current))),
                }
            },
            operation,
        )?;
        Ok(selected.map(|(value, _)| value))
    }

    /// Selects the first least value through charged complete-value comparison.
    pub fn min<'values, T: super::cost::DecodeCost + Ord>(
        &self,
        values: &'values [T],
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        self.min_by(
            values,
            |left, right| self.compare(left, right, operation),
            operation,
        )
    }

    /// Selects the last greatest value through charged complete-value comparison.
    pub fn max<'values, T: super::cost::DecodeCost + Ord>(
        &self,
        values: &'values [T],
        operation: &'static str,
    ) -> Result<Option<&'values T>, CodecError> {
        self.max_by(
            values,
            |left, right| self.compare(left, right, operation),
            operation,
        )
    }

    /// Searches from the end; callbacks admit input-sized child work.
    pub fn rposition_by<T>(
        &self,
        values: &[T],
        mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<Option<usize>, CodecError> {
        for (reverse_index, value) in self.admit_iter(values, operation)?.rev().enumerate() {
            if predicate(value)? {
                return Ok(Some(values.len() - reverse_index - 1));
            }
        }
        Ok(None)
    }

    /// Finds the first false predicate in a true-prefix slice.
    /// Each visited slot is charged before its fallible predicate runs.
    pub fn partition_point<T>(
        &self,
        values: &[T],
        mut predicate: impl FnMut(&T) -> Result<bool, CodecError>,
        operation: &'static str,
    ) -> Result<usize, CodecError> {
        let mut lower = 0;
        let mut upper = values.len();
        while lower < upper {
            self.charge_work(1, operation)?;
            let middle = lower + (upper - lower) / 2;
            if predicate(&values[middle])? {
                lower = middle + 1;
            } else {
                upper = middle;
            }
        }
        Ok(lower)
    }

    /// Searches a sorted slice. The comparator admits its own child work.
    pub fn binary_search_by<T>(
        &self,
        values: &[T],
        mut compare: impl FnMut(&T) -> Result<std::cmp::Ordering, CodecError>,
        operation: &'static str,
    ) -> Result<Result<usize, usize>, CodecError> {
        let index = self.partition_point(
            values,
            |value| Ok(compare(value)? == std::cmp::Ordering::Less),
            operation,
        )?;
        if let Some(value) = values.get(index) {
            self.charge_work(1, operation)?;
            if compare(value)? == std::cmp::Ordering::Equal {
                return Ok(Ok(index));
            }
        }
        Ok(Err(index))
    }

    /// Searches a sorted slice with complete-value comparison charges.
    pub fn binary_search<T: super::cost::DecodeCost + Ord>(
        &self,
        values: &[T],
        key: &T,
        operation: &'static str,
    ) -> Result<Result<usize, usize>, CodecError> {
        self.binary_search_by(
            values,
            |value| self.compare(value, key, operation),
            operation,
        )
    }

    /// Searches sorted projected keys; extraction admits its child work.
    pub fn binary_search_by_key<T, K: super::cost::DecodeCost + Ord>(
        &self,
        values: &[T],
        key: &K,
        mut project: impl FnMut(&T) -> Result<K, CodecError>,
        operation: &'static str,
    ) -> Result<Result<usize, usize>, CodecError> {
        self.binary_search_by(
            values,
            |value| self.compare(&project(value)?, key, operation),
            operation,
        )
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
    mod iteration;
    use crate::decode::iter_source::IterSource;
    use crate::decode::ResourceFailure;
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use crate::CodecError;

    fn assert_range_overflow<S: IterSource>(source: S, prior_work: u64) {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        ctx.charge_work(prior_work, "prior range work")
            .expect("prior work fits");

        let Err(first) = ctx.admit_iter(source, "range bound") else {
            panic!("unrepresentable range bound refuses");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.reason, ResourceFailure::BudgetExceeded);
        assert_eq!(first.limit, u64::MAX);
        assert_eq!(first.used, prior_work);
        assert_eq!(first.additional, u64::MAX);
        assert_eq!(first.operation, "range bound");
        assert_eq!(ctx.resource_refusal(), Some(first));

        let CodecError::ResourceLimit(repeated) = ctx
            .charge_work(0, "later work")
            .expect_err("overflow refusal fuses the work budget")
        else {
            panic!("resource refusal");
        };
        assert_eq!(repeated, first);
    }

    #[test]
    fn empty_needle_is_never_a_match() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(
            ctx.find_bytes(b"abc", b"", "search").expect("admission"),
            None
        );
        assert_eq!(
            ctx.find_bytes_from(b"abc", b"", 1, "search")
                .expect("admission"),
            None
        );
        assert_eq!(
            ctx.find_bytes_in(b"abc", b"", 0, 3, "search")
                .expect("admission"),
            None
        );
        assert!(!ctx
            .contains_bytes(b"abc", b"", "search")
            .expect("admission"));
        assert_eq!(
            ctx.find_bytes_iter(b"abc", b"", "search")
                .expect("admission")
                .collect::<Vec<_>>(),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn finds_absolute_and_ranged_offsets() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let haystack = b"xxabcxxabc";
        assert_eq!(
            ctx.find_bytes(haystack, b"abc", "search")
                .expect("admission"),
            Some(2)
        );
        assert_eq!(
            ctx.find_bytes_from(haystack, b"abc", 3, "search")
                .expect("admission"),
            Some(7)
        );
        assert_eq!(
            ctx.find_bytes_in(haystack, b"abc", 3, 10, "search")
                .expect("admission"),
            Some(7)
        );
        assert_eq!(
            ctx.find_bytes_in(haystack, b"abc", 3, 6, "search")
                .expect("admission"),
            None
        );
        assert!(ctx
            .contains_bytes(haystack, b"abc", "search")
            .expect("admission"));
        assert_eq!(
            ctx.find_bytes_iter(haystack, b"abc", "search")
                .expect("admission")
                .collect::<Vec<_>>(),
            [2, 7]
        );
    }

    #[test]
    fn charged_byte_searches_preserve_binary_offsets_and_nonoverlapping_matches() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let bytes = b"\xffabab\0";
        assert_eq!(
            ctx.find_bytes(bytes, b"ab", "find").expect("admission"),
            Some(1)
        );
        assert_eq!(
            ctx.rfind_bytes(bytes, b"ab", "reverse").expect("admission"),
            Some(3)
        );
        assert_eq!(
            ctx.find_bytes_from(bytes, b"ab", 2, "from")
                .expect("admission"),
            Some(3)
        );
        assert_eq!(
            ctx.find_bytes_in(bytes, b"ab", 2, 5, "range")
                .expect("admission"),
            Some(3)
        );
        assert_eq!(
            ctx.find_bytes_in(bytes, b"ab", 2, 4, "range")
                .expect("admission"),
            None
        );
        assert!(ctx
            .contains_bytes(bytes, b"\0", "contains")
            .expect("admission"));
        assert_eq!(
            ctx.find_bytes_iter(bytes, b"ab", "matches")
                .expect("admission")
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert_eq!(
            ctx.find_bytes_iter(b"aaaaa", b"aa", "matches")
                .expect("admission")
                .collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(
            ctx.find_bytes_from(bytes, b"ab", usize::MAX, "invalid")
                .expect("no range"),
            None
        );
        assert_eq!(
            ctx.find_bytes_in(bytes, b"ab", 3, 2, "invalid")
                .expect("no range"),
            None
        );
        assert_eq!(
            ctx.find_bytes_in(bytes, b"ab", 0, usize::MAX, "invalid")
                .expect("no range"),
            None
        );
    }

    #[test]
    fn charged_byte_searches_admit_both_extents_without_storage() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Three searches each admit six haystack bytes and two needle bytes.
        policy.limits.max_work_units = 24;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            ctx.find_bytes(b"ababab", b"ab", "forward")
                .expect("admission"),
            Some(0)
        );
        assert_eq!(
            ctx.rfind_bytes(b"ababab", b"ab", "reverse")
                .expect("admission"),
            Some(4)
        );
        assert_eq!(
            ctx.find_bytes_iter(b"ababab", b"ab", "iterator")
                .expect("admission")
                .count(),
            3
        );
        let CodecError::ResourceLimit(limit) = ctx.charge_work(1, "probe").expect_err("exact work")
        else {
            panic!("refusal")
        };
        assert_eq!(limit.used, 24);
    }

    #[test]
    fn charged_byte_searches_refuse_before_search_and_keep_original_refusal() {
        let arena = DecodeArena::new();
        for iterator in [false, true] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 7;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
            let error = if iterator {
                ctx.find_bytes_iter(b"ababab", b"ab", "iterator")
                    .expect_err("refusal")
            } else {
                ctx.find_bytes(b"ababab", b"ab", "forward")
                    .expect_err("refusal")
            };
            let CodecError::ResourceLimit(first) = error else {
                panic!("refusal")
            };
            assert_eq!(first.used, 0);
            assert_eq!(first.additional, 8);
            let CodecError::ResourceLimit(second) = ctx
                .find_bytes_in(b"a", b"a", usize::MAX, 0, "invalid")
                .expect_err("fused")
            else {
                panic!("refusal")
            };
            assert_eq!(first, second);
        }
    }

    #[test]
    fn empty_byte_patterns_have_no_matches_or_scan_work() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(ctx.find_bytes(b"abc", b"", "empty").expect("no scan"), None);
        assert_eq!(
            ctx.rfind_bytes(b"abc", b"", "empty").expect("no scan"),
            None
        );
        assert!(!ctx.contains_bytes(b"abc", b"", "empty").expect("no scan"));
        assert_eq!(
            ctx.find_bytes_iter(b"abc", b"", "empty")
                .expect("no scan")
                .count(),
            0
        );
        let CodecError::ResourceLimit(first) = ctx.charge_work(1, "refusal").expect_err("refusal")
        else {
            panic!("refusal")
        };
        let CodecError::ResourceLimit(second) = ctx
            .find_bytes_iter(b"abc", b"", "empty")
            .expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }

    #[test]
    fn admitted_utf16_adapter_consumes_only_remaining_text() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut characters = ctx.admit_iter("A😀é", "text").expect("admission");
        assert_eq!(characters.next(), Some('A'));
        assert_eq!(
            characters.encode_utf16().collect::<Vec<_>>(),
            "😀é".encode_utf16().collect::<Vec<_>>()
        );
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("refusal")
        };
        // One admission covers the seven original UTF-8 bytes, including the consumed prefix.
        assert_eq!(limit.used, 7);
    }

    #[test]
    fn charged_text_validation_preserves_standard_results_and_counts_bytes() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        assert_eq!(
            ctx.validate_utf8("é".as_bytes(), "UTF-8")
                .expect("admission"),
            Ok("é")
        );
        let invalid = [0xff, b'a'];
        let invalid_error = ctx
            .validate_utf8(&invalid, "UTF-8")
            .expect("admission")
            .expect_err("invalid leading byte");
        assert_eq!(invalid_error.valid_up_to(), 0);
        assert_eq!(invalid_error.error_len(), Some(1));
        assert!(ctx
            .eq_ignore_ascii_case("Ab", "aB", "ASCII case")
            .expect("admission"));
        assert!(!ctx
            .eq_ignore_ascii_case("é", "É", "ASCII case")
            .expect("admission"));
        assert!(!ctx
            .eq_ignore_ascii_case("a", "ab", "ASCII case")
            .expect("length mismatch"));
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("resource refusal");
        };
        // Two two-byte UTF-8 scans and two two-byte ASCII comparisons.
        assert_eq!(limit.used, 8);
    }

    #[test]
    fn charged_text_validation_refuses_before_standard_operation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let error = ctx
            .validate_utf8(&[0xff, 0xff], "UTF-8")
            .expect_err("work refusal precedes invalid UTF-8");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal");
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!(limit.operation, "UTF-8");
        let error = ctx
            .eq_ignore_ascii_case("ab", "AB", "ASCII case")
            .expect_err("fused refusal");
        let CodecError::ResourceLimit(repeated) = error else {
            panic!("resource refusal");
        };
        assert_eq!(repeated, limit);
    }

    #[test]
    fn iteration_charges_before_first_element() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let visited = std::cell::Cell::new(0);
        let values = [1, 2, 3];
        let source = ctx.admit_iter(&values, "iteration").expect("admission");
        let mut admitted = source.inspect(|_| visited.set(visited.get() + 1));
        assert_eq!(visited.get(), 0);
        assert_eq!(admitted.next(), Some(&1));
        assert_eq!(visited.get(), 1);
        assert!(admitted.any(|value| *value == 3));
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
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
        let error = result
            .map(|source| source.inspect(|_| visited.set(visited.get() + 1)))
            .expect_err("refusal");
        assert_eq!(visited.get(), 0);
        let limit = error;
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert_eq!(limit.operation, "iteration");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    }

    #[test]
    fn unsigned_range_admission_charges_exact_bound_and_refuses_one_below() {
        macro_rules! assert_exact_and_one_below {
            ($source:expr) => {{
                let source = $source;
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 3;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
                assert_eq!(
                    ctx.admit_iter(&source, "integer range")
                        .expect("exact range bound fits")
                        .collect::<Vec<_>>(),
                    [0, 1, 2]
                );

                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 2;
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
                let refusal = ctx
                    .admit_iter(&source, "integer range")
                    .expect_err("one unit below the range bound refuses");
                assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
                assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
                assert_eq!(refusal.used, 0);
                assert_eq!(refusal.additional, 3);
                assert_eq!(ctx.resource_refusal(), Some(refusal));
            }};
        }

        assert_exact_and_one_below!(0_u8..3_u8);
        assert_exact_and_one_below!(0_u8..=2_u8);
        assert_exact_and_one_below!(0_u16..3_u16);
        assert_exact_and_one_below!(0_u16..=2_u16);
        assert_exact_and_one_below!(0_u32..3_u32);
        assert_exact_and_one_below!(0_u32..=2_u32);
        assert_exact_and_one_below!(0_u64..3_u64);
        assert_exact_and_one_below!(0_u64..=2_u64);
        assert_exact_and_one_below!(0_u128..3_u128);
        assert_exact_and_one_below!(0_u128..=2_u128);
        assert_exact_and_one_below!(0_usize..3_usize);
        assert_exact_and_one_below!(0_usize..=2_usize);
    }

    #[test]
    fn empty_and_reversed_ranges_admit_zero_visits() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");

        let empty = std::ops::Range {
            start: 3_u8,
            end: 3_u8,
        };
        let reversed = std::ops::Range {
            start: 4_u16,
            end: 2_u16,
        };
        let reversed_inclusive = std::ops::RangeInclusive::new(3_u32, 2_u32);
        let reversed_usize = std::ops::RangeInclusive::new(5_usize, 2_usize);
        assert_eq!(ctx.admit_iter(&empty, "empty").expect("empty").count(), 0);
        assert_eq!(
            ctx.admit_iter(&reversed, "reversed")
                .expect("empty")
                .count(),
            0
        );
        assert_eq!(
            ctx.admit_iter(&reversed_inclusive, "reversed")
                .expect("empty")
                .count(),
            0
        );
        assert_eq!(
            ctx.admit_iter(&reversed_usize, "reversed")
                .expect("empty")
                .count(),
            0
        );

        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(1, "probe")
            .expect_err("empty range admission charges no work")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, 0);
        assert_eq!(limit.additional, 1);
    }

    #[test]
    fn partially_consumed_inclusive_range_admits_only_remaining_values() {
        let mut source = (u64::MAX - 2)..=u64::MAX;
        assert_eq!(source.next(), Some(u64::MAX - 2));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 2;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        assert_eq!(
            ctx.admit_iter(&source, "remaining range")
                .expect("remaining bound fits")
                .collect::<Vec<_>>(),
            [u64::MAX - 1, u64::MAX]
        );
    }

    #[test]
    fn overflowing_range_bounds_fuse_at_the_work_limit() {
        assert_range_overflow(0_u64..=u64::MAX, 0);
        assert_range_overflow(0_u128..=u128::MAX, u64::MAX);
    }

    #[test]
    fn maximum_u64_half_open_range_fits_the_work_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let source = 0_u64..u64::MAX;
        let _admitted = ctx
            .admit_iter(&source, "maximum range")
            .expect("u64::MAX visits fit the work limit");

        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(1, "probe")
            .expect_err("one more unit exceeds the work limit")
        else {
            panic!("resource refusal");
        };
        assert_eq!(limit.used, u64::MAX);
        assert_eq!(limit.additional, 1);
    }

    #[test]
    fn iteration_sources_charge_text_bytes_and_hash_capacity() {
        use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let vector = vec![1, 2];
        let boxed = vec![1, 2].into_boxed_slice();
        let queue = VecDeque::from([1, 2]);
        let tree = BTreeMap::from([(1, 2)]);
        let ordered = BTreeSet::from([1, 2]);
        let mut map = HashMap::with_capacity(100);
        map.insert(1, 2);
        let mut set = HashSet::with_capacity(100);
        set.insert(1);
        assert_eq!(
            ctx.admit_iter(&vector, "vector").expect("vector").count(),
            2
        );
        assert_eq!(ctx.admit_iter(&boxed, "box").expect("box").count(), 2);
        assert_eq!(ctx.admit_iter(&queue, "queue").expect("queue").count(), 2);
        assert_eq!(ctx.admit_iter(&tree, "tree").expect("tree").count(), 1);
        assert_eq!(ctx.admit_iter(&ordered, "set").expect("set").count(), 2);
        assert_eq!(ctx.admit_iter("é", "text").expect("text").count(), 1);
        assert_eq!(
            ctx.admit_iter("é".as_bytes(), "bytes")
                .expect("bytes")
                .count(),
            2
        );
        assert_eq!(ctx.admit_iter(&map, "map").expect("map").count(), 1);
        assert_eq!(ctx.admit_iter(&set, "set").expect("set").count(), 1);
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("resource refusal");
        };
        // Nine collection slots, four text bytes, and both hash capacity scans.
        assert_eq!(
            limit.used,
            13 + super::u64_from_index(map.capacity() + set.capacity())
        );
    }

    #[test]
    fn iteration_slice_adapters_consume_admission() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let size = std::num::NonZeroUsize::new(2).expect("nonzero");
        let values = [1, 2, 3];
        let windows: Vec<_> = ctx
            .admit_iter(&values, "windows")
            .expect("admission")
            .windows(size)
            .collect();
        assert_eq!(windows, [&[1, 2][..], &[2, 3][..]]);
        let chunks: Vec<_> = ctx
            .admit_iter(&values, "chunks")
            .expect("admission")
            .chunks(size)
            .rev()
            .collect();
        assert_eq!(chunks, [&[3][..], &[1, 2][..]]);
    }

    #[test]
    fn charged_searches_preserve_short_circuit_and_results() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let values = [1, 2, 3];
        assert!(ctx
            .any_by(&values, |value| Ok(*value == 2), "any")
            .expect("any"));
        assert!(!ctx
            .all_by(&values, |value| Ok(*value == 1), "all")
            .expect("all"));
        assert_eq!(
            ctx.find_by(&values, |value| Ok(*value == 2), "find")
                .expect("find"),
            Some(&2)
        );
        assert_eq!(
            ctx.find_map(&values, |value| Ok((*value == 2).then_some(7)), "find map")
                .expect("find map"),
            Some(7)
        );
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("resource refusal");
        };
        // Four searches visit two slots each before their short-circuit return.
        assert_eq!(limit.used, 8);
    }

    #[test]
    fn charged_reductions_keep_empty_results_and_tie_order() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let values = [(1, 0), (3, 1), (1, 2), (3, 3)];
        assert_eq!(ctx.count(&values, "count").expect("count"), 4);
        assert_eq!(
            ctx.sum(
                &[1_u64, 2, 3],
                |total: u64, value| {
                    total
                        .checked_add(*value)
                        .ok_or_else(|| ctx.refuse_codec_limit("sum", u64::MAX, u64::MAX))
                },
                "sum"
            )
            .expect("sum"),
            6
        );
        assert_eq!(
            ctx.fold(&values, 10, |total, value| Ok(total + value.0), "fold")
                .expect("fold"),
            18
        );
        assert_eq!(
            ctx.min_by(&values, |a, b| Ok(a.0.cmp(&b.0)), "min")
                .expect("min"),
            Some(&values[0])
        );
        assert_eq!(
            ctx.max_by(&values, |a, b| Ok(a.0.cmp(&b.0)), "max")
                .expect("max"),
            Some(&values[3])
        );
        assert_eq!(
            ctx.min_by_key(&values, |value| Ok(value.0), |a, b| Ok(a.cmp(b)), "min key")
                .expect("min key"),
            Some(&values[0])
        );
        assert_eq!(
            ctx.max_by_key(&values, |value| Ok(value.0), |a, b| Ok(a.cmp(b)), "max key")
                .expect("max key"),
            Some(&values[3])
        );
        assert_eq!(
            ctx.min_by::<u8>(&[], |a, b| Ok(a.cmp(b)), "empty")
                .expect("empty"),
            None
        );
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("resource refusal");
        };
        // Six four-slot traversals and one three-slot sum; the empty scan is free.
        assert_eq!(limit.used, 27);
    }

    #[test]
    fn charged_reductions_refuse_before_callback_and_keep_child_error() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let called = std::cell::Cell::new(false);
        let error = ctx
            .fold(
                &[1],
                (),
                |(), _| {
                    called.set(true);
                    Ok(())
                },
                "fold",
            )
            .expect_err("refusal");
        assert!(!called.get());
        let CodecError::ResourceLimit(limit) = error else {
            panic!("refusal");
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let error = ctx
            .find_map::<_, u8>(
                &[1],
                |_| Err(ctx.refuse_codec_limit("child", 0, 1)),
                "find map",
            )
            .expect_err("child refusal");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("refusal");
        };
        assert_eq!(limit.operation, "child");
        assert_eq!(ctx.resource_refusal(), Some(limit));
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
    #[test]
    fn charged_sorted_searches_keep_insertion_points_and_first_equal() {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let values = [1_u8, 3, 3, 7];
        for (key, expected) in [
            (0, Err(0)),
            (1, Ok(0)),
            (2, Err(1)),
            (3, Ok(1)),
            (4, Err(3)),
            (7, Ok(3)),
            (8, Err(4)),
        ] {
            assert_eq!(
                ctx.binary_search(&values, &key, "search")
                    .expect("admission"),
                expected
            );
        }
        assert_eq!(
            ctx.binary_search::<u8>(&[], &1, "empty")
                .expect("admission"),
            Err(0)
        );
        assert_eq!(
            ctx.partition_point(&values, |value| Ok(*value < 3), "partition")
                .expect("admission"),
            1
        );
        assert_eq!(
            ctx.binary_search_by_key(&values, &6_u8, |value| Ok(*value * 2), "projection")
                .expect("admission"),
            Ok(1)
        );
        assert_eq!(
            ctx.rposition_by(&values, |value| Ok(*value == 3), "reverse search")
                .expect("admission"),
            Some(2)
        );
        assert_eq!(ctx.min(&values, "minimum").expect("admission"), Some(&1));
        assert_eq!(ctx.max(&values, "maximum").expect("admission"), Some(&7));
    }

    #[test]
    fn sorted_search_refuses_before_predicate_and_keeps_child_refusal() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let called = std::cell::Cell::new(false);
        let CodecError::ResourceLimit(first) = ctx
            .partition_point(
                &[1],
                |_| {
                    called.set(true);
                    Ok(true)
                },
                "partition",
            )
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert!(!called.get());
        let CodecError::ResourceLimit(repeated) = ctx
            .rposition_by(
                &[1],
                |_| {
                    called.set(true);
                    Ok(true)
                },
                "reverse",
            )
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(repeated, first);
        assert!(!called.get());
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let CodecError::ResourceLimit(child) = ctx
            .binary_search_by(
                &[1],
                |_| Err(ctx.refuse_codec_limit("child", 0, 1)),
                "search",
            )
            .expect_err("child refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(child.operation, "child");
        assert_eq!(ctx.resource_refusal(), Some(child));
    }
}
