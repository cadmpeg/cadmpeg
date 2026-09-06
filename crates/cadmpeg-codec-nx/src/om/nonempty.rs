// SPDX-License-Identifier: Apache-2.0
//! A sequence that contains its first element at construction.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonEmpty<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmpty<T> {
    pub(crate) fn new(values: impl IntoIterator<Item = T>) -> Option<Self> {
        let mut values = values.into_iter();
        Some(Self { first: values.next()?, rest: values.collect() })
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        std::iter::once(&self.first).chain(&self.rest)
    }

    pub(crate) fn last(&self) -> &T {
        self.rest.last().unwrap_or(&self.first)
    }

    pub(crate) fn map<U>(self, mut map: impl FnMut(T) -> U) -> NonEmpty<U> {
        NonEmpty { first: map(self.first), rest: self.rest.into_iter().map(map).collect() }
    }
}
