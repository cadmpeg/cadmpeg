// SPDX-License-Identifier: Apache-2.0
//! A forwarding `serde::Serializer` adapter that refuses a non-finite float.
//!
//! `serde_json` writes `NaN` and `±inf` as `null`, so a document carrying a
//! non-finite float digests as if it carried `null` and the digest is not total
//! over what the IR can hold. The adapter wraps a JSON serializer, refuses a
//! non-finite `f64` or `f32`, and delegates every other method unchanged.
//!
//! The refusal travels as the inner serializer's own error, because that is the
//! only error type a nested `Serialize` implementation can return. The refused
//! value is recorded in a [`FiniteGuard`] the whole walk shares, so the caller
//! reads back which float was refused.

use std::cell::Cell;
use std::fmt::Display;

use serde::ser::{
    Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// Records the float that a [`FiniteSerializer`] walk refused.
#[derive(Debug, Default)]
pub(super) struct FiniteGuard {
    refused: Cell<Option<f64>>,
}

impl FiniteGuard {
    /// Returns a guard that has refused nothing.
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Returns the refused float, if this walk refused one.
    pub(super) fn refused(&self) -> Option<f64> {
        self.refused.get()
    }

    /// Records `value` as refused and returns the error the walk carries.
    fn refuse<E: serde::ser::Error>(&self, value: f64) -> E {
        self.refused.set(Some(value));
        E::custom(format_args!("non-finite float {value}"))
    }
}

/// Serializes a value through `inner`, refusing every non-finite float.
pub(super) struct FiniteSerializer<'guard, S> {
    inner: S,
    guard: &'guard FiniteGuard,
}

impl<'guard, S> FiniteSerializer<'guard, S> {
    /// Returns an adapter over `inner` that reports refusals through `guard`.
    pub(super) fn new(inner: S, guard: &'guard FiniteGuard) -> Self {
        Self { inner, guard }
    }
}

/// Wraps a nested value so it serializes through the adapter as well.
struct FiniteValue<'guard, 'value, T: ?Sized> {
    value: &'value T,
    guard: &'guard FiniteGuard,
}

impl<T: ?Sized + Serialize> Serialize for FiniteValue<'_, '_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value
            .serialize(FiniteSerializer::new(serializer, self.guard))
    }
}

/// Wraps a compound serializer so its elements serialize through the adapter.
pub(super) struct FiniteCompound<'guard, C> {
    inner: C,
    guard: &'guard FiniteGuard,
}

impl<'guard, C> FiniteCompound<'guard, C> {
    fn new(inner: C, guard: &'guard FiniteGuard) -> Self {
        Self { inner, guard }
    }

    fn wrap<'value, T: ?Sized>(&self, value: &'value T) -> FiniteValue<'guard, 'value, T> {
        FiniteValue {
            value,
            guard: self.guard,
        }
    }
}

impl<'guard, S: Serializer> Serializer for FiniteSerializer<'guard, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = FiniteCompound<'guard, S::SerializeSeq>;
    type SerializeTuple = FiniteCompound<'guard, S::SerializeTuple>;
    type SerializeTupleStruct = FiniteCompound<'guard, S::SerializeTupleStruct>;
    type SerializeTupleVariant = FiniteCompound<'guard, S::SerializeTupleVariant>;
    type SerializeMap = FiniteCompound<'guard, S::SerializeMap>;
    type SerializeStruct = FiniteCompound<'guard, S::SerializeStruct>;
    type SerializeStructVariant = FiniteCompound<'guard, S::SerializeStructVariant>;

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        if value.is_finite() {
            self.inner.serialize_f64(value)
        } else {
            Err(self.guard.refuse(value))
        }
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        if value.is_finite() {
            self.inner.serialize_f32(value)
        } else {
            Err(self.guard.refuse(f64::from(value)))
        }
    }

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_bool(value)
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i8(value)
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i16(value)
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i32(value)
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i64(value)
    }

    fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_i128(value)
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u8(value)
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u16(value)
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u32(value)
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u64(value)
    }

    fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_u128(value)
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_char(value)
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_str(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_bytes(value)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_none()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let guard = self.guard;
        self.inner.serialize_some(&FiniteValue { value, guard })
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit()
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit_struct(name)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_unit_variant(name, index, variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_newtype_struct(name, &FiniteValue { value, guard })
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_newtype_variant(name, index, variant, &FiniteValue { value, guard })
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_seq(len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_tuple(len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_tuple_struct(name, len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_tuple_variant(name, index, variant, len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_map(len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_struct(name, len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_struct_variant(name, index, variant, len)
            .map(|inner| FiniteCompound::new(inner, guard))
    }

    fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.inner.collect_str(value)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<C: SerializeSeq> SerializeSeq for FiniteCompound<'_, C> {
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

impl<C: SerializeTuple> SerializeTuple for FiniteCompound<'_, C> {
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

impl<C: SerializeTupleStruct> SerializeTupleStruct for FiniteCompound<'_, C> {
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

impl<C: SerializeTupleVariant> SerializeTupleVariant for FiniteCompound<'_, C> {
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

impl<C: SerializeMap> SerializeMap for FiniteCompound<'_, C> {
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

impl<C: SerializeStruct> SerializeStruct for FiniteCompound<'_, C> {
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

impl<C: SerializeStructVariant> SerializeStructVariant for FiniteCompound<'_, C> {
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
