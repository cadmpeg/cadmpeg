// SPDX-License-Identifier: Apache-2.0
//! Typed reads of stored native records, charged for what the reader asks for.
//!
//! A codec-owned type reads a stored record through [`Reader`], which borrows
//! the stored value and admits each request before the reader acts on it. The
//! request names what the reader keeps:
//!
//! - a scalar request keeps the scalar's width;
//! - a `string` or `str` request keeps a `String` header and the text;
//! - an identifier request matches a struct field or variant and keeps
//!   nothing, and an ignored value keeps nothing;
//! - an absent option keeps one tag byte;
//! - a sequence keeps a `Vec` header and each element one collection item and
//!   whatever its own requests keep, paid again as work for one move when the
//!   vector grows;
//! - a map keeps a header and, per member, one collection item and B-tree
//!   node storage for a `String` key and a `Value` value, plus the key
//!   comparisons of one insertion;
//! - a struct keeps nothing of its own; its fields pay for themselves;
//! - a self-describing request (`deserialize_any`) keeps one `Value` per node,
//!   which bounds a `Value` target and serde's buffered content, and treats an
//!   object as a map.
//!
//! Every visit pays one work unit, text pays its bytes as work, and every
//! container is entered as one nesting level.
//!
//! The bound is per request, so a reader's own layout can exceed it by a
//! factor fixed by its types, never by the file: struct padding and enum tags
//! are not charged, a map whose key and value are wider than a `String` and a
//! `Value` stores wider nodes, and a vector past serde's preallocation cap can
//! hold up to twice its elements while it grows. Types using
//! `#[serde(flatten)]` or `#[serde(untagged)]` buffer the members they read
//! through `deserialize_any`, which is charged, and then read the buffer again
//! without this reader, once per member or per variant tried.

use std::cell::Cell;
use std::io;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use serde::de::value::BorrowedStrDeserializer;
use serde::de::{
    self, DeserializeSeed, Deserializer as _, EnumAccess, MapAccess, SeqAccess, Unexpected,
    VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;
use serde_json::{Error, Map, Value};

pub(super) const TYPED_READ: &str = "load typed native record";

const STRING_HEADER: usize = std::mem::size_of::<String>();
const VEC_HEADER: usize = std::mem::size_of::<Vec<u8>>();
const MAP_HEADER: usize = std::mem::size_of::<std::collections::BTreeMap<u8, u8>>();
const VALUE_WIDTH: usize = std::mem::size_of::<Value>();
const RAW_VALUE_TOKEN: &str = "$serde_json::private::RawValue";

/// The budget account a typed read draws on.
#[derive(Clone, Copy)]
pub(super) struct Account<'a> {
    ctx: &'a DecodeContext<'a>,
    /// Bytes the read has kept so far.
    kept: &'a Cell<u64>,
}

impl<'a> Account<'a> {
    pub(super) fn new(ctx: &'a DecodeContext<'a>, kept: &'a Cell<u64>) -> Self {
        Self { ctx, kept }
    }

    fn admit(self, work: usize, kept: usize) -> Result<(), Error> {
        let work = u64_from_index(work);
        let kept = u64_from_index(kept);
        self.ctx
            .charge_work(work, TYPED_READ)
            .and_then(|()| self.ctx.charge_retained(kept, TYPED_READ))
            .map_err(|_| refused())?;
        // Every kept byte was charged first, so the total stays within the
        // retained counter.
        self.kept
            .set(self.kept.get().checked_add(kept).ok_or_else(refused)?);
        Ok(())
    }

    fn text(self, text: &str, kept: usize) -> Result<(), Error> {
        self.admit(text.len(), kept)
    }

    fn enter(self) -> Result<cadmpeg_core::decode::DepthGuard<'a>, Error> {
        self.ctx.enter_nested(TYPED_READ).map_err(|_| refused())
    }
}

/// The error a reader sees for a budget refusal. The session is fused by
/// then, and the caller reports the fused limit instead of this text.
fn refused() -> Error {
    de::Error::custom("native record read refused by the decode budget")
}

