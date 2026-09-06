// SPDX-License-Identifier: Apache-2.0
//! Explicit entries of a byte-counted branch lane with one implicit slot.

use serde::{Deserialize, Deserializer, Serialize};

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

    pub(crate) fn map_indexed<U>(self, mut f: impl FnMut(usize, T) -> U) -> BranchItems<U> {
        BranchItems(
            self.0
                .into_iter()
                .enumerate()
                .map(|(i, item)| f(i, item))
                .collect(),
        )
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for BranchItems<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(Vec::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
