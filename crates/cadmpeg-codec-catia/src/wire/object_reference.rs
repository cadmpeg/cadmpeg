// SPDX-License-Identifier: Apache-2.0
//! Object reference with its exact sparse-byte encoding retained.

use serde::{Deserialize, Serialize};

use crate::wire::tokens::object_ref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ObjectReferenceEncoding {
    Inline,
    U8,
    ShiftedU8,
    U16,
    HighU8,
    LowHighU8,
    ShiftedU16,
    U24,
}

/// A source object reference whose value fits the retained addressing form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ObjectReferenceWire")]
pub(crate) struct ObjectReference {
    value: u32,
    encoding: ObjectReferenceEncoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectReferenceWire {
    value: u32,
    encoding: ObjectReferenceEncoding,
}

impl TryFrom<ObjectReferenceWire> for ObjectReference {
    type Error = &'static str;

    fn try_from(wire: ObjectReferenceWire) -> Result<Self, Self::Error> {
        let valid = match wire.encoding {
            ObjectReferenceEncoding::Inline => wire.value <= 127,
            ObjectReferenceEncoding::U8 => u8::try_from(wire.value).is_ok(),
            ObjectReferenceEncoding::ShiftedU8 => wire.value & !0x0000_ff00 == 0,
            ObjectReferenceEncoding::U16 => u16::try_from(wire.value).is_ok(),
            ObjectReferenceEncoding::HighU8 => wire.value & !0x00ff_0000 == 0,
            ObjectReferenceEncoding::LowHighU8 => wire.value & !0x00ff_00ff == 0,
            ObjectReferenceEncoding::ShiftedU16 => wire.value & !0x00ff_ff00 == 0,
            ObjectReferenceEncoding::U24 => wire.value <= 0x00ff_ffff,
        };
        if !valid {
            return Err("object reference does not fit its source encoding");
        }
        Ok(Self {
            value: wire.value,
            encoding: wire.encoding,
        })
    }
}

impl ObjectReference {
    pub(crate) fn read(bytes: &[u8], at: &mut usize) -> Option<Self> {
        let encoding = match *bytes.get(*at)? {
            0x80..=0xff => ObjectReferenceEncoding::Inline,
            0x08 => ObjectReferenceEncoding::U8,
            0x10 => ObjectReferenceEncoding::ShiftedU8,
            0x18 => ObjectReferenceEncoding::U16,
            0x20 => ObjectReferenceEncoding::HighU8,
            0x28 => ObjectReferenceEncoding::LowHighU8,
            0x30 => ObjectReferenceEncoding::ShiftedU16,
            0x38 => ObjectReferenceEncoding::U24,
            _ => return None,
        };
        let value = object_ref(bytes, at, true)?;
        Some(Self { value, encoding })
    }

    pub(crate) const fn value(self) -> u32 {
        self.value
    }

    #[cfg(test)]
    pub(crate) const fn byte_len(self) -> u8 {
        match self.encoding {
            ObjectReferenceEncoding::Inline => 1,
            ObjectReferenceEncoding::U8
            | ObjectReferenceEncoding::ShiftedU8
            | ObjectReferenceEncoding::HighU8 => 2,
            ObjectReferenceEncoding::U16
            | ObjectReferenceEncoding::LowHighU8
            | ObjectReferenceEncoding::ShiftedU16 => 3,
            ObjectReferenceEncoding::U24 => 4,
        }
    }
}
