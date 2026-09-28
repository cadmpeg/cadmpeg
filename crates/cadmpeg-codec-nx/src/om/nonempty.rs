// SPDX-License-Identifier: Apache-2.0
//! A sequence that contains its first element at construction.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonEmpty<T> {
    first: T,
    rest: Vec<T>,
}

impl<T> NonEmpty<T> {
    pub(crate) fn from_vec(mut values: Vec<T>) -> Option<Self> {
        if values.is_empty() {
            return None;
        }
        let first = values.remove(0);
        Some(Self { first, rest: values })
    }
    pub(crate) fn new(values: impl IntoIterator<Item = T>) -> Option<Self> {
        let mut values = values.into_iter();
        Some(Self {
            first: values.next()?,
            rest: values.collect(),
        })
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + Clone {
        std::iter::once(&self.first).chain(&self.rest)
    }

    pub(crate) fn len(&self) -> usize {
        1 + self.rest.len()
    }

    pub(crate) fn first(&self) -> &T {
        &self.first
    }

    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index == 0 {
            Some(&self.first)
        } else {
            self.rest.get(index - 1)
        }
    }

    pub(crate) fn last(&self) -> &T {
        self.rest.last().unwrap_or(&self.first)
    }

    pub(crate) fn map<U>(self, mut map: impl FnMut(T) -> U) -> NonEmpty<U> {
        NonEmpty {
            first: map(self.first),
            rest: self.rest.into_iter().map(map).collect(),
        }
    }

    pub(crate) fn map_charged<U>(self, ctx: &DecodeContext<'_>, mut map: impl FnMut(T) -> U) -> Result<NonEmpty<U>, CodecError> {
        let first = map(self.first);
        let mut rest = Vec::new();
        for value in self.rest {
            ctx.charge_collection_items(1, "nx nonempty mapped entries")?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<U>()), "nx nonempty mapped entries")?;
            rest.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("nx nonempty mapped entries", 0, 1))?;
            rest.push(map(value));
        }
        Ok(NonEmpty { first, rest })
    }
}

impl<T> NonEmpty<Option<T>> {
    pub(super) fn transpose(self) -> Option<NonEmpty<T>> {
        Some(NonEmpty {
            first: self.first?,
            rest: self.rest.into_iter().collect::<Option<Vec<_>>>()?,
        })
    }
}

impl<T> IntoIterator for NonEmpty<T> {
    type Item = T;
    type IntoIter = std::iter::Chain<std::iter::Once<T>, std::vec::IntoIter<T>>;
    fn into_iter(self) -> Self::IntoIter {
        std::iter::once(self.first).chain(self.rest)
    }
}
