// SPDX-License-Identifier: Apache-2.0
//! Identity-aware serialization for graph composition.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

use serde::ser::{
    Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant, Serializer,
};

/// Serialize an entity with rewritten typed identities, preserving ordinary text.
///
/// Each distinct identity is mapped once per serialization. Invalid identities
/// and mappings that collapse distinct identities are refused before a map can
/// discard an entry. Numeric values and unmarked map keys retain their shape.
/// The wrapped serializer determines the output format.
pub fn identities<'a, T: Serialize + ?Sized, F: Fn(&str) -> String + 'a>(
    value: &'a T,
    map: F,
) -> impl Serialize + 'a {
    Rewritten { value, map }
}

struct Rewritten<'a, T: ?Sized, F> {
    value: &'a T,
    map: F,
}

impl<T: Serialize + ?Sized, F: Fn(&str) -> String> Serialize for Rewritten<'_, T, F> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let guard = RewriteState {
            map: &self.map,
            targets: RefCell::new(BTreeMap::new()),
            occupied: RefCell::new(BTreeSet::new()),
            refused: RefCell::new(None),
        };
        let _scope = super::ReferenceWalkScope::enter();
        let result = self
            .value
            .serialize(IdentitySerializer::new(serializer, &guard));
        if let Some(error) = guard.refused.into_inner() {
            return Err(serde::ser::Error::custom(error));
        }
        result
    }
}

struct RewriteState<'a> {
    map: &'a dyn Fn(&str) -> String,
    targets: RefCell<BTreeMap<String, String>>,
    occupied: RefCell<BTreeSet<String>>,
    refused: RefCell<Option<String>>,
}

impl RewriteState<'_> {
    fn refuse<E: serde::ser::Error>(&self, message: String) -> E {
        self.refused
            .borrow_mut()
            .get_or_insert_with(|| message.clone());
        E::custom(message)
    }

    fn target<E: serde::ser::Error>(&self, source: &str) -> Result<String, E> {
        if let Some(target) = self.targets.borrow().get(source) {
            return Ok(target.clone());
        }
        let target = (self.map)(source);
        if !crate::ids::is_valid_identity(&target) {
            return Err(self.refuse(format!(
                "identity {source} rewrites to invalid identity {target:?}"
            )));
        }
        if !self.occupied.borrow_mut().insert(target.clone()) {
            return Err(self.refuse(format!(
                "identity {source} collides at rewritten identity {target}"
            )));
        }
        self.targets
            .borrow_mut()
            .insert(source.to_owned(), target.clone());
        Ok(target)
    }
}

/// Rewrites typed identity markers and forwards other values unchanged.
struct IdentitySerializer<'guard, S> {
    inner: S,
    guard: &'guard RewriteState<'guard>,
}

impl<'guard, S> IdentitySerializer<'guard, S> {
    /// Shares one identity mapping across all nested values.
    fn new(inner: S, guard: &'guard RewriteState<'guard>) -> Self {
        Self { inner, guard }
    }
}

/// Wraps a nested value so it serializes through the adapter as well.
struct IdentityValue<'guard, 'value, T: ?Sized> {
    value: &'value T,
    guard: &'guard RewriteState<'guard>,
}

impl<T: ?Sized + Serialize> Serialize for IdentityValue<'_, '_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value
            .serialize(IdentitySerializer::new(serializer, self.guard))
    }
}

/// Wraps a compound serializer so its elements serialize through the adapter.
struct IdentityCompound<'guard, C> {
    inner: C,
    guard: &'guard RewriteState<'guard>,
}

impl<'guard, C> IdentityCompound<'guard, C> {
    fn new(inner: C, guard: &'guard RewriteState<'guard>) -> Self {
        Self { inner, guard }
    }

    fn wrap<'value, T: ?Sized>(&self, value: &'value T) -> IdentityValue<'guard, 'value, T> {
        IdentityValue {
            value,
            guard: self.guard,
        }
    }
}

impl<'guard, S: Serializer> Serializer for IdentitySerializer<'guard, S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = IdentityCompound<'guard, S::SerializeSeq>;
    type SerializeTuple = IdentityCompound<'guard, S::SerializeTuple>;
    type SerializeTupleStruct = IdentityCompound<'guard, S::SerializeTupleStruct>;
    type SerializeTupleVariant = IdentityCompound<'guard, S::SerializeTupleVariant>;
    type SerializeMap = IdentityCompound<'guard, S::SerializeMap>;
    type SerializeStruct = IdentityCompound<'guard, S::SerializeStruct>;
    type SerializeStructVariant = IdentityCompound<'guard, S::SerializeStructVariant>;

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_f64(value)
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
        self.inner.serialize_f32(value)
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
        self.inner.serialize_some(&IdentityValue { value, guard })
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
        if name == super::REFERENCE_ID_MARKER {
            let value = serde_json::to_value(value).map_err(|error| {
                self.guard
                    .refuse::<S::Error>(format!("typed identity serialization failed: {error}"))
            })?;
            let Some(source) = value.as_str() else {
                return Err(self
                    .guard
                    .refuse("typed identity did not serialize as text".into()));
            };
            let target = self.guard.target::<S::Error>(source)?;
            self.inner.serialize_str(&target)
        } else {
            let guard = self.guard;
            self.inner
                .serialize_newtype_struct(name, &IdentityValue { value, guard })
        }
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
            .serialize_newtype_variant(name, index, variant, &IdentityValue { value, guard })
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_seq(len)
            .map(|inner| IdentityCompound::new(inner, guard))
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_tuple(len)
            .map(|inner| IdentityCompound::new(inner, guard))
    }

    fn serialize_tuple_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_tuple_struct(name, len)
            .map(|inner| IdentityCompound::new(inner, guard))
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
            .map(|inner| IdentityCompound::new(inner, guard))
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_map(len)
            .map(|inner| IdentityCompound::new(inner, guard))
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        let guard = self.guard;
        self.inner
            .serialize_struct(name, len)
            .map(|inner| IdentityCompound::new(inner, guard))
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
            .map(|inner| IdentityCompound::new(inner, guard))
    }

    fn collect_str<T: ?Sized + Display>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        self.inner.collect_str(value)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }
}

impl<C: SerializeSeq> SerializeSeq for IdentityCompound<'_, C> {
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

impl<C: SerializeTuple> SerializeTuple for IdentityCompound<'_, C> {
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

impl<C: SerializeTupleStruct> SerializeTupleStruct for IdentityCompound<'_, C> {
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

impl<C: SerializeTupleVariant> SerializeTupleVariant for IdentityCompound<'_, C> {
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

impl<C: SerializeMap> SerializeMap for IdentityCompound<'_, C> {
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

impl<C: SerializeStruct> SerializeStruct for IdentityCompound<'_, C> {
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

impl<C: SerializeStructVariant> SerializeStructVariant for IdentityCompound<'_, C> {
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

#[cfg(test)]
mod tests;
