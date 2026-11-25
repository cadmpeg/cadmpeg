// SPDX-License-Identifier: Apache-2.0
//! Type-erased `serde::Serializer` and `serde::Deserializer` adapters.
//!
//! Generic `Serialize` and `Deserialize` implementations monomorphize the full
//! reachable type graph once per concrete serializer or deserializer in every
//! crate that invokes them. Routing hub wire types through [`serialize_with`]
//! and [`deserialize_with`] compiles each reachable type once against
//! [`SerAdapter`] and [`DeAdapter`], while forwarding every `serde` event,
//! visitor callback, and concrete error unchanged.

use std::fmt;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// Error produced while driving an erased [`Serializer`].
#[derive(Debug)]
pub struct SerError {
    inner: bool,
    message: String,
}

impl SerError {
    fn inner(error: &impl fmt::Display) -> Self {
        Self {
            inner: true,
            message: error.to_string(),
        }
    }

    fn consumed() -> Self {
        Self {
            inner: false,
            message: "erased serializer consumed twice".to_owned(),
        }
    }

    fn into_error<E: ser::Error>(self, inner: Option<E>) -> E {
        if self.inner {
            if let Some(error) = inner {
                return error;
            }
        }
        E::custom(self.message)
    }
}

impl fmt::Display for SerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inner && self.message.is_empty() {
            formatter.write_str("serializer error")
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for SerError {}

impl ser::Error for SerError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self {
            inner: false,
            message: message.to_string(),
        }
    }
}

/// Object-safe value accepted by [`ErasedSerializer`].
pub trait ErasedSerialize {
    /// Serializes `self` into `serializer`.
    fn erased_serialize(&self, serializer: &mut dyn ErasedSerializer) -> Result<(), SerError>;
}

impl<T: Serialize + ?Sized> ErasedSerialize for T {
    fn erased_serialize(&self, serializer: &mut dyn ErasedSerializer) -> Result<(), SerError> {
        self.serialize(SerAdapter(serializer))
    }
}

