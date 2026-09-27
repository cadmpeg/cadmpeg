// SPDX-License-Identifier: Apache-2.0
//! Canonical [`serde_json::Value`] construction for typed native records.
//!
//! Builds exactly the value `serde_json::to_value` produces for a record of
//! finite numbers — the `Value` scalar conventions apply (an `f32` widens to
//! `f64`, an integer map key becomes its decimal string) — and adds the three
//! admissions the plain value serializer does not make: a NaN or infinite
//! number is refused by its member path, where `serde_json::to_value` writes
//! `null`; object keys must be distinct; and a `RawValue` payload is read
//! through one-container replay, so a record nested deeper than the JSON
//! parser's recursion limit is still admitted.
//!
//! The serializer recurses one frame per container of the record it is handed,
//! and a record field holding a `serde_json::Value` states its own shape, so
//! the descent is counted against [`MAX_NATIVE_NESTING_DEPTH`].
#![deny(clippy::disallowed_methods)]

use std::fmt::{Display, Write as _};
use std::io::{self, Write};

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

pub(super) trait ByteSink {
    fn write_bytes(&self, bytes: &[u8]) -> io::Result<()>;
}

impl<F: Fn(&[u8]) -> io::Result<()>> ByteSink for F {
    fn write_bytes(&self, bytes: &[u8]) -> io::Result<()> {
        self(bytes)
    }
}

struct SinkWriter<'a>(&'a dyn ByteSink);

impl Write for SinkWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write_bytes(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn emit<T: Serialize + ?Sized>(sink: Option<&dyn ByteSink>, value: &T) -> Result<(), CanonError> {
    if let Some(sink) = sink {
        serde_json::to_writer(SinkWriter(sink), value)?;
    }
    Ok(())
}

fn emit_bytes(sink: Option<&dyn ByteSink>, bytes: &[u8]) -> Result<(), CanonError> {
    if let Some(sink) = sink {
        sink.write_bytes(bytes).map_err(serde_json::Error::io)?;
    }
    Ok(())
}

fn emit_escaped_fragment(sink: Option<&dyn ByteSink>, fragment: &str) -> Result<(), CanonError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = fragment.as_bytes();
    let mut start = 0;
    for (index, byte) in bytes.iter().copied().enumerate() {
        let short = match byte {
            b'"' => Some(b"\\\"".as_slice()),
            b'\\' => Some(b"\\\\".as_slice()),
            b'\n' => Some(b"\\n".as_slice()),
            b'\r' => Some(b"\\r".as_slice()),
            b'\t' => Some(b"\\t".as_slice()),
            0x08 => Some(b"\\b".as_slice()),
            0x0c => Some(b"\\f".as_slice()),
            _ => None,
        };
        if let Some(escaped) = short {
            emit_bytes(sink, &bytes[start..index])?;
            emit_bytes(sink, escaped)?;
            start = index + 1;
        } else if byte < 0x20 {
            emit_bytes(sink, &bytes[start..index])?;
            emit_bytes(
                sink,
                &[
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 0x0f)],
                ],
            )?;
            start = index + 1;
        }
    }
    emit_bytes(sink, &bytes[start..])
}

struct ChargingDisplayText<'a> {
    text: String,
    sink: Option<&'a dyn ByteSink>,
    refusal: Option<CanonError>,
}

impl std::fmt::Write for ChargingDisplayText<'_> {
    fn write_str(&mut self, fragment: &str) -> std::fmt::Result {
        if let Err(error) = emit_escaped_fragment(self.sink, fragment) {
            self.refusal = Some(error);
            return Err(std::fmt::Error);
        }
        self.text.push_str(fragment);
        Ok(())
    }
}

fn collect_display_text<T: Display + ?Sized>(
    value: &T,
    sink: Option<&dyn ByteSink>,
) -> Result<String, CanonError> {
    emit_bytes(sink, b"\"")?;
    let mut writer = ChargingDisplayText {
        text: String::new(),
        sink,
        refusal: None,
    };
    if write!(&mut writer, "{value}").is_err() {
        return Err(writer
            .refusal
            .unwrap_or_else(|| ser::Error::custom("a formatting error occurred")));
    }
    emit_bytes(sink, b"\"")?;
    Ok(writer.text)
}

/// One member step from a record to a value inside it.
#[derive(Debug)]
pub(super) enum Step {
    /// An object member, an externally tagged variant, or a struct field.
    Key(String),
    /// A sequence element.
    Index(usize),
}

/// A value the canonical serializer refuses.
#[derive(Debug)]
pub(super) enum CanonError {
    /// A NaN or infinite number, which JSON cannot state. The steps from the
    /// record to the number are collected innermost first while the refusal
    /// returns through the containers that hold it.
    NonFinite(Vec<Step>),
    /// Any other refusal.
    Json(serde_json::Error),
}

