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
//! one of these functions, so a restated key is refused by name. A map whose
//! key is not a JSON string is an array of `[key, value]` pairs sorted by key
//! ([`pairs`]), and a set is an array sorted by value ([`set`]), in the order
//! of the key or member type's `Ord`. Each reads that order as the one
//! spelling: it refuses a repeated key or member and an array out of order.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Debug, Display};
use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer, Error, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeSeq, Serializer};

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

/// Reads an open JSON object, refusing duplicate keys at every nested depth.
pub fn json_object<'de, D>(
    deserializer: D,
) -> Result<serde_json::Map<String, serde_json::Value>, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_map(JsonObject)
}

struct JsonObject;

impl<'de> Visitor<'de> for JsonObject {
    type Value = serde_json::Map<String, serde_json::Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON object whose keys are distinct")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut fields = serde_json::Map::new();
        while let Some(key) = access.next_key::<String>()? {
            if fields.contains_key(&key) {
                return Err(A::Error::custom(format!("duplicate key {key}")));
            }
            let JsonValue(value) = access
                .next_value::<JsonValue>()
                .map_err(|error| A::Error::custom(format!("key {key}: {error}")))?;
            fields.insert(key, value);
        }
        Ok(fields)
    }
}

struct JsonValue(serde_json::Value);

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonValueVisitor).map(Self)
    }
}

struct JsonValueVisitor;

impl<'de> Visitor<'de> for JsonValueVisitor {
    type Value = serde_json::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value with distinct object keys")
    }

    fn visit_bool<E: Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| E::custom("JSON number is not finite"))
    }

    fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(value.into())
    }

    fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
        Ok(serde_json::Value::Null)
    }

    fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
        self.visit_unit()
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(JsonValue(value)) = access.next_element::<JsonValue>()? {
            values.push(value);
        }
        Ok(serde_json::Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
        JsonObject.visit_map(access).map(serde_json::Value::Object)
    }
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

/// A map with a key that is not a JSON string, as `[key, value]` pairs.
pub mod pairs {
    use super::{
        fmt, BTreeMap, Debug, Deserialize, Deserializer, Error, PhantomData, SeqAccess, Serialize,
        SerializeSeq, Serializer, Visitor,
    };

