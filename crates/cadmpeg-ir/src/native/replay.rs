// SPDX-License-Identifier: Apache-2.0
//! Replay stored JSON text into a serializer or a single field.
//!
//! [`emit`] drives a serializer from the parse; [`field`] materializes one
//! member while skipping the rest.

use std::borrow::Cow;
use std::fmt;

use serde::{de, ser};
use serde_json::{value::RawValue, Value};

/// Emit the JSON value held in `json` into `serializer`.
///
pub(super) fn emit<S: ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let emitted = de::Deserializer::deserialize_any(&mut deserializer, Emit(serializer))
        .map_err(de_to_ser)?;
    deserializer.end().map_err(de_to_ser)?;
    Ok(emitted)
}

/// Parse the `name` member of the JSON object held in `json`.
///
pub(super) fn field(json: &str, name: &str) -> Result<Option<Value>, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let found = de::Deserializer::deserialize_map(&mut deserializer, PickField(name))?;
    deserializer.end()?;
    Ok(found)
}

/// Read an owned type from a constructor-produced canonical JSON record.
///
/// Common records use the direct parser. Deeper records go through replay's
/// one-container reads, then the Value deserializer, which has no JSON nesting
/// ceiling. The cutoff is a conservative fast-path bound, not an admission
/// limit. Scanning avoids materializing a second tree for every typed read.
pub(super) fn parse<T: de::DeserializeOwned>(json: &str) -> Result<T, serde_json::Error> {
    const DIRECT_JSON_DEPTH: usize = 64;
    let mut depth = 0;
    let mut bytes = json.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'"' => {
                while let Some(quoted) = bytes.next() {
                    match quoted {
                        b'\\' => {
                            bytes.next();
                        }
                        b'"' => break,
                        _ => {}
                    }
                }
            }
            b'[' | b'{' => {
                depth += 1;
                if depth > DIRECT_JSON_DEPTH {
                    return T::deserialize(emit(json, serde_json::value::Serializer)?);
                }
            }
            // The input is a complete object written by the native
            // constructors; a closing container always has an opening one.
            b']' | b'}' => depth -= 1,
            _ => {}
        }
    }
    serde_json::from_str(json)
}

fn ser_to_de<S: ser::Error, D: de::Error>(error: S) -> D {
    de::Error::custom(error)
}

fn de_to_ser<D: de::Error, S: ser::Error>(error: D) -> S {
    ser::Error::custom(error)
}

/// Writes whatever it is handed straight into a serializer.
struct Emit<S>(S);

impl<'de, S: ser::Serializer> de::Visitor<'de> for Emit<S> {
    type Value = S::Ok;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.serialize_unit().map_err(ser_to_de)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        self.0.serialize_bool(value).map_err(ser_to_de)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.0.serialize_i64(value).map_err(ser_to_de)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        self.0.serialize_u64(value).map_err(ser_to_de)
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        self.0.serialize_i128(value).map_err(ser_to_de)
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Self::Value, E> {
        self.0.serialize_u128(value).map_err(ser_to_de)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.0.serialize_f64(value).map_err(ser_to_de)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.0.serialize_str(value).map_err(ser_to_de)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut sequence = self
            .0
            .serialize_seq(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(value) = access.next_element::<&RawValue>()? {
            ser::SerializeSeq::serialize_element(&mut sequence, &Replay(value.get()))
                .map_err(ser_to_de)?;
        }
        ser::SerializeSeq::end(sequence).map_err(ser_to_de)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = self
            .0
            .serialize_map(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(key) = access.next_key_seed(Key)? {
            ser::SerializeMap::serialize_key(&mut map, key.as_ref()).map_err(ser_to_de)?;
            let value = access.next_value::<&RawValue>()?;
            ser::SerializeMap::serialize_value(&mut map, &Replay(value.get()))
                .map_err(ser_to_de)?;
        }
        ser::SerializeMap::end(map).map_err(ser_to_de)
    }
}

/// A repeatable borrowed JSON value handed to an arbitrary serializer.
///
/// A serializer may read a value more than once, or skip it. Consume the raw
/// child before handing it over, then start a fresh parse for each read.
/// Each parser reads only one container; `RawValue` scans its children without
/// the parser recursion limit. No parsed child tree is retained here.
struct Replay<'a>(&'a str);

impl ser::Serialize for Replay<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        emit(self.0, serializer)
    }
}

