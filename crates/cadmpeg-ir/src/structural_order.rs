// SPDX-License-Identifier: Apache-2.0
//! The derived structural order of §2.5 for the witness geometry that a pick's
//! identity key compares.
//!
//! §2.5 orders a struct by its fields in declared order, an enum by its
//! variants in declared order and then by payload, `None` before `Some`, a
//! list lexicographically, a string by code point, and a number by value, with
//! one zero. `#[derive(cadmpeg_derive::StructuralOrd)]` states the first two
//! from a type's declaration; this module states the rest. §5.6 requires no
//! order of witness geometry otherwise, so the order is this trait and not
//! [`Ord`].

use std::borrow::Cow;
use std::cmp::Ordering;
use std::num::{NonZeroI64, NonZeroU32};

use cadmpeg_core::text::NonBlankString;

/// A total order on a value of a wire type, in §2.5's derived structural
/// order.
pub trait StructuralOrd {
    /// Compares `self` with `other` in §2.5's derived structural order.
    fn structural_cmp(&self, other: &Self) -> Ordering;
}

macro_rules! by_ord {
    ($($ty:ty),* $(,)?) => {
        $(
            impl StructuralOrd for $ty {
                fn structural_cmp(&self, other: &Self) -> Ordering {
                    self.cmp(other)
                }
            }
        )*
    };
}

// A boolean orders `false` before `true`.
by_ord!(bool, u8, u16, u32, u64, usize, i8, i16, i32, i64, char, NonZeroI64, NonZeroU32);

impl StructuralOrd for f64 {
    /// By value: −0 and 0 are one number.
    fn structural_cmp(&self, other: &Self) -> Ordering {
        (self + 0.0).total_cmp(&(other + 0.0))
    }
}

impl StructuralOrd for str {
    /// By code point, which UTF-8 byte order follows.
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.cmp(other)
    }
}

impl StructuralOrd for String {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.as_str().structural_cmp(other.as_str())
    }
}

impl StructuralOrd for Cow<'_, str> {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.as_ref().structural_cmp(other.as_ref())
    }
}

impl StructuralOrd for NonBlankString {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.as_str().structural_cmp(other.as_str())
    }
}

impl<T: StructuralOrd> StructuralOrd for Option<T> {
    /// `None` before `Some`.
    fn structural_cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(left), Some(right)) => left.structural_cmp(right),
        }
    }
}

impl<T: StructuralOrd> StructuralOrd for [T] {
    /// Lexicographically: the first unequal member decides, else the shorter
    /// list is first.
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.iter()
            .zip(other)
            .map(|(left, right)| left.structural_cmp(right))
            .find(|order| order.is_ne())
            .unwrap_or_else(|| self.len().cmp(&other.len()))
    }
}

impl<T: StructuralOrd, const N: usize> StructuralOrd for [T; N] {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.as_slice().structural_cmp(other.as_slice())
    }
}

impl<T: StructuralOrd> StructuralOrd for Vec<T> {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.as_slice().structural_cmp(other.as_slice())
    }
}

impl<A: StructuralOrd, B: StructuralOrd> StructuralOrd for (A, B) {
    /// A tuple is written as a list, so it compares member by member.
    fn structural_cmp(&self, other: &Self) -> Ordering {
        self.0
            .structural_cmp(&other.0)
            .then_with(|| self.1.structural_cmp(&other.1))
    }
}

impl<T: StructuralOrd + ?Sized> StructuralOrd for &T {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        (**self).structural_cmp(&**other)
    }
}

impl<T: StructuralOrd + ?Sized> StructuralOrd for Box<T> {
    fn structural_cmp(&self, other: &Self) -> Ordering {
        (**self).structural_cmp(&**other)
    }
}

#[cfg(test)]
mod tests;