    /// Writes the entries in key order.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error for a key or value it cannot write.
    pub fn serialize<S, K, V>(map: &BTreeMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
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

    /// Reads the entries in key order, refusing a key stated twice.
    ///
    /// # Errors
    ///
    /// Names the restated key, or the first key out of order.
    pub fn deserialize<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
    where
        D: Deserializer<'de>,
        K: Deserialize<'de> + Ord + Debug,
        V: Deserialize<'de>,
    {
        struct Pairs<K, V>(PhantomData<fn() -> (K, V)>);

        impl<'de, K, V> Visitor<'de> for Pairs<K, V>
        where
            K: Deserialize<'de> + Ord + Debug,
            V: Deserialize<'de>,
        {
            type Value = BTreeMap<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter
                    .write_str("an array of [key, value] pairs in strictly increasing key order")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut out = BTreeMap::new();
                while let Some((key, value)) = access.next_element::<(K, V)>()? {
                    if out.contains_key(&key) {
                        return Err(A::Error::custom(format_args!(
                            "key {key:?} is stated twice"
                        )));
                    }
                    if out.last_key_value().is_some_and(|(last, _)| key < *last) {
                        return Err(A::Error::custom(format_args!(
                            "key {key:?} is out of key order"
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
pub mod set {
    use super::{
        fmt, BTreeSet, Deserialize, Deserializer, Error, PhantomData, SeqAccess, Serialize,
        Serializer, Visitor,
    };

    /// Writes the members in order.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error for a member it cannot write.
    pub fn serialize<S, T>(set: &BTreeSet<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        serializer.collect_seq(set)
    }

    /// Reads the members in order, refusing a member stated twice.
    ///
    /// # Errors
    ///
    /// Refuses the array at the restated member, or at the first member out
    /// of order.
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<BTreeSet<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Ord,
    {
        struct Members<T>(PhantomData<fn() -> T>);

        impl<'de, T: Deserialize<'de> + Ord> Visitor<'de> for Members<T> {
            type Value = BTreeSet<T>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an array of values in strictly increasing order")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut out = BTreeSet::new();
                while let Some(value) = access.next_element::<T>()? {
                    if out.contains(&value) {
                        return Err(A::Error::custom("a set states a member twice"));
                    }
                    if out.last().is_some_and(|last| value < *last) {
                        return Err(A::Error::custom("a set states a member out of order"));
                    }
                    out.insert(value);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_seq(Members(PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::{btree_map, json_object};

    #[test]
    fn open_json_objects_preserve_values_and_refuse_nested_duplicate_keys() {
        #[derive(Debug, serde::Deserialize)]
        struct Object(
            #[serde(deserialize_with = "json_object")] serde_json::Map<String, serde_json::Value>,
        );

        let control = r#"{"name":"text","signed":-9223372036854775808,"unsigned":18446744073709551615,"float":1.25,"true":true,"false":false,"null":null,"array":[{},[],{"name":"other"}],"object":{"name":"nested"}}"#;
        let parsed: Object = serde_json::from_str(control).expect("control object");
        assert_eq!(
            serde_json::Value::Object(parsed.0),
            serde_json::from_str::<serde_json::Value>(control).expect("control value"),
        );
        for (wire, key) in [
            (r#"{"name":1,"name":2}"#, "name"),
            (r#"{"name":1,"na\u006de":2}"#, "name"),
            (r#"{"outer":{"nested":1,"nested":2}}"#, "nested"),
            (r#"{"outer":[{"nested":1,"nested":2}]}"#, "nested"),
        ] {
            let error =
                serde_json::from_str::<Object>(wire).expect_err("a restated key is refused");
            assert!(
                error.to_string().contains(&format!("duplicate key {key}")),
                "{error}"
            );
        }
    }

    #[derive(Debug, serde::Deserialize)]
    struct Maps {
        #[serde(deserialize_with = "btree_map")]
        ordered: BTreeMap<String, u64>,
    }

    #[test]
    fn map_errors_name_the_source_key_and_refuse_duplicates_before_their_values() {
        let admitted: Maps =
            serde_json::from_str(r#"{"ordered":{"one":1}}"#).expect("distinct keys are admitted");
        assert_eq!(admitted.ordered["one"], 1);
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
            let wire = format!(r#"{{"ordered":{{{entries}}}}}"#);
            let error =
                serde_json::from_str::<Maps>(&wire).expect_err("the map refuses this entry");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Sorted {
        #[serde(with = "super::pairs")]
        pairs: BTreeMap<u32, String>,
        #[serde(with = "super::set")]
        set: BTreeSet<u32>,
    }

    #[test]
    fn pairs_and_sets_write_sorted_arrays_and_refuse_a_repeated_key_or_member() {
        let wire = r#"{"pairs":[[1,"a"],[2,"b"]],"set":[1,2]}"#;
        let sorted: Sorted = serde_json::from_str(wire).expect("distinct entries are admitted");
        assert_eq!(serde_json::to_string(&sorted).expect("writes"), wire);
        for (wire, expected) in [
            (
                r#"{"pairs":[[1,"a"],[1,"b"]],"set":[]}"#,
                "key 1 is stated twice",
            ),
            (
                r#"{"pairs":[[1,"a"],[2,"b"],[1,"c"]],"set":[]}"#,
                "key 1 is stated twice",
            ),
            (r#"{"pairs":[],"set":[1,1]}"#, "a set states a member twice"),
            (
                r#"{"pairs":[],"set":[1,2,1]}"#,
                "a set states a member twice",
            ),
        ] {
            let error = serde_json::from_str::<Sorted>(wire).expect_err("a repeat is refused");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn pairs_and_sets_refuse_an_array_out_of_order() {
        for (wire, expected) in [
            (
                r#"{"pairs":[[2,"b"],[1,"a"]],"set":[]}"#,
                "key 1 is out of key order",
            ),
            (
                r#"{"pairs":[[1,"a"],[3,"c"],[2,"b"]],"set":[]}"#,
                "key 2 is out of key order",
            ),
            (
                r#"{"pairs":[],"set":[2,1]}"#,
                "a set states a member out of order",
            ),
            (
                r#"{"pairs":[],"set":[1,3,2]}"#,
                "a set states a member out of order",
            ),
        ] {
            let error =
                serde_json::from_str::<Sorted>(wire).expect_err("an unsorted array is refused");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }
}
