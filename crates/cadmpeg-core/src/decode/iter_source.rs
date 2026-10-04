// SPDX-License-Identifier: Apache-2.0
//! Closed traversal sources with bounds available before the first visit.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use super::u64_from_index;

mod sealed {
    pub trait Sealed {}
}

/// A core-owned source whose complete traversal bound is known in advance.
/// Hash traversal includes vacant buckets; text traversal counts bytes.
pub trait IterSource: sealed::Sealed {
    /// Traversal without allocating element storage.
    type Iter<'a>: Iterator
    where
        Self: 'a;

    /// Returns the traversal work bound without visiting an element.
    fn visit_bound(&self) -> Result<u64, VisitBoundError>;
    /// Constructs the raw traversal after the caller admits its bound.
    fn source_iter(&self) -> Self::Iter<'_>;
}

/// The exact visit bound cannot fit in the session work counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisitBoundError {
    /// The range has more than `u64::MAX` visits.
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

impl<T> sealed::Sealed for [T] {}
impl<T> IterSource for [T] {
    type Iter<'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.len()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

macro_rules! slice_source {
    ($type:ty) => {
        impl<T> sealed::Sealed for $type {}
        impl<T> IterSource for $type {
            type Iter<'a>
                = std::slice::Iter<'a, T>
            where
                T: 'a;
            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                Ok(u64_from_index(self.len()))
            }
            fn source_iter(&self) -> Self::Iter<'_> {
                self.iter()
            }
        }
    };
}
slice_source!(Vec<T>);
slice_source!(Box<[T]>);

impl<T, const N: usize> sealed::Sealed for [T; N] {}
impl<T, const N: usize> IterSource for [T; N] {
    type Iter<'a>
        = std::slice::Iter<'a, T>
    where
        T: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(N))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

impl sealed::Sealed for str {}
impl IterSource for str {
    type Iter<'a> = std::str::Chars<'a>;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.len()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.chars()
    }
}

impl<T> sealed::Sealed for VecDeque<T> {}
impl<T> IterSource for VecDeque<T> {
    type Iter<'a>
        = std::collections::vec_deque::Iter<'a, T>
    where
        T: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.len()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

impl<K, V, S> sealed::Sealed for HashMap<K, V, S> {}
impl<K, V, S> IterSource for HashMap<K, V, S> {
    type Iter<'a>
        = std::collections::hash_map::Iter<'a, K, V>
    where
        K: 'a,
        V: 'a,
        S: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.capacity()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

impl<T, S> sealed::Sealed for HashSet<T, S> {}
impl<T, S> IterSource for HashSet<T, S> {
    type Iter<'a>
        = std::collections::hash_set::Iter<'a, T>
    where
        T: 'a,
        S: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.capacity()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

impl<K, V> sealed::Sealed for BTreeMap<K, V> {}
impl<K, V> IterSource for BTreeMap<K, V> {
    type Iter<'a>
        = std::collections::btree_map::Iter<'a, K, V>
    where
        K: 'a,
        V: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.len()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

impl<T> sealed::Sealed for BTreeSet<T> {}
impl<T> IterSource for BTreeSet<T> {
    type Iter<'a>
        = std::collections::btree_set::Iter<'a, T>
    where
        T: 'a;
    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        Ok(u64_from_index(self.len()))
    }
    fn source_iter(&self) -> Self::Iter<'_> {
        self.iter()
    }
}

macro_rules! unsigned_range_source {
    ($type:ty) => {
        impl sealed::Sealed for std::ops::Range<$type> {}
        impl IterSource for std::ops::Range<$type> {
            type Iter<'a>
                = std::ops::Range<$type>
            where
                Self: 'a;

            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                half_open_range_bound(u128::from(self.start), u128::from(self.end))
            }

            fn source_iter(&self) -> Self::Iter<'_> {
                self.clone()
            }
        }

        impl sealed::Sealed for std::ops::RangeInclusive<$type> {}
        impl IterSource for std::ops::RangeInclusive<$type> {
            type Iter<'a>
                = std::ops::RangeInclusive<$type>
            where
                Self: 'a;

            fn visit_bound(&self) -> Result<u64, VisitBoundError> {
                if self.is_empty() {
                    return Ok(0);
                }
                inclusive_range_bound(u128::from(*self.start()), u128::from(*self.end()))
            }

            fn source_iter(&self) -> Self::Iter<'_> {
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
    type Iter<'a>
        = std::ops::Range<usize>
    where
        Self: 'a;

    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        let start = u128::try_from(self.start).map_err(|_| VisitBoundError::ExceedsU64)?;
        let end = u128::try_from(self.end).map_err(|_| VisitBoundError::ExceedsU64)?;
        half_open_range_bound(start, end)
    }

    fn source_iter(&self) -> Self::Iter<'_> {
        self.clone()
    }
}

impl sealed::Sealed for std::ops::RangeInclusive<usize> {}
impl IterSource for std::ops::RangeInclusive<usize> {
    type Iter<'a>
        = std::ops::RangeInclusive<usize>
    where
        Self: 'a;

    fn visit_bound(&self) -> Result<u64, VisitBoundError> {
        if self.is_empty() {
            return Ok(0);
        }
        let start = u128::try_from(*self.start()).map_err(|_| VisitBoundError::ExceedsU64)?;
        let end = u128::try_from(*self.end()).map_err(|_| VisitBoundError::ExceedsU64)?;
        inclusive_range_bound(start, end)
    }

    fn source_iter(&self) -> Self::Iter<'_> {
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
