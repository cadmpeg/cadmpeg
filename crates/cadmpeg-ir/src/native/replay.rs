// SPDX-License-Identifier: Apache-2.0
//! Read stored JSON text.
//!
//! [`emit`] drives a serializer from the parse, reading one container at a
//! time. Each container is a parse of its own, so the parser's own recursion
//! limit never accumulates; the descent is counted here instead, against the
//! one bound every native record field answers to.
//!
//! The other readers take native record text, whose nesting the record writer
//! bounded: [`transcode`] writes it into a serializer in one pass, [`value`]
//! parses it, and [`member`], [`members`] and [`Elements`] find member and
//! element texts without building their values.

use std::borrow::Cow;
use std::cell::RefCell;
use std::fmt;

use serde::{de, ser};
use serde_json::value::RawValue;
use serde_json::Value;

use super::nests_too_deep_message;

/// Emit the JSON value held in `json` into `serializer`, entering at most
/// `depth` further containers.
///
/// The counter bounds this function's own recursion, whatever serializer it
/// drives. A serializer that counts as well is handed the same remainder, so
/// the two refuse at the same container.
pub(super) fn emit<S: ser::Serializer>(
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

/// Write the JSON value held in `json` into `serializer` in one pass.
///
/// The text is a stored native record, which the writer bounded at
/// [`super::MAX_NATIVE_NESTING_DEPTH`] containers per field, so the parser's
/// recursion limit is lifted. Each value is read once, as `serializer` asks
/// for it; [`emit`] serves a serializer that reads a value more than once.
pub(super) fn transcode<S: ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer.disable_recursion_limit();
    let written = de::Deserializer::deserialize_any(&mut deserializer, Transcode(serializer))
        .map_err(de_to_ser)?;
    deserializer.end().map_err(de_to_ser)?;
    Ok(written)
}

/// Writes each value it visits into a serializer.
struct Transcode<S>(S);

impl<'de, S: ser::Serializer> de::Visitor<'de> for Transcode<S> {
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
            access.next_value_seed(MapValue(&mut map))?;
        }
        ser::SerializeMap::end(map).map_err(ser_to_de)
    }
}

/// Writes one sequence element as the parser reaches it.
struct Element<'s, C>(&'s mut C);

impl<'de, C: ser::SerializeSeq> de::DeserializeSeed<'de> for Element<'_, C> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0
            .serialize_element(&Pending::new(deserializer))
            .map_err(ser_to_de)
    }
}

/// Writes one map value as the parser reaches it.
struct MapValue<'s, C>(&'s mut C);

impl<'de, C: ser::SerializeMap> de::DeserializeSeed<'de> for MapValue<'_, C> {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        self.0
            .serialize_value(&Pending::new(deserializer))
            .map_err(ser_to_de)
    }
}

/// A value the parser has reached but not read, handed to a serializer.
struct Pending<D>(RefCell<Option<D>>);

impl<D> Pending<D> {
    fn new(deserializer: D) -> Self {
        Self(RefCell::new(Some(deserializer)))
    }
}

impl<'de, D: de::Deserializer<'de>> ser::Serialize for Pending<D> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let deserializer = self
            .0
            .borrow_mut()
            .take()
            .ok_or_else(|| ser::Error::custom("a stored native value is read once"))?;
        deserializer
            .deserialize_any(Transcode(serializer))
            .map_err(de_to_ser)
    }
}

/// Parse the JSON value held in `json`, which the native record writer
/// bounded at [`super::MAX_NATIVE_NESTING_DEPTH`] containers per field.
///
/// One pass reads the text with the parser's recursion limit lifted. An object
/// key is plain text: the `serde_json` `Value` reader would treat a first key
/// spelling its `RawValue` protocol token as embedded JSON.
pub(super) fn value(json: &str) -> serde_json::Result<Value> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer.disable_recursion_limit();
    let value = de::DeserializeSeed::deserialize(Plain, &mut deserializer)?;
    deserializer.end()?;
    Ok(value)
}

/// Builds a [`Value`] whose object keys are plain text.
#[derive(Clone, Copy)]
struct Plain;

impl<'de> de::DeserializeSeed<'de> for Plain {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> de::Visitor<'de> for Plain {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("native record text holds a non-finite number"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(access.size_hint().unwrap_or(0));
        while let Some(item) = access.next_element_seed(self)? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut entries = serde_json::Map::new();
        while let Some(key) = access.next_key::<String>()? {
            let value = access.next_value_seed(self)?;
            entries.insert(key, value);
        }
        Ok(Value::Object(entries))
    }
}

/// The raw text of the member `name` of the JSON object held in `json`.
///
/// The other members are scanned past without being built. Text that is not
/// an object, or holds no such member, answers `None`.
pub(super) fn member<'a>(json: &'a str, name: &str) -> Option<&'a RawValue> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let found = de::Deserializer::deserialize_map(&mut deserializer, Member { name }).ok()?;
    deserializer.end().ok()?;
    found
}

