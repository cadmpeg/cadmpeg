// SPDX-License-Identifier: Apache-2.0
//! Replaying stored JSON text into a serializer or a single field.
//!
//! A [`NativeRecord`](super::NativeRecord) keeps its fields as canonical JSON
//! text, so every emit and every field read starts from that text. Going
//! through [`serde_json::Value`] to get there builds a separately allocated map
//! node, key string, and enum cell for every field at every depth, holds them
//! for the length of the operation, and then walks them a second time to emit
//! or to pick one member out. This module reads the text once and does the work
//! as it goes: [`emit`] drives a serializer directly from the parse, and
//! [`field`] materializes the one member asked for while skipping the rest.
//!
//! Emitting through a serializer rather than splicing the text keeps the
//! caller's formatting: a pretty writer indents a native record the same way it
//! indents every other document entity, which is what the document digest is
//! taken over.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;

use serde::{de, ser};
use serde_json::Value;

/// Emit the JSON value held in `json` into `serializer`.
///
/// The bytes produced are the ones the serializer would produce for the
/// equivalent [`Value`], with one deliberate difference: sequence and map
/// lengths are not known ahead of the parse, so they reach the serializer as
/// `None`. JSON writers ignore the hint — an empty container is written the
/// same either way — and the record shape already carries a flattened field map,
/// which no length-prefixed format can encode regardless.
///
/// # Panics
///
/// Panics if `json` is not a complete JSON value. Callers hold text they
/// produced by serializing.
pub(super) fn emit<S: ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let emitted = de::Deserializer::deserialize_any(&mut deserializer, Emit(serializer))
        .map_err(de_to_ser)?;
    deserializer
        .end()
        .expect("stored record text is one complete JSON value");
    Ok(emitted)
}

/// Parse the `name` member of the JSON object held in `json`.
///
/// Members other than `name` are skipped without being materialized, so the
/// cost is one scan of the text plus a tree for the one member returned.
///
/// # Panics
///
/// Panics if `json` is not a complete JSON object.
pub(super) fn field(json: &str, name: &str) -> Option<Value> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let found = de::Deserializer::deserialize_map(&mut deserializer, PickField(name))
        .expect("stored record text is one complete JSON object");
    deserializer
        .end()
        .expect("stored record text is one complete JSON object");
    found
}

/// Serializer errors surface to the deserializer driving the replay, and the
/// deserializer's error surfaces back to the serializer that asked for the
/// value. Neither trait lets a foreign error type through, so the message
/// crosses each boundary and the concrete type does not.
fn ser_to_de<S: ser::Error, D: de::Error>(error: S) -> D {
    de::Error::custom(error)
}

/// See [`ser_to_de`].
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
        while access.next_element_seed(Element(&mut sequence))?.is_some() {}
        ser::SerializeSeq::end(sequence).map_err(ser_to_de)
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut map = self
            .0
            .serialize_map(access.size_hint())
            .map_err(ser_to_de)?;
        while let Some(key) = access.next_key_seed(Key)? {
            ser::SerializeMap::serialize_key(&mut map, key.as_ref()).map_err(ser_to_de)?;
            access.next_value_seed(Entry(&mut map))?;
        }
        ser::SerializeMap::end(map).map_err(ser_to_de)
    }
}

/// A `Serialize` that draws its value from a deserializer instead of memory.
///
/// Serde hands a nested element to `serialize_element`/`serialize_value` as
/// something serializable, which is where the replay recurses: the element's
/// deserializer is parked here and unparked when the serializer asks for it.
struct Replay<D>(RefCell<Option<D>>);

impl<'de, D: de::Deserializer<'de>> ser::Serialize for Replay<D> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0
            .borrow_mut()
            .take()
            .expect("a replayed value is serialized once")
            .deserialize_any(Emit(serializer))
            .map_err(de_to_ser)
    }
}

/// Replays one sequence element into `S`.
struct Element<'a, S>(&'a mut S);

impl<'de, S: ser::SerializeSeq> de::DeserializeSeed<'de> for Element<'_, S> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0
            .serialize_element(&Replay(RefCell::new(Some(deserializer))))
            .map_err(ser_to_de)
    }
}

/// Replays one map value into `S`.
struct Entry<'a, S>(&'a mut S);

impl<'de, S: ser::SerializeMap> de::DeserializeSeed<'de> for Entry<'_, S> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0
            .serialize_value(&Replay(RefCell::new(Some(deserializer))))
            .map_err(ser_to_de)
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
                found = Some(access.next_value()?);
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

    /// Text covering the shapes the replay has to reproduce: every scalar
    /// kind, empty and populated containers, escapes in both keys and values,
    /// and key names reused at a different depth.
    const TEXT: &str = concat!(
        r#"{"id":"pin#0","a":[null,true,false,-1,0,1.5,2.0,1e-7,10000000000.0],"#,
        r#""b":{"":[],"c\td":{},"nested":{"a":[[[1]]]}},"#,
        r#""é key":"quote\" back\\ tab\t bell\u0007 é","z":18446744073709551615}"#
    );

    /// The replay produces the value a `Value` parse would, so every
    /// serializer sees the same thing either way.
    #[test]
    fn emits_the_value_a_parse_would_produce() {
        let replayed = emit(TEXT, serde_json::value::Serializer).unwrap();
        assert_eq!(replayed, serde_json::from_str::<Value>(TEXT).unwrap());
    }

    /// A compact re-emit reproduces the source text byte for byte.
    #[test]
    fn compact_emission_reproduces_the_source_text() {
        let mut out = Vec::new();
        emit(TEXT, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), TEXT);
    }

    /// Picking one member matches parsing the whole object and removing it,
    /// including for absent members and for keys carrying escapes.
    #[test]
    fn picks_the_member_a_full_parse_would_yield() {
        let whole = serde_json::from_str::<serde_json::Map<String, Value>>(TEXT).unwrap();
        for name in ["id", "a", "b", "é key", "z", "missing", ""] {
            assert_eq!(field(TEXT, name), whole.get(name).cloned(), "{name}");
        }
    }
}
