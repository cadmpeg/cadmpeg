// SPDX-License-Identifier: Apache-2.0
//! Sealed iteration sources with upfront or per-step admission.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use crate::CodecError;

use super::{u64_from_index, DecodeContext, ResourceLimit};

pub(super) mod sealed {
    use super::{DecodeContext, IterSource, ResourceLimit};
    use crate::decode::scan::AdmittedIter;

    pub trait Sealed {
        fn admit<'context, 'arena>(
            self,
            context: &'context DecodeContext<'arena>,
            operation: &'static str,
        ) -> Result<AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
        where
            Self: IterSource + Sized,
            'arena: 'context;
    }

    pub trait AdmissionMode {}
}

/// Marks an iterator whose complete work bound is charged before traversal.
#[doc(hidden)]
#[derive(Debug)]
pub struct Precharged(pub(super) ());

impl sealed::AdmissionMode for Precharged {}

/// Marks an iterator whose work is charged before each call to `next`.
#[doc(hidden)]
#[derive(Debug)]
pub struct Stepwise(());

/// Charges an unknown iterator before each source step.
#[doc(hidden)]
#[derive(Debug)]
pub struct IncrementalAdmission<'context, 'arena> {
    context: &'context DecodeContext<'arena>,
    operation: &'static str,
    finished: bool,
}

impl sealed::AdmissionMode for IncrementalAdmission<'_, '_> {}

/// Defines how an admitted iterator yields and reports traversal errors.
#[doc(hidden)]
pub trait AdmissionMode<I: Iterator>: sealed::AdmissionMode {
    /// Item type yielded by the admitted iterator.
    type Item;

    /// Charges and advances one source step.
    fn next(&mut self, source: &mut I) -> Option<Self::Item>;

    /// Returns a size hint that does not admit source work.
    fn size_hint(&self, source: &I) -> (usize, Option<usize>);
}

impl<I: Iterator> AdmissionMode<I> for Precharged {
    type Item = I::Item;

    fn next(&mut self, source: &mut I) -> Option<Self::Item> {
        source.next()
    }

    fn size_hint(&self, source: &I) -> (usize, Option<usize>) {
        source.size_hint()
    }
}

impl<I: Iterator> AdmissionMode<I> for IncrementalAdmission<'_, '_> {
    type Item = Result<I::Item, CodecError>;

    fn next(&mut self, source: &mut I) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        match self.context.next_charged(source, self.operation) {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => {
                self.finished = true;
                None
            }
            Err(error) => {
                self.finished = true;
                Some(Err(error))
            }
        }
    }

    fn size_hint(&self, _: &I) -> (usize, Option<usize>) {
        if self.finished {
            (0, Some(0))
        } else {
            (0, None)
        }
    }
}

/// A sealed source admitted before it yields values.
///
/// A `Some` visit bound is charged in full before `into_iter` runs. `None`
/// selects per-step admission, which charges every source `next`, including
/// the terminal probe. Unknown sources do not use `Iterator::size_hint` as a
/// work bound.
pub trait IterSource: sealed::Sealed {
    /// Iterator produced when this source is consumed.
    type Iter: Iterator;

    /// Admission category used by operations that require an upfront bound.
    type AdmissionKind;

    /// Mode that charges the source traversal.
    type Mode<'context, 'arena>: AdmissionMode<Self::Iter>
    where
        'arena: 'context;

    /// Returns a known upfront work bound, or `None` for per-step admission.
    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError>;

    /// Consumes the source to construct its iterator.
    fn into_iter(self) -> Self::Iter;

}