/// Object-safe [`Serializer`].
pub trait ErasedSerializer {
    /// Writes a `bool`.
    fn erased_serialize_bool(&mut self, value: bool) -> Result<(), SerError>;
    /// Writes an `i8`.
    fn erased_serialize_i8(&mut self, value: i8) -> Result<(), SerError>;
    /// Writes an `i16`.
    fn erased_serialize_i16(&mut self, value: i16) -> Result<(), SerError>;
    /// Writes an `i32`.
    fn erased_serialize_i32(&mut self, value: i32) -> Result<(), SerError>;
    /// Writes an `i64`.
    fn erased_serialize_i64(&mut self, value: i64) -> Result<(), SerError>;
    /// Writes an `i128`.
    fn erased_serialize_i128(&mut self, value: i128) -> Result<(), SerError>;
    /// Writes a `u8`.
    fn erased_serialize_u8(&mut self, value: u8) -> Result<(), SerError>;
    /// Writes a `u16`.
    fn erased_serialize_u16(&mut self, value: u16) -> Result<(), SerError>;
    /// Writes a `u32`.
    fn erased_serialize_u32(&mut self, value: u32) -> Result<(), SerError>;
    /// Writes a `u64`.
    fn erased_serialize_u64(&mut self, value: u64) -> Result<(), SerError>;
    /// Writes a `u128`.
    fn erased_serialize_u128(&mut self, value: u128) -> Result<(), SerError>;
    /// Writes an `f32`.
    fn erased_serialize_f32(&mut self, value: f32) -> Result<(), SerError>;
    /// Writes an `f64`.
    fn erased_serialize_f64(&mut self, value: f64) -> Result<(), SerError>;
    /// Writes a `char`.
    fn erased_serialize_char(&mut self, value: char) -> Result<(), SerError>;
    /// Writes a string slice.
    fn erased_serialize_str(&mut self, value: &str) -> Result<(), SerError>;
    /// Writes a byte slice.
    fn erased_serialize_bytes(&mut self, value: &[u8]) -> Result<(), SerError>;
    /// Writes `None`.
    fn erased_serialize_none(&mut self) -> Result<(), SerError>;
    /// Writes `Some(value)`.
    fn erased_serialize_some(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Writes `()`.
    fn erased_serialize_unit(&mut self) -> Result<(), SerError>;
    /// Writes a unit struct.
    fn erased_serialize_unit_struct(&mut self, name: &'static str) -> Result<(), SerError>;
    /// Writes a unit variant.
    fn erased_serialize_unit_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<(), SerError>;
    /// Writes a newtype struct.
    fn erased_serialize_newtype_struct(
        &mut self,
        name: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError>;
    /// Writes a newtype variant.
    fn erased_serialize_newtype_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError>;
    /// Begins a sequence.
    fn erased_serialize_seq(
        &mut self,
        len: Option<usize>,
    ) -> Result<Box<dyn ErasedSerializeSeq + '_>, SerError>;
    /// Begins a tuple.
    fn erased_serialize_tuple(
        &mut self,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTuple + '_>, SerError>;
    /// Begins a tuple struct.
    fn erased_serialize_tuple_struct(
        &mut self,
        name: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTupleStruct + '_>, SerError>;
    /// Begins a tuple variant.
    fn erased_serialize_tuple_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTupleVariant + '_>, SerError>;
    /// Begins a map.
    fn erased_serialize_map(
        &mut self,
        len: Option<usize>,
    ) -> Result<Box<dyn ErasedSerializeMap + '_>, SerError>;
    /// Begins a struct.
    fn erased_serialize_struct(
        &mut self,
        name: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeStruct + '_>, SerError>;
    /// Begins a struct variant.
    fn erased_serialize_struct_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeStructVariant + '_>, SerError>;
    /// Writes formatted display text.
    fn erased_collect_str(&mut self, value: &dyn fmt::Display) -> Result<(), SerError>;
    /// Reports whether the underlying serializer is human-readable.
    fn erased_is_human_readable(&self) -> bool;
}

/// Object-safe [`SerializeSeq`].
pub trait ErasedSerializeSeq {
    /// Serializes one element.
    fn erased_serialize_element(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Completes the sequence.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeTuple`].
pub trait ErasedSerializeTuple {
    /// Serializes one element.
    fn erased_serialize_element(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Completes the tuple.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeTupleStruct`].
pub trait ErasedSerializeTupleStruct {
    /// Serializes one field.
    fn erased_serialize_field(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Completes the tuple struct.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeTupleVariant`].
pub trait ErasedSerializeTupleVariant {
    /// Serializes one field.
    fn erased_serialize_field(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Completes the tuple variant.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeMap`].
pub trait ErasedSerializeMap {
    /// Serializes one key.
    fn erased_serialize_key(&mut self, key: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Serializes one value.
    fn erased_serialize_value(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError>;
    /// Serializes one key-value entry.
    fn erased_serialize_entry(
        &mut self,
        key: &dyn ErasedSerialize,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError>;
    /// Completes the map.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeStruct`].
pub trait ErasedSerializeStruct {
    /// Serializes one field.
    fn erased_serialize_field(
        &mut self,
        key: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError>;
    /// Skips one field.
    fn erased_skip_field(&mut self, key: &'static str) -> Result<(), SerError>;
    /// Completes the struct.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Object-safe [`SerializeStructVariant`].
pub trait ErasedSerializeStructVariant {
    /// Serializes one field.
    fn erased_serialize_field(
        &mut self,
        key: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError>;
    /// Skips one field.
    fn erased_skip_field(&mut self, key: &'static str) -> Result<(), SerError>;
    /// Completes the struct variant.
    fn erased_end(self: Box<Self>) -> Result<(), SerError>;
}

/// Concrete [`Serializer`] forwarding into a type-erased [`ErasedSerializer`].
pub struct SerAdapter<'a>(pub &'a mut dyn ErasedSerializer);

/// Concrete [`SerializeSeq`] forwarding into a type-erased sequence serializer.
pub struct ErasedSeq<'a>(Box<dyn ErasedSerializeSeq + 'a>);

/// Concrete [`SerializeTuple`] forwarding into a type-erased tuple serializer.
pub struct ErasedTuple<'a>(Box<dyn ErasedSerializeTuple + 'a>);

/// Concrete [`SerializeTupleStruct`] forwarding into a type-erased tuple-struct serializer.
pub struct ErasedTupleStruct<'a>(Box<dyn ErasedSerializeTupleStruct + 'a>);

/// Concrete [`SerializeTupleVariant`] forwarding into a type-erased tuple-variant serializer.
pub struct ErasedTupleVariant<'a>(Box<dyn ErasedSerializeTupleVariant + 'a>);

/// Concrete [`SerializeMap`] forwarding into a type-erased map serializer.
pub struct ErasedMap<'a>(Box<dyn ErasedSerializeMap + 'a>);

/// Concrete [`SerializeStruct`] forwarding into a type-erased struct serializer.
pub struct ErasedStruct<'a>(Box<dyn ErasedSerializeStruct + 'a>);

/// Concrete [`SerializeStructVariant`] forwarding into a type-erased struct-variant serializer.
pub struct ErasedStructVariant<'a>(Box<dyn ErasedSerializeStructVariant + 'a>);

impl<'a> Serializer for SerAdapter<'a> {
    type Ok = ();
    type Error = SerError;
    type SerializeSeq = ErasedSeq<'a>;
    type SerializeTuple = ErasedTuple<'a>;
    type SerializeTupleStruct = ErasedTupleStruct<'a>;
    type SerializeTupleVariant = ErasedTupleVariant<'a>;
    type SerializeMap = ErasedMap<'a>;
    type SerializeStruct = ErasedStruct<'a>;
    type SerializeStructVariant = ErasedStructVariant<'a>;

    fn serialize_bool(self, value: bool) -> Result<(), SerError> {
        self.0.erased_serialize_bool(value)
    }
    fn serialize_i8(self, value: i8) -> Result<(), SerError> {
        self.0.erased_serialize_i8(value)
    }
    fn serialize_i16(self, value: i16) -> Result<(), SerError> {
        self.0.erased_serialize_i16(value)
    }
    fn serialize_i32(self, value: i32) -> Result<(), SerError> {
        self.0.erased_serialize_i32(value)
    }
    fn serialize_i64(self, value: i64) -> Result<(), SerError> {
        self.0.erased_serialize_i64(value)
    }
    fn serialize_i128(self, value: i128) -> Result<(), SerError> {
        self.0.erased_serialize_i128(value)
    }
    fn serialize_u8(self, value: u8) -> Result<(), SerError> {
        self.0.erased_serialize_u8(value)
    }
    fn serialize_u16(self, value: u16) -> Result<(), SerError> {
        self.0.erased_serialize_u16(value)
    }
    fn serialize_u32(self, value: u32) -> Result<(), SerError> {
        self.0.erased_serialize_u32(value)
    }
    fn serialize_u64(self, value: u64) -> Result<(), SerError> {
        self.0.erased_serialize_u64(value)
    }
    fn serialize_u128(self, value: u128) -> Result<(), SerError> {
        self.0.erased_serialize_u128(value)
    }
    fn serialize_f32(self, value: f32) -> Result<(), SerError> {
        self.0.erased_serialize_f32(value)
    }
    fn serialize_f64(self, value: f64) -> Result<(), SerError> {
        self.0.erased_serialize_f64(value)
    }
    fn serialize_char(self, value: char) -> Result<(), SerError> {
        self.0.erased_serialize_char(value)
    }
    fn serialize_str(self, value: &str) -> Result<(), SerError> {
        self.0.erased_serialize_str(value)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<(), SerError> {
        self.0.erased_serialize_bytes(value)
    }
    fn serialize_none(self) -> Result<(), SerError> {
        self.0.erased_serialize_none()
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_some(&value)
    }
    fn serialize_unit(self) -> Result<(), SerError> {
        self.0.erased_serialize_unit()
    }
    fn serialize_unit_struct(self, name: &'static str) -> Result<(), SerError> {
        self.0.erased_serialize_unit_struct(name)
    }
    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<(), SerError> {
        self.0
            .erased_serialize_unit_variant(name, variant_index, variant)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        self.0.erased_serialize_newtype_struct(name, &value)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        self.0
            .erased_serialize_newtype_variant(name, variant_index, variant, &value)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, SerError> {
        self.0.erased_serialize_seq(len).map(ErasedSeq)
    }
    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, SerError> {
        self.0.erased_serialize_tuple(len).map(ErasedTuple)
    }
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, SerError> {
        self.0
            .erased_serialize_tuple_struct(name, len)
            .map(ErasedTupleStruct)
    }
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, SerError> {
        self.0
            .erased_serialize_tuple_variant(name, variant_index, variant, len)
            .map(ErasedTupleVariant)
    }
    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, SerError> {
        self.0.erased_serialize_map(len).map(ErasedMap)
    }
    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, SerError> {
        self.0.erased_serialize_struct(name, len).map(ErasedStruct)
    }
    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, SerError> {
        self.0
            .erased_serialize_struct_variant(name, variant_index, variant, len)
            .map(ErasedStructVariant)
    }
    fn collect_str<T: ?Sized + fmt::Display>(self, value: &T) -> Result<(), SerError> {
        self.0.erased_collect_str(&value)
    }
    fn is_human_readable(&self) -> bool {
        self.0.erased_is_human_readable()
    }
}

impl SerializeSeq for ErasedSeq<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_element(&value)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeTuple for ErasedTuple<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_element(&value)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeTupleStruct for ErasedTupleStruct<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_field(&value)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeTupleVariant for ErasedTupleVariant<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_field(&value)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeMap for ErasedMap<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), SerError> {
        self.0.erased_serialize_key(&key)
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.erased_serialize_value(&value)
    }
    fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), SerError> {
        self.0.erased_serialize_entry(&key, &value)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeStruct for ErasedStruct<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        self.0.erased_serialize_field(key, &value)
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), SerError> {
        self.0.erased_skip_field(key)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

impl SerializeStructVariant for ErasedStructVariant<'_> {
    type Ok = ();
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        self.0.erased_serialize_field(key, &value)
    }
    fn skip_field(&mut self, key: &'static str) -> Result<(), SerError> {
        self.0.erased_skip_field(key)
    }
    fn end(self) -> Result<(), SerError> {
        self.0.erased_end()
    }
}

struct WrapSerialize<'a>(&'a dyn ErasedSerialize);

impl Serialize for WrapSerialize<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_with(serializer, |adapter| self.0.erased_serialize(adapter.0))
    }
}

struct ConcreteSerializer<'a, S: Serializer> {
    inner: Option<S>,
    ok: &'a mut Option<S::Ok>,
    error: &'a mut Option<S::Error>,
}

impl<S: Serializer> ConcreteSerializer<'_, S> {
    fn finish(&mut self, result: Result<S::Ok, S::Error>) -> Result<(), SerError> {
        match result {
            Ok(ok) => {
                *self.ok = Some(ok);
                Ok(())
            }
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }

    fn take(&mut self) -> Result<S, SerError> {
        self.inner.take().ok_or_else(SerError::consumed)
    }
}

struct ConcreteCompound<'a, C, OkT, ErrT> {
    inner: Option<C>,
    ok: &'a mut Option<OkT>,
    error: &'a mut Option<ErrT>,
}

impl<C, OkT, ErrT: fmt::Display> ConcreteCompound<'_, C, OkT, ErrT> {
    fn take(&mut self) -> Result<C, SerError> {
        self.inner.take().ok_or_else(SerError::consumed)
    }

    fn get_mut(&mut self) -> Result<&mut C, SerError> {
        self.inner.as_mut().ok_or_else(SerError::consumed)
    }

    fn record_unit(&mut self, result: Result<(), ErrT>) -> Result<(), SerError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }

    fn record_ok(&mut self, result: Result<OkT, ErrT>) -> Result<(), SerError> {
        match result {
            Ok(ok) => {
                *self.ok = Some(ok);
                Ok(())
            }
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
}

impl<C: SerializeSeq> ErasedSerializeSeq for ConcreteCompound<'_, C, C::Ok, C::Error> {
    fn erased_serialize_element(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_element(&WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeTuple> ErasedSerializeTuple for ConcreteCompound<'_, C, C::Ok, C::Error> {
    fn erased_serialize_element(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_element(&WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeTupleStruct> ErasedSerializeTupleStruct
    for ConcreteCompound<'_, C, C::Ok, C::Error>
{
    fn erased_serialize_field(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_field(&WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeTupleVariant> ErasedSerializeTupleVariant
    for ConcreteCompound<'_, C, C::Ok, C::Error>
{
    fn erased_serialize_field(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_field(&WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeMap> ErasedSerializeMap for ConcreteCompound<'_, C, C::Ok, C::Error> {
    fn erased_serialize_key(&mut self, key: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_key(&WrapSerialize(key));
        self.record_unit(result)
    }
    fn erased_serialize_value(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_value(&WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_serialize_entry(
        &mut self,
        key: &dyn ErasedSerialize,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError> {
        let result = self
            .get_mut()?
            .serialize_entry(&WrapSerialize(key), &WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeStruct> ErasedSerializeStruct for ConcreteCompound<'_, C, C::Ok, C::Error> {
    fn erased_serialize_field(
        &mut self,
        key: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_field(key, &WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_skip_field(&mut self, key: &'static str) -> Result<(), SerError> {
        let result = self.get_mut()?.skip_field(key);
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<C: SerializeStructVariant> ErasedSerializeStructVariant
    for ConcreteCompound<'_, C, C::Ok, C::Error>
{
    fn erased_serialize_field(
        &mut self,
        key: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError> {
        let result = self.get_mut()?.serialize_field(key, &WrapSerialize(value));
        self.record_unit(result)
    }
    fn erased_skip_field(&mut self, key: &'static str) -> Result<(), SerError> {
        let result = self.get_mut()?.skip_field(key);
        self.record_unit(result)
    }
    fn erased_end(mut self: Box<Self>) -> Result<(), SerError> {
        let result = self.take()?.end();
        self.record_ok(result)
    }
}

impl<S: Serializer> ErasedSerializer for ConcreteSerializer<'_, S> {
    fn erased_serialize_bool(&mut self, value: bool) -> Result<(), SerError> {
        let result = self.take()?.serialize_bool(value);
        self.finish(result)
    }
    fn erased_serialize_i8(&mut self, value: i8) -> Result<(), SerError> {
        let result = self.take()?.serialize_i8(value);
        self.finish(result)
    }
    fn erased_serialize_i16(&mut self, value: i16) -> Result<(), SerError> {
        let result = self.take()?.serialize_i16(value);
        self.finish(result)
    }
    fn erased_serialize_i32(&mut self, value: i32) -> Result<(), SerError> {
        let result = self.take()?.serialize_i32(value);
        self.finish(result)
    }
    fn erased_serialize_i64(&mut self, value: i64) -> Result<(), SerError> {
        let result = self.take()?.serialize_i64(value);
        self.finish(result)
    }
    fn erased_serialize_i128(&mut self, value: i128) -> Result<(), SerError> {
        let result = self.take()?.serialize_i128(value);
        self.finish(result)
    }
    fn erased_serialize_u8(&mut self, value: u8) -> Result<(), SerError> {
        let result = self.take()?.serialize_u8(value);
        self.finish(result)
    }
    fn erased_serialize_u16(&mut self, value: u16) -> Result<(), SerError> {
        let result = self.take()?.serialize_u16(value);
        self.finish(result)
    }
    fn erased_serialize_u32(&mut self, value: u32) -> Result<(), SerError> {
        let result = self.take()?.serialize_u32(value);
        self.finish(result)
    }
    fn erased_serialize_u64(&mut self, value: u64) -> Result<(), SerError> {
        let result = self.take()?.serialize_u64(value);
        self.finish(result)
    }
    fn erased_serialize_u128(&mut self, value: u128) -> Result<(), SerError> {
        let result = self.take()?.serialize_u128(value);
        self.finish(result)
    }
    fn erased_serialize_f32(&mut self, value: f32) -> Result<(), SerError> {
        let result = self.take()?.serialize_f32(value);
        self.finish(result)
    }
    fn erased_serialize_f64(&mut self, value: f64) -> Result<(), SerError> {
        let result = self.take()?.serialize_f64(value);
        self.finish(result)
    }
    fn erased_serialize_char(&mut self, value: char) -> Result<(), SerError> {
        let result = self.take()?.serialize_char(value);
        self.finish(result)
    }
    fn erased_serialize_str(&mut self, value: &str) -> Result<(), SerError> {
        let result = self.take()?.serialize_str(value);
        self.finish(result)
    }
    fn erased_serialize_bytes(&mut self, value: &[u8]) -> Result<(), SerError> {
        let result = self.take()?.serialize_bytes(value);
        self.finish(result)
    }
    fn erased_serialize_none(&mut self) -> Result<(), SerError> {
        let result = self.take()?.serialize_none();
        self.finish(result)
    }
    fn erased_serialize_some(&mut self, value: &dyn ErasedSerialize) -> Result<(), SerError> {
        let result = self.take()?.serialize_some(&WrapSerialize(value));
        self.finish(result)
    }
    fn erased_serialize_unit(&mut self) -> Result<(), SerError> {
        let result = self.take()?.serialize_unit();
        self.finish(result)
    }
    fn erased_serialize_unit_struct(&mut self, name: &'static str) -> Result<(), SerError> {
        let result = self.take()?.serialize_unit_struct(name);
        self.finish(result)
    }
    fn erased_serialize_unit_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<(), SerError> {
        let result = self
            .take()?
            .serialize_unit_variant(name, variant_index, variant);
        self.finish(result)
    }
    fn erased_serialize_newtype_struct(
        &mut self,
        name: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError> {
        let result = self
            .take()?
            .serialize_newtype_struct(name, &WrapSerialize(value));
        self.finish(result)
    }
    fn erased_serialize_newtype_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &dyn ErasedSerialize,
    ) -> Result<(), SerError> {
        let result = self.take()?.serialize_newtype_variant(
            name,
            variant_index,
            variant,
            &WrapSerialize(value),
        );
        self.finish(result)
    }
    fn erased_serialize_seq(
        &mut self,
        len: Option<usize>,
    ) -> Result<Box<dyn ErasedSerializeSeq + '_>, SerError> {
        match self.take()?.serialize_seq(len) {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_tuple(
        &mut self,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTuple + '_>, SerError> {
        match self.take()?.serialize_tuple(len) {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_tuple_struct(
        &mut self,
        name: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTupleStruct + '_>, SerError> {
        match self.take()?.serialize_tuple_struct(name, len) {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_tuple_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeTupleVariant + '_>, SerError> {
        match self
            .take()?
            .serialize_tuple_variant(name, variant_index, variant, len)
        {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_map(
        &mut self,
        len: Option<usize>,
    ) -> Result<Box<dyn ErasedSerializeMap + '_>, SerError> {
        match self.take()?.serialize_map(len) {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_struct(
        &mut self,
        name: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeStruct + '_>, SerError> {
        match self.take()?.serialize_struct(name, len) {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_serialize_struct_variant(
        &mut self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Box<dyn ErasedSerializeStructVariant + '_>, SerError> {
        match self
            .take()?
            .serialize_struct_variant(name, variant_index, variant, len)
        {
            Ok(inner) => Ok(Box::new(ConcreteCompound {
                inner: Some(inner),
                ok: &mut *self.ok,
                error: &mut *self.error,
            })),
            Err(error) => {
                let ser_error = SerError::inner(&error);
                *self.error = Some(error);
                Err(ser_error)
            }
        }
    }
    fn erased_collect_str(&mut self, value: &dyn fmt::Display) -> Result<(), SerError> {
        let result = self.take()?.collect_str(&DisplayWrap(value));
        self.finish(result)
    }
    fn erased_is_human_readable(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(Serializer::is_human_readable)
    }
}

struct DisplayWrap<'a>(&'a dyn fmt::Display);

impl fmt::Display for DisplayWrap<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Drives `body` with a [`SerAdapter`] backed by `serializer`.
pub fn serialize_with<S: Serializer>(
    serializer: S,
    body: impl FnOnce(SerAdapter<'_>) -> Result<(), SerError>,
) -> Result<S::Ok, S::Error> {
    let mut ok = None;
    let mut error = None;
    let mut concrete = ConcreteSerializer {
        inner: Some(serializer),
        ok: &mut ok,
        error: &mut error,
    };
    match body(SerAdapter(&mut concrete)) {
        Ok(()) => ok.ok_or_else(|| ser::Error::custom("erased serializer produced no value")),
        Err(ser_error) => Err(ser_error.into_error(error)),
    }
}

/// Serializes an erased value directly into a [`serde_json::Value`] inside
/// `cadmpeg-core`, keeping `serde_json::value::Serializer` compiled once.
pub fn serialize_to_json_value(
    body: &dyn Fn(SerAdapter<'_>) -> Result<(), SerError>,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_with(serde_json::value::Serializer, body)
}

/// Serializes `value` into a [`serde_json::Value`] through [`SerAdapter`],
/// keeping `serde_json::value::Serializer` compiled once in `cadmpeg-core`.
pub fn to_json_value<T: Serialize + ?Sized>(
    value: &T,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_to_json_value(&|adapter| value.serialize(adapter))
}

/// Serializes an erased value directly into a compact JSON string inside
/// `cadmpeg-core`, keeping `serde_json::Serializer` compiled once.
pub fn serialize_to_json_string(
    body: &dyn Fn(SerAdapter<'_>) -> Result<(), SerError>,
) -> Result<String, serde_json::Error> {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::new(&mut bytes);
    serialize_with(&mut serializer, body)?;
    String::from_utf8(bytes).map_err(ser::Error::custom)
}

/// Serializes `value` into a compact JSON string through [`SerAdapter`],
/// keeping `serde_json::Serializer` compiled once in `cadmpeg-core`.
pub fn to_json_string<T: Serialize + ?Sized>(value: &T) -> Result<String, serde_json::Error> {
    serialize_to_json_string(&|adapter| value.serialize(adapter))
}

/// Error produced while driving an erased [`Deserializer`].
#[derive(Debug)]
pub struct DeError {
    inner: bool,
    message: String,
}

impl DeError {
    fn inner(error: &impl fmt::Display) -> Self {
        Self {
            inner: true,
            message: error.to_string(),
        }
    }

    fn consumed() -> Self {
        Self {
            inner: false,
            message: "erased deserializer consumed twice".to_owned(),
        }
    }

    fn into_error<E: de::Error>(self, inner: Option<E>) -> E {
        if self.inner {
            if let Some(error) = inner {
                return error;
            }
        }
        E::custom(self.message)
    }
}

impl fmt::Display for DeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.inner && self.message.is_empty() {
            formatter.write_str("deserializer error")
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for DeError {}

impl de::Error for DeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self {
            inner: false,
            message: message.to_string(),
        }
    }

    fn invalid_type(unexp: de::Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        Self::custom(serde_json::Error::invalid_type(unexp, exp))
    }

    fn invalid_value(unexp: de::Unexpected<'_>, exp: &dyn de::Expected) -> Self {
        Self::custom(serde_json::Error::invalid_value(unexp, exp))
    }

    fn invalid_length(len: usize, exp: &dyn de::Expected) -> Self {
        Self::custom(serde_json::Error::invalid_length(len, exp))
    }

    fn unknown_variant(variant: &str, expected: &'static [&'static str]) -> Self {
        Self::custom(serde_json::Error::unknown_variant(variant, expected))
    }

    fn unknown_field(field: &str, expected: &'static [&'static str]) -> Self {
        Self::custom(serde_json::Error::unknown_field(field, expected))
    }

    fn missing_field(field: &'static str) -> Self {
        Self::custom(serde_json::Error::missing_field(field))
    }

    fn duplicate_field(field: &'static str) -> Self {
        Self::custom(serde_json::Error::duplicate_field(field))
    }
}

/// Object-safe [`DeserializeSeed`].
pub trait ErasedDeserializeSeed<'de> {
    /// Deserializes a value from `deserializer`.
    fn erased_deserialize(
        &mut self,
        deserializer: &mut dyn ErasedDeserializer<'de>,
    ) -> Result<(), DeError>;
}

/// Object-safe [`SeqAccess`].
pub trait ErasedSeqAccess<'de> {
    /// Reads the next sequence element into `seed`, returning `false` at end.
    fn erased_next_element_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError>;
    /// Returns the size hint, if known.
    fn erased_size_hint(&self) -> Option<usize>;
}

/// Object-safe [`MapAccess`].
pub trait ErasedMapAccess<'de> {
    /// Reads the next map key into `seed`, returning `false` at end.
    fn erased_next_key_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError>;
    /// Reads the next map value into `seed`.
    fn erased_next_value_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<(), DeError>;
    /// Reads the next map entry into `kseed` and `vseed`, returning `false` at end.
    fn erased_next_entry_seed(
        &mut self,
        kseed: &mut dyn ErasedDeserializeSeed<'de>,
        vseed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError>;
    /// Returns the size hint, if known.
    fn erased_size_hint(&self) -> Option<usize>;
}

/// Object-safe [`VariantAccess`].
pub trait ErasedVariantAccess<'de> {
    /// Completes a unit variant.
    fn erased_unit_variant(self: Box<Self>) -> Result<(), DeError>;
    /// Reads a newtype variant payload into `seed`.
    fn erased_newtype_variant_seed(
        self: Box<Self>,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<(), DeError>;
    /// Reads a tuple variant payload into `visitor`.
    fn erased_tuple_variant(
        self: Box<Self>,
        len: usize,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Reads a struct variant payload into `visitor`.
    fn erased_struct_variant(
        self: Box<Self>,
        fields: &'static [&'static str],
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
}

/// Object-safe [`EnumAccess`].
pub trait ErasedEnumAccess<'de> {
    /// Reads the variant discriminant into `seed` and returns the variant payload reader.
    fn erased_variant_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<Box<dyn ErasedVariantAccess<'de> + '_>, DeError>;
}

/// One deserialization event delivered to an [`ErasedVisitor`].
pub enum VisitEvent<'a, 'de> {
    /// A `bool`.
    Bool(bool),
    /// A signed integer widened to `i64`.
    I64(i64),
    /// An unsigned integer widened to `u64`.
    U64(u64),
    /// A floating-point number widened to `f64`.
    F64(f64),
    /// A borrowed string.
    Str(&'a str),
    /// A `'de`-borrowed string.
    BorrowedStr(&'de str),
    /// An owned string.
    String(String),
    /// Borrowed bytes.
    Bytes(&'a [u8]),
    /// `None`.
    None,
    /// `Some`.
    Some(&'a mut dyn ErasedDeserializer<'de>),
    /// `()`.
    Unit,
    /// A newtype struct.
    NewtypeStruct(&'a mut dyn ErasedDeserializer<'de>),
    /// A sequence.
    Seq(&'a mut dyn ErasedSeqAccess<'de>),
    /// A map.
    Map(&'a mut dyn ErasedMapAccess<'de>),
    /// An enum.
    Enum(&'a mut dyn ErasedEnumAccess<'de>),
}

/// Object-safe [`Visitor`].
pub trait ErasedVisitor<'de> {
    /// Formats the expectation description.
    fn erased_expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Visits one deserialization event.
    fn erased_visit(&mut self, event: VisitEvent<'_, 'de>) -> Result<(), DeError>;
}

/// Object-safe [`Deserializer`].
pub trait ErasedDeserializer<'de> {
    /// Drives `deserialize_any`.
    fn erased_deserialize_any(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_bool`.
    fn erased_deserialize_bool(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_i8`.
    fn erased_deserialize_i8(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_i16`.
    fn erased_deserialize_i16(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_i32`.
    fn erased_deserialize_i32(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_i64`.
    fn erased_deserialize_i64(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_i128`.
    fn erased_deserialize_i128(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_u8`.
    fn erased_deserialize_u8(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_u16`.
    fn erased_deserialize_u16(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_u32`.
    fn erased_deserialize_u32(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_u64`.
    fn erased_deserialize_u64(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_u128`.
    fn erased_deserialize_u128(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_f32`.
    fn erased_deserialize_f32(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_f64`.
    fn erased_deserialize_f64(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_char`.
    fn erased_deserialize_char(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_str`.
    fn erased_deserialize_str(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_string`.
    fn erased_deserialize_string(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_bytes`.
    fn erased_deserialize_bytes(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_byte_buf`.
    fn erased_deserialize_byte_buf(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_option`.
    fn erased_deserialize_option(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_unit`.
    fn erased_deserialize_unit(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_unit_struct`.
    fn erased_deserialize_unit_struct(
        &mut self,
        name: &'static str,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_newtype_struct`.
    fn erased_deserialize_newtype_struct(
        &mut self,
        name: &'static str,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_seq`.
    fn erased_deserialize_seq(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_tuple`.
    fn erased_deserialize_tuple(
        &mut self,
        len: usize,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_tuple_struct`.
    fn erased_deserialize_tuple_struct(
        &mut self,
        name: &'static str,
        len: usize,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_map`.
    fn erased_deserialize_map(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_struct`.
    fn erased_deserialize_struct(
        &mut self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_enum`.
    fn erased_deserialize_enum(
        &mut self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_identifier`.
    fn erased_deserialize_identifier(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Drives `deserialize_ignored_any`.
    fn erased_deserialize_ignored_any(
        &mut self,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError>;
    /// Reports whether the underlying deserializer is human-readable.
    fn erased_is_human_readable(&self) -> bool;
}

/// Concrete [`Deserializer`] forwarding into a type-erased [`ErasedDeserializer`].
pub struct DeAdapter<'a, 'de>(pub &'a mut dyn ErasedDeserializer<'de>);

struct SeedSlot<S, T> {
    seed: Option<S>,
    output: Option<T>,
}

impl<'de, S: DeserializeSeed<'de>> ErasedDeserializeSeed<'de> for SeedSlot<S, S::Value> {
    fn erased_deserialize(
        &mut self,
        deserializer: &mut dyn ErasedDeserializer<'de>,
    ) -> Result<(), DeError> {
        let seed = self.seed.take().ok_or_else(DeError::consumed)?;
        self.output = Some(seed.deserialize(DeAdapter(deserializer))?);
        Ok(())
    }
}

struct VisitorSlot<V, T> {
    visitor: Option<V>,
    output: Option<T>,
}

impl<'de, V: Visitor<'de>> VisitorSlot<V, V::Value> {
    fn take(&mut self) -> Result<V, DeError> {
        self.visitor.take().ok_or_else(DeError::consumed)
    }

    fn store(&mut self, result: Result<V::Value, DeError>) -> Result<(), DeError> {
        self.output = Some(result?);
        Ok(())
    }
}

struct SeqAdapter<'a, 'de>(&'a mut dyn ErasedSeqAccess<'de>);

impl<'de> SeqAccess<'de> for SeqAdapter<'_, 'de> {
    type Error = DeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, DeError> {
        let mut slot = SeedSlot {
            seed: Some(seed),
            output: None,
        };
        if self.0.erased_next_element_seed(&mut slot)? {
            slot.output
                .ok_or_else(|| de::Error::custom("erased sequence seed produced no value"))
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.erased_size_hint()
    }
}

struct MapAdapter<'a, 'de>(&'a mut dyn ErasedMapAccess<'de>);

impl<'de> MapAccess<'de> for MapAdapter<'_, 'de> {
    type Error = DeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, DeError> {
        let mut slot = SeedSlot {
            seed: Some(seed),
            output: None,
        };
        if self.0.erased_next_key_seed(&mut slot)? {
            slot.output
                .ok_or_else(|| de::Error::custom("erased map key seed produced no value"))
                .map(Some)
        } else {
            Ok(None)
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, DeError> {
        let mut slot = SeedSlot {
            seed: Some(seed),
            output: None,
        };
        self.0.erased_next_value_seed(&mut slot)?;
        slot.output
            .ok_or_else(|| de::Error::custom("erased map value seed produced no value"))
    }

    fn next_entry_seed<K: DeserializeSeed<'de>, V: DeserializeSeed<'de>>(
        &mut self,
        kseed: K,
        vseed: V,
    ) -> Result<Option<(K::Value, V::Value)>, DeError> {
        let mut kslot = SeedSlot {
            seed: Some(kseed),
            output: None,
        };
        let mut vslot = SeedSlot {
            seed: Some(vseed),
            output: None,
        };
        if self.0.erased_next_entry_seed(&mut kslot, &mut vslot)? {
            let key = kslot
                .output
                .ok_or_else(|| de::Error::custom("erased map key seed produced no value"))?;
            let value = vslot
                .output
                .ok_or_else(|| de::Error::custom("erased map value seed produced no value"))?;
            Ok(Some((key, value)))
        } else {
            Ok(None)
        }
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.erased_size_hint()
    }
}

struct VariantAdapter<'a, 'de>(Box<dyn ErasedVariantAccess<'de> + 'a>);

impl<'de> VariantAccess<'de> for VariantAdapter<'_, 'de> {
    type Error = DeError;

    fn unit_variant(self) -> Result<(), DeError> {
        self.0.erased_unit_variant()
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, DeError> {
        let mut slot = SeedSlot {
            seed: Some(seed),
            output: None,
        };
        self.0.erased_newtype_variant_seed(&mut slot)?;
        slot.output
            .ok_or_else(|| de::Error::custom("erased newtype variant produced no value"))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, DeError> {
        let mut slot = VisitorSlot {
            visitor: Some(visitor),
            output: None,
        };
        self.0.erased_tuple_variant(len, &mut slot)?;
        slot.output
            .ok_or_else(|| de::Error::custom("erased tuple variant produced no value"))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, DeError> {
        let mut slot = VisitorSlot {
            visitor: Some(visitor),
            output: None,
        };
        self.0.erased_struct_variant(fields, &mut slot)?;
        slot.output
            .ok_or_else(|| de::Error::custom("erased struct variant produced no value"))
    }
}

struct EnumAdapter<'a, 'de>(&'a mut dyn ErasedEnumAccess<'de>);

impl<'a, 'de> EnumAccess<'de> for EnumAdapter<'a, 'de> {
    type Error = DeError;
    type Variant = VariantAdapter<'a, 'de>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), DeError> {
        let mut slot = SeedSlot {
            seed: Some(seed),
            output: None,
        };
        let variant = self.0.erased_variant_seed(&mut slot)?;
        let value = slot
            .output
            .ok_or_else(|| de::Error::custom("erased enum seed produced no value"))?;
        Ok((value, VariantAdapter(variant)))
    }
}

impl<'de, V: Visitor<'de>> ErasedVisitor<'de> for VisitorSlot<V, V::Value> {
    fn erased_expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.visitor {
            Some(visitor) => visitor.expecting(formatter),
            None => formatter.write_str("a value"),
        }
    }

    fn erased_visit(&mut self, event: VisitEvent<'_, 'de>) -> Result<(), DeError> {
        let visitor = self.take()?;
        let result = match event {
            VisitEvent::Bool(value) => visitor.visit_bool(value),
            VisitEvent::I64(value) => visitor.visit_i64(value),
            VisitEvent::U64(value) => visitor.visit_u64(value),
            VisitEvent::F64(value) => visitor.visit_f64(value),
            VisitEvent::Str(value) => visitor.visit_str(value),
            VisitEvent::BorrowedStr(value) => visitor.visit_borrowed_str(value),
            VisitEvent::String(value) => visitor.visit_string(value),
            VisitEvent::Bytes(value) => visitor.visit_bytes(value),
            VisitEvent::None => visitor.visit_none(),
            VisitEvent::Some(deserializer) => visitor.visit_some(DeAdapter(deserializer)),
            VisitEvent::Unit => visitor.visit_unit(),
            VisitEvent::NewtypeStruct(deserializer) => {
                visitor.visit_newtype_struct(DeAdapter(deserializer))
            }
            VisitEvent::Seq(seq) => visitor.visit_seq(SeqAdapter(seq)),
            VisitEvent::Map(map) => visitor.visit_map(MapAdapter(map)),
            VisitEvent::Enum(data) => visitor.visit_enum(EnumAdapter(data)),
        };
        self.store(result)
    }
}

macro_rules! forward_de_adapter {
    ($($method:ident => $erased:ident ($($arg:ident: $ty:ty),*)),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, $($arg: $ty,)* visitor: V) -> Result<V::Value, DeError> {
            let mut slot = VisitorSlot {
                visitor: Some(visitor),
                output: None,
            };
            self.0.$erased($($arg,)* &mut slot)?;
            slot.output
                .ok_or_else(|| de::Error::custom("erased visitor produced no value"))
        }
    )*};
}

impl<'de> Deserializer<'de> for DeAdapter<'_, 'de> {
    type Error = DeError;

    forward_de_adapter!(
        deserialize_any => erased_deserialize_any(),
        deserialize_bool => erased_deserialize_bool(),
        deserialize_i8 => erased_deserialize_i8(),
        deserialize_i16 => erased_deserialize_i16(),
        deserialize_i32 => erased_deserialize_i32(),
        deserialize_i64 => erased_deserialize_i64(),
        deserialize_i128 => erased_deserialize_i128(),
        deserialize_u8 => erased_deserialize_u8(),
        deserialize_u16 => erased_deserialize_u16(),
        deserialize_u32 => erased_deserialize_u32(),
        deserialize_u64 => erased_deserialize_u64(),
        deserialize_u128 => erased_deserialize_u128(),
        deserialize_f32 => erased_deserialize_f32(),
        deserialize_f64 => erased_deserialize_f64(),
        deserialize_char => erased_deserialize_char(),
        deserialize_str => erased_deserialize_str(),
        deserialize_string => erased_deserialize_string(),
        deserialize_bytes => erased_deserialize_bytes(),
        deserialize_byte_buf => erased_deserialize_byte_buf(),
        deserialize_option => erased_deserialize_option(),
        deserialize_unit => erased_deserialize_unit(),
        deserialize_unit_struct => erased_deserialize_unit_struct(name: &'static str),
        deserialize_newtype_struct => erased_deserialize_newtype_struct(name: &'static str),
        deserialize_seq => erased_deserialize_seq(),
        deserialize_tuple => erased_deserialize_tuple(len: usize),
        deserialize_tuple_struct => erased_deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_map => erased_deserialize_map(),
        deserialize_struct => erased_deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum => erased_deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier => erased_deserialize_identifier(),
        deserialize_ignored_any => erased_deserialize_ignored_any(),
    );

    fn is_human_readable(&self) -> bool {
        self.0.erased_is_human_readable()
    }
}

struct WrapSeed<'a, 'de>(&'a mut dyn ErasedDeserializeSeed<'de>);

impl<'de> DeserializeSeed<'de> for WrapSeed<'_, 'de> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let mut error = None;
        let mut concrete = ConcreteDeserializer {
            inner: Some(deserializer),
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self.0.erased_deserialize(&mut concrete) {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }
}

struct ConcreteSeqAccess<'a, 'de, A: SeqAccess<'de>> {
    inner: A,
    error: &'a mut Option<A::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, A: SeqAccess<'de>> ErasedSeqAccess<'de> for ConcreteSeqAccess<'_, 'de, A> {
    fn erased_next_element_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError> {
        match self.inner.next_element_seed(WrapSeed(seed)) {
            Ok(Some(())) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => {
                let de_error = DeError::inner(&error);
                *self.error = Some(error);
                Err(de_error)
            }
        }
    }

    fn erased_size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

struct ConcreteMapAccess<'a, 'de, A: MapAccess<'de>> {
    inner: A,
    error: &'a mut Option<A::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, A: MapAccess<'de>> ErasedMapAccess<'de> for ConcreteMapAccess<'_, 'de, A> {
    fn erased_next_key_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError> {
        match self.inner.next_key_seed(WrapSeed(seed)) {
            Ok(Some(())) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => {
                let de_error = DeError::inner(&error);
                *self.error = Some(error);
                Err(de_error)
            }
        }
    }

    fn erased_next_value_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<(), DeError> {
        match self.inner.next_value_seed(WrapSeed(seed)) {
            Ok(()) => Ok(()),
            Err(error) => {
                let de_error = DeError::inner(&error);
                *self.error = Some(error);
                Err(de_error)
            }
        }
    }

    fn erased_next_entry_seed(
        &mut self,
        kseed: &mut dyn ErasedDeserializeSeed<'de>,
        vseed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<bool, DeError> {
        match self.inner.next_entry_seed(WrapSeed(kseed), WrapSeed(vseed)) {
            Ok(Some(((), ()))) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => {
                let de_error = DeError::inner(&error);
                *self.error = Some(error);
                Err(de_error)
            }
        }
    }

    fn erased_size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

struct ConcreteVariantAccess<'a, 'de, A: VariantAccess<'de>> {
    inner: A,
    error: &'a mut Option<A::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, A: VariantAccess<'de>> ErasedVariantAccess<'de> for ConcreteVariantAccess<'_, 'de, A> {
    fn erased_unit_variant(self: Box<Self>) -> Result<(), DeError> {
        let Self { inner, error, .. } = *self;
        match inner.unit_variant() {
            Ok(()) => Ok(()),
            Err(err) => {
                let de_error = DeError::inner(&err);
                *error = Some(err);
                Err(de_error)
            }
        }
    }

    fn erased_newtype_variant_seed(
        self: Box<Self>,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<(), DeError> {
        let Self { inner, error, .. } = *self;
        match inner.newtype_variant_seed(WrapSeed(seed)) {
            Ok(()) => Ok(()),
            Err(err) => {
                let de_error = DeError::inner(&err);
                *error = Some(err);
                Err(de_error)
            }
        }
    }

    fn erased_tuple_variant(
        self: Box<Self>,
        len: usize,
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError> {
        let Self { inner, error, .. } = *self;
        match inner.tuple_variant(len, WrapVisitor(visitor)) {
            Ok(()) => Ok(()),
            Err(err) => {
                let de_error = DeError::inner(&err);
                *error = Some(err);
                Err(de_error)
            }
        }
    }

    fn erased_struct_variant(
        self: Box<Self>,
        fields: &'static [&'static str],
        visitor: &mut dyn ErasedVisitor<'de>,
    ) -> Result<(), DeError> {
        let Self { inner, error, .. } = *self;
        match inner.struct_variant(fields, WrapVisitor(visitor)) {
            Ok(()) => Ok(()),
            Err(err) => {
                let de_error = DeError::inner(&err);
                *error = Some(err);
                Err(de_error)
            }
        }
    }
}

struct ConcreteEnumAccess<'a, 'de, A: EnumAccess<'de>> {
    inner: Option<A>,
    error: &'a mut Option<A::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, A: EnumAccess<'de>> ErasedEnumAccess<'de> for ConcreteEnumAccess<'_, 'de, A> {
    fn erased_variant_seed(
        &mut self,
        seed: &mut dyn ErasedDeserializeSeed<'de>,
    ) -> Result<Box<dyn ErasedVariantAccess<'de> + '_>, DeError> {
        let inner = self.inner.take().ok_or_else(DeError::consumed)?;
        match inner.variant_seed(WrapSeed(seed)) {
            Ok(((), variant)) => Ok(Box::new(ConcreteVariantAccess {
                inner: variant,
                error: &mut *self.error,
                _marker: std::marker::PhantomData,
            })),
            Err(error) => {
                let de_error = DeError::inner(&error);
                *self.error = Some(error);
                Err(de_error)
            }
        }
    }
}

struct WrapVisitor<'a, 'de>(&'a mut dyn ErasedVisitor<'de>);

impl<'de> Visitor<'de> for WrapVisitor<'_, 'de> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.erased_expecting(formatter)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::Bool(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_i8<E: de::Error>(self, value: i8) -> Result<(), E> {
        self.visit_i64(i64::from(value))
    }

    fn visit_i16<E: de::Error>(self, value: i16) -> Result<(), E> {
        self.visit_i64(i64::from(value))
    }

    fn visit_i32<E: de::Error>(self, value: i32) -> Result<(), E> {
        self.visit_i64(i64::from(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::I64(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<(), E> {
        match i64::try_from(value) {
            Ok(narrow) => self.visit_i64(narrow),
            Err(_) => Err(E::invalid_type(
                de::Unexpected::Other("128-bit integer"),
                &self,
            )),
        }
    }

    fn visit_u8<E: de::Error>(self, value: u8) -> Result<(), E> {
        self.visit_u64(u64::from(value))
    }

    fn visit_u16<E: de::Error>(self, value: u16) -> Result<(), E> {
        self.visit_u64(u64::from(value))
    }

    fn visit_u32<E: de::Error>(self, value: u32) -> Result<(), E> {
        self.visit_u64(u64::from(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::U64(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<(), E> {
        match u64::try_from(value) {
            Ok(narrow) => self.visit_u64(narrow),
            Err(_) => Err(E::invalid_type(
                de::Unexpected::Other("128-bit unsigned integer"),
                &self,
            )),
        }
    }

    fn visit_f32<E: de::Error>(self, value: f32) -> Result<(), E> {
        self.visit_f64(f64::from(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::F64(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_char<E: de::Error>(self, value: char) -> Result<(), E> {
        let mut buf = [0u8; 4];
        self.visit_str(value.encode_utf8(&mut buf))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::Str(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::BorrowedStr(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::String(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_bytes<E: de::Error>(self, value: &[u8]) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::Bytes(value))
            .map_err(|err| err.into_error(None))
    }

    fn visit_borrowed_bytes<E: de::Error>(self, value: &'de [u8]) -> Result<(), E> {
        self.visit_bytes(value)
    }

    fn visit_byte_buf<E: de::Error>(self, value: Vec<u8>) -> Result<(), E> {
        self.visit_bytes(&value)
    }

    fn visit_none<E: de::Error>(self) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::None)
            .map_err(|err| err.into_error(None))
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        self.0
            .erased_visit(VisitEvent::Unit)
            .map_err(|err| err.into_error(None))
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let mut error = None;
        let mut concrete = ConcreteDeserializer {
            inner: Some(deserializer),
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self.0.erased_visit(VisitEvent::Some(&mut concrete)) {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        let mut error = None;
        let mut concrete = ConcreteDeserializer {
            inner: Some(deserializer),
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self
            .0
            .erased_visit(VisitEvent::NewtypeStruct(&mut concrete))
        {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<(), A::Error> {
        let mut error = None;
        let mut concrete = ConcreteSeqAccess {
            inner: seq,
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self.0.erased_visit(VisitEvent::Seq(&mut concrete)) {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<(), A::Error> {
        let mut error = None;
        let mut concrete = ConcreteMapAccess {
            inner: map,
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self.0.erased_visit(VisitEvent::Map(&mut concrete)) {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<(), A::Error> {
        let mut error = None;
        let mut concrete = ConcreteEnumAccess {
            inner: Some(data),
            error: &mut error,
            _marker: std::marker::PhantomData,
        };
        match self.0.erased_visit(VisitEvent::Enum(&mut concrete)) {
            Ok(()) => Ok(()),
            Err(de_error) => Err(de_error.into_error(error)),
        }
    }
}

struct ConcreteDeserializer<'a, 'de, D: Deserializer<'de>> {
    inner: Option<D>,
    error: &'a mut Option<D::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

macro_rules! forward_concrete_de {
    ($($erased:ident => $method:ident ($($arg:ident: $ty:ty),*)),* $(,)?) => {$(
        fn $erased(&mut self, $($arg: $ty,)* visitor: &mut dyn ErasedVisitor<'de>) -> Result<(), DeError> {
            let inner = self.inner.take().ok_or_else(DeError::consumed)?;
            match inner.$method($($arg,)* WrapVisitor(visitor)) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let de_error = DeError::inner(&error);
                    *self.error = Some(error);
                    Err(de_error)
                }
            }
        }
    )*};
}

impl<'de, D: Deserializer<'de>> ErasedDeserializer<'de> for ConcreteDeserializer<'_, 'de, D> {
    forward_concrete_de!(
        erased_deserialize_any => deserialize_any(),
        erased_deserialize_bool => deserialize_bool(),
        erased_deserialize_i8 => deserialize_i8(),
        erased_deserialize_i16 => deserialize_i16(),
        erased_deserialize_i32 => deserialize_i32(),
        erased_deserialize_i64 => deserialize_i64(),
        erased_deserialize_i128 => deserialize_i128(),
        erased_deserialize_u8 => deserialize_u8(),
        erased_deserialize_u16 => deserialize_u16(),
        erased_deserialize_u32 => deserialize_u32(),
        erased_deserialize_u64 => deserialize_u64(),
        erased_deserialize_u128 => deserialize_u128(),
        erased_deserialize_f32 => deserialize_f32(),
        erased_deserialize_f64 => deserialize_f64(),
        erased_deserialize_char => deserialize_char(),
        erased_deserialize_str => deserialize_str(),
        erased_deserialize_string => deserialize_string(),
        erased_deserialize_bytes => deserialize_bytes(),
        erased_deserialize_byte_buf => deserialize_byte_buf(),
        erased_deserialize_option => deserialize_option(),
        erased_deserialize_unit => deserialize_unit(),
        erased_deserialize_unit_struct => deserialize_unit_struct(name: &'static str),
        erased_deserialize_newtype_struct => deserialize_newtype_struct(name: &'static str),
        erased_deserialize_seq => deserialize_seq(),
        erased_deserialize_tuple => deserialize_tuple(len: usize),
        erased_deserialize_tuple_struct => deserialize_tuple_struct(name: &'static str, len: usize),
        erased_deserialize_map => deserialize_map(),
        erased_deserialize_struct => deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        erased_deserialize_enum => deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        erased_deserialize_identifier => deserialize_identifier(),
        erased_deserialize_ignored_any => deserialize_ignored_any(),
    );

    fn erased_is_human_readable(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(Deserializer::is_human_readable)
    }
}

/// Stack guard holding an erased [`Deserializer`] and its concrete error slot.
pub struct DeGuard<'de, D: Deserializer<'de>> {
    inner: Option<D>,
    error: Option<D::Error>,
    _marker: std::marker::PhantomData<&'de ()>,
}

impl<'de, D: Deserializer<'de>> DeGuard<'de, D> {
    /// Wraps `deserializer` for type-erased deserialization.
    pub const fn new(deserializer: D) -> Self {
        Self {
            inner: Some(deserializer),
            error: None,
            _marker: std::marker::PhantomData,
        }
    }

    /// Borrows a [`DeAdapter`] backed by the stored deserializer.
    pub fn adapter(&mut self) -> DeAdapter<'_, 'de> {
        DeAdapter(self)
    }

    /// Converts the erased result back into the concrete deserializer's error type.
    pub fn finish<T>(mut self, result: Result<T, DeError>) -> Result<T, D::Error> {
        match result {
            Ok(value) => Ok(value),
            Err(de_error) => Err(de_error.into_error(self.error.take())),
        }
    }
}

macro_rules! forward_de_guard {
    ($($erased:ident => $method:ident ($($arg:ident: $ty:ty),*)),* $(,)?) => {$(
        fn $erased(&mut self, $($arg: $ty,)* visitor: &mut dyn ErasedVisitor<'de>) -> Result<(), DeError> {
            let inner = self.inner.take().ok_or_else(DeError::consumed)?;
            match inner.$method($($arg,)* WrapVisitor(visitor)) {
                Ok(()) => Ok(()),
                Err(error) => {
                    let de_error = DeError::inner(&error);
                    self.error = Some(error);
                    Err(de_error)
                }
            }
        }
    )*};
}

impl<'de, D: Deserializer<'de>> ErasedDeserializer<'de> for DeGuard<'de, D> {
    forward_de_guard!(
        erased_deserialize_any => deserialize_any(),
        erased_deserialize_bool => deserialize_bool(),
        erased_deserialize_i8 => deserialize_i8(),
        erased_deserialize_i16 => deserialize_i16(),
        erased_deserialize_i32 => deserialize_i32(),
        erased_deserialize_i64 => deserialize_i64(),
        erased_deserialize_i128 => deserialize_i128(),
        erased_deserialize_u8 => deserialize_u8(),
        erased_deserialize_u16 => deserialize_u16(),
        erased_deserialize_u32 => deserialize_u32(),
        erased_deserialize_u64 => deserialize_u64(),
        erased_deserialize_u128 => deserialize_u128(),
        erased_deserialize_f32 => deserialize_f32(),
        erased_deserialize_f64 => deserialize_f64(),
        erased_deserialize_char => deserialize_char(),
        erased_deserialize_str => deserialize_str(),
        erased_deserialize_string => deserialize_string(),
        erased_deserialize_bytes => deserialize_bytes(),
        erased_deserialize_byte_buf => deserialize_byte_buf(),
        erased_deserialize_option => deserialize_option(),
        erased_deserialize_unit => deserialize_unit(),
        erased_deserialize_unit_struct => deserialize_unit_struct(name: &'static str),
        erased_deserialize_newtype_struct => deserialize_newtype_struct(name: &'static str),
        erased_deserialize_seq => deserialize_seq(),
        erased_deserialize_tuple => deserialize_tuple(len: usize),
        erased_deserialize_tuple_struct => deserialize_tuple_struct(name: &'static str, len: usize),
        erased_deserialize_map => deserialize_map(),
        erased_deserialize_struct => deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        erased_deserialize_enum => deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        erased_deserialize_identifier => deserialize_identifier(),
        erased_deserialize_ignored_any => deserialize_ignored_any(),
    );

    fn erased_is_human_readable(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(Deserializer::is_human_readable)
    }
}

/// Deserializes `T` through a [`DeAdapter`] backed by `deserializer`.
pub fn deserialize<'de, T: serde::Deserialize<'de>, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    let mut guard = DeGuard::new(deserializer);
    let value = T::deserialize(guard.adapter());
    guard.finish(value)
}

/// Deserializes `T` through a [`DeAdapter`] using a non-generic reader `read`.
pub fn deserialize_with<'de, T, D: Deserializer<'de>>(
    deserializer: D,
    read: fn(DeAdapter<'_, 'de>) -> Result<T, DeError>,
) -> Result<T, D::Error> {
    let mut guard = DeGuard::new(deserializer);
    let value = read(guard.adapter());
    guard.finish(value)
}

/// Drives `read` with a [`DeAdapter`] backed by `deserializer`.
pub fn deserialize_erased<'de, D: Deserializer<'de>>(
    deserializer: D,
    read: &mut dyn FnMut(DeAdapter<'_, 'de>) -> Result<(), DeError>,
) -> Result<(), D::Error> {
    let mut guard = DeGuard::new(deserializer);
    let outcome = read(guard.adapter());
    guard.finish(outcome)
}

/// Drives `read` with a [`DeAdapter`] backed by `value`, keeping
/// `serde_json::Value`'s erased deserializer compiled once in `cadmpeg-core`.
pub fn with_json_value_deserializer<'de>(
    value: &'de serde_json::Value,
    read: &mut dyn FnMut(DeAdapter<'_, 'de>) -> Result<(), DeError>,
) -> Result<(), serde_json::Error> {
    deserialize_erased(value, read)
}

/// Deserializes `T` from a [`serde_json::Value`] through [`DeAdapter`],
/// keeping `serde_json::Value`'s erased deserializer compiled once in `cadmpeg-core`.
pub fn from_json_value<T: de::DeserializeOwned>(
    value: &serde_json::Value,
) -> Result<T, serde_json::Error> {
    let mut slot = None;
    with_json_value_deserializer(value, &mut |adapter| {
        slot = Some(T::deserialize(adapter)?);
        Ok(())
    })?;
    slot.ok_or_else(|| de::Error::custom("erased deserializer produced no value"))
}
