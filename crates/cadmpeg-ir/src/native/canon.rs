// SPDX-License-Identifier: Apache-2.0
//! Canonical JSON text for typed native records.
//!
//! Writes exactly the compact text `serde_json` writes for the
//! `serde_json::to_value` tree of a record of finite numbers: object keys in
//! key order at every depth, and the `Value` scalar conventions (an `f32`
//! widens to `f64`, an integer map key becomes its decimal string). It adds the
//! three admissions the plain value serializer does not make: a NaN or
//! infinite number is refused by its member path, where `serde_json::to_value`
//! writes `null`; object keys must be distinct; and a `RawValue` payload is
//! read through one-container replay, so a record nested deeper than the JSON
//! parser's recursion limit is still admitted.
//!
//! Every value is written in place into one output buffer. An object's members
//! are written in arrival order; when they did not arrive in key order, the
//! object's text is rewritten in key order when it closes.
//!
//! The serializer recurses one frame per container of the record it is handed,
//! and a record field holding a `serde_json::Value` states its own shape, so
//! the descent is counted against [`MAX_NATIVE_NESTING_DEPTH`].

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{Display, Write as _};
use std::io::{self, Write};
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use serde::ser::{self, Serialize};

use super::{nests_too_deep_message, NativeConvertError, MAX_NATIVE_NESTING_DEPTH};

// serde_json's RawValue Serialize protocol. The RawValue owner fixtures check
// this spelling against the dependency's actual serializer.
const RAW_VALUE_STRUCT: &str = "$serde_json::private::RawValue";
/// The depth-account operation of the canonical native value walk.
pub(super) const WORK: &str = "construct canonical native value";

/// What one serialized value leaves for the container that holds it.
pub(super) enum Written {
    /// The value's text is complete in the output.
    Text,
    /// An object whose members are in the output but not yet closed, kept
    /// apart so the record assembler can hoist its `id` member.
    Object(Members),
}

impl Written {
    /// Complete an object's text in `out`. Other values are complete already.
    pub(super) fn place(self, out: &mut Vec<u8>) {
        if let Self::Object(members) = self {
            members.close(out);
        }
    }
}

/// One object member written into the output.
struct Member {
    /// The member's `"key":value` text.
    text: Range<usize>,
    /// Where the value text starts.
    value: usize,
}

/// An open object's distinct members, written into the output in arrival
/// order and keyed by raw (unescaped) key.
///
/// An open object's text is always the tail of the output: a container writes
/// nothing more until the object it holds is placed.
pub(super) struct Members {
    /// Where the object's `{` is.
    start: usize,
    entries: BTreeMap<Cow<'static, str>, Member>,
    /// Whether the members arrived in key order, so the text is in order too.
    in_order: bool,
}

impl Members {
    /// Close the object in `out`, rewriting its members in key order when they
    /// arrived in another order.
    fn close(self, out: &mut Vec<u8>) {
        if !self.in_order {
            let body_start = self.start + 1;
            let body = out.split_off(body_start);
            for (index, member) in self.entries.values().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(
                    &body[member.text.start - body_start..member.text.end - body_start],
                );
            }
        }
        out.push(b'}');
    }

    /// The record this root object states: its string `id` member, and its
    /// text with `id` first and the other members in key order.
    pub(super) fn into_record(
        mut self,
        out: Vec<u8>,
    ) -> Result<(crate::ids::Identity, Box<str>), NativeConvertError> {
        let id_first = self.start == 0
            && self.in_order
            && self
                .entries
                .first_key_value()
                .is_some_and(|(key, _)| key == "id");
        let member = self
            .entries
            .remove("id")
            .ok_or(NativeConvertError::MissingId)?;
        let id = serde_json::from_slice::<String>(&out[member.value..member.text.end])
            .map_err(|_| NativeConvertError::MissingId)?;
        let id = crate::ids::Identity::new(id)?;
        let json = if id_first {
            let mut json = out;
            json.push(b'}');
            json
        } else {
            let mut json = Vec::new();
            json.push(b'{');
            json.extend_from_slice(&out[member.text]);
            for member in self.entries.values() {
                json.push(b',');
                json.extend_from_slice(&out[member.text.clone()]);
            }
            json.push(b'}');
            json
        };
        let json = String::from_utf8(json).map_err(|_| {
            NativeConvertError::ConversionMessage("canonical native text is not UTF-8".to_owned())
        })?;
        Ok((id, json.into_boxed_str()))
    }
}