impl CanonError {
    /// This refusal, stated from one container further out.
    fn within(self, step: impl FnOnce() -> Step) -> Self {
        match self {
            Self::NonFinite(mut steps) => {
                steps.push(step());
                Self::NonFinite(steps)
            }
            Self::Json(error) => Self::Json(error),
        }
    }

    /// The member path of a refused number, outermost step first: a key is
    /// joined by `.` and an element index is written `[index]`.
    pub(super) fn field_path(steps: &[Step]) -> String {
        let mut path = String::new();
        for step in steps.iter().rev() {
            match step {
                Step::Key(key) => {
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(key);
                }
                Step::Index(index) => {
                    path.push('[');
                    path.push_str(&index.to_string());
                    path.push(']');
                }
            }
        }
        path
    }
}

impl std::fmt::Display for CanonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite(steps) => write!(
                f,
                "field {} holds a non-finite number",
                Self::field_path(steps)
            ),
            Self::Json(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CanonError {}

impl ser::Error for CanonError {
    fn custom<T: Display>(message: T) -> Self {
        Self::Json(<serde_json::Error as ser::Error>::custom(message))
    }
}

impl From<serde_json::Error> for CanonError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// A finite number as a JSON value, or the refusal of a NaN or infinite one.
fn number(value: f64, sink: Option<&dyn ByteSink>) -> Result<Node, CanonError> {
    if !value.is_finite() {
        return Err(CanonError::NonFinite(Vec::new()));
    }
    emit(sink, &value)?;
    serde_json::Number::from_f64(value)
        .map(|number| Node::Value(Value::Number(number)))
        .ok_or(CanonError::NonFinite(Vec::new()))
}

/// The canonical-value serializer. Every `serialize_*` returns a [`Node`].
pub(super) struct CanonValue<'a> {
    /// Containers this value may still enter.
    depth: usize,
    sink: Option<&'a dyn ByteSink>,
    raw_text: bool,
}

type Error = CanonError;

impl<'a> CanonValue<'a> {
    /// The serializer for one whole typed record.
    ///
    /// A record's own object is the container a stored record never holds: it
    /// keeps the members as fields and measures each field on its own. One
    /// container beyond the field bound therefore admits exactly a
    /// [`MAX_NATIVE_NESTING_DEPTH`]-deep field.
    #[cfg(test)]
    pub(super) const fn for_record() -> Self {
        Self {
            depth: MAX_NATIVE_NESTING_DEPTH + 1,
            sink: None,
            raw_text: false,
        }
    }

    pub(super) const fn for_record_with_sink(sink: Option<&'a dyn ByteSink>) -> Self {
        Self {
            depth: MAX_NATIVE_NESTING_DEPTH + 1,
            sink,
            raw_text: false,
        }
    }

    /// The serializer for a child value that may enter `depth` containers.
    const fn within(depth: usize, sink: Option<&'a dyn ByteSink>) -> Self {
        Self {
            depth,
            sink,
            raw_text: false,
        }
    }

    const fn raw_text(depth: usize, sink: Option<&'a dyn ByteSink>) -> Self {
        Self {
            depth,
            sink,
            raw_text: true,
        }
    }

    /// The budget left after entering one container, or the refusal.
    fn enter(&self) -> Result<usize, Error> {
        match self.depth.checked_sub(1) {
            Some(depth) => Ok(depth),
            None => Err(ser::Error::custom(nests_too_deep_message())),
        }
    }
}

impl<'a> ser::Serializer for CanonValue<'a> {
    type Ok = Node;
    type Error = Error;
    type SerializeSeq = CanonSeq<'a>;
    type SerializeTuple = CanonSeq<'a>;
    type SerializeTupleStruct = CanonSeq<'a>;
    type SerializeTupleVariant = CanonVariantSeq<'a>;
    type SerializeMap = CanonMap<'a>;
    type SerializeStruct = CanonStruct<'a>;
    type SerializeStructVariant = CanonVariantMap<'a>;