fn admit_source<'context, 'arena, S, MakeMode>(
    source: S,
    context: &'context DecodeContext<'arena>,
    operation: &'static str,
    make_mode: MakeMode,
) -> Result<super::scan::AdmittedIter<S::Iter, S::Mode<'context, 'arena>>, ResourceLimit>
where
    S: IterSource,
    'arena: 'context,
    MakeMode: FnOnce(
        &'context DecodeContext<'arena>,
        &'static str,
    ) -> S::Mode<'context, 'arena>,
{
    let bound = match source.visit_bound() {
        Ok(bound) => bound,
        Err(VisitBoundError::ExceedsU64) => {
            return Err(context.budget.work_bound_overflow_limit(operation));
        }
    };
    if let Some(bound) = bound {
        context.charge_work_limit(bound, operation)?;
    }
    let iter = source.into_iter();
    let mode = make_mode(context, operation);
    Ok(super::scan::AdmittedIter::from_admitted(iter, mode))
}

/// The exact visit bound cannot fit in the session work counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisitBoundError {
    /// The range has more than `u64::MAX` visits.
    ExceedsU64,
}

/// An arbitrary iterator whose work is admitted before each source step.
///
/// Each call to the wrapped iterator's `next` must perform one bounded step.
/// Callers admit variable-work adapters through a bounded base source before
/// constructing this wrapper.
#[derive(Debug)]
pub struct IncrementalSource<I>(I);

impl<I: Iterator> IncrementalSource<I> {
    /// Selects per-step admission for an iterator with unknown traversal length.
    pub fn new(source: I) -> Self {
        Self(source)
    }
}

impl<I: Iterator> sealed::Sealed for IncrementalSource<I> {
    fn admit<'context, 'arena>(
        self,
        context: &'context DecodeContext<'arena>,
        operation: &'static str,
    ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
    where
        'arena: 'context,
    {
        admit_source(self, context, operation, |context, operation| {
            IncrementalAdmission {
                context,
                operation,
                finished: false,
            }
        })
    }
}

impl<I: Iterator> IterSource for IncrementalSource<I> {
    type Iter = I;
    type AdmissionKind = Stepwise;
    type Mode<'context, 'arena> = IncrementalAdmission<'context, 'arena>
    where
        'arena: 'context;

    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
        Ok(None)
    }

    fn into_iter(self) -> Self::Iter {
        self.0
    }

}

fn checked_u64_bound(bound: u128) -> Result<u64, VisitBoundError> {
    u64::try_from(bound).map_err(|_| VisitBoundError::ExceedsU64)
}

