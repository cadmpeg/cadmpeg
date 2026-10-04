// SPDX-License-Identifier: Apache-2.0
//! Structural serde values admitted by the caller's decode context.

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, DepthGuard, ResourceLimit, ScopedReservation,
};
use cadmpeg_core::CodecError;
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Serialize, Serializer};
use serde_value::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem::size_of;

/// A projected value whose reservation outlives its storage.
#[derive(Debug)]
pub struct Projection<'ctx> {
    value: Value,
    _storage: ScopedReservation<'ctx>,
}

impl std::ops::Deref for Projection<'_> {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.value
    }
}
impl std::ops::DerefMut for Projection<'_> {
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.value
    }
}

/// Project each node directly, preserving the source floating-point values.
pub fn project<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    value: &(impl Serialize + ?Sized),
    operation: &'static str,
) -> Result<Projection<'ctx>, CodecError> {
    let storage = RefCell::new(ctx.reserve_scoped(0, operation)?);
    let refusal = RefCell::new(None);
    let result = value.serialize(Projector {
        ctx,
        storage: &storage,
        refusal: &refusal,
        operation,
    });
    if let Some(limit) = refusal.into_inner() {
        return Err(CodecError::ResourceLimit(limit));
    }
    ctx.charge_work(0, operation)?;
    let value = result.map_err(|error| error.into_codec(ctx, operation))?;
    Ok(Projection {
        value,
        _storage: storage.into_inner(),
    })
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Resource(#[from] CodecError),
    #[error(transparent)]
    Serde(serde_value::SerializerError),
}
impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Serde(serde::ser::Error::custom(message))
    }
}
impl Error {
    fn into_codec(self, ctx: &DecodeContext<'_>, operation: &'static str) -> CodecError {
        match self {
            Self::Resource(error) => error,
            Self::Serde(error) => match ctx.format_retained(
                format_args!("structural projection failed: {error}"),
                operation,
            ) {
                Ok(message) => CodecError::Malformed(message),
                Err(error) => error,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct Projector<'s, 'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    storage: &'s RefCell<ScopedReservation<'ctx>>,
    operation: &'static str,
    refusal: &'s RefCell<Option<ResourceLimit>>,
}
impl<'s, 'ctx, 'arena> Projector<'s, 'ctx, 'arena> {
    fn admit<T>(self, result: Result<T, CodecError>) -> Result<T, Error> {
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            let mut refusal = self.refusal.borrow_mut();
            if refusal.is_none() {
                *refusal = Some(*limit);
            }
        }
        result.map_err(Error::Resource)
    }
    fn node(self) -> Result<DepthGuard<'ctx>, Error> {
        self.ctx.charge_work(1, self.operation)?;
        Ok(self.ctx.enter_nested(self.operation)?)
    }
    fn source_steps(self, count: usize) -> Result<(), Error> {
        self.admit(self.ctx.charge_work(u64_from_index(count), self.operation))
    }
    fn text(self, text: &str) -> Result<Value, Error> {
        let _depth = self.node()?;
        self.ctx
            .charge_work(u64_from_index(text.len()), self.operation)?;
        Ok(Value::String(self.admit(self.ctx.copy_scoped_text(
            text,
            &mut self.storage.borrow_mut(),
            self.operation,
        ))?))
    }
    fn boxed(self, value: Value) -> Result<Box<Value>, Error> {
        self.ctx.charge_collection_items(1, self.operation)?;
        self.storage
            .borrow_mut()
            .grow(u64_from_index(size_of::<Value>()))?;
        Ok(Box::new(value))
    }
    fn sequence(
        self,
        variant: Option<&'static str>,
        source_steps: usize,
    ) -> Result<Sequence<'s, 'ctx, 'arena>, Error> {
        let depth = self.node()?;
        self.source_steps(source_steps)?;
        let variant_depth = variant.map(|_| self.node()).transpose()?;
        let variant = variant.map(|name| self.text(name)).transpose()?;
        Ok(Sequence {
            values: Vec::new(),
            variant,
            projector: self,
            _depth: depth,
            _variant_depth: variant_depth,
        })
    }
    fn map(
        self,
        variant: Option<&'static str>,
        source_steps: usize,
    ) -> Result<Object<'s, 'ctx, 'arena>, Error> {
        let depth = self.node()?;
        self.source_steps(source_steps)?;
        let variant_depth = variant.map(|_| self.node()).transpose()?;
        let variant = variant.map(|name| self.text(name)).transpose()?;
        Ok(Object {
            values: BTreeMap::new(),
            key: None,
            longest: 0,
            variant,
            projector: self,
            _depth: depth,
            _variant_depth: variant_depth,
        })
    }
    fn entry(
        self,
        entries: &mut BTreeMap<Value, Value>,
        key: Value,
        value: Value,
        longest: &mut u64,
    ) -> Result<(), Error> {
        *longest = (*longest).max(key_work(self.ctx, &key, self.operation)?);
        let comparisons = u64_from_index(entries.len())
            .checked_add(1)
            .and_then(|count| count.checked_mul(*longest))
            .ok_or_else(|| {
                self.ctx
                    .refuse_codec_limit(self.operation, u64::MAX - 1, u64::MAX)
            })?;
        self.ctx.charge_work(comparisons, self.operation)?;
        if let Some(previous) = entries.get_mut(&key) {
            *previous = value;
        } else if !self.admit(self.ctx.insert_scoped_btree_map_if_vacant(
            &mut self.storage.borrow_mut(),
            entries,
            key,
            value,
            self.operation,
            self.operation,
        ))? {
            return Err(serde::ser::Error::custom(
                "structural map lost a vacant key",
            ));
        }
        Ok(())
    }
    fn variant(self, key: Option<Value>, value: Value) -> Result<Value, Error> {
        let Some(key) = key else {
            return Ok(value);
        };
        let mut entries = BTreeMap::new();
        self.entry(&mut entries, key, value, &mut 0)?;
        Ok(Value::Map(entries))
    }
}