    fn serialize_bool(self, value: bool) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::Bool(value)))
    }

    fn serialize_i8(self, value: i8) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i16(self, value: i16) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i32(self, value: i32) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i64(self, value: i64) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_i128(self, value: i128) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        if let Ok(value) = i64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_u8(self, value: u8) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u16(self, value: u16) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u32(self, value: u32) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u64(self, value: u64) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::from(value)))
    }

    fn serialize_u128(self, value: u128) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        if let Ok(value) = u64::try_from(value) {
            return Ok(Node::Value(Value::from(value)));
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_f32(self, value: f32) -> Result<Node, Error> {
        number(f64::from(value), self.sink)
    }

    fn serialize_f64(self, value: f64) -> Result<Node, Error> {
        number(value, self.sink)
    }

    fn serialize_char(self, value: char) -> Result<Node, Error> {
        emit(self.sink, &value)?;
        Ok(Node::Value(Value::String(value.to_string())))
    }

    fn serialize_str(self, value: &str) -> Result<Node, Error> {
        if self.raw_text {
            emit_bytes(self.sink, value.as_bytes())?;
        } else {
            emit(self.sink, value)?;
        }
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
        emit_bytes(self.sink, b"null")?;
        Ok(Node::Value(Value::Null))
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Node, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Node, Error> {
        emit_bytes(self.sink, b"null")?;
        Ok(Node::Value(Value::Null))
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Node, Error> {
        emit_bytes(self.sink, b"null")?;
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
        emit_bytes(self.sink, b"{")?;
        emit(self.sink, variant)?;
        emit_bytes(self.sink, b":")?;
        let inner = value
            .serialize(CanonValue::within(depth, self.sink))
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))?
            .into_value();
        emit_bytes(self.sink, b"}")?;
        Ok(tagged(variant, inner))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<CanonSeq<'a>, Error> {
        emit_bytes(self.sink, b"[")?;
        Ok(CanonSeq {
            out: Vec::new(),
            depth: self.enter()?,
            sink: self.sink,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<CanonSeq<'a>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<CanonSeq<'a>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantSeq<'a>, Error> {
        let depth = self.enter()?;
        emit_bytes(self.sink, b"{")?;
        emit(self.sink, variant)?;
        emit_bytes(self.sink, b":")?;
        Ok(CanonVariantSeq {
            variant,
            seq: CanonValue::within(depth, self.sink).serialize_seq(Some(len))?,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<CanonMap<'a>, Error> {
        emit_bytes(self.sink, b"{")?;
        Ok(CanonMap {
            entries: Map::new(),
            key: None,
            depth: self.enter()?,
            sink: self.sink,
        })
    }

    /// `RawValue`'s struct protocol carries one JSON value and is no container
    /// of its own, so the payload replays with this value's whole budget.
    fn serialize_struct(self, name: &'static str, len: usize) -> Result<CanonStruct<'a>, Error> {
        if name == RAW_VALUE_STRUCT {
            Ok(CanonStruct::Raw {
                depth: self.depth,
                parsed: None,
                sink: self.sink,
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
    ) -> Result<CanonVariantMap<'a>, Error> {
        let depth = self.enter()?;
        emit_bytes(self.sink, b"{")?;
        emit(self.sink, variant)?;
        emit_bytes(self.sink, b":")?;
        Ok(CanonVariantMap {
            variant,
            map: CanonValue::within(depth, self.sink).serialize_map(Some(len))?,
        })
    }

    /// Charge each escaped fragment before it extends the stored text.
    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<Node, Error> {
        Ok(Node::Value(Value::String(collect_display_text(
            value, self.sink,
        )?)))
    }
}

/// A sequence collected in visit order.
pub(super) struct CanonSeq<'a> {
    out: Vec<Value>,
    /// Containers each element may still enter.
    depth: usize,
    sink: Option<&'a dyn ByteSink>,
}

impl ser::SerializeSeq for CanonSeq<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let index = self.out.len();
        if index != 0 {
            emit_bytes(self.sink, b",")?;
        }
        let element = value
            .serialize(CanonValue::within(self.depth, self.sink))
            .map_err(|error| error.within(|| Step::Index(index)))?
            .into_value();
        self.out.push(element);
        Ok(())
    }

    fn end(self) -> Result<Node, Error> {
        emit_bytes(self.sink, b"]")?;
        Ok(Node::Value(Value::Array(self.out)))
    }
}

impl ser::SerializeTuple for CanonSeq<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Node, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for CanonSeq<'_> {
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
pub(super) struct CanonVariantSeq<'a> {
    variant: &'static str,
    seq: CanonSeq<'a>,
}

impl ser::SerializeTupleVariant for CanonVariantSeq<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let variant = self.variant;
        ser::SerializeSeq::serialize_element(&mut self.seq, value)
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))
    }

    fn end(self) -> Result<Node, Error> {
        let sink = self.seq.sink;
        let inner = ser::SerializeSeq::end(self.seq)?.into_value();
        emit_bytes(sink, b"}")?;
        Ok(tagged(self.variant, inner))
    }
}

/// An object's distinct members, keyed by raw (unescaped) key.
pub(super) struct CanonMap<'a> {
    entries: Map<String, Value>,
    key: Option<String>,
    /// Containers each member value may still enter.
    depth: usize,
    sink: Option<&'a dyn ByteSink>,
}

