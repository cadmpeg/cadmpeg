// SPDX-License-Identifier: Apache-2.0
//! Streaming canonical JSON for typed native records.
//!
//! Renders exactly the text `serde_json::to_string` produces for the
//! [`serde_json::Value`] tree of a record — compact, with recursively
//! sorted object keys and the `Value` scalar conventions (a non-finite
//! float is `null`, an `f32` widens to `f64` before rendering, an integer
//! map key becomes its decimal string) — without building the tree. Only
//! objects buffer their members, for the sort; scalars and sequences
//! append as they are visited.
#![deny(clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::ser::{self, Serialize};

/// One serialized value: rendered text, or a buffered object kept apart so
/// the record assembler can hoist its `id` member.
pub(super) enum Node {
    /// Any non-object value, fully rendered.
    Text(String),
    /// An object's members, rendered per raw (unescaped) key.
    Object(BTreeMap<String, String>),
}

impl Node {
    /// Render this value as canonical JSON text.
    pub(super) fn render(self) -> String {
        match self {
            Node::Text(text) => text,
            Node::Object(entries) => render_object(&entries),
        }
    }
}

/// Render buffered object members in sorted key order.
fn render_object(entries: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    for (ordinal, (key, value)) in entries.iter().enumerate() {
        if ordinal > 0 {
            out.push(',');
        }
        out.push_str(&escape_key(key));
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
    out
}

/// Render a raw key as a JSON string.
fn escape_key(key: &str) -> String {
    serde_json::to_string(key).expect("a string always renders")
}

/// Render one finite or non-finite double the way `serde_json::Value` does.
fn render_f64(value: f64) -> String {
    if value.is_finite() {
        serde_json::to_string(&value).expect("a finite double always renders")
    } else {
        "null".to_owned()
    }
}

/// The canonical-value serializer. Every `serialize_*` returns a [`Node`].
pub(super) struct CanonValue;

type Error = serde_json::Error;

impl ser::Serializer for CanonValue {
    type Ok = Node;
    type Error = Error;
    type SerializeSeq = CanonSeq;
    type SerializeTuple = CanonSeq;
    type SerializeTupleStruct = CanonSeq;
    type SerializeTupleVariant = CanonVariantSeq;
    type SerializeMap = CanonMap;
    type SerializeStruct = CanonMap;
    type SerializeStructVariant = CanonVariantMap;

    fn serialize_bool(self, value: bool) -> Result<Node, Error> {
        Ok(Node::Text(if value { "true" } else { "false" }.to_owned()))
    }

    fn serialize_i8(self, value: i8) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_i16(self, value: i16) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_i32(self, value: i32) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_i64(self, value: i64) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_i128(self, value: i128) -> Result<Node, Error> {
        if let Ok(value) = i64::try_from(value) {
            return Ok(Node::Text(value.to_string()));
        }
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Text(value.to_string()));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_u8(self, value: u8) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_u16(self, value: u16) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_u32(self, value: u32) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_u64(self, value: u64) -> Result<Node, Error> {
        Ok(Node::Text(value.to_string()))
    }

    fn serialize_u128(self, value: u128) -> Result<Node, Error> {
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Text(value.to_string()));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_f32(self, value: f32) -> Result<Node, Error> {
        Ok(Node::Text(render_f64(f64::from(value))))
    }

    fn serialize_f64(self, value: f64) -> Result<Node, Error> {
        Ok(Node::Text(render_f64(value)))
    }

    fn serialize_char(self, value: char) -> Result<Node, Error> {
        self.serialize_str(&value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<Node, Error> {
        Ok(Node::Text(serde_json::to_string(value)?))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Node, Error> {
        let mut out = String::from("[");
        for (ordinal, byte) in value.iter().enumerate() {
            if ordinal > 0 {
                out.push(',');
            }
            write!(out, "{byte}").expect("a string accepts every byte");
        }
        out.push(']');
        Ok(Node::Text(out))
    }

    fn serialize_none(self) -> Result<Node, Error> {
        Ok(Node::Text("null".to_owned()))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Node, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Node, Error> {
        Ok(Node::Text("null".to_owned()))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node, Error> {
        Ok(Node::Text("null".to_owned()))
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
        let inner = value.serialize(CanonValue)?.render();
        Ok(Node::Text(format!("{{{}:{inner}}}", escape_key(variant))))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<CanonSeq, Error> {
        Ok(CanonSeq {
            out: String::from("["),
            any: false,
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
        Ok(CanonVariantSeq {
            variant,
            seq: self.serialize_seq(Some(len))?,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<CanonMap, Error> {
        Ok(CanonMap {
            entries: BTreeMap::new(),
            key: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<CanonMap, Error> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantMap, Error> {
        Ok(CanonVariantMap {
            variant,
            map: self.serialize_map(Some(len))?,
        })
    }
}

/// A sequence rendered in visit order.
pub(super) struct CanonSeq {
    out: String,
    any: bool,
}

impl ser::SerializeSeq for CanonSeq {
    type Ok = Node;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        if self.any {
            self.out.push(',');
        }
        self.any = true;
        self.out.push_str(&value.serialize(CanonValue)?.render());
        Ok(())
    }

    fn end(mut self) -> Result<Node, Error> {
        self.out.push(']');
        Ok(Node::Text(self.out))
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
        let inner = ser::SerializeSeq::end(self.seq)?.render();
        Ok(Node::Text(format!(
            "{{{}:{inner}}}",
            escape_key(self.variant)
        )))
    }
}

/// An object's members, buffered raw-key to rendered-value. A repeated key
/// keeps the last value, as a `Value` map insert does.
pub(super) struct CanonMap {
    entries: BTreeMap<String, String>,
    key: Option<String>,
}

impl ser::SerializeMap for CanonMap {
    type Ok = Node;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key.serialize(CanonKey)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("value serialized before key"))?;
        self.entries
            .insert(key, value.serialize(CanonValue)?.render());
        Ok(())
    }

    fn end(self) -> Result<Node, Error> {
        Ok(Node::Object(self.entries))
    }
}

impl ser::SerializeStruct for CanonMap {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.entries
            .insert(key.to_owned(), value.serialize(CanonValue)?.render());
        Ok(())
    }

    fn end(self) -> Result<Node, Error> {
        Ok(Node::Object(self.entries))
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
        ser::SerializeStruct::serialize_field(&mut self.map, key, value)
    }

    fn end(self) -> Result<Node, Error> {
        let inner = ser::SerializeStruct::end(self.map)?.render();
        Ok(Node::Text(format!(
            "{{{}:{inner}}}",
            escape_key(self.variant)
        )))
    }
}

/// Map-key serializer with `serde_json::Value`'s key conventions: strings
/// pass through, an integer or character becomes its string form, and any
/// other shape is rejected.
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