/// Write the opening of an externally tagged variant: `{"Variant":`.
fn open_variant(out: &mut Vec<u8>, variant: &str) -> serde_json::Result<()> {
    out.push(b'{');
    serde_json::to_writer(&mut *out, variant)?;
    out.push(b':');
    Ok(())
}

/// The canonical text of one value that may enter `depth` containers.
pub(super) fn to_text<'a, T: Serialize + ?Sized>(
    ctx: &'a DecodeContext<'a>,
    depth: usize,
    value: &T,
) -> Result<String, CanonError> {
    let mut out = Vec::new();
    value
        .serialize(CanonText::within(ctx, depth, &mut out))?
        .place(&mut out);
    String::from_utf8(out)
        .map_err(|_| <CanonError as ser::Error>::custom("canonical native text is not UTF-8"))
}

/// The text a `Display` value writes.
pub(super) fn collect_display_text<T: Display + ?Sized>(value: &T) -> Result<String, CanonError> {
    let mut text = String::new();
    write!(text, "{value}")
        .map_err(|_| <CanonError as ser::Error>::custom("a formatting error occurred"))?;
    Ok(text)
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
    Resource(CodecError),
    Message(String),
}

impl CanonError {
    /// The native conversion failure, or the session refusal behind it.
    ///
    /// A refusal raised inside a replayed container reaches this point as
    /// message text; the sticky session refusal names it again.
    pub(super) fn into_native(self, ctx: &DecodeContext<'_>) -> super::NativeConvertError {
        if let Some(limit) = ctx.resource_refusal() {
            return super::NativeConvertError::Resource(cadmpeg_core::CodecError::ResourceLimit(
                limit,
            ));
        }
        match self {
            Self::NonFinite(steps) => super::NativeConvertError::NonFiniteNumber {
                field: Self::field_path(&steps),
            },
            Self::Json(error) => super::NativeConvertError::Serde(error),
            Self::Resource(error) => super::NativeConvertError::Resource(error),
            Self::Message(message) => super::NativeConvertError::ConversionMessage(message),
        }
    }

    /// This refusal, stated from one container further out.
    pub(super) fn within(self, step: impl FnOnce() -> Step) -> Self {
        let Self::NonFinite(mut steps) = self else {
            return self;
        };
        steps.push(step());
        Self::NonFinite(steps)
    }

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
            Self::NonFinite(steps) => {
                f.write_str("field ")?;
                let mut started = false;
                for step in steps.iter().rev() {
                    match step {
                        Step::Key(key) => {
                            if started {
                                f.write_str(".")?;
                            }
                            f.write_str(key)?;
                            started |= !key.is_empty();
                        }
                        Step::Index(index) => {
                            write!(f, "[{index}]")?;
                            started = true;
                        }
                    }
                }
                f.write_str(" holds a non-finite number")
            }
            Self::Json(error) => error.fmt(f),
            Self::Resource(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for CanonError {}

impl ser::Error for CanonError {
    fn custom<T: Display>(message: T) -> Self {
        Self::Json(<serde_json::Error as ser::Error>::custom(message))
    }
}

impl From<CodecError> for CanonError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<serde_json::Error> for CanonError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Enter one container of a value that may enter `depth` containers: the
/// budget left inside it, or the refusal.
fn enter<'a>(ctx: &'a DecodeContext<'a>, depth: usize) -> Result<(usize, DepthGuard<'a>), Error> {
    match depth.checked_sub(1) {
        Some(depth) => {
            let nested = ctx.enter_nested(WORK)?;
            Ok((depth, nested))
        }
        None => Err(ser::Error::custom(nests_too_deep_message())),
    }
}

/// Open an object of a value that may enter `depth` containers.
fn open_map<'a, 'o>(
    ctx: &'a DecodeContext<'a>,
    depth: usize,
    out: &'o mut Vec<u8>,
) -> Result<CanonMap<'a, 'o>, Error> {
    let (depth, nested) = enter(ctx, depth)?;
    let start = out.len();
    out.push(b'{');
    Ok(CanonMap {
        ctx,
        _nested: nested,
        out,
        members: Members {
            start,
            entries: BTreeMap::new(),
            in_order: true,
        },
        key: None,
        depth,
    })
}

/// The canonical-text serializer. Every `serialize_*` writes into `out`; an
/// object is left open for its container to place.
pub(super) struct CanonText<'a, 'o> {
    ctx: &'a DecodeContext<'a>,
    /// Containers this value may still enter.
    depth: usize,
    out: &'o mut Vec<u8>,
}

type Error = CanonError;

impl<'a, 'o> CanonText<'a, 'o> {
    /// The serializer for one whole typed record.
    ///
    /// A record's own object is the container a stored record never holds: it
    /// keeps the members as fields and measures each field on its own. One
    /// container beyond the field bound therefore admits exactly a
    /// [`MAX_NATIVE_NESTING_DEPTH`]-deep field.
    pub(super) fn for_record(ctx: &'a DecodeContext<'a>, out: &'o mut Vec<u8>) -> Self {
        Self::within(ctx, MAX_NATIVE_NESTING_DEPTH + 1, out)
    }

