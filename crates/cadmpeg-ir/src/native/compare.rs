// SPDX-License-Identifier: Apache-2.0
//! Compare a typed serialization against a stored record's text without
//! parsing its payload into a value tree.
//!
//! The stored text is canonical: `serde_json` wrote it from a value tree, and the
//! expected side renders each scalar the same way, so equal scalars have equal
//! text.

use super::{canon, replay, NativeConvertError, NativeRecord, MAX_NATIVE_NESTING_DEPTH};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard};
use serde::{ser, Serialize};
use std::collections::BTreeSet;

type Error = canon::CanonError;
const OPERATION: &str = "compare native record";
const RAW_VALUE_STRUCT: &str = "$serde_json::private::RawValue";

/// One stored JSON value, as its text, or the absence of one.
#[derive(Clone, Copy)]
enum Borrowed<'a> {
    Text(&'a str),
    Missing,
}
impl<'a> Borrowed<'a> {
    /// States whether this value is the text `expected` renders to.
    fn renders<T: Serialize + ?Sized>(self, expected: &T) -> bool {
        let Self::Text(actual) = self else {
            return false;
        };
        let mut rest = SameText {
            rest: actual.as_bytes(),
        };
        serde_json::to_writer(&mut rest, expected).is_ok() && rest.rest.is_empty()
    }
    /// States whether this value is `expected` once parsed.
    fn parses_to(self, expected: &serde_json::Value) -> bool {
        let Self::Text(actual) = self else {
            return false;
        };
        replay::value(actual).is_ok_and(|actual| actual == *expected)
    }
    /// The members of this object, read in one scan, or `None` when this is
    /// not an object.
    fn members(self, ctx: &DecodeContext<'_>) -> Result<Option<replay::MemberTexts<'a>>, Error> {
        let Self::Text(json) = self else {
            return Ok(None);
        };
        let Some(count) = replay::member_count(json) else {
            return Ok(None);
        };
        ctx.charge_work(u64_from_index(count), OPERATION)?;
        let mut members = Vec::new();
        ctx.reserve_vec(&mut members, count, OPERATION)?;
        Ok(replay::members(json, &mut members).then_some(members))
    }
    fn elements(self) -> Option<replay::Elements<'a>> {
        match self {
            Self::Text(json) => replay::Elements::of(json),
            Self::Missing => None,
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Text(json) => json.len(),
            Self::Missing => 0,
        }
    }
}

/// Accepts written bytes while they continue the stored text.
struct SameText<'a> {
    rest: &'a [u8],
}
impl std::io::Write for SameText<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        match self.rest.strip_prefix(bytes) {
            Some(rest) => {
                self.rest = rest;
                Ok(bytes.len())
            }
            None => Err(std::io::Error::other("differs from the stored text")),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn matches<T: Serialize + ?Sized>(
    ctx: &DecodeContext<'_>,
    actual: &NativeRecord,
    expected: &T,
) -> Result<bool, NativeConvertError> {
    let (matched, _storage) = ctx.with_scoped_storage(OPERATION, || {
        expected
            .serialize(Matcher {
                ctx,
                actual: Borrowed::Text(&actual.json),
                remaining: MAX_NATIVE_NESTING_DEPTH + 1,
            })
            .map_err(|error| error.into_native(ctx))
    })?;
    Ok(matched)
}

#[derive(Clone, Copy)]
struct Matcher<'a, 'arena> {
    ctx: &'a DecodeContext<'arena>,
    actual: Borrowed<'a>,
    remaining: usize,
}
impl<'a, 'arena> Matcher<'a, 'arena> {
    fn container(self) -> Result<DepthGuard<'a>, Error> {
        self.ctx.charge_work(1, OPERATION)?;
        if self.remaining == 0 {
            return Err(<Error as ser::Error>::custom(
                super::nests_too_deep_message(),
            ));
        }
        Ok(self.ctx.enter_nested(OPERATION)?)
    }
    fn child(self, actual: Borrowed<'a>) -> Result<Self, Error> {
        Ok(Self {
            actual,
            remaining: self
                .remaining
                .checked_sub(1)
                .ok_or_else(|| <Error as ser::Error>::custom(super::nests_too_deep_message()))?,
            ..self
        })
    }
    fn scalar(self, value: impl Serialize) -> Result<bool, Error> {
        let expected = value
            .serialize(canon::CanonValue::for_record_with_sink(self.ctx, None))?
            .into_value();
        Ok(self.actual.renders(&expected))
    }
    fn sequence(
        self,
        outer: Option<DepthGuard<'a>>,
        parent_matches: bool,
    ) -> Result<Sequence<'a, 'arena>, Error> {
        let nested = self.container()?;
        let actual = self.actual.elements();
        Ok(Sequence {
            matcher: self,
            matched: parent_matches && actual.is_some(),
            actual,
            index: 0,
            _nested: nested,
            _outer: outer,
        })
    }
    fn map(
        self,
        outer: Option<DepthGuard<'a>>,
        parent_matches: bool,
    ) -> Result<Object<'a, 'arena>, Error> {
        let nested = self.container()?;
        let members = self.actual.members(self.ctx)?;
        Ok(Object {
            matcher: self,
            matched: parent_matches && members.is_some(),
            members,
            seen: BTreeSet::new(),
            key: None,
            raw: false,
            _nested: nested,
            _outer: outer,
        })
    }
    fn variant(self, variant: &'static str) -> Result<(Self, DepthGuard<'a>, bool), Error> {
        let outer = self.container()?;
        self.ctx
            .charge_work(u64_from_index(variant.len()), OPERATION)?;
        let members = self.actual.members(self.ctx)?.unwrap_or_default();
        let payload = match members.as_slice() {
            [(key, payload)] if key == variant => Borrowed::Text(payload),
            _ => Borrowed::Missing,
        };
        Ok((self.child(payload)?, outer, members.len() == 1))
    }
}

