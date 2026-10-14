// SPDX-License-Identifier: Apache-2.0
//! Structural serde values projected under the caller's depth account.

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;
use serde::ser::{
    SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Serialize, Serializer};
use serde_value::Value;
use std::collections::BTreeMap;
use std::marker::PhantomData;

/// A projected value.
#[derive(Debug)]
pub struct Projection<'ctx> {
    value: Value,
    _ctx: PhantomData<&'ctx ()>,
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
    let value = value
        .serialize(Projector { ctx, operation })
        .map_err(Error::into_codec)?;
    Ok(Projection {
        value,
        _ctx: PhantomData,
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
    fn into_codec(self) -> CodecError {
        match self {
            Self::Resource(error) => error,
            Self::Serde(error) => {
                CodecError::malformed(format_args!("structural projection failed: {error}"))
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Projector<'ctx, 'arena> {
    ctx: &'ctx DecodeContext<'arena>,
    operation: &'static str,
}
impl<'ctx, 'arena> Projector<'ctx, 'arena> {
    fn node(self) -> Result<DepthGuard<'ctx>, Error> {
        Ok(self.ctx.enter_nested(self.operation)?)
    }
    fn text(self, text: &str) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::String(text.to_owned()))
    }
    fn sequence(self, variant: Option<&'static str>) -> Result<Sequence<'ctx, 'arena>, Error> {
        let depth = self.node()?;
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
    fn map(self, variant: Option<&'static str>) -> Result<Object<'ctx, 'arena>, Error> {
        let depth = self.node()?;
        let variant_depth = variant.map(|_| self.node()).transpose()?;
        let variant = variant.map(|name| self.text(name)).transpose()?;
        Ok(Object {
            values: BTreeMap::new(),
            key: None,
            variant,
            projector: self,
            _depth: depth,
            _variant_depth: variant_depth,
        })
    }
}

fn tag_variant(key: Option<Value>, value: Value) -> Value {
    let Some(key) = key else {
        return value;
    };
    Value::Map(BTreeMap::from([(key, value)]))
}

macro_rules! project_scalar {
    ($($method:ident($type:ty) => $variant:ident),* $(,)?) => {$(
        fn $method(self, value: $type) -> Result<Value, Error> {
            let _depth = self.node()?;
            Ok(Value::$variant(value))
        }
    )*};
}
impl<'ctx, 'arena> Serializer for Projector<'ctx, 'arena> {
    type Ok = Value;
    type Error = Error;
    type SerializeSeq = Sequence<'ctx, 'arena>;
    type SerializeTuple = Sequence<'ctx, 'arena>;
    type SerializeTupleStruct = Sequence<'ctx, 'arena>;
    type SerializeTupleVariant = Sequence<'ctx, 'arena>;
    type SerializeMap = Object<'ctx, 'arena>;
    type SerializeStruct = Object<'ctx, 'arena>;
    type SerializeStructVariant = Object<'ctx, 'arena>;
    project_scalar! { serialize_bool(bool) => Bool, serialize_i8(i8) => I8, serialize_i16(i16) => I16, serialize_i32(i32) => I32, serialize_i64(i64) => I64, serialize_u8(u8) => U8, serialize_u16(u16) => U16, serialize_u32(u32) => U32, serialize_u64(u64) => U64, serialize_f32(f32) => F32, serialize_f64(f64) => F64, serialize_char(char) => Char }
    fn serialize_str(self, value: &str) -> Result<Value, Error> {
        self.text(value)
    }
    fn collect_str<T: std::fmt::Display + ?Sized>(self, value: &T) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::String(value.to_string()))
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::Bytes(value.to_vec()))
    }
    fn serialize_none(self) -> Result<Value, Error> {
        let _depth = self.node()?;
        Ok(Value::Option(None))
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
        let _depth = self.node()?;
        let value = value.serialize(self)?;
        Ok(Value::Option(Some(Box::new(value))))
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
        Ok(Value::Newtype(Box::new(value)))
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
        Ok(tag_variant(Some(key), value))
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        self.sequence(None)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        self.sequence(None)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.sequence(None)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        self.sequence(Some(variant))
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.map(None)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        self.map(None)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        self.map(Some(variant))
    }
}

struct Sequence<'ctx, 'arena> {
    values: Vec<Value>,
    variant: Option<Value>,
    projector: Projector<'ctx, 'arena>,
    _depth: DepthGuard<'ctx>,
    _variant_depth: Option<DepthGuard<'ctx>>,
}
impl Sequence<'_, '_> {
    fn field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let value = value.serialize(self.projector)?;
        self.values.push(value);
        Ok(())
    }
    fn finish(self) -> Value {
        tag_variant(self.variant, Value::Seq(self.values))
    }
}
macro_rules! sequence_impl {
    ($trait:ident, $field:ident) => {
        impl $trait for Sequence<'_, '_> {
            type Ok = Value;
            type Error = Error;
            fn $field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
                self.field(value)
            }
            fn end(self) -> Result<Value, Error> {
                Ok(self.finish())
            }
        }
    };
}
sequence_impl!(SerializeSeq, serialize_element);
sequence_impl!(SerializeTuple, serialize_element);
sequence_impl!(SerializeTupleStruct, serialize_field);
sequence_impl!(SerializeTupleVariant, serialize_field);

struct Object<'ctx, 'arena> {
    values: BTreeMap<Value, Value>,
    key: Option<Value>,
    variant: Option<Value>,
    projector: Projector<'ctx, 'arena>,
    _depth: DepthGuard<'ctx>,
    _variant_depth: Option<DepthGuard<'ctx>>,
}
impl Object<'_, '_> {
    fn field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Error> {
        let key = self.projector.text(key)?;
        let value = value.serialize(self.projector)?;
        self.values.insert(key, value);
        Ok(())
    }
    fn finish(self) -> Result<Value, Error> {
        if self.key.is_some() {
            return Err(serde::ser::Error::custom("structural map key has no value"));
        }
        Ok(tag_variant(self.variant, Value::Map(self.values)))
    }
}
impl SerializeMap for Object<'_, '_> {
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
        self.values.insert(key, value);
        Ok(())
    }
    fn end(self) -> Result<Value, Error> {
        self.finish()
    }
}
macro_rules! object_impl {
    ($trait:ident) => {
        impl $trait for Object<'_, '_> {
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