/// Text length of a value's JSON form, each written chunk admitted as work.
struct CountedText<'a> {
    account: Account<'a>,
    len: usize,
}

impl io::Write for CountedText<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.account
            .admit(bytes.len(), 0)
            .map_err(|_| io::Error::other("native record read refused"))?;
        self.len = self
            .len
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("native record text overflows"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A stored value read on behalf of a typed reader.
pub(super) struct Reader<'a> {
    pub(super) account: Account<'a>,
    pub(super) value: &'a Value,
}

macro_rules! scalar {
    ($($method:ident: $width:ty;)*) => {$(
        fn $method<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
            self.account.admit(1, std::mem::size_of::<$width>())?;
            self.value.$method(visitor)
        }
    )*};
}

impl<'a> Reader<'a> {
    fn within(&self, value: &'a Value) -> Self {
        Self {
            account: self.account,
            value,
        }
    }

    fn sequence<V: Visitor<'a>>(
        self,
        items: &'a [Value],
        header: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        let _depth = self.account.enter()?;
        self.account.admit(1, header)?;
        let mut access = Elements {
            account: self.account,
            items: items.iter(),
        };
        let read = visitor.visit_seq(&mut access)?;
        if access.items.len() == 0 {
            Ok(read)
        } else {
            Err(de::Error::invalid_length(
                items.len(),
                &"fewer elements in array",
            ))
        }
    }

    fn object<V: Visitor<'a>>(
        self,
        members: &'a Map<String, Value>,
        kept: usize,
        stores_nodes: bool,
        visitor: V,
    ) -> Result<V::Value, Error> {
        let _depth = self.account.enter()?;
        self.account.admit(1, kept)?;
        visitor.visit_map(Members::new(
            self.account,
            members.iter().map(|(key, value)| (key.as_str(), value)),
            stores_nodes,
        ))
    }
}

impl<'a> de::Deserializer<'a> for Reader<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null | Value::Bool(_) | Value::Number(_) => {
                self.account.admit(1, VALUE_WIDTH)?;
                self.value.deserialize_any(visitor)
            }
            Value::String(text) => {
                self.account.text(text, VALUE_WIDTH + text.len())?;
                visitor.visit_borrowed_str(text)
            }
            Value::Array(items) => self.sequence(items, VALUE_WIDTH, visitor),
            Value::Object(members) => self.object(members, VALUE_WIDTH, true, visitor),
        }
    }

    scalar! {
        deserialize_bool: bool;
        deserialize_i8: i8;
        deserialize_i16: i16;
        deserialize_i32: i32;
        deserialize_i64: i64;
        deserialize_i128: i128;
        deserialize_u8: u8;
        deserialize_u16: u16;
        deserialize_u32: u32;
        deserialize_u64: u64;
        deserialize_u128: u128;
        deserialize_f32: f32;
        deserialize_f64: f64;
        deserialize_char: char;
        deserialize_unit: ();
    }

    fn deserialize_unit_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_str<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::String(text) => {
                self.account.text(text, STRING_HEADER + text.len())?;
                visitor.visit_borrowed_str(text)
            }
            _ => self.value.deserialize_str(visitor),
        }
    }

    fn deserialize_string<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Array(items) => self.sequence(items, VEC_HEADER, visitor),
            _ => self.deserialize_str(visitor),
        }
    }

    fn deserialize_byte_buf<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        if self.value.is_null() {
            self.account.admit(1, 1)?;
            visitor.visit_none()
        } else {
            self.account.admit(1, 0)?;
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        if name == RAW_VALUE_TOKEN {
            // The stored value is written out as the raw text the reader keeps.
            let mut text = CountedText {
                account: self.account,
                len: 0,
            };
            serde_json::to_writer(&mut text, self.value).map_err(|_| refused())?;
            self.account.admit(1, STRING_HEADER + text.len)?;
            return self.value.deserialize_newtype_struct(name, visitor);
        }
        self.account.admit(1, 0)?;
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Array(items) => self.sequence(items, VEC_HEADER, visitor),
            _ => self.value.deserialize_seq(visitor),
        }
    }

    fn deserialize_tuple<V: Visitor<'a>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Array(items) => self.sequence(items, 0, visitor),
            _ => self.value.deserialize_tuple(len, visitor),
        }
    }

    fn deserialize_tuple_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Object(members) => self.object(members, MAP_HEADER, true, visitor),
            _ => self.value.deserialize_map(visitor),
        }
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::Object(members) => self.object(members, 0, false, visitor),
            Value::Array(items) => self.sequence(items, 0, visitor),
            _ => self.value.deserialize_struct(name, fields, visitor),
        }
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        match self.value {
            Value::String(variant) => {
                self.account.text(variant, 0)?;
                visitor.visit_enum(BorrowedStrDeserializer::new(variant))
            }
            Value::Object(members) => {
                let mut entries = members.iter();
                let (Some((variant, value)), None) = (entries.next(), entries.next()) else {
                    return Err(de::Error::invalid_value(
                        Unexpected::Map,
                        &"map with a single key",
                    ));
                };
                let _depth = self.account.enter()?;
                self.account.text(variant, 0)?;
                visitor.visit_enum(Variant {
                    reader: self.within(value),
                    name: variant,
                })
            }
            _ => self.value.deserialize_enum(name, variants, visitor),
        }
    }

    fn deserialize_identifier<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::String(text) => {
                self.account.text(text, 0)?;
                visitor.visit_borrowed_str(text)
            }
            _ => self.value.deserialize_identifier(visitor),
        }
    }

    fn deserialize_ignored_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.account.admit(1, 0)?;
        visitor.visit_unit()
    }
}

