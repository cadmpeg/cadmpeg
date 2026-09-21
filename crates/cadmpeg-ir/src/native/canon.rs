// SPDX-License-Identifier: Apache-2.0
//! Canonical [`serde_json::Value`] construction for typed native records.
//!
//! Builds exactly the value `serde_json::to_value` produces for a record —
//! the `Value` scalar conventions apply (a non-finite float is `null`, an
//! `f32` widens to `f64`, an integer map key becomes its decimal string) —
//! and adds the two admissions the plain value serializer does not make:
//! object keys must be distinct, and a `RawValue` payload is read through
//! one-container replay, so a record nested deeper than the JSON parser's
//! recursion limit is still admitted.
//!
//! The serializer recurses one frame per container of the record it is handed,
//! and a record field holding a `serde_json::Value` states its own shape, so
//! the descent is counted against [`MAX_NATIVE_NESTING_DEPTH`].
#![deny(clippy::disallowed_methods)]

use serde::ser::{self, Serialize};
use serde_json::{Map, Value};

use super::{nests_too_deep_message, MAX_NATIVE_NESTING_DEPTH};

// serde_json's RawValue Serialize protocol. The RawValue owner fixtures check
// this spelling against the dependency's actual serializer.
const RAW_VALUE_STRUCT: &str = "$serde_json::private::RawValue";

/// One serialized value: any value, or an object kept apart so the record
/// assembler can hoist its `id` member.
pub(super) enum Node {
    /// Any non-object value.
    Value(Value),
    /// An object's members, keyed by raw (unescaped) key.
    Object(Map<String, Value>),
}

impl Node {
    /// This value.
    fn into_value(self) -> Value {
        match self {
            Node::Value(value) => value,
            Node::Object(entries) => Value::Object(entries),
        }
    }

    /// Render this value as canonical JSON text: the tests' oracle for what a
    /// record carrying this node serializes to.
    #[cfg(test)]
    fn render(self) -> String {
        self.into_value().to_string()
    }
}

/// An externally tagged variant: `{"Variant": payload}`.
fn tagged(variant: &str, payload: Value) -> Node {
    let mut entries = Map::new();
    entries.insert(variant.to_owned(), payload);
    Node::Value(Value::Object(entries))
}

/// The canonical-value serializer. Every `serialize_*` returns a [`Node`].
pub(super) struct CanonValue {
    /// Containers this value may still enter.
    depth: usize,
}

type Error = serde_json::Error;

impl CanonValue {
    /// The serializer for one whole typed record.
    ///
    /// A record's own object is the container a stored record never holds: it
    /// keeps the members as fields and measures each field on its own. One
    /// container beyond the field bound therefore admits exactly a
    /// [`MAX_NATIVE_NESTING_DEPTH`]-deep field.
    pub(super) const fn for_record() -> Self {
        Self {
            depth: MAX_NATIVE_NESTING_DEPTH + 1,
        }
    }

    /// The serializer for a child value that may enter `depth` containers.
    const fn within(depth: usize) -> Self {
        Self { depth }
    }

    /// The budget left after entering one container, or the refusal.
    fn enter(self) -> Result<usize, Error> {
        match self.depth.checked_sub(1) {
            Some(depth) => Ok(depth),
            None => Err(ser::Error::custom(nests_too_deep_message())),
        }
    }
}

impl ser::Serializer for CanonValue {
    type Ok = Node;
    type Error = Error;
    type SerializeSeq = CanonSeq;
    type SerializeTuple = CanonSeq;
    type SerializeTupleStruct = CanonSeq;
    type SerializeTupleVariant = CanonVariantSeq;
    type SerializeMap = CanonMap;
    type SerializeStruct = CanonStruct;
    type SerializeStructVariant = CanonVariantMap;

    fn serialize_bool(self, value: bool) -> Result<Node, Error> {
        Ok(Node::Value(Value::Bool(value)))
    }