    /// The serializer for a child value that may enter `depth` containers.
    pub(super) fn within(ctx: &'a DecodeContext<'a>, depth: usize, out: &'o mut Vec<u8>) -> Self {
        Self { ctx, depth, out }
    }

    /// Write a value whose text `serde_json` already writes canonically.
    fn scalar<T: Serialize + ?Sized>(self, value: &T) -> Result<Written, Error> {
        serde_json::to_writer(self.out, value)?;
        Ok(Written::Text)
    }

    /// Write a finite number, or refuse a NaN or infinite one.
    fn number(self, value: f64) -> Result<Written, Error> {
        if !value.is_finite() {
            return Err(CanonError::NonFinite(Vec::new()));
        }
        self.scalar(&value)
    }

    fn null(self) -> Written {
        self.out.extend_from_slice(b"null");
        Written::Text
    }
}

impl<'a, 'o> ser::Serializer for CanonText<'a, 'o> {
    type Ok = Written;
    type Error = Error;
    type SerializeSeq = CanonSeq<'a, 'o>;
    type SerializeTuple = CanonSeq<'a, 'o>;
    type SerializeTupleStruct = CanonSeq<'a, 'o>;
    type SerializeTupleVariant = CanonVariantSeq<'a, 'o>;
    type SerializeMap = CanonMap<'a, 'o>;
    type SerializeStruct = CanonStruct<'a, 'o>;
    type SerializeStructVariant = CanonVariantMap<'a, 'o>;

