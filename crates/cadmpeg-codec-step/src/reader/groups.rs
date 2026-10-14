// SPDX-License-Identifier: Apache-2.0
//! Nonempty groups with an inline first member.

#[derive(Debug)]
pub(super) struct NonemptyGroup<T> {
    first: T,
    additional: Vec<T>,
}

impl<T> NonemptyGroup<T> {
    pub(super) fn new(first: T) -> Self {
        Self {
            first,
            additional: Vec::new(),
        }
    }

    pub(super) fn first(&self) -> &T {
        &self.first
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.first).chain(self.additional.iter())
    }

    pub(super) fn push(&mut self, value: T) {
        self.additional.push(value);
    }
}

impl<'a, T> IntoIterator for &'a NonemptyGroup<T> {
    type Item = &'a T;
    type IntoIter = std::iter::Chain<std::iter::Once<&'a T>, std::slice::Iter<'a, T>>;

    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(&self.first).chain(self.additional.iter())
    }
}