    fn serialize_i8(self, value: i8) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i16(self, value: i16) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i32(self, value: i32) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i64(self, value: i64) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i128(self, value: i128) -> Result<Node, Error> {
        if let Ok(value) = i64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_u8(self, value: u8) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u16(self, value: u16) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u32(self, value: u32) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u64(self, value: u64) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u128(self, value: u128) -> Result<Node, Error> {
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_f32(self, value: f32) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(f64::from(value))))
    }

    fn serialize_f64(self, value: f64) -> Result<Node, Error> {
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_char(self, value: char) -> Result<Node, Error> {
        self.serialize_str(&value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<Node, Error> {
        Ok(Node::Value(Value::String(value.to_owned())))
    }

    /// Bytes render as the JSON array of their values, so they enter a
    /// container and are counted through [`Self::serialize_seq`].
    fn serialize_bytes(self, value: &[u8]) -> Result<Node, Error> {
        let mut bytes = self.serialize_seq(Some(value.len()))?;
        for byte in value {
            ser::SerializeSeq::serialize_element(&mut bytes, byte)?;
        }
        ser::SerializeSeq::end(bytes)
    }

    fn serialize_none(self) -> Result<Node, Error> {
        Ok(Node::Value(Value::Null))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Node, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Node, Error> {
        Ok(Node::Value(Value::Null))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node, Error> {
        Ok(Node::Value(Value::Null))
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Node, Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Node, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Node, Error> {
        let depth = self.enter()?;
        let inner = value.serialize(CanonValue::within(depth))?.into_value();
        Ok(tagged(variant, inner))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<CanonSeq, Error> {
        Ok(CanonSeq {
            out: Vec::new(),
            depth: self.enter()?,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<CanonSeq, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(self, _name: &'static str, len: usize) -> Result<CanonSeq, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantSeq, Error> {
        let depth = self.enter()?;
        Ok(CanonVariantSeq {
            variant,
            seq: CanonValue::within(depth).serialize_seq(Some(len))?,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<CanonMap, Error> {
        Ok(CanonMap {
            entries: Map::new(),
            key: None,
            depth: self.enter()?,
        })
    }

    /// `RawValue`'s struct protocol carries one JSON value and is no container
    /// of its own, so the payload replays with this value's whole budget.
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<CanonStruct, Error> {
        if name == RAW_VALUE_STRUCT {
            Ok(CanonStruct::Raw {
                depth: self.depth,
                parsed: None,
            })
        } else {
            self.serialize_map(Some(len)).map(CanonStruct::Object)
        }
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantMap, Error> {
        let depth = self.enter()?;
        Ok(CanonVariantMap {
            variant,
            map: CanonValue::within(depth).serialize_map(Some(len))?,
        })
    }
}

/// A sequence collected in visit order.
pub(super) struct CanonSeq {
    out: Vec<Value>,
    /// Containers each element may still enter.
    depth: usize,
}

impl ser::SerializeSeq for CanonSeq {
    type Ok = Node;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let element = value
            .serialize(CanonValue::within(self.depth))?
            .into_value();
        self.out.push(element);
        Ok(())
    }

    fn end(self) -> Result<Node, Error> {
        Ok(Node::Value(Value::Array(self.out)))
    }
}

impl ser::SerializeTuple for CanonSeq {
    type Ok = Node;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Node, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for CanonSeq {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Node, Error> {
        ser::SerializeSeq::end(self)
    }
}

/// An externally tagged tuple variant: `{"Variant":[...]}`.
pub(super) struct CanonVariantSeq {
    variant: &'static str,
    seq: CanonSeq,
}

impl ser::SerializeTupleVariant for CanonVariantSeq {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(&mut self.seq, value)
    }

    fn end(self) -> Result<Node, Error> {
        let inner = ser::SerializeSeq::end(self.seq)?.into_value();
        Ok(tagged(self.variant, inner))
    }
}

/// An object's distinct members, keyed by raw (unescaped) key.
pub(super) struct CanonMap {
    entries: Map<String, Value>,
    key: Option<String>,
    /// Containers each member value may still enter.
    depth: usize,
}

impl CanonMap {
    fn insert<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), Error> {
        let depth = self.depth;
        match self.entries.entry(key) {
            serde_json::map::Entry::Vacant(entry) => {
                let value = value.serialize(CanonValue::within(depth))?.into_value();
                entry.insert(value);
                Ok(())
            }
            serde_json::map::Entry::Occupied(entry) => {
                Err(ser::Error::custom(format!("duplicate key {}", entry.key())))
            }
        }
    }
}

impl ser::SerializeMap for CanonMap {
    type Ok = Node;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom(
                "key serialized before the preceding value",
            ));
        }
        self.key = Some(key.serialize(CanonKey)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("value serialized before key"))?;
        self.insert(key, value)
    }

    fn end(self) -> Result<Node, Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom("map ended before the pending value"));
        }
        Ok(Node::Object(self.entries))
    }
}

/// Ordinary struct members or one JSON value carried by `RawValue`'s protocol.
pub(super) enum CanonStruct {
    Object(CanonMap),
    Raw {
        /// Containers the payload may enter.
        depth: usize,
        /// The replayed payload, once its one field has arrived.
        parsed: Option<Node>,
    },
}

impl ser::SerializeStruct for CanonStruct {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        match self {
            Self::Object(map) => map.insert(key.to_owned(), value),
            Self::Raw { depth, parsed } => {
                if key != RAW_VALUE_STRUCT || parsed.is_some() {
                    return Err(ser::Error::custom(
                        "raw JSON requires exactly one payload field",
                    ));
                }
                let serde_json::Value::String(json) =
                    value.serialize(serde_json::value::Serializer)?
                else {
                    return Err(ser::Error::custom("raw JSON payload must be a string"));
                };
                // Replay through the same canonical constructor, so raw objects
                // obey duplicate-key, number, ordering and depth semantics too.
                // The replay counts the text's containers and this serializer
                // counts the containers it is driven through, from the same
                // remaining budget, so both refuse at the same container.
                *parsed = Some(super::replay::emit(
                    &json,
                    CanonValue::within(*depth),
                    *depth,
                )?);
                Ok(())
            }
        }
    }

    fn end(self) -> Result<Node, Error> {
        match self {
            Self::Object(map) => ser::SerializeMap::end(map),
            Self::Raw {
                parsed: Some(parsed),
                ..
            } => Ok(parsed),
            Self::Raw { parsed: None, .. } => {
                Err(ser::Error::custom("raw JSON has no payload field"))
            }
        }
    }
}

/// An externally tagged struct variant: `{"Variant":{...}}`.
pub(super) struct CanonVariantMap {
    variant: &'static str,
    map: CanonMap,
}

impl ser::SerializeStructVariant for CanonVariantMap {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.map.insert(key.to_owned(), value)
    }

    fn end(self) -> Result<Node, Error> {
        let inner = ser::SerializeMap::end(self.map)?.into_value();
        Ok(tagged(self.variant, inner))
    }
}

/// Map-key serializer with `serde_json::Value`'s key conventions: strings
/// pass through, scalar keys and unit variants use their string forms, and
/// compound or absent keys are rejected. Floating-point keys must be finite.
struct CanonKey;

fn key_must_be_a_string() -> Error {
    ser::Error::custom("key must be a string")
}

impl ser::Serializer for CanonKey {
    type Ok = String;
    type Error = Error;
    type SerializeSeq = ser::Impossible<String, Error>;
    type SerializeTuple = ser::Impossible<String, Error>;
    type SerializeTupleStruct = ser::Impossible<String, Error>;
    type SerializeTupleVariant = ser::Impossible<String, Error>;
    type SerializeMap = ser::Impossible<String, Error>;
    type SerializeStruct = ser::Impossible<String, Error>;
    type SerializeStructVariant = ser::Impossible<String, Error>;

    fn serialize_bool(self, value: bool) -> Result<String, Error> {
        Ok(if value { "true" } else { "false" }.to_owned())
    }

    fn serialize_i8(self, value: i8) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_i16(self, value: i16) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_i32(self, value: i32) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_i64(self, value: i64) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_i128(self, value: i128) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_u8(self, value: u8) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_u16(self, value: u16) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_u32(self, value: u32) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_u64(self, value: u64) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_u128(self, value: u128) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_f32(self, value: f32) -> Result<String, Error> {
        if value.is_finite() {
            serde_json::to_string(&value)
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_f64(self, value: f64) -> Result<String, Error> {
        if value.is_finite() {
            serde_json::to_string(&value)
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_char(self, value: char) -> Result<String, Error> {
        Ok(value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<String, Error> {
        Ok(value.to_owned())
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_none(self) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, _value: &T) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit(self) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<String, Error> {
        Ok(variant.to_owned())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<String, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<String, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        Err(key_must_be_a_string())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Err(key_must_be_a_string())
    }
}

#[cfg(test)]
mod tests;