    fn serialize_bool(self, value: bool) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_i8(self, value: i8) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_i16(self, value: i16) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_i32(self, value: i32) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_i64(self, value: i64) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_i128(self, value: i128) -> Result<Written, Error> {
        if let Ok(value) = i64::try_from(value) {
            return self.scalar(&value);
        }
        if let Ok(value) = u64::try_from(value) {
            return self.scalar(&value);
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_u8(self, value: u8) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_u16(self, value: u16) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_u32(self, value: u32) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_u64(self, value: u64) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_u128(self, value: u128) -> Result<Written, Error> {
        if let Ok(value) = u64::try_from(value) {
            return self.scalar(&value);
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_f32(self, value: f32) -> Result<Written, Error> {
        self.number(f64::from(value))
    }

    fn serialize_f64(self, value: f64) -> Result<Written, Error> {
        self.number(value)
    }

    fn serialize_char(self, value: char) -> Result<Written, Error> {
        self.scalar(&value)
    }

    fn serialize_str(self, value: &str) -> Result<Written, Error> {
        self.scalar(value)
    }

    /// Bytes render as the JSON array of their values, so they enter a
    /// container and are counted through [`Self::serialize_seq`].
    fn serialize_bytes(self, value: &[u8]) -> Result<Written, Error> {
        let mut bytes = self.serialize_seq(Some(value.len()))?;
        for byte in value {
            ser::SerializeSeq::serialize_element(&mut bytes, byte)?;
        }
        ser::SerializeSeq::end(bytes)
    }

    fn serialize_none(self) -> Result<Written, Error> {
        Ok(self.null())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Written, Error> {
        let _depth = self.ctx.enter_nested(WORK)?;

        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Written, Error> {
        Ok(self.null())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Written, Error> {
        Ok(self.null())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Written, Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Written, Error> {
        let _depth = self.ctx.enter_nested(WORK)?;

        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Written, Error> {
        let (depth, _nested) = enter(self.ctx, self.depth)?;
        open_variant(self.out, variant)?;
        value
            .serialize(CanonText::within(self.ctx, depth, &mut *self.out))
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))?
            .place(self.out);
        self.out.push(b'}');
        Ok(Written::Text)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<CanonSeq<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth)?;
        self.out.push(b'[');
        Ok(CanonSeq {
            ctx: self.ctx,
            _nested: nested,
            out: self.out,
            len: 0,
            depth,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<CanonSeq<'a, 'o>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<CanonSeq<'a, 'o>, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantSeq<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth)?;
        open_variant(self.out, variant)?;
        Ok(CanonVariantSeq {
            _nested: nested,
            variant,
            seq: CanonText::within(self.ctx, depth, self.out).serialize_seq(Some(len))?,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<CanonMap<'a, 'o>, Error> {
        open_map(self.ctx, self.depth, self.out)
    }

    /// `RawValue`'s struct protocol carries one JSON value and is no container
    /// of its own, so the payload replays with this value's whole budget.
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<CanonStruct<'a, 'o>, Error> {
        if name == RAW_VALUE_STRUCT {
            Ok(CanonStruct::Raw {
                ctx: self.ctx,
                depth: self.depth,
                out: self.out,
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
        _len: usize,
    ) -> Result<CanonVariantMap<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth)?;
        open_variant(self.out, variant)?;
        Ok(CanonVariantMap {
            _nested: nested,
            variant,
            map: open_map(self.ctx, depth, self.out)?,
        })
    }

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<Written, Error> {
        let text = collect_display_text(value)?;
        self.scalar(text.as_str())
    }
}

/// A sequence written in visit order.
pub(super) struct CanonSeq<'a, 'o> {
    ctx: &'a DecodeContext<'a>,
    _nested: DepthGuard<'a>,
    out: &'o mut Vec<u8>,
    /// Elements written so far.
    len: usize,
    /// Containers each element may still enter.
    depth: usize,
}

impl<'o> CanonSeq<'_, 'o> {
    /// Write the closing bracket and give the output back.
    fn close(self) -> &'o mut Vec<u8> {
        self.out.push(b']');
        self.out
    }
}

impl ser::SerializeSeq for CanonSeq<'_, '_> {
    type Ok = Written;
    type Error = Error;

    /// A refused element leaves no text behind.
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let start = self.out.len();
        if self.len > 0 {
            self.out.push(b',');
        }
        let written = value
            .serialize(CanonText::within(self.ctx, self.depth, &mut *self.out))
            .map(|element| element.place(self.out));
        match written {
            Ok(()) => {
                self.len += 1;
                Ok(())
            }
            Err(error) => {
                self.out.truncate(start);
                let index = self.len;
                Err(error.within(|| Step::Index(index)))
            }
        }
    }

    fn end(self) -> Result<Written, Error> {
        self.close();
        Ok(Written::Text)
    }
}

impl ser::SerializeTuple for CanonSeq<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Written, Error> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for CanonSeq<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Written, Error> {
        ser::SerializeSeq::end(self)
    }
}

/// An externally tagged tuple variant: `{"Variant":[...]}`.
pub(super) struct CanonVariantSeq<'a, 'o> {
    _nested: DepthGuard<'a>,
    variant: &'static str,
    seq: CanonSeq<'a, 'o>,
}

impl ser::SerializeTupleVariant for CanonVariantSeq<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let variant = self.variant;
        ser::SerializeSeq::serialize_element(&mut self.seq, value)
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))
    }

    fn end(self) -> Result<Written, Error> {
        self.seq.close().push(b'}');
        Ok(Written::Text)
    }
}

/// An open object.
pub(super) struct CanonMap<'a, 'o> {
    ctx: &'a DecodeContext<'a>,
    _nested: DepthGuard<'a>,
    out: &'o mut Vec<u8>,
    members: Members,
    key: Option<String>,
    /// Containers each member value may still enter.
    depth: usize,
}

impl<'o> CanonMap<'_, 'o> {
    /// Admit one member. A refused value leaves no text behind.
    fn insert<T: Serialize + ?Sized>(
        &mut self,
        key: Cow<'static, str>,
        value: &T,
    ) -> Result<(), Error> {
        let entries = &mut self.members.entries;
        let after_last = entries.last_key_value().is_none_or(|(last, _)| key > *last);
        if !after_last && entries.contains_key(&key) {
            return Err(CanonError::Message(format!("duplicate key {key}")));
        }
        let out = &mut *self.out;
        let separator = out.len();
        if !entries.is_empty() {
            out.push(b',');
        }
        let text = out.len();
        match write_member(self.ctx, self.depth, out, &key, value) {
            Ok(value_start) => {
                let member = Member {
                    text: text..out.len(),
                    value: value_start,
                };
                entries.insert(key, member);
                self.members.in_order &= after_last;
                Ok(())
            }
            Err(error) => {
                out.truncate(separator);
                Err(error.within(|| Step::Key(key.into_owned())))
            }
        }
    }

    /// The open object and its output, once no key waits for its value.
    fn finish(self) -> Result<(Members, &'o mut Vec<u8>), Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom("map ended before the pending value"));
        }
        Ok((self.members, self.out))
    }
}

impl ser::SerializeMap for CanonMap<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(ser::Error::custom(
                "key serialized before the preceding value",
            ));
        }
        self.key = Some(key.serialize(CanonKey { ctx: self.ctx })?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("value serialized before key"))?;
        self.insert(Cow::Owned(key), value)
    }

    fn end(self) -> Result<Written, Error> {
        let (members, _) = self.finish()?;
        Ok(Written::Object(members))
    }
}