macro_rules! scalars {
    ($($method:ident($ty:ty)),* $(,)?) => {
        $(fn $method(self, value: $ty) -> Result<bool, Error> { self.scalar(value) })*
    };
}
impl<'a, 'arena> ser::Serializer for Matcher<'a, 'arena> {
    type Ok = bool;
    type Error = Error;
    type SerializeSeq = Sequence<'a, 'arena>;
    type SerializeTuple = Sequence<'a, 'arena>;
    type SerializeTupleStruct = Sequence<'a, 'arena>;
    type SerializeTupleVariant = Sequence<'a, 'arena>;
    type SerializeMap = Object<'a, 'arena>;
    type SerializeStruct = Object<'a, 'arena>;
    type SerializeStructVariant = Object<'a, 'arena>;
    scalars!(
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64)
    );
    fn serialize_char(self, value: char) -> Result<bool, Error> {
        self.serialize_str(value.encode_utf8(&mut [0; 4]))
    }
    fn serialize_str(self, value: &str) -> Result<bool, Error> {
        let work = u64_from_index(value.len()).checked_add(1).ok_or_else(|| {
            self.ctx
                .refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
        })?;
        self.ctx.charge_work(work, OPERATION)?;
        Ok(self.actual.renders(value))
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<bool, Error> {
        let mut sequence = self.sequence(None, true)?;
        for byte in value {
            ser::SerializeSeq::serialize_element(&mut sequence, byte)?;
        }
        ser::SerializeSeq::end(sequence)
    }
    fn serialize_none(self) -> Result<bool, Error> {
        self.serialize_unit()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<bool, Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<bool, Error> {
        self.scalar(())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<bool, Error> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<bool, Error> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<bool, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<bool, Error> {
        let (inner, _outer, matched) = self.variant(variant)?;
        Ok(value.serialize(inner)? && matched)
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        self.sequence(None, true)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        self.sequence(None, true)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.sequence(None, true)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        let (inner, outer, matched) = self.variant(variant)?;
        inner.sequence(Some(outer), matched)
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.map(None, true)
    }
    fn serialize_struct(
        self,
        name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        let mut object = self.map(None, true)?;
        if name == RAW_VALUE_STRUCT {
            object.raw = true;
            object.matched = true;
        }
        Ok(object)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        let (inner, outer, matched) = self.variant(variant)?;
        inner.map(Some(outer), matched)
    }
    fn collect_str<T: std::fmt::Display + ?Sized>(self, value: &T) -> Result<bool, Error> {
        // Canonical display serialization admits its growing string before writing.
        let expected = canon::CanonValue::for_record_with_sink(self.ctx, None)
            .collect_str(value)?
            .into_value();
        Ok(expected.is_string() && self.actual.renders(&expected))
    }
}

struct Sequence<'a, 'arena> {
    matcher: Matcher<'a, 'arena>,
    actual: Option<replay::Elements<'a>>,
    index: usize,
    matched: bool,
    _nested: DepthGuard<'a>,
    _outer: Option<DepthGuard<'a>>,
}
impl Sequence<'_, '_> {
    fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let actual = self
            .actual
            .as_mut()
            .and_then(Iterator::next)
            .map_or(Borrowed::Missing, Borrowed::Text);
        self.matched &= value
            .serialize(self.matcher.child(actual)?)
            .map_err(|error| {
                error.within(self.matcher.ctx, || Ok(canon::Step::Index(self.index)))
            })?;
        self.index = self
            .index
            .checked_add(1)
            .ok_or_else(|| <Error as ser::Error>::custom("native sequence length overflow"))?;
        Ok(())
    }
    fn finish(self) -> bool {
        self.matched && self.actual.is_some_and(|mut rest| rest.next().is_none())
    }
}
macro_rules! sequences {
    ($($trait:ident::$method:ident),* $(,)?) => {
        $(impl ser::$trait for Sequence<'_, '_> {
            type Ok = bool;
            type Error = Error;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> { self.element(value) }
            fn end(self) -> Result<bool, Error> { Ok(self.finish()) }
        })*
    };
}
sequences!(
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
    SerializeTupleVariant::serialize_field
);

struct Object<'a, 'arena> {
    matcher: Matcher<'a, 'arena>,
    /// The stored object's members, or `None` when it is not an object.
    members: Option<replay::MemberTexts<'a>>,
    seen: BTreeSet<String>,
    key: Option<String>,
    matched: bool,
    raw: bool,
    _nested: DepthGuard<'a>,
    _outer: Option<DepthGuard<'a>>,
}
impl Object<'_, '_> {
    fn field<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), Error> {
        if self.raw {
            if key != RAW_VALUE_STRUCT || !self.seen.is_empty() {
                return Err(<Error as ser::Error>::custom(
                    "invalid raw native comparison field",
                ));
            }
            let expected = RawField(value)
                .serialize(canon::CanonValue::within(
                    self.matcher.ctx,
                    self.matcher.remaining,
                    None,
                ))?
                .into_value();
            self.matcher
                .ctx
                .charge_work(u64_from_index(self.matcher.actual.len()), OPERATION)?;
            self.matched &= self.matcher.actual.parses_to(&expected);
            self.matcher
                .ctx
                .insert_btree_set(&mut self.seen, key, OPERATION)?;
            return Ok(());
        }
        let members = self.members.as_deref().unwrap_or_default();
        let work = u64_from_index(key.len())
            .checked_add(1)
            .and_then(|bytes| bytes.checked_mul(u64_from_index(members.len())))
            .ok_or_else(|| {
                self.matcher
                    .ctx
                    .refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX)
            })?;
        self.matcher.ctx.charge_work(work, OPERATION)?;
        let actual = members
            .iter()
            .find(|(member, _)| *member == key)
            .map_or(Borrowed::Missing, |(_, value)| Borrowed::Text(value));
        self.matched &= value
            .serialize(self.matcher.child(actual)?)
            .map_err(|error| {
                error.within(self.matcher.ctx, || {
                    self.matcher
                        .ctx
                        .copy_retained_text(&key, OPERATION)
                        .map(canon::Step::Key)
                        .map_err(Error::from)
                })
            })?;
        if !self
            .matcher
            .ctx
            .insert_btree_set(&mut self.seen, key, OPERATION)?
        {
            return Err(<Error as ser::Error>::custom(
                "duplicate native comparison key",
            ));
        }
        Ok(())
    }
    fn finish(self) -> Result<bool, Error> {
        if self.key.is_some() {
            return Err(<Error as ser::Error>::custom("native map key has no value"));
        }
        Ok(self.matched
            && if self.raw {
                self.seen.len() == 1
            } else {
                self.members.as_ref().map(Vec::len) == Some(self.seen.len())
            })
    }
}
impl ser::SerializeMap for Object<'_, '_> {
    type Ok = bool;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(<Error as ser::Error>::custom("native map key has no value"));
        }
        self.key = Some(key.serialize(canon::CanonKey {
            ctx: self.matcher.ctx,
            sink: None,
        })?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .ok_or_else(|| <Error as ser::Error>::custom("native map value has no key"))?;
        self.field(key, value)
    }
    fn end(self) -> Result<bool, Error> {
        self.finish()
    }
}
macro_rules! objects {
    ($($trait:ident),* $(,)?) => {
        $(impl ser::$trait for Object<'_, '_> {
            type Ok = bool;
            type Error = Error;
            fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
                self.field(self.matcher.ctx.copy_retained_text(key, OPERATION)?, value)
            }
            fn end(self) -> Result<bool, Error> { self.finish() }
        })*
    };
}
objects!(SerializeStruct, SerializeStructVariant);

struct RawField<'a, T: ?Sized>(&'a T);
impl<T: Serialize + ?Sized> Serialize for RawField<'_, T> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_struct(RAW_VALUE_STRUCT, 1)?;
        ser::SerializeStruct::serialize_field(&mut object, RAW_VALUE_STRUCT, self.0)?;
        ser::SerializeStruct::end(object)
    }
}

#[cfg(test)]
mod tests;