fn key_work(
    ctx: &DecodeContext<'_>,
    value: &Value,
    operation: &'static str,
) -> Result<u64, CodecError> {
    let _depth = ctx.enter_nested(operation)?;
    ctx.charge_work(1, operation)?;
    let mut count = 1_u64;
    match value {
        Value::String(text) => {
            ctx.charge_work(u64_from_index(text.len()), operation)?;
            count = count
                .checked_add(u64_from_index(text.len()))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        Value::Bytes(bytes) => {
            ctx.charge_work(u64_from_index(bytes.len()), operation)?;
            count = count
                .checked_add(u64_from_index(bytes.len()))
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        Value::Map(entries) => {
            for (key, value) in ctx.admit_iter(entries, operation)? {
                for child in [key, value] {
                    count = count
                        .checked_add(key_work(ctx, child, operation)?)
                        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
                }
            }
        }
        Value::Seq(values) => {
            for value in ctx.admit_iter(values, operation)? {
                count = count
                    .checked_add(key_work(ctx, value, operation)?)
                    .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
            }
        }
        Value::Option(Some(value)) | Value::Newtype(value) => {
            count = count
                .checked_add(key_work(ctx, value, operation)?)
                .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        }
        _ => {}
    }
    Ok(count)
}

macro_rules! project_scalar {
    ($($method:ident($type:ty) => $variant:ident),* $(,)?) => {$(
        fn $method(self, value: $type) -> Result<Value, Error> {
            let _depth = self.node()?;
            Ok(Value::$variant(value))
        }
    )*};
}
impl<'s, 'ctx, 'arena> Serializer for Projector<'s, 'ctx, 'arena> {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = Sequence<'s, 'ctx, 'arena>;
    type SerializeTuple = Sequence<'s, 'ctx, 'arena>;
    type SerializeTupleStruct = Sequence<'s, 'ctx, 'arena>;
    type SerializeTupleVariant = Sequence<'s, 'ctx, 'arena>;
    type SerializeMap = Object<'s, 'ctx, 'arena>;
    type SerializeStruct = Object<'s, 'ctx, 'arena>;
    type SerializeStructVariant = Object<'s, 'ctx, 'arena>;
    project_scalar! { serialize_bool(bool) => Bool, serialize_i8(i8) => I8, serialize_i16(i16) => I16, serialize_i32(i32) => I32, serialize_i64(i64) => I64, serialize_u8(u8) => U8, serialize_u16(u16) => U16, serialize_u32(u32) => U32, serialize_u64(u64) => U64, serialize_f32(f32) => F32, serialize_f64(f64) => F64, serialize_char(char) => Char }
    fn serialize_str(self, value: &str) -> Result<Value, Error> {
        self.text(value)
    }
    fn collect_str<T: std::fmt::Display + ?Sized>(self, value: &T) -> Result<Value, Error> {
        let _depth = self.node()?;
        let text = self.admit(self.storage.borrow_mut().with_storage(|| {
            self.ctx
                .format_retained(format_args!("{value}"), self.operation)
        }))?;
        Ok(Value::String(text))
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Value, Error> {
        let _depth = self.node()?;
        let bytes = self.admit(
            self.storage
                .borrow_mut()
                .with_storage(|| self.ctx.copy_slice(value, self.operation)),
        )?;
        Ok(Value::Bytes(bytes))
    }
    fn serialize_none(self) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::Option(None))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
        let _depth = self.node()?;
        let value = value.serialize(self)?;
        Ok(Value::Option(Some(self.boxed(value)?)))
    }
    fn serialize_unit(self) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::Unit)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, Error> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        self.serialize_str(variant)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        let _depth = self.node()?;
        let value = value.serialize(self)?;
        Ok(Value::Newtype(self.boxed(value)?))
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        let _depth = self.node()?;
        let key = self.text(variant)?;
        let value = value.serialize(self)?;
        self.variant(Some(key), value)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        self.sequence(None, len.unwrap_or(0))
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Error> {
        self.sequence(None, len)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.sequence(None, len)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        self.sequence(Some(variant), len)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.map(None, len.unwrap_or(0))
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        self.map(None, len)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        self.map(Some(variant), len)
    }
}

