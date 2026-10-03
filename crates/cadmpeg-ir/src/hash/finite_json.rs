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
//! value is recorded in a `FiniteGuard` the whole walk shares, so the caller
//! reads back which float was refused.

use std::cell::{Cell, RefCell};

use cadmpeg_core::decode::ResourceLimit;

mod admission;
use admission::{Admission, StandardAdmission};
use std::fmt::Display;

use serde::ser::{
    Error as _, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// Canonical JSON could not be written.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalJsonError {
    /// The caller's resource policy refused canonical ordering.
    #[error(transparent)]
    Resource(#[from] cadmpeg_core::CodecError),
    /// A float on the value is not finite, so canonical JSON cannot state it.
    #[error("canonical JSON holds no non-finite float: {value}")]
    NonFinite {
        /// The refused float.
        value: f64,
    },
    /// The value does not serialize as canonical JSON.
    #[error("canonical JSON serialization: {0}")]
    Serialize(serde_json::Error),
}

/// Writes `value` as canonical pretty JSON, refusing a non-finite float.
///
/// The bytes are the ones `serde_json::to_writer_pretty` produces. The float
/// refusal is the adapter's, not `serde_json`'s: `serde_json` writes a
/// non-finite float as `null`.
///
/// The guard is consulted on both arms. A `Serialize` implementation that
/// catches the adapter's refusal and completes would otherwise outrun it and
/// produce canonical JSON for a value the adapter had already refused.
pub(super) fn write_canonical_json<A: Admission, W: std::io::Write, T: Serialize + ?Sized>(
    admission: A,
    writer: W,
    value: &T,
) -> Result<(), CanonicalJsonError> {
    let guard = FiniteGuard::new(admission);
    let mut json = serde_json::Serializer::pretty(writer);
    let outcome = value.serialize(FiniteSerializer::new(&mut json, &guard));
    if let Some(limit) = guard.resource.borrow_mut().take() {
        return Err(CanonicalJsonError::Resource(limit.into()));
    }
    if let Some(value) = guard.refused() {
        return Err(CanonicalJsonError::NonFinite { value });
    }
    outcome.map_err(CanonicalJsonError::Serialize)
}

/// Renders `value` as canonical pretty JSON text, refusing a non-finite float.
///
/// This is the one route every production write of an IR document takes: the
/// CADIR encoder, `CadIr::to_canonical_json`, and the decode sidecar. A
/// document written any other way would spell a non-finite float `null`.
pub fn to_canonical_json_string<T: Serialize + ?Sized>(
    value: &T,
) -> Result<String, CanonicalJsonError> {
    let mut bytes = Vec::new();
    write_canonical_json(StandardAdmission, &mut bytes, value)?;
    String::from_utf8(bytes)
        .map_err(|error| CanonicalJsonError::Serialize(serde_json::Error::custom(error)))
}

/// Records the float that a [`FiniteSerializer`] walk refused.
struct FiniteGuard<A: Admission> {
    refused: Cell<Option<f64>>,
    admission: A,
    resource: RefCell<Option<ResourceLimit>>,
}

impl<A: Admission> FiniteGuard<A> {
    fn admit<T, E: serde::ser::Error>(&self, result: Result<T, A::Error>) -> Result<T, E> {
        result.map_err(|error| {
            if let Some(limit) = A::resource(error) {
                let mut refusal = self.resource.borrow_mut();
                if refusal.is_none() {
                    *refusal = Some(limit);
                }
            }
            E::custom("canonical JSON resource admission refused")
        })
    }

    fn enter<E: serde::ser::Error>(&self) -> Result<A::Depth<'_>, E> {
        self.admit(self.admission.enter())
    }

    fn text<E: serde::ser::Error>(&self, bytes: usize) -> Result<(), E> {
        self.admit(self.admission.text(bytes))
    }

    /// Returns a guard that has refused nothing.
    fn new(admission: A) -> Self {
        Self {
            refused: Cell::new(None),
            admission,
            resource: RefCell::new(None),
        }
    }

    /// Returns the refused float, if this walk refused one.
    fn refused(&self) -> Option<f64> {
        self.refused.get()
    }

    /// Records `value` as refused and returns the error the walk carries.
    fn refuse<E: serde::ser::Error>(&self, value: f64) -> E {
        self.refused.set(Some(value));
        E::custom(format_args!("non-finite float {value}"))
    }
}