/// The number of members of the JSON object held in `json`, or `None` when
/// the text is not an object.
pub(super) fn member_count(json: &str) -> Option<usize> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    let count = de::Deserializer::deserialize_map(&mut deserializer, Count).ok()?;
    deserializer.end().ok()?;
    Some(count)
}

/// Counts an object's members and scans past their values.
struct Count;

impl<'de> de::Visitor<'de> for Count {
    type Value = usize;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<usize, A::Error> {
        let mut count = 0_usize;
        while access.next_key::<de::IgnoredAny>()?.is_some() {
            access.next_value::<&RawValue>()?;
            count += 1;
        }
        Ok(count)
    }
}

/// An object's member keys, each with its value text.
pub(super) type MemberTexts<'a> = Vec<(Cow<'a, str>, &'a str)>;

/// Push each member of the JSON object held in `json` onto `members`: its key
/// and its value text. The caller reserved one slot per member. States
/// whether `json` is an object.
pub(super) fn members<'a>(json: &'a str, members: &mut MemberTexts<'a>) -> bool {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    de::Deserializer::deserialize_map(&mut deserializer, Members { members }).is_ok()
        && deserializer.end().is_ok()
}

/// Collects an object's member keys and value texts.
struct Members<'m, 'a> {
    members: &'m mut MemberTexts<'a>,
}

impl<'a> de::Visitor<'a> for Members<'_, 'a> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: de::MapAccess<'a>>(self, mut access: A) -> Result<(), A::Error> {
        while let Some(key) = access.next_key_seed(Key)? {
            let value = access.next_value::<&RawValue>()?;
            self.members.push((key, value.get()));
        }
        Ok(())
    }
}

/// The element texts of one JSON array, read one at a time.
pub(super) struct Elements<'a> {
    /// Text after the elements already read, or `None` after the last one.
    rest: Option<&'a str>,
}

impl<'a> Elements<'a> {
    /// The elements of the array held in `json`, or `None` when the text is
    /// not an array.
    pub(super) fn of(json: &'a str) -> Option<Self> {
        let rest = json.trim_start().strip_prefix('[')?;
        Some(Self { rest: Some(rest) })
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest.take()?.trim_start();
        if rest.starts_with(']') {
            return None;
        }
        let mut elements = serde_json::Deserializer::from_str(rest).into_iter::<&RawValue>();
        let element = elements.next()?.ok()?;
        let after = rest.get(elements.byte_offset()..)?.trim_start();
        self.rest = Some(after.strip_prefix(',').unwrap_or(after));
        Some(element.get())
    }
}

/// Finds one member of an object and scans past the others.
struct Member<'n> {
    name: &'n str,
}

impl<'de> de::Visitor<'de> for Member<'_> {
    type Value = Option<&'de RawValue>;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: de::MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
        let mut found = None;
        while let Some(key) = access.next_key_seed(Key)? {
            let value = access.next_value::<&RawValue>()?;
            if found.is_none() && key == self.name {
                found = Some(value);
            }
        }
        Ok(found)
    }
}

/// The refusal a container past the bound reports.
fn nests_too_deep<E: de::Error>() -> E {
    de::Error::custom(nests_too_deep_message())
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
        emit(self.json, serializer, self.depth)
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

    use super::emit;
    use crate::native::MAX_NATIVE_NESTING_DEPTH;
    use serde_json::Value;

    /// Replay one whole native record field.
    fn emit_field<S: serde::ser::Serializer>(json: &str, serializer: S) -> Result<S::Ok, S::Error> {
        emit(json, serializer, MAX_NATIVE_NESTING_DEPTH)
    }

    const TEXT: &str = concat!(
        r#"{"id":"pin#0","a":[null,true,false,-1,0,1.5,2.0,1e-7,10000000000.0],"#,
        r#""b":{"":[],"c\td":{},"nested":{"a":[[[1]]]}},"#,
        r#""é key":"quote\" back\\ tab\t bell\u0007 é","z":18446744073709551615}"#
    );

    #[test]
    fn emits_the_value_a_parse_would_produce() {
        let replayed = emit_field(TEXT, serde_json::value::Serializer).unwrap();
        assert_eq!(replayed, serde_json::from_str::<Value>(TEXT).unwrap());
    }

    #[test]
    fn compact_emission_reproduces_the_source_text() {
        let mut out = Vec::new();
        emit_field(TEXT, &mut serde_json::Serializer::new(&mut out)).unwrap();
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
        emit_field(&admitted, &mut serde_json::Serializer::new(&mut out)).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), admitted);

        let error = emit_field(
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
