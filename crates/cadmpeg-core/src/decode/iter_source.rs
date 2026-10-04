// SPDX-License-Identifier: Apache-2.0
//! Closed traversal sources with bounds available before the first visit.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::u64_from_index;

mod sealed {
    pub trait Sealed {}
}

/// A core-owned source whose complete traversal bound is known in advance.
///
/// The source is consumed: a borrowed collection yields references, a mutable
/// borrow yields mutable references and an owned collection moves its values.
/// Hash traversal includes vacant buckets; text traversal counts bytes.
pub trait IterSource: sealed::Sealed {
    /// Traversal without allocating element storage.
    type Iter: Iterator;

    /// Returns the traversal work bound without visiting an element.
    fn visit_bound(&self) -> Result<u64, VisitBoundError>;

    /// Constructs the raw traversal after the caller admits its bound.
    fn source_iter(self) -> Self::Iter;
}

/// The exact visit bound cannot fit in the session work counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisitBoundError {
    /// The traversal has more than `u64::MAX` visits.
    ExceedsU64,
}

fn checked_u64_bound(bound: u128) -> Result<u64, VisitBoundError> {
    u64::try_from(bound).map_err(|_| VisitBoundError::ExceedsU64)
}

fn half_open_range_bound(start: u128, end: u128) -> Result<u64, VisitBoundError> {
    if start >= end {
        return Ok(0);
    }
    let count = end.checked_sub(start).ok_or(VisitBoundError::ExceedsU64)?;
    checked_u64_bound(count)
}

fn inclusive_range_bound(start: u128, end: u128) -> Result<u64, VisitBoundError> {
    let count = end
        .checked_sub(start)
        .and_then(|distance| distance.checked_add(1))
        .ok_or(VisitBoundError::ExceedsU64)?;
    checked_u64_bound(count)
}

/// Implements a source whose bound is an existing length.
macro_rules! counted_source {
    ([$($generic:tt)*] $source:ty => $iter:ty; |$value:ident| $bound:expr, $traversal:expr) => {
        impl<$($generic)*> sealed::Sealed for $source {}
        impl<$($generic)*> IterSource for $source {
            type Iter = $iter;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                let $value = self;
                Ok(u64_from_index($bound))
            }
            fn source_iter(self) -> Self::Iter {
                let $value = self;
                $traversal
            }
        }
    };
}