/// Write `"key":value` and state where the value starts.
fn write_member<'a, T: Serialize + ?Sized>(
    ctx: &'a DecodeContext<'a>,
    depth: usize,
    out: &mut Vec<u8>,
    key: &str,
    value: &T,
) -> Result<usize, Error> {
    serde_json::to_writer(&mut *out, key)?;
    out.push(b':');
    let value_start = out.len();
    value
        .serialize(CanonText::within(ctx, depth, &mut *out))?
        .place(out);
    Ok(value_start)
}

/// Ordinary struct members or one JSON value carried by `RawValue`'s protocol.
pub(super) enum CanonStruct<'a, 'o> {
    Object(CanonMap<'a, 'o>),
    Raw {
        ctx: &'a DecodeContext<'a>,
        /// Containers the payload may enter.
        depth: usize,
        out: &'o mut Vec<u8>,
        /// The replayed payload, once its one field has arrived.
        parsed: Option<Written>,
    },
}

impl ser::SerializeStruct for CanonStruct<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        match self {
            Self::Object(map) => map.insert(Cow::Borrowed(key), value),
            Self::Raw {
                ctx,
                depth,
                out,
                parsed,
            } => {
                if key != RAW_VALUE_STRUCT || parsed.is_some() {
                    return Err(ser::Error::custom(
                        "raw JSON requires exactly one payload field",
                    ));
                }
                let text = to_text(ctx, *depth, value)?;
                let Ok(json) = serde_json::from_str::<String>(&text) else {
                    return Err(ser::Error::custom("raw JSON payload must be a string"));
                };
                // Replay through the same canonical writer, so raw objects
                // obey duplicate-key, number, ordering and depth semantics too.
                // The replay counts the text's containers and this serializer
                // counts the containers it is driven through, from the same
                // remaining budget, so both refuse at the same container.
                let start = out.len();
                match super::replay::emit(&json, CanonText::within(ctx, *depth, out), *depth) {
                    Ok(replayed) => {
                        *parsed = Some(replayed);
                        Ok(())
                    }
                    Err(error) => {
                        out.truncate(start);
                        Err(error)
                    }
                }
            }
        }
    }

    fn end(self) -> Result<Written, Error> {
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
pub(super) struct CanonVariantMap<'a, 'o> {
    _nested: DepthGuard<'a>,
    variant: &'static str,
    map: CanonMap<'a, 'o>,
}

impl ser::SerializeStructVariant for CanonVariantMap<'_, '_> {
    type Ok = Written;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let variant = self.variant;
        self.map
            .insert(Cow::Borrowed(key), value)
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))
    }

    fn end(self) -> Result<Written, Error> {
        let (members, out) = self.map.finish()?;
        members.close(out);
        out.push(b'}');
        Ok(Written::Text)
    }
}

/// Map-key serializer with `serde_json::Value`'s key conventions: strings
/// pass through, scalar keys and unit variants use their string forms, and
/// compound or absent keys are rejected. Floating-point keys must be finite.
pub(super) struct CanonKey<'a> {
    pub(super) ctx: &'a DecodeContext<'a>,
}

struct NumberText {
    bytes: [u8; 128],
    len: usize,
}

impl Write for NumberText {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self
            .len
            .checked_add(bytes.len())
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| io::Error::other("native number text exceeds its fixed buffer"))?;
        self.bytes[self.len..end].copy_from_slice(bytes);
        self.len = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn number_key_text<T: Serialize>(value: &T) -> Result<String, Error> {
    let mut text = NumberText {
        bytes: [0; 128],
        len: 0,
    };
    serde_json::to_writer(&mut text, value)?;
    let text = std::str::from_utf8(&text.bytes[..text.len])
        .map_err(|_| <Error as ser::Error>::custom("native number text is not UTF-8"))?;
    Ok(text.to_owned())
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
        Ok(text.to_owned())
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
            number_key_text(&value)
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_f64(self, value: f64) -> Result<String, Error> {
        if value.is_finite() {
            number_key_text(&value)
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
        let _depth = self.ctx.enter_nested(WORK)?;

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

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<String, Error> {
        collect_display_text(value)
    }
}

#[cfg(test)]
mod tests;
