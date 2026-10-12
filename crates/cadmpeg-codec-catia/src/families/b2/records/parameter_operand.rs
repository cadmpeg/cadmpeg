//! Compact operand identity and exact encoding of a parameter-space record.

use serde::{Deserialize, Serialize};

use crate::wire::bytes::compact_int;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ParameterOperandEncoding {
    Inline,
    U8,
    U16,
    U24,
    U32,
}

/// A compact integer with its source encoding retained and range checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ParameterOperandWire")]
pub(crate) struct B2ParameterOperand {
    value: u32,
    encoding: ParameterOperandEncoding,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParameterOperandWire {
    value: u32,
    encoding: ParameterOperandEncoding,
}

impl TryFrom<ParameterOperandWire> for B2ParameterOperand {
    type Error = &'static str;

    fn try_from(wire: ParameterOperandWire) -> Result<Self, Self::Error> {
        let maximum = match wire.encoding {
            ParameterOperandEncoding::Inline => 63,
            ParameterOperandEncoding::U8 => u32::from(u8::MAX),
            ParameterOperandEncoding::U16 => u32::from(u16::MAX),
            ParameterOperandEncoding::U24 => 0x00ff_ffff,
            ParameterOperandEncoding::U32 => u32::MAX,
        };
        if wire.value > maximum {
            return Err("parameter operand does not fit its compact encoding");
        }
        Ok(Self {
            value: wire.value,
            encoding: wire.encoding,
        })
    }
}

impl B2ParameterOperand {
    pub(super) fn read(bytes: &[u8], at: &mut usize) -> Option<Self> {
        let start = *at;
        let value = compact_int(bytes, at)?;
        let encoding = match *at - start {
            1 => ParameterOperandEncoding::Inline,
            2 => ParameterOperandEncoding::U8,
            3 => ParameterOperandEncoding::U16,
            4 => ParameterOperandEncoding::U24,
            5 => ParameterOperandEncoding::U32,
            _ => return None,
        };
        Some(Self { value, encoding })
    }

    pub(crate) const fn value(self) -> u32 {
        self.value
    }

    pub(crate) const fn byte_len(self) -> u8 {
        match self.encoding {
            ParameterOperandEncoding::Inline => 1,
            ParameterOperandEncoding::U8 => 2,
            ParameterOperandEncoding::U16 => 3,
            ParameterOperandEncoding::U24 => 4,
            ParameterOperandEncoding::U32 => 5,
        }
    }
}
