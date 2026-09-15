// SPDX-License-Identifier: Apache-2.0
//! Map readers that refuse a duplicate object key.
//!
//! `serde_json` hands every object key to the visitor, duplicates included,
//! and a derived `BTreeMap` or `HashMap` keeps the last. A document that
//! states one identity twice therefore reads back with one of the two values
//! silently gone, and the map type cannot see it: by the time the map exists,
//! the first value is already overwritten.
//!
//! Every map whose key is an identity or a source-supplied name reads through
//! one of these functions, so a restated key is refused by name.

use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display};
use std::hash::Hash;
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer, Error, MapAccess, Visitor};

/// Reads a `BTreeMap`, refusing a key the document states twice.
///
/// # Errors
///
/// Names the restated key.
pub fn btree_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Ord + Display,
    V: Deserialize<'de>,
{
    deserializer.deserialize_map(DistinctBTreeMap(PhantomData))
}

/// Reads a `HashMap`, refusing a key the document states twice.
///
/// # Errors
///
/// Names the restated key.
pub fn hash_map<'de, D, K, V>(deserializer: D) -> Result<HashMap<K, V>, D::Error>
where
    D: Deserializer<'de>,
    K: Deserialize<'de> + Eq + Hash + Display,
    V: Deserialize<'de>,
{
    deserializer.deserialize_map(DistinctHashMap(PhantomData))
}

struct DistinctBTreeMap<K, V>(PhantomData<fn() -> (K, V)>);

impl<'de, K, V> Visitor<'de> for DistinctBTreeMap<K, V>
where
    K: Deserialize<'de> + Ord + Display,
    V: Deserialize<'de>,
{
    type Value = BTreeMap<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map whose keys are distinct")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = BTreeMap::new();
        while let Some(key) = access.next_key::<K>()? {
            // The lookup precedes the insert so the refusal can name the key
            // the document restated. Rendering every key instead would cost
            // one allocation per entry on the reading path.
            if map.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate key {key}")));
            }
            let value = access
                .next_value::<V>()
                .map_err(|error| A::Error::custom(format!("key {key}: {error}")))?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

struct DistinctHashMap<K, V>(PhantomData<fn() -> (K, V)>);

impl<'de, K, V> Visitor<'de> for DistinctHashMap<K, V>
where
    K: Deserialize<'de> + Eq + Hash + Display,
    V: Deserialize<'de>,
{
    type Value = HashMap<K, V>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map whose keys are distinct")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = HashMap::new();
        while let Some(key) = access.next_key::<K>()? {
            // As above: look up first so the refusal names the restated key
            // without rendering every key the document states.
            if map.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate key {key}")));
            }
            let value = access
                .next_value::<V>()
                .map_err(|error| A::Error::custom(format!("key {key}: {error}")))?;
            map.insert(key, value);
        }
        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    struct Maps {
        #[serde(deserialize_with = "btree_map")]
        ordered: BTreeMap<String, u64>,
        #[serde(deserialize_with = "hash_map")]
        hashed: HashMap<String, u64>,
    }

    #[test]
    fn map_errors_name_the_source_key_and_refuse_duplicates_before_their_values() {
        let admitted: Maps =
            serde_json::from_str(r#"{"ordered":{"one":1},"hashed":{"two":2}}"#).unwrap();
        assert_eq!(admitted.ordered["one"], 1);
        assert_eq!(admitted.hashed["two"], 2);
        for map in ["ordered", "hashed"] {
            let other = if map == "ordered" {
                "hashed"
            } else {
                "ordered"
            };
            for (entries, expected) in [
                (
                    r#""source-record":false"#,
                    "key source-record: invalid type",
                ),
                (
                    r#""source-record":1,"source-record":2"#,
                    "duplicate key source-record",
                ),
                (
                    r#""source-record":1,"source-record":false"#,
                    "duplicate key source-record",
                ),
            ] {
                let wire = format!(r#"{{"{map}":{{{entries}}},"{other}":{{}}}}"#);
                let error = serde_json::from_str::<Maps>(&wire).unwrap_err();
                assert!(error.to_string().contains(expected), "{error}");
            }
        }
    }
}