counted_source!(['a, T] &'a [T] => std::slice::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!(['a, T] &'a Vec<T> => std::slice::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!(['a, T] &'a Box<[T]> => std::slice::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!(['a, T, const N: usize] &'a [T; N] => std::slice::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!(['a, T] &'a mut [T] => std::slice::IterMut<'a, T>; |values| values.len(), values.iter_mut());
counted_source!(['a, T] &'a mut Vec<T> => std::slice::IterMut<'a, T>; |values| values.len(), values.iter_mut());
counted_source!(['a, T, const N: usize] &'a mut [T; N] => std::slice::IterMut<'a, T>; |values| values.len(), values.iter_mut());
counted_source!([T] Vec<T> => std::vec::IntoIter<T>; |values| values.len(), IntoIterator::into_iter(values));
counted_source!([T, const N: usize] [T; N] => std::array::IntoIter<T, N>; |values| values.len(), IntoIterator::into_iter(values));
counted_source!([T] Option<T> => std::option::IntoIter<T>; |value| usize::from(value.is_some()), IntoIterator::into_iter(value));
counted_source!(['a, T] &'a Option<T> => std::option::Iter<'a, T>; |value| usize::from(value.is_some()), value.iter());
counted_source!(['a] &'a str => std::str::Chars<'a>; |text| text.len(), text.chars());
counted_source!(['a] &'a String => std::str::Chars<'a>; |text| text.len(), text.chars());
counted_source!(['a, T] &'a VecDeque<T> => std::collections::vec_deque::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!(['a, K, V, S] &'a HashMap<K, V, S> => std::collections::hash_map::Iter<'a, K, V>; |values| values.capacity(), values.iter());
counted_source!(['a, T, S] &'a HashSet<T, S> => std::collections::hash_set::Iter<'a, T>; |values| values.capacity(), values.iter());
counted_source!(['a, K, V] &'a BTreeMap<K, V> => std::collections::btree_map::Iter<'a, K, V>; |values| values.len(), values.iter());
counted_source!(['a, K, V] &'a mut BTreeMap<K, V> => std::collections::btree_map::IterMut<'a, K, V>; |values| values.len(), values.iter_mut());
counted_source!([K, V] BTreeMap<K, V> => std::collections::btree_map::IntoIter<K, V>; |values| values.len(), IntoIterator::into_iter(values));
counted_source!(['a, T] &'a BTreeSet<T> => std::collections::btree_set::Iter<'a, T>; |values| values.len(), values.iter());
counted_source!([T] BTreeSet<T> => std::collections::btree_set::IntoIter<T>; |values| values.len(), IntoIterator::into_iter(values));
counted_source!(['a] &'a serde_json::Map<String, serde_json::Value> => serde_json::map::Iter<'a>; |values| values.len(), values.iter());
counted_source!([] serde_json::Map<String, serde_json::Value> => serde_json::map::IntoIter; |values| values.len(), IntoIterator::into_iter(values));

impl sealed::Sealed for &crate::dialect::DialectLayers {}
impl<'a> IterSource for &'a crate::dialect::DialectLayers {
    type Iter = std::iter::Chain<
        std::iter::Once<&'a crate::dialect::DialectMatch>,
        std::slice::Iter<'a, crate::dialect::DialectMatch>,
    >;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        checked_u64_bound(u128::from(u64_from_index(self.extra_layer_count())) + 1)
    }
    fn source_iter(self) -> Self::Iter {
        self.iter()
    }
}

macro_rules! unsigned_range_source {
    ($type:ty) => {
        impl sealed::Sealed for std::ops::Range<$type> {}
        impl IterSource for std::ops::Range<$type> {
            type Iter = std::ops::Range<$type>;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                half_open_range_bound(u128::from(self.start), u128::from(self.end))
            }
            fn source_iter(self) -> Self::Iter {
                self
            }
        }

        impl sealed::Sealed for &std::ops::Range<$type> {}
        impl IterSource for &std::ops::Range<$type> {
            type Iter = std::ops::Range<$type>;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                (*self).clone().visit_bound()
            }
            fn source_iter(self) -> Self::Iter {
                self.clone()
            }
        }

        impl sealed::Sealed for std::ops::RangeInclusive<$type> {}
        impl IterSource for std::ops::RangeInclusive<$type> {
            type Iter = std::ops::RangeInclusive<$type>;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                if self.is_empty() {
                    return Ok(0);
                }
                inclusive_range_bound(u128::from(*self.start()), u128::from(*self.end()))
            }
            fn source_iter(self) -> Self::Iter {
                self
            }
        }

        impl sealed::Sealed for &std::ops::RangeInclusive<$type> {}
        impl IterSource for &std::ops::RangeInclusive<$type> {
            type Iter = std::ops::RangeInclusive<$type>;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                (*self).clone().visit_bound()
            }
            fn source_iter(self) -> Self::Iter {
                self.clone()
            }
        }
    };
}

unsigned_range_source!(u8);
unsigned_range_source!(u16);
unsigned_range_source!(u32);
unsigned_range_source!(u64);
unsigned_range_source!(u128);

impl sealed::Sealed for std::ops::Range<usize> {}
impl IterSource for std::ops::Range<usize> {
    type Iter = std::ops::Range<usize>;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        let start = u128::try_from(self.start).map_err(|_| VisitBoundError::ExceedsU64)?;
        let end = u128::try_from(self.end).map_err(|_| VisitBoundError::ExceedsU64)?;
        half_open_range_bound(start, end)
    }
    fn source_iter(self) -> Self::Iter {
        self
    }
}

impl sealed::Sealed for &std::ops::Range<usize> {}
impl IterSource for &std::ops::Range<usize> {
    type Iter = std::ops::Range<usize>;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        (*self).clone().visit_bound()
    }
    fn source_iter(self) -> Self::Iter {
        self.clone()
    }
}

impl sealed::Sealed for std::ops::RangeInclusive<usize> {}
impl IterSource for std::ops::RangeInclusive<usize> {
    type Iter = std::ops::RangeInclusive<usize>;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        if self.is_empty() {
            return Ok(0);
        }
        let start = u128::try_from(*self.start()).map_err(|_| VisitBoundError::ExceedsU64)?;
        let end = u128::try_from(*self.end()).map_err(|_| VisitBoundError::ExceedsU64)?;
        inclusive_range_bound(start, end)
    }
    fn source_iter(self) -> Self::Iter {
        self
    }
}

impl sealed::Sealed for &std::ops::RangeInclusive<usize> {}
impl IterSource for &std::ops::RangeInclusive<usize> {
    type Iter = std::ops::RangeInclusive<usize>;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        (*self).clone().visit_bound()
    }
    fn source_iter(self) -> Self::Iter {
        self.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{IterSource, VisitBoundError};

    #[test]
    fn unsigned_ranges_report_exact_bounds() {
        assert_eq!((2_u8..5_u8).visit_bound(), Ok(3));
        assert_eq!((2_u16..5_u16).visit_bound(), Ok(3));
        assert_eq!((2_u32..5_u32).visit_bound(), Ok(3));
        assert_eq!((2_u64..5_u64).visit_bound(), Ok(3));
        assert_eq!((2_u128..5_u128).visit_bound(), Ok(3));
        assert_eq!((2_usize..5_usize).visit_bound(), Ok(3));
        assert_eq!((2_u8..=4_u8).visit_bound(), Ok(3));
        assert_eq!((2_u16..=4_u16).visit_bound(), Ok(3));
        assert_eq!((2_u32..=4_u32).visit_bound(), Ok(3));
        assert_eq!((2_u64..=4_u64).visit_bound(), Ok(3));
        assert_eq!((2_u128..=4_u128).visit_bound(), Ok(3));
        assert_eq!((2_usize..=4_usize).visit_bound(), Ok(3));
    }

    #[test]
    fn empty_and_reversed_ranges_have_zero_bounds() {
        assert_eq!(
            std::ops::Range {
                start: 3_u8,
                end: 3_u8,
            }
            .visit_bound(),
            Ok(0)
        );
        assert_eq!(
            std::ops::Range {
                start: 4_u16,
                end: 2_u16,
            }
            .visit_bound(),
            Ok(0)
        );
        assert_eq!(
            std::ops::RangeInclusive::new(3_u32, 2_u32).visit_bound(),
            Ok(0)
        );
        assert_eq!(
            std::ops::RangeInclusive::new(3_u64, 2_u64).visit_bound(),
            Ok(0)
        );
        assert_eq!(
            std::ops::RangeInclusive::new(5_u128, 2_u128).visit_bound(),
            Ok(0)
        );
        assert_eq!(
            std::ops::RangeInclusive::new(5_usize, 2_usize).visit_bound(),
            Ok(0)
        );
    }

    #[test]
    fn inclusive_ranges_preserve_partial_and_exhausted_maximum_state() {
        let mut partial = (u64::MAX - 2)..=u64::MAX;
        assert_eq!(partial.visit_bound(), Ok(3));
        assert_eq!(partial.next(), Some(u64::MAX - 2));
        assert_eq!(partial.visit_bound(), Ok(2));
        assert_eq!(partial.next(), Some(u64::MAX - 1));
        assert_eq!(partial.next(), Some(u64::MAX));
        assert_eq!(partial.visit_bound(), Ok(0));

        let mut singleton = u128::MAX..=u128::MAX;
        assert_eq!(singleton.visit_bound(), Ok(1));
        assert_eq!(singleton.next(), Some(u128::MAX));
        assert_eq!(singleton.visit_bound(), Ok(0));
    }

    #[test]
    fn near_maximum_u128_inclusive_range_has_a_small_bound() {
        let range = (u128::MAX - 3)..=u128::MAX;
        assert_eq!(range.visit_bound(), Ok(4));
    }

    #[test]
    fn range_bounds_reject_values_above_u64() {
        assert_eq!(
            (0_u64..=u64::MAX).visit_bound(),
            Err(VisitBoundError::ExceedsU64)
        );
        assert_eq!(
            (0_u128..u128::MAX).visit_bound(),
            Err(VisitBoundError::ExceedsU64)
        );
        assert_eq!(
            (0_u128..=u128::MAX).visit_bound(),
            Err(VisitBoundError::ExceedsU64)
        );
        let expected = u64::try_from(usize::MAX).map_err(|_| VisitBoundError::ExceedsU64);
        assert_eq!((0_usize..usize::MAX).visit_bound(), expected);
    }
}
