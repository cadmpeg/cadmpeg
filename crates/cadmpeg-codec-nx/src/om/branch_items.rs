// SPDX-License-Identifier: Apache-2.0
//! Explicit entries of a byte-counted branch lane with one implicit slot.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct BranchItems<T>(Vec<T>, #[serde(skip)] u8);

impl<T> BranchItems<T> {
    pub(crate) fn new(items: Vec<T>) -> Result<Self, &'static str> {
        if !(1..=254).contains(&items.len()) {
            return Err("branch items must contain 1 through 254 entries");
        }
        let count = u8::try_from(items.len() + 1)
            .map_err(|_| "branch items must contain 1 through 254 entries")?;
        Ok(Self(items, count))
    }

    pub(crate) fn declared_count(&self) -> u8 {
        self.1
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub(crate) fn into_vec(self) -> Vec<T> {
        self.0
    }

    pub(crate) fn map_indexed<U>(self, mut f: impl FnMut(usize, T) -> U) -> BranchItems<U> {
        BranchItems(
            self.0
                .into_iter()
                .enumerate()
                .map(|(i, item)| f(i, item))
                .collect(),
            self.1,
        )
    }

    pub(crate) fn map_indexed_charged<U>(
        self,
        ctx: &DecodeContext<'_>,
        mut f: impl FnMut(usize, T) -> U,
    ) -> Result<BranchItems<U>, CodecError> {
        let count = self.0.len();
        let mut mapped = ctx.collection_vec(count, "NX branch item mapping")?;
        for (index, item) in ctx
            .admit_iter(self.0, "NX branch item mapping visits")?
            .enumerate()
        {
            mapped.push(f(index, item));
        }
        Ok(BranchItems(mapped, self.1))
    }

    pub(crate) fn try_map_indexed_charged<U>(
        self,
        ctx: &DecodeContext<'_>,
        mut f: impl FnMut(usize, T) -> Result<U, CodecError>,
    ) -> Result<BranchItems<U>, CodecError> {
        let count = self.0.len();
        let mut mapped = ctx.collection_vec(count, "NX branch item mapping")?;
        let mut items = self.0.into_iter().enumerate();
        while let Some((index, item)) =
            ctx.next_charged(&mut items, "NX fallible branch item mapping visits")?
        {
            mapped.push(f(index, item)?);
        }
        Ok(BranchItems(mapped, self.1))
    }
}

impl<T> BranchItems<Option<T>> {
    #[cfg(test)]
    pub(crate) fn transpose(self) -> Option<BranchItems<T>> {
        Some(BranchItems(
            self.0.into_iter().collect::<Option<Vec<_>>>()?,
            self.1,
        ))
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for BranchItems<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn branch_mapping_refuses_at_its_own_visit_boundary() {
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX branch item mapping visits",
            |ctx| {
                super::BranchItems::new(vec![1, 2])
                    .unwrap()
                    .map_indexed_charged(ctx, |index, item| index + item)
            },
        );
        crate::test_support::resource_refusal_at(
            &[],
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "NX fallible branch item mapping visits",
            |ctx| {
                super::BranchItems::new(vec![1, 2])
                    .unwrap()
                    .try_map_indexed_charged(ctx, |index, item| Ok(index + item))
            },
        );
    }

    use super::BranchItems;

    #[test]
    fn branch_count_includes_one_implicit_slot_and_fits_a_byte() {
        for len in [1, 254] {
            let items = BranchItems::new(vec![0; len]).unwrap();
            assert_eq!(usize::from(items.declared_count()), len + 1);
            let mapped = items.map_indexed(|index, _| index);
            assert_eq!(mapped.len(), len);
            assert_eq!(usize::from(mapped.declared_count()), len + 1);
        }
        for len in [0, 255] {
            assert!(BranchItems::new(vec![0; len]).is_err());
            let json = serde_json::to_string(&vec![0; len]).unwrap();
            assert!(serde_json::from_str::<BranchItems<u8>>(&json).is_err());
        }
    }
    #[test]
    fn resolution_retains_the_declared_count_or_rejects_the_whole_lane() {
        let items = BranchItems::new(vec![Some(0), Some(1)])
            .unwrap()
            .transpose()
            .unwrap();
        assert_eq!(items.declared_count(), 3);
        assert_eq!(items.as_slice(), [0, 1]);
        assert!(BranchItems::new(vec![Some(0), None])
            .unwrap()
            .transpose()
            .is_none());
    }
}
