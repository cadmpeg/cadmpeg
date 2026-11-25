// SPDX-License-Identifier: Apache-2.0
//! Pretty-printing JSON serializer with float filtering, depth admission, and
//! struct-walk callbacks.
//!
//! Hosting the concrete `serde_json::Serializer<&mut dyn Write, PrettyFormatter>`
//! instantiation in `cadmpeg-core` keeps JSON formatting and string escaping
//! compiled once under `cadmpeg-core`'s optimization level instead of being
//! re-monomorphized inside unoptimized downstream crates.

use std::cell::{Cell, RefCell};
use std::collections::hash_map::DefaultHasher;
use std::fmt::Display;
use std::hash::Hasher;

use serde::ser::{
    Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use sha2::{Digest as _, Sha256};

use crate::decode::{DecodeContext, DepthGuard, ResourceLimit};
use crate::serde_erase::{serialize_with, SerAdapter, SerError};
use crate::CodecError;

/// Non-generic `DefaultHasher` wrapper compiled inside `cadmpeg-core` so
/// unoptimized downstream crates hash record content without unoptimized
/// `SipHash` instantiations.
#[derive(Default)]
pub struct ContentHasher(DefaultHasher);

impl ContentHasher {
    /// Creates a new hasher initialized with the default `SipHash` keys.
    #[must_use]
    pub fn new() -> Self {
        Self(DefaultHasher::new())
    }
}

impl Hasher for ContentHasher {
    fn finish(&self) -> u64 {
        self.0.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    fn write_u8(&mut self, i: u8) {
        self.0.write_u8(i);
    }

    fn write_u64(&mut self, i: u64) {
        self.0.write_u64(i);
    }

    fn write_i64(&mut self, i: i64) {
        self.0.write_i64(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.0.write_usize(i);
    }
}

/// How a pretty-JSON walk writes floating-point numbers.
#[derive(Clone, Copy)]
pub enum JsonFloats {
    /// Refuse a non-finite float and write negative zero as zero.
    Canonical,
    /// Write every float as `serde_json` writes it, negative zero as zero.
    SerdeJson,
    /// Write every float as `serde_json::to_value` stores it, negative zero as
    /// zero: an `f32` as the `f64` it widens to.
    Value,
}

/// Receives the structs and maps a pretty-JSON walk writes, in writing order.
pub trait JsonStructs {
    /// A struct named `name` begins: its JSON text follows.
    fn begin(&self, name: &'static str);
    /// The innermost struct that began has ended: its JSON text precedes.
    fn end(&self);
    /// A map begins: its JSON text follows.
    fn map(&self) {}
}

/// Outcome of a pretty-JSON serialization walk.
#[derive(Debug)]
pub enum FiniteJsonError {
    /// The caller's resource policy refused traversal depth.
    Resource(ResourceLimit),
    /// A float on the value is not finite.
    NonFinite(f64),
    /// Serialization or I/O failed.
    Serialize(serde_json::Error),
    /// Flushing the digest buffer failed.
    Write(std::io::Error),
}

struct DigestWriter(Sha256);

impl std::io::Write for DigestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Buffer size in bytes used when streaming canonical JSON into SHA-256.
pub const DIGEST_BUFFER_BYTES: usize = 64 * 1024;

/// Computes the SHA-256 of canonical pretty JSON for an erased value, keeping
/// both `serde_json::Serializer::pretty` and `BufWriter<DigestWriter>` compiled
/// inside `cadmpeg-core`.
pub fn canonical_json_sha256_erased(
    admission: Option<(&DecodeContext<'_>, &'static str)>,
    write: &dyn Fn(SerAdapter<'_>) -> Result<(), SerError>,
) -> Result<[u8; 32], FiniteJsonError> {
    let mut buffered =
        std::io::BufWriter::with_capacity(DIGEST_BUFFER_BYTES, DigestWriter(Sha256::new()));
    write_pretty_json_erased(admission, JsonFloats::Canonical, None, &mut buffered, write)?;
    let writer = buffered
        .into_inner()
        .map_err(|error| FiniteJsonError::Write(error.into_error()))?;
    Ok(writer.0.finalize().into())
}

/// Writes an erased value as pretty JSON through `writer` with float policy
/// `floats`, optional depth admission `admission`, and optional struct callbacks.
pub fn write_pretty_json_erased(
    admission: Option<(&DecodeContext<'_>, &'static str)>,
    floats: JsonFloats,
    structs: Option<&dyn JsonStructs>,
    writer: &mut dyn std::io::Write,
    write: &dyn Fn(SerAdapter<'_>) -> Result<(), SerError>,
) -> Result<(), FiniteJsonError> {
    let guard = FiniteGuard::new(admission, floats, structs);
    let mut json = serde_json::Serializer::pretty(writer);
    let outcome = serialize_with(FiniteSerializer::new(&mut json, &guard), write);
    if let Some(limit) = guard.resource.borrow_mut().take() {
        return Err(FiniteJsonError::Resource(limit));
    }
    if let Some(value) = guard.refused() {
        return Err(FiniteJsonError::NonFinite(value));
    }
    outcome.map_err(FiniteJsonError::Serialize)
}

struct FiniteGuard<'a, 'input> {
    refused: Cell<Option<f64>>,
    admission: Option<(&'a DecodeContext<'input>, &'static str)>,
    floats: JsonFloats,
    structs: Option<&'a dyn JsonStructs>,
    resource: RefCell<Option<ResourceLimit>>,
}

impl<'a, 'input> FiniteGuard<'a, 'input> {
    fn new(
        admission: Option<(&'a DecodeContext<'input>, &'static str)>,
        floats: JsonFloats,
        structs: Option<&'a dyn JsonStructs>,
    ) -> Self {
        Self {
            refused: Cell::new(None),
            admission,
            floats,
            structs,
            resource: RefCell::new(None),
        }
    }

    fn enter<E: serde::ser::Error>(&self) -> Result<Option<DepthGuard<'_>>, E> {
        let Some((ctx, operation)) = self.admission else {
            return Ok(None);
        };
        match ctx.enter_nested(operation) {
            Ok(depth) => Ok(Some(depth)),
            Err(error) => {
                if let CodecError::ResourceLimit(limit) = error {
                    let mut refusal = self.resource.borrow_mut();
                    if refusal.is_none() {
                        *refusal = Some(limit);
                    }
                }
                Err(E::custom("canonical JSON resource admission refused"))
            }
        }
    }

    fn refused(&self) -> Option<f64> {
        self.refused.get()
    }

    fn refuse<E: serde::ser::Error>(&self, value: f64) -> E {
        self.refused.set(Some(value));
        E::custom(format_args!("non-finite float {value}"))
    }
}

struct FiniteSerializer<'guard, 'ctx, 'input, S> {
    inner: S,
    guard: &'guard FiniteGuard<'ctx, 'input>,
}

impl<'guard, 'ctx, 'input, S> FiniteSerializer<'guard, 'ctx, 'input, S> {
    fn new(inner: S, guard: &'guard FiniteGuard<'ctx, 'input>) -> Self {
        Self { inner, guard }
    }
}

struct FiniteValue<'guard, 'ctx, 'input, 'value, T: ?Sized> {
    value: &'value T,
    guard: &'guard FiniteGuard<'ctx, 'input>,
}

impl<T: ?Sized + Serialize> Serialize for FiniteValue<'_, '_, '_, '_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value
            .serialize(FiniteSerializer::new(serializer, self.guard))
    }
}

struct FiniteCompound<'guard, 'ctx, 'input, C> {
    inner: C,
    guard: &'guard FiniteGuard<'ctx, 'input>,
    _depth: Option<DepthGuard<'guard>>,
    _variant_depth: Option<DepthGuard<'guard>>,
}

impl<'guard, 'ctx, 'input, C> FiniteCompound<'guard, 'ctx, 'input, C> {
    fn new(
        inner: C,
        guard: &'guard FiniteGuard<'ctx, 'input>,
        depth: Option<DepthGuard<'guard>>,
        variant_depth: Option<DepthGuard<'guard>>,
    ) -> Self {
        Self {
            inner,
            guard,
            _depth: depth,
            _variant_depth: variant_depth,
        }
    }

    fn wrap<'value, T: ?Sized>(
        &self,
        value: &'value T,
    ) -> FiniteValue<'guard, 'ctx, 'input, 'value, T> {
        FiniteValue {
            value,
            guard: self.guard,
        }
    }
}

impl<'guard, 'ctx, 'input, S: Serializer> Serializer for FiniteSerializer<'guard, 'ctx, 'input, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = FiniteCompound<'guard, 'ctx, 'input, S::SerializeSeq>;
    type SerializeTuple = FiniteCompound<'guard, 'ctx, 'input, S::SerializeTuple>;
    type SerializeTupleStruct = FiniteCompound<'guard, 'ctx, 'input, S::SerializeTupleStruct>;
    type SerializeTupleVariant = FiniteCompound<'guard, 'ctx, 'input, S::SerializeTupleVariant>;
    type SerializeMap = FiniteCompound<'guard, 'ctx, 'input, S::SerializeMap>;
    type SerializeStruct = FiniteCompound<'guard, 'ctx, 'input, S::SerializeStruct>;
    type SerializeStructVariant = FiniteCompound<'guard, 'ctx, 'input, S::SerializeStructVariant>;

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        match self.guard.floats {
            JsonFloats::SerdeJson | JsonFloats::Value => self.inner.serialize_f64(value + 0.0),
            JsonFloats::Canonical if value.is_finite() => self.inner.serialize_f64(value + 0.0),
            JsonFloats::Canonical => Err(self.guard.refuse(value)),
        }
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        match self.guard.floats {
            JsonFloats::SerdeJson => self.inner.serialize_f32(value + 0.0),
            JsonFloats::Value => self.inner.serialize_f64(f64::from(value) + 0.0),
            JsonFloats::Canonical if value.is_finite() => self.inner.serialize_f32(value + 0.0),
            JsonFloats::Canonical => Err(self.guard.refuse(f64::from(value))),
        }
    }

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_bool(value)
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_i8(value)
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_i16(value)
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_i32(value)
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_i64(value)
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_i128(value)
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_u8(value)
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_u16(value)
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_u32(value)
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_u64(value)
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_u128(value)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_char(value)
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_str(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_bytes(value)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_none()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner.serialize_some(&FiniteValue { value, guard })
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_unit()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.serialize_unit_variant(name, index, variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        if name == "$serde_json::private::RawValue" {
            let guard = self.guard;
            return self
                .inner
                .serialize_newtype_struct(name, &FiniteValue { value, guard });
        }
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_newtype_variant(name, index, variant, &FiniteValue { value, guard })
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_seq(len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, None))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_tuple(len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, None))
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_tuple_struct(name, len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, None))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let variant_depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_tuple_variant(name, index, variant, len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, variant_depth))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        if let Some(structs) = guard.structs {
            structs.map();
        }
        self.inner
            .serialize_map(len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, None))
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        if let Some(structs) = guard.structs {
            structs.begin(name);
        }
        self.inner
            .serialize_struct(name, len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, None))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let variant_depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
        self.inner
            .serialize_struct_variant(name, index, variant, len)
            .map(|inner| FiniteCompound::new(inner, guard, depth, variant_depth))
    }

    fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.inner.collect_str(value)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<C: SerializeSeq> SerializeSeq for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_element(&wrapped)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

impl<C: SerializeTuple> SerializeTuple for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_element(&wrapped)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

impl<C: SerializeTupleStruct> SerializeTupleStruct for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(&wrapped)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

impl<C: SerializeTupleVariant> SerializeTupleVariant for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(&wrapped)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

impl<C: SerializeMap> SerializeMap for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(key);
        self.inner.serialize_key(&wrapped)
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_value(&wrapped)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

impl<C: SerializeStruct> SerializeStruct for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(name, &wrapped)
    }

    fn skip_field(&mut self, name: &'static str) -> Result<(), Self::Error> {
        self.inner.skip_field(name)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        let structs = self.guard.structs;
        let ok = self.inner.end()?;
        if let Some(structs) = structs {
            structs.end();
        }
        Ok(ok)
    }
}

impl<C: SerializeStructVariant> SerializeStructVariant for FiniteCompound<'_, '_, '_, C> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        let wrapped = self.wrap(value);
        self.inner.serialize_field(name, &wrapped)
    }

    fn skip_field(&mut self, name: &'static str) -> Result<(), Self::Error> {
        self.inner.skip_field(name)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}