struct Elements<'a> {
    account: Account<'a>,
    items: std::slice::Iter<'a, Value>,
}

impl<'a> SeqAccess<'a> for Elements<'a> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'a>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(item) = self.items.next() else {
            return Ok(None);
        };
        self.account
            .ctx
            .charge_collection_items(1, TYPED_READ)
            .map_err(|_| refused())?;
        let before = self.account.kept.get();
        let element = seed.deserialize(Reader {
            account: self.account,
            value: item,
        })?;
        // The element is moved into the reader's vector once it returns.
        let moved = self.account.kept.get() - before;
        self.account
            .admit(usize::try_from(moved).map_err(|_| refused())?, 0)?;
        Ok(Some(element))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

/// Object members read as struct fields or as map entries. A map stores each
/// member in a B-tree node; struct fields are matched and stored in place.
pub(super) struct Members<'a, I> {
    account: Account<'a>,
    entries: I,
    value: Option<&'a Value>,
    stores_nodes: bool,
    stored: usize,
}

impl<'a, I> Members<'a, I> {
    pub(super) fn new(account: Account<'a>, members: I, stores_nodes: bool) -> Self {
        Self {
            account,
            entries: members,
            value: None,
            stores_nodes,
            stored: 0,
        }
    }

    /// Admits one more map entry: its slot, its share of node storage and the
    /// key comparisons that place it, at most eleven per node on its path.
    fn admit_entry(&mut self, key: &str) -> Result<(), Error> {
        let ctx = self.account.ctx;
        ctx.admit_btree_node_storage::<String, Value>(self.stored, TYPED_READ)
            .and_then(|()| ctx.charge_collection_items(1, TYPED_READ))
            .map_err(|_| refused())?;
        let levels = usize::try_from(
            u64_from_index(self.stored)
                .checked_add(1)
                .map_or(0, |n| u64::from(n.ilog(6)) + 1),
        )
        .map_err(|_| refused())?;
        let comparisons = self.stored.min(levels.checked_mul(11).ok_or_else(refused)?);
        self.account
            .admit(key.len().checked_mul(comparisons).ok_or_else(refused)?, 0)?;
        self.stored += 1;
        Ok(())
    }
}

impl<'a, I: Iterator<Item = (&'a str, &'a Value)>> MapAccess<'a> for Members<'a, I> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'a>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, value)) = self.entries.next() else {
            return Ok(None);
        };
        if self.stores_nodes {
            self.admit_entry(key)?;
        }
        self.value = Some(value);
        seed.deserialize(Key {
            account: self.account,
            key,
        })
        .map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'a>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let value = self
            .value
            .take()
            .ok_or_else(|| de::Error::custom("value is missing"))?;
        seed.deserialize(Reader {
            account: self.account,
            value,
        })
    }
}

