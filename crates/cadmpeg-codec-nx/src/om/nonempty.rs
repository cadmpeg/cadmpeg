// SPDX-License-Identifier: Apache-2.0
//! Nonempty sequences with charged collection and allocation-free transfer.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonEmpty<T> {
    initial: Vec<T>,
    last: T,
}

impl<T> NonEmpty<T> {
    /// Transfer admitted storage without allocation, copying or shifting elements.
    pub(crate) fn from_admitted_vec(mut values: Vec<T>) -> Option<Self> {
        let last = values.pop()?;
        Some(Self {
            initial: values,
            last,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(values: impl IntoIterator<Item = T>) -> Option<Self> {
        Self::from_admitted_vec(values.into_iter().collect())
    }

    pub(crate) fn new_charged(
        ctx: &DecodeContext<'_>,
        values: impl IntoIterator<Item = T>,
    ) -> Result<Option<Self>, CodecError> {
        Ok(Self::from_admitted_vec(
            ctx.collect_retained_vec(values, "NX nonempty entries")?,
        ))
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + Clone {
        self.initial.iter().chain(std::iter::once(&self.last))
    }

    pub(crate) fn len(&self) -> usize {
        self.initial.len() + 1
    }
    pub(crate) fn first(&self) -> &T {
        self.initial.first().unwrap_or(&self.last)
    }
    pub(crate) fn last(&self) -> &T {
        &self.last
    }
    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index == self.initial.len() {
            Some(&self.last)
        } else {
            self.initial.get(index)
        }
    }

    pub(crate) fn map_charged<U>(
        self,
        ctx: &DecodeContext<'_>,
        mut map: impl FnMut(T) -> U,
    ) -> Result<NonEmpty<U>, CodecError> {
        let mut initial = Vec::new();
        for value in self.initial {
            ctx.charge_work(1, "NX nonempty mapped entries")?;
            ctx.reserve_retained_vec(&mut initial, 1, "nx nonempty mapped entries")?;
            initial.push(map(value));
        }
        ctx.charge_work(1, "NX nonempty mapped entries")?;
        ctx.charge_collection_items(1, "NX nonempty mapped entries")?;
        Ok(NonEmpty {
            initial,
            last: map(self.last),
        })
    }

    pub(crate) fn try_map_charged<U>(
        self,
        ctx: &DecodeContext<'_>,
        mut map: impl FnMut(T) -> Option<U>,
    ) -> Result<Option<NonEmpty<U>>, CodecError> {
        let mut initial = Vec::new();
        for value in self.initial {
            ctx.charge_work(1, "NX nonempty mapped entries")?;
            let Some(value) = map(value) else {
                return Ok(None);
            };
            ctx.reserve_retained_vec(&mut initial, 1, "NX nonempty mapped entries")?;
            initial.push(value);
        }
        ctx.charge_work(1, "NX nonempty mapped entries")?;
        ctx.charge_collection_items(1, "NX nonempty mapped entries")?;
        let Some(last) = map(self.last) else {
            return Ok(None);
        };
        Ok(Some(NonEmpty { initial, last }))
    }
}

impl<T> NonEmpty<Option<T>> {
    pub(super) fn transpose_charged(
        self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<NonEmpty<T>>, CodecError> {
        self.try_map_charged(ctx, |value| value)
    }
}

impl<T> IntoIterator for NonEmpty<T> {
    type Item = T;
    type IntoIter = std::iter::Chain<std::vec::IntoIter<T>, std::iter::Once<T>>;
    fn into_iter(self) -> Self::IntoIter {
        self.initial.into_iter().chain(std::iter::once(self.last))
    }
}

#[cfg(test)]
mod tests {
    use super::NonEmpty;

    #[test]
    fn nonempty_admitted_transfer_preserves_storage_and_sequence() {
        let mut values = Vec::with_capacity(32);
        values.extend([10, 20, 30]);
        let storage = values.as_ptr();
        let capacity = values.capacity();
        let values = NonEmpty::from_admitted_vec(values).unwrap();
        assert_eq!(values.initial.as_ptr(), storage);
        assert_eq!(values.initial.capacity(), capacity);
        assert_eq!(values.iter().copied().collect::<Vec<_>>(), [10, 20, 30]);
        assert_eq!((*values.first(), *values.last(), values.len()), (10, 30, 3));
        assert_eq!(values.get(2), Some(&30));
        assert_eq!(values.get(3), None);
        assert_eq!(values.into_iter().collect::<Vec<_>>(), [10, 20, 30]);
        assert!(NonEmpty::<u8>::from_admitted_vec(Vec::new()).is_none());
    }

    #[test]
    fn nonempty_mapping_preserves_visitation_order_and_singletons() {
        crate::test_support::with_decode_context(|ctx| {
            for input in [vec![10], vec![10, 20, 30]] {
                let mut visited = Vec::new();
                let mapped = NonEmpty::from_admitted_vec(input.clone())
                    .unwrap()
                    .map_charged(ctx, |value| {
                        visited.push(value);
                        value + 1
                    })
                    .unwrap();
                assert_eq!(visited, input);
                assert_eq!(
                    mapped.into_iter().collect::<Vec<_>>(),
                    input.iter().map(|value| value + 1).collect::<Vec<_>>()
                );
                visited.clear();
                let mapped = NonEmpty::from_admitted_vec(input.clone())
                    .unwrap()
                    .try_map_charged(ctx, |value| {
                        visited.push(value);
                        Some(value + 1)
                    })
                    .unwrap()
                    .unwrap();
                assert_eq!(visited, input);
                assert_eq!(
                    mapped.into_iter().collect::<Vec<_>>(),
                    input.iter().map(|value| value + 1).collect::<Vec<_>>()
                );
            }
        });
    }

    #[test]
    fn nonempty_collection_refuses_unadmitted_work() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_work_units = 0,
            |ctx| {
                assert!(
                    matches!(NonEmpty::new_charged(ctx, [1, 2]), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
                );
            },
        );
    }

    #[test]
    fn nonempty_collection_refuses_the_live_context() {
        crate::test_support::with_decode_context_over(
            &[],
            |policy| policy.limits.max_collection_items = 0,
            |ctx| {
                assert!(matches!(
                    NonEmpty::new_charged(ctx, [1, 2]),
                    Err(cadmpeg_core::CodecError::ResourceLimit(_))
                ));
            },
        );
    }
}
