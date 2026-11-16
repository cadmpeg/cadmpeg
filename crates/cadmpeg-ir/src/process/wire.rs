// SPDX-License-Identifier: Apache-2.0
//! The set wire an optional set field reads through.
//!
//! `cadmpeg_core::named_optional_field!` reads an optional field through its
//! type's own reader, and a `BTreeSet` merges a repeated member. An optional
//! set reads [`Distinct`] instead and converts it.

use std::collections::BTreeSet;

use serde::de::{self, Deserialize, Deserializer};

/// A set read from an array that refuses a member stated twice.
pub(super) struct Distinct<T>(pub(super) BTreeSet<T>);

impl<'de, T: Deserialize<'de> + Ord> Deserialize<'de> for Distinct<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let members = Vec::<T>::deserialize(deserializer)?;
        let stated = members.len();
        let set = BTreeSet::from_iter(members);
        if set.len() != stated {
            return Err(de::Error::custom("a set states a member twice"));
        }
        Ok(Distinct(set))
    }
}

impl<T> From<Distinct<T>> for BTreeSet<T> {
    fn from(set: Distinct<T>) -> Self {
        set.0
    }
}