/// Reads a JSON object key, borrowing from the source text when it has no
/// escapes.
struct Key;

impl<'de> de::DeserializeSeed<'de> for Key {
    type Value = Cow<'de, str>;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(Key)
    }
}

impl<'de> de::Visitor<'de> for Key {
    type Value = Cow<'de, str>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value.to_owned()))
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
        Ok(Cow::Borrowed(value))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Cow::Owned(value))
    }
}

/// Materializes one named member of a JSON object and discards the others.
struct PickField<'a>(&'a str);

impl<'de> de::Visitor<'de> for PickField<'_> {
    type Value = Option<Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut found = None;
        // Every remaining key is read even once the member is found, because
        // the deserializer checks that the object was consumed to its closing
        // brace. Skipping a value costs a scan and no allocation.
        while let Some(key) = access.next_key_seed(Key)? {
            if found.is_none() && key.as_ref() == self.0 {
                let raw = access.next_value::<&RawValue>()?;
                found = Some(emit(raw.get(), serde_json::value::Serializer).map_err(ser_to_de)?);
            } else {
                access.next_value::<de::IgnoredAny>()?;
            }
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{emit, field};
    use serde_json::Value;

    const TEXT: &str = concat!(
        r#"{"id":"pin#0","a":[null,true,false,-1,0,1.5,2.0,1e-7,10000000000.0],"#,
        r#""b":{"":[],"c\td":{},"nested":{"a":[[[1]]]}},"#,
        r#""é key":"quote\" back\\ tab\t bell\u0007 é","z":18446744073709551615}"#
    );

    #[test]
    fn emits_the_value_a_parse_would_produce() {
        let replayed = emit(TEXT, serde_json::value::Serializer).unwrap();
        assert_eq!(replayed, serde_json::from_str::<Value>(TEXT).unwrap());
    }

    #[test]
    fn compact_emission_reproduces_the_source_text() {
        let mut out = Vec::new();
        emit(TEXT, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), TEXT);
    }

    #[test]
    fn a_borrowed_member_can_be_serialized_more_than_once() {
        let member = super::Replay(TEXT);
        let expected = serde_json::from_str::<Value>(TEXT).unwrap();
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
        assert_eq!(serde_json::to_string(&member).unwrap(), TEXT);
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
    }

    #[test]
    fn direct_and_deep_typed_reads_preserve_container_depth_and_quoted_brackets() {
        for depth in [0, 31, 63, 64, 65, 127, 128, 140] {
            let mut nested = serde_json::json!({"value": "[\\\"{}]", "number": u64::MAX});
            for _ in 0..depth {
                nested = Value::Array(vec![nested]);
            }
            let expected = serde_json::json!({"[\\\"{}]": nested});
            let text = expected.to_string();
            assert_eq!(super::parse::<Value>(&text).unwrap(), expected, "{depth}");
        }
    }

    /// Picking one member matches parsing the whole object and removing it,
    /// including for absent members and for keys carrying escapes.
    #[test]
    fn picks_the_member_a_full_parse_would_yield() {
        let whole = serde_json::from_str::<serde_json::Map<String, Value>>(TEXT).unwrap();
        for name in ["id", "a", "b", "é key", "z", "missing", ""] {
            assert_eq!(
                field(TEXT, name).unwrap(),
                whole.get(name).cloned(),
                "{name}"
            );
        }
    }
}