impl CanonMap<'_> {
    fn insert<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), Error> {
        let depth = self.depth;
        match self.entries.entry(key) {
            serde_json::map::Entry::Vacant(entry) => {
                let value = value
                    .serialize(CanonValue::within(depth, self.sink))
                    .map_err(|error| error.within(|| Step::Key(entry.key().clone())))?
                    .into_value();
                entry.insert(value);
                Ok(())
            }
            serde_json::map::Entry::Occupied(entry) => {
                Err(ser::Error::custom(format!("duplicate key {}", entry.key())))
            }
        }
    }

    fn insert_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        if !self.entries.is_empty() {
            emit_bytes(self.sink, b",")?;
        }
        emit(self.sink, key)?;
        emit_bytes(self.sink, b":")?;
        self.insert(key.to_owned(), value)
    }
}

impl ser::SerializeMap for CanonMap<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom(
                "key serialized before the preceding value",
            ));
        }
        if !self.entries.is_empty() {
            emit_bytes(self.sink, b",")?;
        }
        self.key = Some(key.serialize(CanonKey { sink: self.sink })?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("value serialized before key"))?;
        emit_bytes(self.sink, b":")?;
        self.insert(key, value)
    }

    fn end(self) -> Result<Node, Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom("map ended before the pending value"));
        }
        emit_bytes(self.sink, b"}")?;
        Ok(Node::Object(self.entries))
    }
}

/// Ordinary struct members or one JSON value carried by `RawValue`'s protocol.
pub(super) enum CanonStruct<'a> {
    Object(CanonMap<'a>),
    Raw {
        /// Containers the payload may enter.
        depth: usize,
        /// The replayed payload, once its one field has arrived.
        parsed: Option<Node>,
        sink: Option<&'a dyn ByteSink>,
    },
}

impl ser::SerializeStruct for CanonStruct<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        match self {
            Self::Object(map) => map.insert_field(key, value),
            Self::Raw {
                depth,
                parsed,
                sink,
            } => {
                if key != RAW_VALUE_STRUCT || parsed.is_some() {
                    return Err(ser::Error::custom(
                        "raw JSON requires exactly one payload field",
                    ));
                }
                let Node::Value(Value::String(json)) =
                    value.serialize(CanonValue::raw_text(*depth, *sink))?
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
                    CanonValue::within(*depth, None),
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
pub(super) struct CanonVariantMap<'a> {
    variant: &'static str,
    map: CanonMap<'a>,
}

impl ser::SerializeStructVariant for CanonVariantMap<'_> {
    type Ok = Node;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let variant = self.variant;
        self.map
            .insert_field(key, value)
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))
    }

    fn end(self) -> Result<Node, Error> {
        let sink = self.map.sink;
        let inner = ser::SerializeMap::end(self.map)?.into_value();
        emit_bytes(sink, b"}")?;
        Ok(tagged(self.variant, inner))
    }
}

/// Map-key serializer with `serde_json::Value`'s key conventions: strings
/// pass through, scalar keys and unit variants use their string forms, and
/// compound or absent keys are rejected. Floating-point keys must be finite.
struct CanonKey<'a> {
    sink: Option<&'a dyn ByteSink>,
}

fn emit_number_key<T: Serialize>(sink: Option<&dyn ByteSink>, value: &T) -> Result<(), Error> {
    emit_bytes(sink, b"\"")?;
    emit(sink, value)?;
    emit_bytes(sink, b"\"")
}

fn key_must_be_a_string() -> Error {
    ser::Error::custom("key must be a string")
}

impl ser::Serializer for CanonKey<'_> {
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
        let text = if value { "true" } else { "false" };
        emit(self.sink, text)?;
        Ok(text.to_owned())
    }

    fn serialize_i8(self, value: i8) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_i16(self, value: i16) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_i32(self, value: i32) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_i64(self, value: i64) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_i128(self, value: i128) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_u8(self, value: u8) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_u16(self, value: u16) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_u32(self, value: u32) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_u64(self, value: u64) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_u128(self, value: u128) -> Result<String, Error> {
        emit_number_key(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_f32(self, value: f32) -> Result<String, Error> {
        if value.is_finite() {
            emit_number_key(self.sink, &value)?;
            Ok(serde_json::to_string(&value)?)
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_f64(self, value: f64) -> Result<String, Error> {
        if value.is_finite() {
            emit_number_key(self.sink, &value)?;
            Ok(serde_json::to_string(&value)?)
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_char(self, value: char) -> Result<String, Error> {
        emit(self.sink, &value)?;
        Ok(value.to_string())
    }

    fn serialize_str(self, value: &str) -> Result<String, Error> {
        emit(self.sink, value)?;
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
        emit(self.sink, variant)?;
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

    /// Charge each escaped fragment before it extends the stored key.
    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<String, Error> {
        collect_display_text(value, self.sink)
    }
}

#[cfg(test)]
mod tests;
