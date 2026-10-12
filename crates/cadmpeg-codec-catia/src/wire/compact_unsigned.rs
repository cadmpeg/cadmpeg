//! Compact unsigned value and its exact source encoding.

use serde::{Deserialize, Serialize};

use crate::wire::bytes::compact_int;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CompactUnsignedEncoding {
    Inline,
    U8,
    U16,
    U24,
    U32,
}

/// A compact integer with its source encoding retained and range checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CompactUnsignedWire")]
pub(crate) struct CompactUnsigned {
    value: u32,
    encoding: CompactUnsignedEncoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactUnsignedWire {
    value: u32,
    encoding: CompactUnsignedEncoding,
}

impl TryFrom<CompactUnsignedWire> for CompactUnsigned {
    type Error = &'static str;

    fn try_from(wire: CompactUnsignedWire) -> Result<Self, Self::Error> {
        let maximum = match wire.encoding {
            CompactUnsignedEncoding::Inline => 63,
            CompactUnsignedEncoding::U8 => u32::from(u8::MAX),
            CompactUnsignedEncoding::U16 => u32::from(u16::MAX),
            CompactUnsignedEncoding::U24 => 0x00ff_ffff,
            CompactUnsignedEncoding::U32 => u32::MAX,
        };
        if wire.value > maximum {
            return Err("unsigned value does not fit its compact encoding");
        }
        Ok(Self {
            value: wire.value,
            encoding: wire.encoding,
        })
    }
}

impl CompactUnsigned {
    pub(crate) fn read(bytes: &[u8], at: &mut usize) -> Option<Self> {
        let start = *at;
        let value = compact_int(bytes, at)?;
        let encoding = match *at - start {
            1 => CompactUnsignedEncoding::Inline,
            2 => CompactUnsignedEncoding::U8,
            3 => CompactUnsignedEncoding::U16,
            4 => CompactUnsignedEncoding::U24,
            5 => CompactUnsignedEncoding::U32,
            _ => return None,
        };
        Some(Self { value, encoding })
    }

    pub(crate) const fn value(self) -> u32 {
        self.value
    }

    pub(crate) const fn byte_len(self) -> u8 {
        match self.encoding {
            CompactUnsignedEncoding::Inline => 1,
            CompactUnsignedEncoding::U8 => 2,
            CompactUnsignedEncoding::U16 => 3,
            CompactUnsignedEncoding::U24 => 4,
            CompactUnsignedEncoding::U32 => 5,
        }
    }
}
