// SPDX-License-Identifier: Apache-2.0
//! Explicit entries of a byte-counted branch lane with one implicit slot.

use serde::{Deserialize, Deserializer, Serialize};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct BranchItems<T>(Vec<T>);

impl<T> BranchItems<T> {
    pub(crate) fn new(items: Vec<T>) -> Result<Self, &'static str> {
        if !(1..=254).contains(&items.len()) {
            return Err("branch items must contain 1 through 254 entries");
        }
        Ok(Self(items))
    }

    pub(crate) fn declared_count(&self) -> u8 {
        (self.0.len() + 1) as u8
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }

    #[cfg(test)]
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
        )
    }

    pub(crate) fn map_indexed_charged<U>(self, ctx: &DecodeContext<'_>, mut f: impl FnMut(usize, T) -> U) -> Result<BranchItems<U>, CodecError> {
        let count = self.0.len();
        let count_u64 = u64_from_index(count);
        let bytes = count_u64.checked_mul(u64_from_index(std::mem::size_of::<U>()))
            .ok_or_else(|| ctx.refuse_codec_limit("NX branch item mapping", u64::MAX, u64::MAX))?;
        ctx.charge_collection_items(count_u64, "NX branch item mapping")?;
        ctx.charge_retained(bytes, "NX branch item mapping")?;
        let mut mapped = Vec::new();
        mapped.try_reserve_exact(count).map_err(|_| ctx.refuse_codec_limit("NX branch item mapping", 0, count_u64))?;
        for (index, item) in self.0.into_iter().enumerate() {
            mapped.push(f(index, item));
        }
        Ok(BranchItems(mapped))
    }
}

impl<T> BranchItems<Option<T>> {
    pub(crate) fn transpose(self) -> Option<BranchItems<T>> {
        Some(BranchItems(self.0.into_iter().collect::<Option<Vec<_>>>()?))
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for BranchItems<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
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
