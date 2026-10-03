// SPDX-License-Identifier: Apache-2.0
//! Closed collection sources with bounds available before the first visit.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

mod sealed {
    pub trait Sealed {}
}

/// A core-owned collection whose complete traversal bound is known in advance.
/// Hash traversal includes vacant buckets; text traversal counts bytes.
pub trait IterSource: sealed::Sealed {
    /// Borrowed traversal without copying or allocating elements.
    type Iter<'a>: Iterator where Self: 'a;

    /// Returns the traversal work bound without visiting an element.
    fn visit_bound(&self) -> usize;
    /// Constructs the raw traversal after the caller admits its bound.
    fn source_iter(&self) -> Self::Iter<'_>;
}

impl<T> sealed::Sealed for [T] {}
impl<T> IterSource for [T] {
    type Iter<'a> = std::slice::Iter<'a, T> where T: 'a;
    fn visit_bound(&self) -> usize { self.len() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

macro_rules! slice_source {
    ($type:ty) => {
        impl<T> sealed::Sealed for $type {}
        impl<T> IterSource for $type {
            type Iter<'a> = std::slice::Iter<'a, T> where T: 'a;
            fn visit_bound(&self) -> usize { self.len() }
            fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
        }
    };
}
slice_source!(Vec<T>);
slice_source!(Box<[T]>);

impl<T, const N: usize> sealed::Sealed for [T; N] {}
impl<T, const N: usize> IterSource for [T; N] {
    type Iter<'a> = std::slice::Iter<'a, T> where T: 'a;
    fn visit_bound(&self) -> usize { N }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

impl sealed::Sealed for str {}
impl IterSource for str {
    type Iter<'a> = std::str::Chars<'a>;
    fn visit_bound(&self) -> usize { self.len() }
    fn source_iter(&self) -> Self::Iter<'_> { self.chars() }
}

impl<T> sealed::Sealed for VecDeque<T> {}
impl<T> IterSource for VecDeque<T> {
    type Iter<'a> = std::collections::vec_deque::Iter<'a, T> where T: 'a;
    fn visit_bound(&self) -> usize { self.len() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

impl<K, V, S> sealed::Sealed for HashMap<K, V, S> {}
impl<K, V, S> IterSource for HashMap<K, V, S> {
    type Iter<'a> = std::collections::hash_map::Iter<'a, K, V> where K: 'a, V: 'a, S: 'a;
    fn visit_bound(&self) -> usize { self.capacity() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

impl<T, S> sealed::Sealed for HashSet<T, S> {}
impl<T, S> IterSource for HashSet<T, S> {
    type Iter<'a> = std::collections::hash_set::Iter<'a, T> where T: 'a, S: 'a;
    fn visit_bound(&self) -> usize { self.capacity() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

impl<K, V> sealed::Sealed for BTreeMap<K, V> {}
impl<K, V> IterSource for BTreeMap<K, V> {
    type Iter<'a> = std::collections::btree_map::Iter<'a, K, V> where K: 'a, V: 'a;
    fn visit_bound(&self) -> usize { self.len() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}

impl<T> sealed::Sealed for BTreeSet<T> {}
impl<T> IterSource for BTreeSet<T> {
    type Iter<'a> = std::collections::btree_set::Iter<'a, T> where T: 'a;
    fn visit_bound(&self) -> usize { self.len() }
    fn source_iter(&self) -> Self::Iter<'_> { self.iter() }
}
