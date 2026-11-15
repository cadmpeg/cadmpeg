// SPDX-License-Identifier: Apache-2.0
//! Readers and writers for the §2.5 forms that a derive cannot state alone.
//!
//! A map whose key is a unit enum is a JSON object keyed by the variant's
//! name. A map whose key is not a JSON string is an array of `[key, value]`
//! pairs sorted by key. A set is an array sorted by value. Each reader refuses
//! a repeated key or member.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

/// A set read from an array that refuses a member stated twice.
pub(super) struct Distinct<T>(pub(super) BTreeSet<T>);

impl<'de, T: Deserialize<'de> + Ord> Deserialize<'de> for Distinct<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        set::deserialize(deserializer).map(Distinct)
    }
}

/// Reads a map keyed by a unit enum from a JSON object, and refuses a key
/// stated twice. The derive writes such a map as an object in key order.
pub(super) fn object<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord + fmt::Debug,
    V: Deserialize<'de>,
{
    struct Object<K, V>(PhantomData<fn() -> (K, V)>);

    impl<'de, K, V> Visitor<'de> for Object<K, V>
    where
        K: Deserialize<'de> + Ord + fmt::Debug,
        V: Deserialize<'de>,
    {
        type Value = BTreeMap<K, V>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a JSON object with distinct keys")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(key) = access.next_key::<K>()? {
                if out.contains_key(&key) {
                    return Err(de::Error::custom(format_args!(
                        "key {key:?} is stated twice"
                    )));
                }
                let value = access.next_value::<V>()?;
                out.insert(key, value);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_map(Object(PhantomData))
}

/// A map with a key that is not a JSON string, as `[key, value]` pairs.
pub(super) mod pairs {
    use super::{
        de, fmt, BTreeMap, Deserialize, Deserializer, PhantomData, SeqAccess, Serialize,
        SerializeSeq, Serializer, Visitor,
    };

    /// Writes the entries in key order.
    pub(in crate::process) fn serialize<S, K, V>(
        map: &BTreeMap<K, V>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        K: Serialize,
        V: Serialize,
    {
        let mut out = serializer.serialize_seq(Some(map.len()))?;
        for entry in map {
            out.serialize_element(&entry)?;
        }
        out.end()
    }

    /// Reads the entries and refuses a key stated twice.
    pub(in crate::process) fn deserialize<'de, D, K, V>(
        deserializer: D,
    ) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord + fmt::Debug,
        V: Deserialize<'de>,
    {
        struct Pairs<K, V>(PhantomData<fn() -> (K, V)>);

        impl<'de, K, V> Visitor<'de> for Pairs<K, V>
        where
            K: Deserialize<'de> + Ord + fmt::Debug,
            V: Deserialize<'de>,
        {
            type Value = BTreeMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an array of [key, value] pairs with distinct keys")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut out = BTreeMap::new();
                while let Some((key, value)) = access.next_element::<(K, V)>()? {
                    if out.contains_key(&key) {
                        return Err(de::Error::custom(format_args!(
                            "key {key:?} is stated twice"
                        )));
                    }
                    out.insert(key, value);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_seq(Pairs(PhantomData))
    }
}

/// A set, as an array sorted by value.
pub(super) mod set {
    use super::{
        de, fmt, BTreeSet, Deserialize, Deserializer, PhantomData, SeqAccess, Serialize,
        Serializer, Visitor,
    };

    /// Writes the members in order.
    pub(in crate::process) fn serialize<S, T>(
        set: &BTreeSet<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        serializer.collect_seq(set)
    }

    /// Reads the members and refuses a member stated twice.
    pub(in crate::process) fn deserialize<'de, D, T>(
        deserializer: D,
    ) -> Result<BTreeSet<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Ord,
    {
        struct Members<T>(PhantomData<fn() -> T>);

        impl<'de, T: Deserialize<'de> + Ord> Visitor<'de> for Members<T> {
            type Value = BTreeSet<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an array of distinct values")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut out = BTreeSet::new();
                while let Some(value) = access.next_element::<T>()? {
                    if !out.insert(value) {
                        return Err(de::Error::custom("a set states a member twice"));
                    }
                }
                Ok(out)
            }
        }

        deserializer.deserialize_seq(Members(PhantomData))
    }
}