/// An object member's key, read as text, an identifier or a number.
struct Key<'a> {
    account: Account<'a>,
    key: &'a str,
}

macro_rules! numeric_key {
    ($($method:ident: $width:ty;)*) => {$(
        fn $method<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
            self.account.text(self.key, std::mem::size_of::<$width>())?;
            let numeric = matches!(self.key.as_bytes().first(), Some(b'0'..=b'9' | b'-'))
                && !self.key.as_bytes().last().is_some_and(u8::is_ascii_whitespace);
            if !numeric {
                return Err(de::Error::invalid_type(Unexpected::Str(self.key), &visitor));
            }
            let mut text = serde_json::Deserializer::from_str(self.key);
            let read = text.$method(visitor)?;
            text.end()?;
            Ok(read)
        }
    )*};
}

impl<'a> de::Deserializer<'a> for Key<'a> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.account
            .text(self.key, STRING_HEADER + self.key.len())?;
        visitor.visit_borrowed_str(self.key)
    }

    numeric_key! {
        deserialize_i8: i8;
        deserialize_i16: i16;
        deserialize_i32: i32;
        deserialize_i64: i64;
        deserialize_i128: i128;
        deserialize_u8: u8;
        deserialize_u16: u16;
        deserialize_u32: u32;
        deserialize_u64: u64;
        deserialize_u128: u128;
        deserialize_f32: f32;
        deserialize_f64: f64;
    }

    fn deserialize_bool<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.account.text(self.key, 1)?;
        match self.key {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(de::Error::invalid_type(Unexpected::Str(self.key), &visitor)),
        }
    }

    fn deserialize_option<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.account.text(self.key, 0)?;
        visitor.visit_enum(BorrowedStrDeserializer::new(self.key))
    }

    fn deserialize_identifier<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.account.text(self.key, 0)?;
        visitor.visit_borrowed_str(self.key)
    }

    fn deserialize_ignored_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        self.account.admit(1, 0)?;
        visitor.visit_unit()
    }

    forward_to_deserialize_any! {
        <W: Visitor<'a>>
        char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
    }
}

struct Variant<'a> {
    reader: Reader<'a>,
    name: &'a str,
}

impl<'a> EnumAccess<'a> for Variant<'a> {
    type Error = Error;
    type Variant = Reader<'a>;

    fn variant_seed<V: DeserializeSeed<'a>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Reader<'a>), Error> {
        let variant = seed.deserialize(BorrowedStrDeserializer::<Error>::new(self.name))?;
        Ok((variant, self.reader))
    }
}

impl<'a> VariantAccess<'a> for Reader<'a> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        self.account.admit(1, 0)?;
        if self.value.is_null() {
            Ok(())
        } else {
            Err(de::Error::invalid_type(
                Unexpected::Other("non-null value"),
                &"unit variant",
            ))
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'a>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'a>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V: Visitor<'a>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_struct("", fields, visitor)
    }
}

/// A whole record: its `id` member, then its fields.
pub(super) struct Record<'a, I> {
    pub(super) account: Account<'a>,
    pub(super) members: I,
}

impl<'a, I: Iterator<Item = (&'a str, &'a Value)>> de::Deserializer<'a> for Record<'a, I> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'a>>(self, visitor: V) -> Result<V::Value, Error> {
        let _depth = self.account.enter()?;
        self.account.admit(1, MAP_HEADER)?;
        visitor.visit_map(Members::new(self.account, self.members, true))
    }

    fn deserialize_struct<V: Visitor<'a>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let _depth = self.account.enter()?;
        self.account.admit(1, 0)?;
        visitor.visit_map(Members::new(self.account, self.members, false))
    }

    forward_to_deserialize_any! {
        <W: Visitor<'a>>
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct map enum identifier
        ignored_any
    }
}

#[cfg(test)]
mod tests;
