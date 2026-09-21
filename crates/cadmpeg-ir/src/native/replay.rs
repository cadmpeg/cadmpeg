// SPDX-License-Identifier: Apache-2.0
//! Replay stored JSON text into a serializer.
//!
//! [`emit`] drives a serializer from the parse, reading one container at a
//! time. Each container is a parse of its own, so the parser's own recursion
//! limit never accumulates; the descent is counted here instead, against the
//! one bound every native record field answers to.

use std::borrow::Cow;
use std::fmt;

use serde::{de, ser};
use serde_json::value::RawValue;

use super::MAX_NATIVE_NESTING_DEPTH;

/// Emit the JSON value held in `json` into `serializer`.
///
/// `json` is one native record field, so it may enter
/// [`MAX_NATIVE_NESTING_DEPTH`] containers.
pub(super) fn emit<S: ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
    emit_within(json, serializer, MAX_NATIVE_NESTING_DEPTH)
}

/// Emit `json`, entering at most `depth` further containers.
fn emit_within<S: ser::Serializer>(
    json: &str,
    serializer: S,
    depth: usize,
) -> Result<S::Ok, S::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let emitted = de::Deserializer::deserialize_any(&mut deserializer, Emit { serializer, depth })
        .map_err(de_to_ser)?;
    deserializer.end().map_err(de_to_ser)?;
    Ok(emitted)
}

/// The refusal a container past the bound reports.
fn nests_too_deep<E: de::Error>() -> E {
    de::Error::custom(format!(
        "native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
    ))
}

fn ser_to_de<S: ser::Error, D: de::Error>(error: S) -> D {
    de::Error::custom(error)
}

fn de_to_ser<D: de::Error, S: ser::Error>(error: D) -> S {
    ser::Error::custom(error)
}

/// Writes whatever it is handed straight into a serializer.
struct Emit<S> {
    /// Where the value is written.
    serializer: S,
    /// Containers this value may still enter.
    depth: usize,
}

impl<'de, S: ser::Serializer> de::Visitor<'de> for Emit<S> {
    type Value = S::Ok;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.serializer.serialize_unit().map_err(ser_to_de)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        self.serializer.serialize_bool(value).map_err(ser_to_de)
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        self.serializer.serialize_i64(value).map_err(ser_to_de)
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        self.serializer.serialize_u64(value).map_err(ser_to_de)
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        self.serializer.serialize_i128(value).map_err(ser_to_de)
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Self::Value, E> {
        self.serializer.serialize_u128(value).map_err(ser_to_de)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        self.serializer.serialize_f64(value).map_err(ser_to_de)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        self.serializer.serialize_str(value).map_err(ser_to_de)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(nests_too_deep());
        };
        let mut sequence = self
            .serializer
            .serialize_seq(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(value) = access.next_element::<&RawValue>()? {
            let child = Replay {
                json: value.get(),
                depth,
            };
            ser::SerializeSeq::serialize_element(&mut sequence, &child).map_err(ser_to_de)?;
        }
        ser::SerializeSeq::end(sequence).map_err(ser_to_de)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let Some(depth) = self.depth.checked_sub(1) else {
            return Err(nests_too_deep());
        };
        let mut map = self
            .serializer
            .serialize_map(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(key) = access.next_key_seed(Key)? {
            ser::SerializeMap::serialize_key(&mut map, key.as_ref()).map_err(ser_to_de)?;
            let value = access.next_value::<&RawValue>()?;
            let child = Replay {
                json: value.get(),
                depth,
            };
            ser::SerializeMap::serialize_value(&mut map, &child).map_err(ser_to_de)?;
        }
        ser::SerializeMap::end(map).map_err(ser_to_de)
    }
}

/// A repeatable borrowed JSON value handed to an arbitrary serializer.
///
/// A serializer may read a value more than once, or skip it. Consume the raw
/// child before handing it over, then start a fresh parse for each read.
/// Each parser reads only one container; `RawValue` scans its children without
/// the parser recursion limit. No parsed child tree is retained here, and the
/// child carries what is left of the value's container budget, so a repeat
/// read restarts from the same depth the first read used.
struct Replay<'a> {
    /// This child's source text.
    json: &'a str,
    /// Containers this child may still enter.
    depth: usize,
}

impl ser::Serialize for Replay<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        emit_within(self.json, serializer, self.depth)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::{emit, MAX_NATIVE_NESTING_DEPTH};
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
        let member = super::Replay {
            json: TEXT,
            depth: MAX_NATIVE_NESTING_DEPTH,
        };
        let expected = serde_json::from_str::<Value>(TEXT).unwrap();
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
        assert_eq!(serde_json::to_string(&member).unwrap(), TEXT);
        assert_eq!(serde_json::to_value(&member).unwrap(), expected);
    }

    /// One budget spans the whole replay: the parser's own limit restarts per
    /// container, so without this counter the descent follows the input text.
    #[test]
    fn one_budget_spans_the_whole_replay() {
        let chain =
            |containers: usize| format!("{}7{}", "[".repeat(containers), "]".repeat(containers));

        let admitted = chain(MAX_NATIVE_NESTING_DEPTH);
        let mut out = Vec::new();
        emit(&admitted, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), admitted);

        let error = emit(
            &chain(MAX_NATIVE_NESTING_DEPTH + 1),
            serde_json::value::Serializer,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(&format!(
                "native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
            )),
            "{error}"
        );
    }
}
