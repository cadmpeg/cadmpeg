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
use std::collections::HashSet;
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

/// Fast JSON string writer for the common case with no characters requiring escaping.
#[inline]
fn write_json_str(out: &mut Vec<u8>, text: &str) -> serde_json::Result<()> {
    let bytes = text.as_bytes();
    if bytes.iter().all(|&b| b >= 0x20 && b != b'"' && b != b'\\') {
        out.reserve(bytes.len() + 2);
        out.push(b'"');
        out.extend_from_slice(bytes);
        out.push(b'"');
        return Ok(());
    }
    serde_json::to_writer(out, text)
}

#[inline]
fn write_u64(out: &mut Vec<u8>, mut value: u64) {
    let mut buf = [0_u8; 20];
    let mut pos = 20;
    loop {
        pos -= 1;
        buf[pos] = b'0' + u8::try_from(value % 10).unwrap_or(0);
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out.extend_from_slice(&buf[pos..]);
}

#[inline]
fn write_i64(out: &mut Vec<u8>, value: i64) {
    if value < 0 {
        out.push(b'-');
    }
    write_u64(out, value.unsigned_abs());
}

/// An open object's distinct members, written into the output in arrival
/// order and keyed by raw (unescaped) key.
///
/// An open object's text is always the tail of the output: a container writes
/// nothing more until the object it holds is placed.
pub(super) struct Members {
    /// Where the object's `{` is.
    start: usize,
    entries: Vec<(Cow<'static, str>, Member)>,
    /// Populated only for objects with more than 24 keys to keep duplicate checks O(1).
    seen: Option<HashSet<Cow<'static, str>>>,
    /// Whether the members arrived in strictly increasing key order.
    in_order: bool,
}

impl Members {
    /// Close the object in `out`, rewriting its members in key order when they
    /// arrived in another order.
    fn close(mut self, out: &mut Vec<u8>) {
        if !self.in_order {
            self.entries
                .sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let body = self.start + 1..out.len();
            reorder(
                out,
                body,
                self.entries.iter().map(|(_, member)| &member.text),
            );
        }
        out.push(b'}');
    }

    /// The record this root object states: its string `id` member, and its
    /// text with `id` first and the other members in key order.
    pub(super) fn into_record(
        mut self,
        out: &mut Vec<u8>,
    ) -> Result<(crate::ids::Identity, Box<str>), NativeConvertError> {
        let id_pos = self
            .entries
            .iter()
            .position(|(key, _)| key == "id")
            .ok_or(NativeConvertError::MissingId)?;
        let member_range = self.entries[id_pos].1.text.clone();
        let member_value = self.entries[id_pos].1.value;
        let raw_id = &out[member_value..member_range.end];
        let id = if raw_id.len() >= 2
            && raw_id[0] == b'"'
            && raw_id[raw_id.len() - 1] == b'"'
            && raw_id[1..raw_id.len() - 1]
                .iter()
                .all(|&b| b >= 0x20 && b != b'\\' && b != b'"')
        {
            let text = std::str::from_utf8(&raw_id[1..raw_id.len() - 1])
                .map_err(|_| NativeConvertError::MissingId)?;
            crate::ids::Identity::new(text)?
        } else {
            let id = cadmpeg_core::serde_erase::json::from_slice::<String>(raw_id)
                .map_err(|_| NativeConvertError::MissingId)?;
            crate::ids::Identity::new(id)?
        };
        let already_canonical = self.start == 0
            && id_pos == 0
            && (self.in_order
                || self.entries[1..]
                    .windows(2)
                    .all(|pair| pair[0].0 < pair[1].0));
        if !already_canonical {
            let (_, id_member) = self.entries.swap_remove(id_pos);
            self.entries
                .sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let body = self.start + 1..out.len();
            let order = std::iter::once(&id_member.text)
                .chain(self.entries.iter().map(|(_, member)| &member.text));
            reorder(out, body, order);
        }
        out.push(b'}');
        let slice = std::str::from_utf8(&out[self.start..]).map_err(|_| {
            NativeConvertError::ConversionMessage("canonical native text is not UTF-8".to_owned())
        })?;
        Ok((id, Box::from(slice)))
    }
}

/// Rewrite the member texts in `body` of `out` in the sequence `order` names.
///
/// `order` names every member text in `body` once, and in `body` the texts
/// follow each other with one comma between neighbours, so the rewritten body
/// has the same length. The longest text moves within `out`; only the others
/// are copied aside, so a large payload member is never held twice.
fn reorder<'m>(
    out: &mut [u8],
    body: Range<usize>,
    order: impl Iterator<Item = &'m Range<usize>> + Clone,
) {
    let Some((longest, longest_text)) =
        order.clone().enumerate().max_by_key(|(_, text)| text.len())
    else {
        return;
    };
    let mut aside = Vec::with_capacity(body.len() - longest_text.len());
    let mut target = body.start;
    for (index, text) in order.clone().enumerate() {
        if index != longest {
            aside.extend_from_slice(&out[text.clone()]);
        }
        if index < longest {
            target += text.len() + 1;
        }
    }
    out.copy_within(longest_text.clone(), target);
    let mut at = body.start;
    let mut read = 0;
    for (index, text) in order.enumerate() {
        if index > 0 {
            out[at] = b',';
            at += 1;
        }
        if index != longest {
            out[at..at + text.len()].copy_from_slice(&aside[read..read + text.len()]);
            read += text.len();
        }
        at += text.len();
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
        .serialize(CanonText::within(ctx, depth, &mut out, None))?
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
fn enter<'a>(
    ctx: &'a DecodeContext<'a>,
    depth: usize,
    min_remaining: &mut Option<&mut usize>,
) -> Result<(usize, DepthGuard<'a>), Error> {
    match depth.checked_sub(1) {
        Some(depth) => {
            let nested = ctx.enter_nested(WORK)?;
            if let Some(min) = min_remaining.as_deref_mut() {
                *min = (*min).min(depth);
            }
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
    mut min_remaining: Option<&'o mut usize>,
    len: Option<usize>,
) -> Result<CanonMap<'a, 'o>, Error> {
    let (depth, nested) = enter(ctx, depth, &mut min_remaining)?;
    let start = out.len();
    out.push(b'{');
    Ok(CanonMap {
        ctx,
        _nested: nested,
        out,
        min_remaining,
        members: Members {
            start,
            entries: Vec::with_capacity(len.unwrap_or(0).min(32)),
            seen: None,
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
    /// Smallest remaining container budget observed while writing this value.
    min_remaining: Option<&'o mut usize>,
}

type Error = CanonError;

impl<'a, 'o> CanonText<'a, 'o> {
    /// The serializer for one whole typed record.
    ///
    /// A record's own object is the container a stored record never holds: it
    /// keeps the members as fields and measures each field on its own. One
    /// container beyond the field bound therefore admits exactly a
    /// [`MAX_NATIVE_NESTING_DEPTH`]-deep field.
    #[cfg(test)]
    pub(super) fn for_record(ctx: &'a DecodeContext<'a>, out: &'o mut Vec<u8>) -> Self {
        Self::within(ctx, MAX_NATIVE_NESTING_DEPTH + 1, out, None)
    }

    /// The serializer for one whole typed record, recording the smallest
    /// remaining container budget in `min_remaining` so callers recover the
    /// exact JSON container nesting depth without rescanning the output.
    pub(super) fn for_record_tracking(
        ctx: &'a DecodeContext<'a>,
        out: &'o mut Vec<u8>,
        min_remaining: &'o mut usize,
    ) -> Self {
        *min_remaining = MAX_NATIVE_NESTING_DEPTH + 1;
        Self::within(ctx, MAX_NATIVE_NESTING_DEPTH + 1, out, Some(min_remaining))
    }

    /// The serializer for a child value that may enter `depth` containers.
    pub(super) fn within(
        ctx: &'a DecodeContext<'a>,
        depth: usize,
        out: &'o mut Vec<u8>,
        min_remaining: Option<&'o mut usize>,
    ) -> Self {
        Self {
            ctx,
            depth,
            out,
            min_remaining,
        }
    }

    /// Write a value whose text `serde_json` already writes canonically.
    fn scalar<T: Serialize + ?Sized>(self, value: &T) -> Result<Written, Error> {
        serde_json::to_writer(self.out, value)?;
        Ok(Written::Text)
    }

    /// Write a finite number, negative zero as zero (§2.5), or refuse a NaN
    /// or infinite one.
    fn number(self, value: f64) -> Result<Written, Error> {
        if !value.is_finite() {
            return Err(CanonError::NonFinite(Vec::new()));
        }
        self.scalar(&(value + 0.0))
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
        self.out
            .extend_from_slice(if value { b"true" } else { b"false" });
        Ok(Written::Text)
    }

    fn serialize_i8(self, value: i8) -> Result<Written, Error> {
        write_i64(self.out, i64::from(value));
        Ok(Written::Text)
    }

    fn serialize_i16(self, value: i16) -> Result<Written, Error> {
        write_i64(self.out, i64::from(value));
        Ok(Written::Text)
    }

    fn serialize_i32(self, value: i32) -> Result<Written, Error> {
        write_i64(self.out, i64::from(value));
        Ok(Written::Text)
    }

    fn serialize_i64(self, value: i64) -> Result<Written, Error> {
        write_i64(self.out, value);
        Ok(Written::Text)
    }

    fn serialize_i128(self, value: i128) -> Result<Written, Error> {
        if let Ok(value) = i64::try_from(value) {
            write_i64(self.out, value);
            return Ok(Written::Text);
        }
        if let Ok(value) = u64::try_from(value) {
            write_u64(self.out, value);
            return Ok(Written::Text);
        }
        Err(ser::Error::custom("number out of range"))
    }

    fn serialize_u8(self, value: u8) -> Result<Written, Error> {
        write_u64(self.out, u64::from(value));
        Ok(Written::Text)
    }

    fn serialize_u16(self, value: u16) -> Result<Written, Error> {
        write_u64(self.out, u64::from(value));
        Ok(Written::Text)
    }

    fn serialize_u32(self, value: u32) -> Result<Written, Error> {
        write_u64(self.out, u64::from(value));
        Ok(Written::Text)
    }

    fn serialize_u64(self, value: u64) -> Result<Written, Error> {
        write_u64(self.out, value);
        Ok(Written::Text)
    }

    fn serialize_u128(self, value: u128) -> Result<Written, Error> {
        if let Ok(value) = u64::try_from(value) {
            write_u64(self.out, value);
            return Ok(Written::Text);
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
        write_json_str(self.out, value)?;
        Ok(Written::Text)
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
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Written, Error> {
        let (depth, _nested) = enter(self.ctx, self.depth, &mut self.min_remaining)?;
        open_variant(self.out, variant)?;
        value
            .serialize(CanonText::within(
                self.ctx,
                depth,
                &mut *self.out,
                self.min_remaining,
            ))
            .map_err(|error| error.within(|| Step::Key(variant.to_owned())))?
            .place(self.out);
        self.out.push(b'}');
        Ok(Written::Text)
    }

    fn serialize_seq(mut self, _len: Option<usize>) -> Result<CanonSeq<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth, &mut self.min_remaining)?;
        self.out.push(b'[');
        Ok(CanonSeq {
            ctx: self.ctx,
            _nested: nested,
            out: self.out,
            min_remaining: self.min_remaining,
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
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantSeq<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth, &mut self.min_remaining)?;
        open_variant(self.out, variant)?;
        Ok(CanonVariantSeq {
            _nested: nested,
            variant,
            seq: CanonText::within(self.ctx, depth, self.out, self.min_remaining)
                .serialize_seq(Some(len))?,
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<CanonMap<'a, 'o>, Error> {
        open_map(self.ctx, self.depth, self.out, self.min_remaining, len)
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
                min_remaining: self.min_remaining,
                parsed: None,
            })
        } else {
            self.serialize_map(Some(len)).map(CanonStruct::Object)
        }
    }

    fn serialize_struct_variant(
        mut self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<CanonVariantMap<'a, 'o>, Error> {
        let (depth, nested) = enter(self.ctx, self.depth, &mut self.min_remaining)?;
        open_variant(self.out, variant)?;
        Ok(CanonVariantMap {
            _nested: nested,
            variant,
            map: open_map(self.ctx, depth, self.out, self.min_remaining, Some(len))?,
        })
    }

    /// The text goes straight into the output, so a long payload such as a
    /// byte string's hexadecimal text is never held twice.
    fn collect_str<T: Display + ?Sized>(self, value: &T) -> Result<Written, Error> {
        let start = self.out.len();
        self.out.push(b'"');
        let mut content = StringContent {
            out: &mut *self.out,
        };
        if write!(content, "{value}").is_err() {
            self.out.truncate(start);
            return Err(ser::Error::custom("a formatting error occurred"));
        }
        self.out.push(b'"');
        Ok(Written::Text)
    }
}

/// Writes text as the content of a JSON string, escaped as `serde_json`
/// escapes it.
struct StringContent<'o> {
    out: &'o mut Vec<u8>,
}

impl std::fmt::Write for StringContent<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        // `serde_json` escapes exactly the control characters, `"` and `\`.
        if text
            .bytes()
            .all(|byte| byte >= 0x20 && byte != b'"' && byte != b'\\')
        {
            self.out.extend_from_slice(text.as_bytes());
            return Ok(());
        }
        let start = self.out.len();
        serde_json::to_writer(&mut *self.out, text).map_err(|_| std::fmt::Error)?;
        self.out.pop();
        self.out.remove(start);
        Ok(())
    }
}

/// A sequence written in visit order.
pub(super) struct CanonSeq<'a, 'o> {
    ctx: &'a DecodeContext<'a>,
    _nested: DepthGuard<'a>,
    out: &'o mut Vec<u8>,
    min_remaining: Option<&'o mut usize>,
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
            .serialize(CanonText::within(
                self.ctx,
                self.depth,
                &mut *self.out,
                self.min_remaining.as_deref_mut(),
            ))
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
    min_remaining: Option<&'o mut usize>,
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
        let after_last = entries.last().is_none_or(|(last, _)| key > *last);
        let duplicate = if let Some(seen) = &self.members.seen {
            seen.contains(&key)
        } else if self.members.in_order && after_last {
            false
        } else {
            entries.iter().any(|(existing, _)| *existing == key)
        };
        if duplicate {
            return Err(CanonError::Message(format!("duplicate key {key}")));
        }
        let out = &mut *self.out;
        let separator = out.len();
        if !entries.is_empty() {
            out.push(b',');
        }
        let text = out.len();
        match write_member(
            self.ctx,
            self.depth,
            out,
            self.min_remaining.as_deref_mut(),
            &key,
            value,
        ) {
            Ok(value_start) => {
                let member = Member {
                    text: text..out.len(),
                    value: value_start,
                };
                if let Some(seen) = &mut self.members.seen {
                    seen.insert(key.clone());
                } else if entries.len() >= 24 {
                    let mut seen = HashSet::with_capacity(entries.len() + 8);
                    for (existing, _) in entries.iter() {
                        seen.insert(existing.clone());
                    }
                    seen.insert(key.clone());
                    self.members.seen = Some(seen);
                }
                entries.push((key, member));
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
    min_remaining: Option<&mut usize>,
    key: &str,
    value: &T,
) -> Result<usize, Error> {
    write_json_str(out, key)?;
    out.push(b':');
    let value_start = out.len();
    value
        .serialize(CanonText::within(ctx, depth, &mut *out, min_remaining))?
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
        min_remaining: Option<&'o mut usize>,
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
                min_remaining,
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
                match super::replay::emit(
                    &json,
                    CanonText::within(ctx, *depth, out, min_remaining.as_deref_mut()),
                    *depth,
                ) {
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
            number_key_text(&(value + 0.0))
        } else {
            Err(ser::Error::custom("float key must be finite"))
        }
    }

    fn serialize_f64(self, value: f64) -> Result<String, Error> {
        if value.is_finite() {
            number_key_text(&(value + 0.0))
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
