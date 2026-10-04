// SPDX-License-Identifier: Apache-2.0
//! Closed sources for vector extension without child cloning.

mod sealed {
    pub trait Source<T> {}
}

/// Couples the exact source extent to its consuming iterator.
pub trait ExtendSource<T>: sealed::Source<T> {
    /// Iterator over the source values.
    type Iter: Iterator<Item = T>;
    /// Returns the exact value count without a scan.
    fn known_len(&self) -> usize;
    /// Moves or copies each admitted inline value exactly once.
    fn into_values(self) -> Self::Iter;
}

impl<T> sealed::Source<T> for Vec<T> {}
impl<T> ExtendSource<T> for Vec<T> {
    type Iter = std::vec::IntoIter<T>;
    fn known_len(&self) -> usize { self.len() }
    fn into_values(self) -> Self::Iter { self.into_iter() }
}
impl<T> sealed::Source<T> for Option<T> {}
impl<T> ExtendSource<T> for Option<T> {
    type Iter = std::option::IntoIter<T>;
    fn known_len(&self) -> usize { usize::from(self.is_some()) }
    fn into_values(self) -> Self::Iter { self.into_iter() }
}
impl<T, const N: usize> sealed::Source<T> for [T; N] {}
impl<T, const N: usize> ExtendSource<T> for [T; N] {
    type Iter = std::array::IntoIter<T, N>;
    fn known_len(&self) -> usize { self.len() }
    fn into_values(self) -> Self::Iter { self.into_iter() }
}
impl<T: Copy> sealed::Source<T> for &[T] {}
impl<'a, T: Copy> ExtendSource<T> for &'a [T] {
    type Iter = std::iter::Copied<std::slice::Iter<'a, T>>;
    fn known_len(&self) -> usize { self.len() }
    fn into_values(self) -> Self::Iter { self.iter().copied() }
}
impl<T: Copy> sealed::Source<T> for &Vec<T> {}
impl<'a, T: Copy> ExtendSource<T> for &'a Vec<T> {
    type Iter = std::iter::Copied<std::slice::Iter<'a, T>>;
    fn known_len(&self) -> usize { self.len() }
    fn into_values(self) -> Self::Iter { self.iter().copied() }
}
impl<T: Copy, const N: usize> sealed::Source<T> for &[T; N] {}
impl<'a, T: Copy, const N: usize> ExtendSource<T> for &'a [T; N] {
    type Iter = std::iter::Copied<std::slice::Iter<'a, T>>;
    fn known_len(&self) -> usize { self.len() }
    fn into_values(self) -> Self::Iter { self.iter().copied() }
}