/// Serializes a value through `inner`, refusing every non-finite float.
struct FiniteSerializer<'guard, S, A: Admission> {
    inner: S,
    guard: &'guard FiniteGuard<A>,
}

impl<'guard, S, A: Admission> FiniteSerializer<'guard, S, A> {
    /// Returns an adapter over `inner` that reports refusals through `guard`.
    fn new(inner: S, guard: &'guard FiniteGuard<A>) -> Self {
        Self { inner, guard }
    }
}

/// Wraps a nested value so it serializes through the adapter as well.
struct FiniteValue<'guard, 'value, T: ?Sized, A: Admission> {
    value: &'value T,
    guard: &'guard FiniteGuard<A>,
}

impl<T: ?Sized + Serialize, A: Admission> Serialize for FiniteValue<'_, '_, T, A> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value
            .serialize(FiniteSerializer::new(serializer, self.guard))
    }
}

/// Wraps a compound serializer so its elements serialize through the adapter.
struct FiniteCompound<'guard, C, A: Admission + 'guard> {
    inner: C,
    guard: &'guard FiniteGuard<A>,
    _depth: A::Depth<'guard>,
    _variant_depth: Option<A::Depth<'guard>>,
}

impl<'guard, C, A: Admission> FiniteCompound<'guard, C, A> {
    fn new(
        inner: C,
        guard: &'guard FiniteGuard<A>,
        depth: A::Depth<'guard>,
        variant_depth: Option<A::Depth<'guard>>,
    ) -> Self {
        Self {
            inner,
            guard,
            _depth: depth,
            _variant_depth: variant_depth,
        }
    }

    fn wrap<'value, T: ?Sized>(&self, value: &'value T) -> FiniteValue<'guard, 'value, T, A> {
        FiniteValue {
            value,
            guard: self.guard,
        }
    }
}

impl<'guard, S: Serializer, A: Admission> Serializer for FiniteSerializer<'guard, S, A> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = FiniteCompound<'guard, S::SerializeSeq, A>;
    type SerializeTuple = FiniteCompound<'guard, S::SerializeTuple, A>;
    type SerializeTupleStruct = FiniteCompound<'guard, S::SerializeTupleStruct, A>;
    type SerializeTupleVariant = FiniteCompound<'guard, S::SerializeTupleVariant, A>;
    type SerializeMap = FiniteCompound<'guard, S::SerializeMap, A>;
    type SerializeStruct = FiniteCompound<'guard, S::SerializeStruct, A>;
    type SerializeStructVariant = FiniteCompound<'guard, S::SerializeStructVariant, A>;

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        if value.is_finite() {
            self.inner.serialize_f64(value)
        } else {
            Err(self.guard.refuse(value))
        }
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        if value.is_finite() {
            self.inner.serialize_f32(value)
        } else {
            Err(self.guard.refuse(f64::from(value)))
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
        self.guard.text::<S::Error>(value.len())?;
        self.inner.serialize_str(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.guard.text::<S::Error>(value.len())?;
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
            .map(|inner| FiniteCompound::new(inner, guard, depth, Some(variant_depth)))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let depth = self.guard.enter::<S::Error>()?;
        let guard = self.guard;
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
            .map(|inner| FiniteCompound::new(inner, guard, depth, Some(variant_depth)))
    }

    fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        let _depth = self.guard.enter::<S::Error>()?;
        self.guard
            .admit(self.guard.admission.collect_str(self.inner, value))?
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<C: SerializeSeq, A: Admission> SerializeSeq for FiniteCompound<'_, C, A> {
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

impl<C: SerializeTuple, A: Admission> SerializeTuple for FiniteCompound<'_, C, A> {
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

impl<C: SerializeTupleStruct, A: Admission> SerializeTupleStruct for FiniteCompound<'_, C, A> {
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

impl<C: SerializeTupleVariant, A: Admission> SerializeTupleVariant for FiniteCompound<'_, C, A> {
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

impl<C: SerializeMap, A: Admission> SerializeMap for FiniteCompound<'_, C, A> {
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

impl<C: SerializeStruct, A: Admission> SerializeStruct for FiniteCompound<'_, C, A> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.guard.text::<C::Error>(name.len())?;
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

impl<C: SerializeStructVariant, A: Admission> SerializeStructVariant for FiniteCompound<'_, C, A> {
    type Ok = C::Ok;
    type Error = C::Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.guard.text::<C::Error>(name.len())?;
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