struct Sequence<'s, 'ctx, 'arena> {
    values: Vec<Value>,
    variant: Option<Value>,
    projector: Projector<'s, 'ctx, 'arena>,
    _depth: DepthGuard<'ctx>,
    _variant_depth: Option<DepthGuard<'ctx>>,
}
impl Sequence<'_, '_, '_> {
    fn field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let value = value.serialize(self.projector)?;
        self.projector.admit(self.projector.ctx.push_scoped_vec(
            &mut self.projector.storage.borrow_mut(),
            &mut self.values,
            value,
            self.projector.operation,
        ))?;
        Ok(())
    }
    fn finish(self) -> Result<Value, Error> {
        self.projector
            .variant(self.variant, Value::Seq(self.values))
    }
}
macro_rules! sequence_impl {
    ($trait:ident, $field:ident) => {
        impl $trait for Sequence<'_, '_, '_> {
            type Ok = Value;
            type Error = Error;
            fn $field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
                self.field(value)
            }
            fn end(self) -> Result<Value, Error> {
                self.finish()
            }
        }
    };
}
sequence_impl!(SerializeSeq, serialize_element);
sequence_impl!(SerializeTuple, serialize_element);
sequence_impl!(SerializeTupleStruct, serialize_field);
sequence_impl!(SerializeTupleVariant, serialize_field);

struct Object<'s, 'ctx, 'arena> {
    values: BTreeMap<Value, Value>,
    key: Option<Value>,
    longest: u64,
    variant: Option<Value>,
    projector: Projector<'s, 'ctx, 'arena>,
    _depth: DepthGuard<'ctx>,
    _variant_depth: Option<DepthGuard<'ctx>>,
}
impl Object<'_, '_, '_> {
    fn field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
        let key = self.projector.text(key)?;
        let value = value.serialize(self.projector)?;
        self.projector
            .entry(&mut self.values, key, value, &mut self.longest)
    }
    fn finish(self) -> Result<Value, Error> {
        if self.key.is_some() {
            return Err(serde::ser::Error::custom("structural map key has no value"));
        }
        self.projector
            .variant(self.variant, Value::Map(self.values))
    }
}
impl SerializeMap for Object<'_, '_, '_> {
    type Ok = Value;
    type Error = Error;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        if self.key.is_some() {
            return Err(serde::ser::Error::custom("structural map key has no value"));
        }
        self.key = Some(key.serialize(self.projector)?);
        Ok(())
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self.key.take().ok_or_else(|| {
            <Error as serde::ser::Error>::custom("structural map value has no key")
        })?;
        let value = value.serialize(self.projector)?;
        self.projector
            .entry(&mut self.values, key, value, &mut self.longest)
    }
    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}
macro_rules! object_impl {
    ($trait:ident) => {
        impl $trait for Object<'_, '_, '_> {
            type Ok = Value;
            type Error = Error;
            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<(), Error> {
                self.field(key, value)
            }
            fn end(self) -> Result<Value, Error> {
                self.finish()
            }
        }
    };
}
object_impl!(SerializeStruct);
object_impl!(SerializeStructVariant);

#[cfg(test)]
mod tests;
