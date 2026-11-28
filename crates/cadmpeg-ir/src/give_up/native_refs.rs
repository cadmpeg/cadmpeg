// SPDX-License-Identifier: Apache-2.0
//! The `native_ref` values of a record, read from its serialization: each
//! names the native record that the neutral record projects (§2).

use serde::de::value::Error;
use serde::ser::{self, Serialize};

/// The key under which a record states the native record it projects.
const NATIVE_REF: &str = "native_ref";

/// Calls `found` with each text value that `value` states under a field or
/// a map key `native_ref`, at any depth.
///
/// # Errors
///
/// An error that the serialization of `value` raises.
pub(super) fn visit<T: Serialize + ?Sized>(
    value: &T,
    found: &mut dyn FnMut(&str),
) -> Result<(), Error> {
    value.serialize(Walker {
        found,
        keyed: false,
    })
}

/// A serializer of one value, which a `native_ref` key holds where `keyed`
/// is set.
struct Walker<'a, 'f> {
    found: &'a mut (dyn FnMut(&str) + 'f),
    keyed: bool,
}

/// A serializer of the members of one compound value.
struct Members<'a, 'f> {
    found: &'a mut (dyn FnMut(&str) + 'f),
    /// Whether the key of the map entry whose value comes next is
    /// `native_ref`.
    keyed: bool,
}

impl<'a, 'f> Walker<'a, 'f> {
    fn members(self) -> Members<'a, 'f> {
        Members {
            found: self.found,
            keyed: false,
        }
    }
}

impl<'a, 'f> ser::Serializer for Walker<'a, 'f> {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Members<'a, 'f>;
    type SerializeTuple = Members<'a, 'f>;
    type SerializeTupleStruct = Members<'a, 'f>;
    type SerializeTupleVariant = Members<'a, 'f>;
    type SerializeMap = Members<'a, 'f>;
    type SerializeStruct = Members<'a, 'f>;
    type SerializeStructVariant = Members<'a, 'f>;

    fn serialize_bool(self, _: bool) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_i8(self, _: i8) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_i16(self, _: i16) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_i32(self, _: i32) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_i64(self, _: i64) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_i128(self, _: i128) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_u8(self, _: u8) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_u16(self, _: u16) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_u32(self, _: u32) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_u64(self, _: u64) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_u128(self, _: u128) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_f32(self, _: f32) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_f64(self, _: f64) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<(), Error> {
        self.serialize_str(value.encode_utf8(&mut [0; 4]))
    }

    fn serialize_str(self, value: &str) -> Result<(), Error> {
        if self.keyed {
            (self.found)(value);
        }
        Ok(())
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(Walker {
            found: self.found,
            keyed: false,
        })
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_tuple(self, _: usize) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Members<'a, 'f>, Error> {
        Ok(self.members())
    }
}

impl Members<'_, '_> {
    /// Reads `value`, which the key `key` holds where there is one.
    fn member<T: Serialize + ?Sized>(&mut self, key: Option<&str>, value: &T) -> Result<(), Error> {
        value.serialize(Walker {
            found: &mut *self.found,
            keyed: key == Some(NATIVE_REF),
        })
    }
}

impl ser::SerializeSeq for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.member(None, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTuple for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.member(None, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.member(None, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.member(None, value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeMap for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        let mut keyed = false;
        let mut probe = |text: &str| keyed = text == NATIVE_REF;
        key.serialize(Walker {
            found: &mut probe,
            keyed: true,
        })?;
        self.keyed = keyed;
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let keyed = std::mem::take(&mut self.keyed);
        self.member(keyed.then_some(NATIVE_REF), value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.member(Some(key), value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for Members<'_, '_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.member(Some(key), value)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}