fn checked_u128_endpoint<T>(value: T) -> Result<u128, VisitBoundError>
where
    u128: TryFrom<T>,
{
    u128::try_from(value).map_err(|_| VisitBoundError::ExceedsU64)
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

macro_rules! upfront_source {
    ([$($generic:tt)+] $source:ty => $iter:ty; $bound_source:ident => $bound:expr; $into_source:ident => $into:expr) => {
        impl<$($generic)+> sealed::Sealed for $source {
            fn admit<'context, 'arena>(
                self,
                context: &'context DecodeContext<'arena>,
                operation: &'static str,
            ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
            where
                'arena: 'context,
            {
                admit_source(self, context, operation, |_, _| Precharged(()))
            }
        }

        impl<$($generic)+> IterSource for $source {
            type Iter = $iter;
            type AdmissionKind = Precharged;
            type Mode<'context, 'arena> = Precharged
            where
                'arena: 'context;

            fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
                let $bound_source = self;
                Ok(Some($bound))
            }

            fn into_iter(self) -> Self::Iter {
                let $into_source = self;
                $into
            }
        }
    };
}

upfront_source!(['a, T: 'a] &'a [T] => std::slice::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!(['a, T: 'a] &'a Vec<T> => std::slice::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!(['a, T: 'a] &'a Box<[T]> => std::slice::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!(['a, T: 'a, const N: usize] &'a [T; N] => std::slice::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!([T, const N: usize] [T; N] => std::array::IntoIter<T, N>; source => u64_from_index(source.len()); source => std::iter::IntoIterator::into_iter(source));
upfront_source!([T] Vec<T> => std::vec::IntoIter<T>; source => u64_from_index(source.len()); source => std::iter::IntoIterator::into_iter(source));
upfront_source!([T] Option<T> => std::option::IntoIter<T>; source => u64::from(u8::from(source.is_some())); source => std::iter::IntoIterator::into_iter(source));
upfront_source!(['a, T: 'a] &'a Option<T> => std::option::Iter<'a, T>; source => u64::from(u8::from(source.is_some())); source => source.iter());
upfront_source!(['a] &'a str => std::str::Chars<'a>; source => u64_from_index(source.len()); source => source.chars());
upfront_source!(['a] &'a String => std::str::Chars<'a>; source => u64_from_index(source.len()); source => source.chars());
upfront_source!(['a, T: 'a] &'a VecDeque<T> => std::collections::vec_deque::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!(['a, K: 'a, V: 'a, S: 'a] &'a HashMap<K, V, S> => std::collections::hash_map::Iter<'a, K, V>; source => u64_from_index(source.capacity()); source => source.iter());
upfront_source!(['a, T: 'a, S: 'a] &'a HashSet<T, S> => std::collections::hash_set::Iter<'a, T>; source => u64_from_index(source.capacity()); source => source.iter());
upfront_source!(['a, K: 'a, V: 'a] &'a BTreeMap<K, V> => std::collections::btree_map::Iter<'a, K, V>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!([K, V] BTreeMap<K, V> => std::collections::btree_map::IntoIter<K, V>; source => u64_from_index(source.len()); source => std::iter::IntoIterator::into_iter(source));
upfront_source!(['a, T: 'a] &'a BTreeSet<T> => std::collections::btree_set::Iter<'a, T>; source => u64_from_index(source.len()); source => source.iter());
upfront_source!(['a] &'a serde_json::Map<String, serde_json::Value> => serde_json::map::Iter<'a>; source => u64_from_index(source.len()); source => source.iter());

impl sealed::Sealed for serde_json::Map<String, serde_json::Value> {
    fn admit<'context, 'arena>(
        self,
        context: &'context DecodeContext<'arena>,
        operation: &'static str,
    ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
    where
        'arena: 'context,
    {
        admit_source(self, context, operation, |_, _| Precharged(()))
    }
}

impl IterSource for serde_json::Map<String, serde_json::Value> {
    type Iter = serde_json::map::IntoIter;
    type AdmissionKind = Precharged;
    type Mode<'context, 'arena> = Precharged
    where
        'arena: 'context;

    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
        Ok(Some(u64_from_index(self.len())))
    }

    fn into_iter(self) -> Self::Iter {
        std::iter::IntoIterator::into_iter(self)
    }
}

impl<'a, T: 'a> sealed::Sealed for &'a mut [T] {
    fn admit<'context, 'arena>(
        self,
        context: &'context DecodeContext<'arena>,
        operation: &'static str,
    ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
    where
        'arena: 'context,
    {
        admit_source(self, context, operation, |_, _| Precharged(()))
    }
}
impl<'a, T: 'a> IterSource for &'a mut [T] {
    type Iter = std::slice::IterMut<'a, T>;
    type AdmissionKind = Precharged;
    type Mode<'context, 'arena> = Precharged
    where
        'arena: 'context;

    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
        Ok(Some(u64_from_index(self.len())))
    }

    fn into_iter(self) -> Self::Iter {
        self.iter_mut()
    }

}

upfront_source!(['a, T: 'a, const N: usize] &'a mut [T; N] => std::slice::IterMut<'a, T>; source => u64_from_index(source.len()); source => source.iter_mut());

impl<'a, T: 'a> sealed::Sealed for &'a mut Vec<T> {
    fn admit<'context, 'arena>(
        self,
        context: &'context DecodeContext<'arena>,
        operation: &'static str,
    ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
    where
        'arena: 'context,
    {
        admit_source(self, context, operation, |_, _| Precharged(()))
    }
}
impl<'a, T: 'a> IterSource for &'a mut Vec<T> {
    type Iter = std::slice::IterMut<'a, T>;
    type AdmissionKind = Precharged;
    type Mode<'context, 'arena> = Precharged
    where
        'arena: 'context;

    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
        Ok(Some(u64_from_index(self.len())))
    }

    fn into_iter(self) -> Self::Iter {
        self.iter_mut()
    }

}

macro_rules! unsigned_range_source {
    ($type:ty) => {
        impl sealed::Sealed for std::ops::Range<$type> {
            fn admit<'context, 'arena>(
                self,
                context: &'context DecodeContext<'arena>,
                operation: &'static str,
            ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
            where
                'arena: 'context,
            {
                admit_source(self, context, operation, |_, _| Precharged(()))
            }
        }
        impl IterSource for std::ops::Range<$type> {
            type Iter = std::ops::Range<$type>;
            type AdmissionKind = Precharged;
            type Mode<'context, 'arena> = Precharged
            where
                'arena: 'context;

            fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
                Ok(Some(half_open_range_bound(
                    checked_u128_endpoint(self.start)?,
                    checked_u128_endpoint(self.end)?,
                )?))
            }

            fn into_iter(self) -> Self::Iter {
                self
            }
        }

        impl<'a> sealed::Sealed for &'a std::ops::Range<$type> {
            fn admit<'context, 'arena>(
                self,
                context: &'context DecodeContext<'arena>,
                operation: &'static str,
            ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
            where
                'arena: 'context,
            {
                admit_source(self, context, operation, |_, _| Precharged(()))
            }
        }
        impl<'a> IterSource for &'a std::ops::Range<$type> {
            type Iter = std::ops::Range<$type>;
            type AdmissionKind = Precharged;
            type Mode<'context, 'arena> = Precharged
            where
                'arena: 'context;

            fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
                Ok(Some(half_open_range_bound(
                    checked_u128_endpoint(self.start)?,
                    checked_u128_endpoint(self.end)?,
                )?))
            }

            fn into_iter(self) -> Self::Iter {
                (*self).clone()
            }
        }

        impl sealed::Sealed for std::ops::RangeInclusive<$type> {
            fn admit<'context, 'arena>(
                self,
                context: &'context DecodeContext<'arena>,
                operation: &'static str,
            ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
            where
                'arena: 'context,
            {
                admit_source(self, context, operation, |_, _| Precharged(()))
            }
        }
        impl IterSource for std::ops::RangeInclusive<$type> {
            type Iter = std::ops::RangeInclusive<$type>;
            type AdmissionKind = Precharged;
            type Mode<'context, 'arena> = Precharged
            where
                'arena: 'context;

            fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
                if self.is_empty() {
                    return Ok(Some(0));
                }
                Ok(Some(inclusive_range_bound(
                    checked_u128_endpoint(*self.start())?,
                    checked_u128_endpoint(*self.end())?,
                )?))
            }

            fn into_iter(self) -> Self::Iter {
                self
            }
        }

        impl<'a> sealed::Sealed for &'a std::ops::RangeInclusive<$type> {
            fn admit<'context, 'arena>(
                self,
                context: &'context DecodeContext<'arena>,
                operation: &'static str,
            ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
            where
                'arena: 'context,
            {
                admit_source(self, context, operation, |_, _| Precharged(()))
            }
        }
        impl<'a> IterSource for &'a std::ops::RangeInclusive<$type> {
            type Iter = std::ops::RangeInclusive<$type>;
            type AdmissionKind = Precharged;
            type Mode<'context, 'arena> = Precharged
            where
                'arena: 'context;

            fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
                if self.is_empty() {
                    return Ok(Some(0));
                }
                Ok(Some(inclusive_range_bound(
                    checked_u128_endpoint(*self.start())?,
                    checked_u128_endpoint(*self.end())?,
                )?))
            }

            fn into_iter(self) -> Self::Iter {
                (*self).clone()
            }
        }
    };
}

unsigned_range_source!(u8);
unsigned_range_source!(u16);
unsigned_range_source!(u32);
unsigned_range_source!(u64);
unsigned_range_source!(u128);
unsigned_range_source!(usize);

impl<'a, 'input: 'a> sealed::Sealed for roxmltree::Children<'a, 'input> {
    fn admit<'context, 'arena>(
        self,
        context: &'context DecodeContext<'arena>,
        operation: &'static str,
    ) -> Result<super::scan::AdmittedIter<<Self as IterSource>::Iter, <Self as IterSource>::Mode<'context, 'arena>>, ResourceLimit>
    where
        'arena: 'context,
    {
        admit_source(self, context, operation, |context, operation| {
            IncrementalAdmission {
                context,
                operation,
                finished: false,
            }
        })
    }
}
impl<'a, 'input: 'a> IterSource for roxmltree::Children<'a, 'input> {
    type Iter = Self;
    type AdmissionKind = Stepwise;
    type Mode<'context, 'arena> = IncrementalAdmission<'context, 'arena>
    where
        'arena: 'context;

    fn visit_bound(&self) -> Result<Option<u64>, VisitBoundError> {
        Ok(None)
    }

    fn into_iter(self) -> Self::Iter {
        self
    }

}

#[cfg(test)]
mod tests {
    use super::{IterSource, VisitBoundError};

    #[test]
    fn unsigned_ranges_report_exact_bounds() {
        assert_eq!((2_u8..5_u8).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u16..5_u16).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u32..5_u32).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u64..5_u64).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u128..5_u128).visit_bound(), Ok(Some(3)));
        assert_eq!((2_usize..5_usize).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u8..=4_u8).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u16..=4_u16).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u32..=4_u32).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u64..=4_u64).visit_bound(), Ok(Some(3)));
        assert_eq!((2_u128..=4_u128).visit_bound(), Ok(Some(3)));
        assert_eq!((2_usize..=4_usize).visit_bound(), Ok(Some(3)));
    }

    #[test]
    fn empty_and_reversed_ranges_have_zero_bounds() {
        assert_eq!(
            std::ops::Range {
                start: 3_u8,
                end: 3_u8,
            }
            .visit_bound(),
            Ok(Some(0))
        );
        assert_eq!(
            std::ops::Range {
                start: 4_u16,
                end: 2_u16,
            }
            .visit_bound(),
            Ok(Some(0))
        );
        assert_eq!(
            std::ops::RangeInclusive::new(3_u32, 2_u32).visit_bound(),
            Ok(Some(0))
        );
        assert_eq!(
            std::ops::RangeInclusive::new(3_u64, 2_u64).visit_bound(),
            Ok(Some(0))
        );
        assert_eq!(
            std::ops::RangeInclusive::new(5_u128, 2_u128).visit_bound(),
            Ok(Some(0))
        );
        assert_eq!(
            std::ops::RangeInclusive::new(5_usize, 2_usize).visit_bound(),
            Ok(Some(0))
        );
    }

    #[test]
    fn inclusive_ranges_preserve_partial_and_exhausted_maximum_state() {
        let mut partial = (u64::MAX - 2)..=u64::MAX;
        assert_eq!(partial.visit_bound(), Ok(Some(3)));
        assert_eq!(partial.next(), Some(u64::MAX - 2));
        assert_eq!(partial.visit_bound(), Ok(Some(2)));
        assert_eq!(partial.next(), Some(u64::MAX - 1));
        assert_eq!(partial.next(), Some(u64::MAX));
        assert_eq!(partial.visit_bound(), Ok(Some(0)));

        let mut singleton = u128::MAX..=u128::MAX;
        assert_eq!(singleton.visit_bound(), Ok(Some(1)));
        assert_eq!(singleton.next(), Some(u128::MAX));
        assert_eq!(singleton.visit_bound(), Ok(Some(0)));
    }

    #[test]
    fn near_maximum_u128_inclusive_range_has_a_small_bound() {
        let range = (u128::MAX - 3)..=u128::MAX;
        assert_eq!(range.visit_bound(), Ok(Some(4)));
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
        let expected = u64::try_from(usize::MAX)
            .map(Some)
            .map_err(|_| VisitBoundError::ExceedsU64);
        assert_eq!((0_usize..usize::MAX).visit_bound(), expected);
    }
}
