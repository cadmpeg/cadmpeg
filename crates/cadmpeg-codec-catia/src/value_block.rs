// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C0B` value blocks.

use cadmpeg_core::decode::View;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::layout::value_block_7c0b as value_block;

/// One exact `7C0B` value block immediately preceding a schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ValueBlockWire", into = "ValueBlockWire")]
pub struct ValueBlock {
    /// Byte offset of the `7C0B` marker.
    pub pos: usize,
    /// Value payload between the six-byte header and terminator.
    pub payload: Vec<u8>,
}

impl ValueBlock {
    pub fn declared_len(&self) -> usize {
        value_block::LEN + self.payload.len()
    }

    pub fn total_len(&self) -> usize {
        self.declared_len() + 1
    }

    pub fn fields(&self) -> Vec<ValueField> {
        tokenize(&self.payload)
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ValueBlockWire {
    pos: usize,
    declared_len: usize,
    payload: Vec<u8>,
}

impl From<ValueBlock> for ValueBlockWire {
    fn from(block: ValueBlock) -> Self {
        Self {
            pos: block.pos,
            declared_len: block.declared_len(),
            payload: block.payload,
        }
    }
}

impl TryFrom<ValueBlockWire> for ValueBlock {
    type Error = &'static str;

    fn try_from(wire: ValueBlockWire) -> Result<Self, Self::Error> {
        let block = Self {
            pos: wire.pos,
            payload: wire.payload,
        };
        if wire.declared_len != block.declared_len() {
            return Err("value block length disagrees with payload");
        }
        Ok(block)
    }
}

/// One through eight inline bytes with a derived length code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "InlineBytesWire", into = "InlineBytesWire")]
pub struct InlineBytes(Vec<u8>);

impl InlineBytes {
    /// Exact inline bytes.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }
    /// Inline length code.
    pub fn code(&self) -> u8 {
        0xe7 + self.0.len() as u8
    }
}

impl TryFrom<Vec<u8>> for InlineBytes {
    type Error = &'static str;
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        if !(1..=8).contains(&bytes.len()) {
            return Err("bytes must contain one through eight inline bytes");
        }
        Ok(Self(bytes))
    }
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct InlineBytesWire {
    code: u8,
    #[serde(with = "cadmpeg_ir::bytes")]
    #[cfg_attr(feature = "schema", schemars(with = "String"))]
    bytes: Vec<u8>,
}

impl From<InlineBytes> for InlineBytesWire {
    fn from(value: InlineBytes) -> Self {
        Self {
            code: value.code(),
            bytes: value.0,
        }
    }
}
impl TryFrom<InlineBytesWire> for InlineBytes {
    type Error = &'static str;
    fn try_from(wire: InlineBytesWire) -> Result<Self, Self::Error> {
        let bytes = Self::try_from(wire.bytes)?;
        if wire.code != bytes.code() {
            return Err("code disagrees with inline bytes length");
        }
        Ok(bytes)
    }
}

/// One token in a `7C0B` value payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub enum ValueField {
    /// `0x32` followed by a source-schema ordinal or terminal absent sentinel.
    SchemaSelector {
        /// Source-schema entry ordinal or its terminal absent-schema sentinel.
        ordinal: u32,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `87 E6` followed by the exact IEEE-754 binary64 bits.
    Binary64 {
        /// Stored little-endian binary64 bits.
        bits: u64,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// Zero-payload `87 E7` or `87 E8` marker.
    Marker {
        /// Marker code, either `E7` or `E8`.
        code: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// One untagged value-program opcode in `E6..E9`.
    Opcode {
        /// Stored opcode byte.
        code: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `0x37` value-packet separator.
    Separator {
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `8E E8..EF 84` followed by one through eight inline bytes.
    Inline {
        /// Exact inline bytes and their derived length code.
        #[serde(flatten)]
        bytes: InlineBytes,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `E5 <length:u32le> <bytes[length]>` length-framed byte string.
    ByteString {
        /// Exact stored bytes.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[cfg_attr(feature = "schema", schemars(with = "String"))]
        bytes: Vec<u8>,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// Compact unsigned atom.
    Atom {
        /// Decoded unsigned value.
        value: u32,
        /// Stored width, one or two bytes.
        width: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `0xFE` value-program terminator.
    Terminator {
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// One byte outside the assigned multi-byte token forms.
    Literal {
        /// Exact stored byte.
        value: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
}

/// Parse every exact `7C0B` value block immediately followed by `7C02`.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<ValueBlock> {
    let mut blocks = Vec::<ValueBlock>::new();
    let mut enclosing_end = 0usize;
    for pos in memchr::memchr_iter(0x7c, bytes) {
        let Some(marker_tail) = pos.checked_add(1) else {
            continue;
        };
        if bytes.get(marker_tail) != Some(&0x0b) {
            continue;
        }
        let declared_end = pos
            .checked_add(value_block::DECLARED_LEN)
            .and_then(|length_offset| View::u32_le_at(bytes, length_offset))
            .and_then(|length| usize::try_from(length).ok())
            .and_then(|length| pos.checked_add(length))
            .and_then(|terminator| terminator.checked_add(1));
        if pos < enclosing_end && declared_end.is_some_and(|end| end <= enclosing_end) {
            continue;
        }
        let Some(block) = parse_candidate(bytes, pos) else {
            continue;
        };
        if let Some(block_end) = block.pos.checked_add(block.total_len()) {
            enclosing_end = enclosing_end.max(block_end);
        }
        blocks.push(block);
    }
    blocks
}

fn parse_candidate(bytes: &[u8], pos: usize) -> Option<ValueBlock> {
    let declared_len =
        usize::try_from(View::u32_le_at(bytes, pos + value_block::DECLARED_LEN)?).ok()?;
    if declared_len < value_block::LEN {
        return None;
    }
    let terminator = pos.checked_add(declared_len)?;
    let next = terminator.checked_add(1)?;
    if bytes.get(terminator) != Some(&0xfe) || bytes.get(next..next + 2) != Some(&[0x7c, 0x02]) {
        return None;
    }
    Some(ValueBlock {
        pos,
        payload: bytes[pos + value_block::LEN..terminator].to_vec(),
    })
}

pub(crate) fn tokenize(payload: &[u8]) -> Vec<ValueField> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < payload.len() {
        let offset = at;
        if payload.get(at..at + 2) == Some(&[0x87, 0xe6]) && at + 10 <= payload.len() {
            let bits = View::u64_le_at(payload, at + 2).expect("checked binary64 extent");
            fields.push(ValueField::Binary64 { bits, offset });
            at += 10;
        } else if payload.get(at) == Some(&0x87)
            && payload
                .get(at + 1)
                .is_some_and(|code| matches!(code, 0xe7 | 0xe8))
        {
            fields.push(ValueField::Marker {
                code: payload[at + 1],
                offset,
            });
            at += 2;
        } else if payload[at] == 0x37 {
            fields.push(ValueField::Separator { offset });
            at += 1;
        } else if payload
            .get(at)
            .is_some_and(|code| (0xe6..=0xe9).contains(code))
        {
            fields.push(ValueField::Opcode {
                code: payload[at],
                offset,
            });
            at += 1;
        } else if payload.get(at) == Some(&0x8e)
            && payload
                .get(at + 1)
                .is_some_and(|code| (0xe8..=0xef).contains(code))
            && payload.get(at + 2) == Some(&0x84)
        {
            let code = payload[at + 1];
            let len = usize::from(code - 0xe7);
            let end = at + 3 + len;
            if end <= payload.len() {
                fields.push(ValueField::Inline {
                    bytes: InlineBytes(payload[at + 3..end].to_vec()),
                    offset,
                });
                at = end;
            } else {
                fields.push(ValueField::Literal {
                    value: payload[at],
                    offset,
                });
                at += 1;
            }
        } else if payload.get(at) == Some(&0xe5) && at + 5 <= payload.len() {
            let len = usize::try_from(
                View::u32_le_at(payload, at + 1).expect("checked byte-string length extent"),
            )
            .ok();
            let end = len.and_then(|len| at.checked_add(5)?.checked_add(len));
            if let Some(end) = end.filter(|end| *end <= payload.len()) {
                fields.push(ValueField::ByteString {
                    bytes: payload[at + 5..end].to_vec(),
                    offset,
                });
                at = end;
            } else {
                fields.push(ValueField::Literal {
                    value: payload[at],
                    offset,
                });
                at += 1;
            }
        } else if payload.get(at) == Some(&0x32) && at + 5 <= payload.len() {
            fields.push(ValueField::SchemaSelector {
                ordinal: View::u32_le_at(payload, at + 1).expect("checked schema-reference extent"),
                offset,
            });
            at += 5;
        } else if payload
            .get(at)
            .is_some_and(|byte| (0x80..=0xd0).contains(byte))
        {
            fields.push(ValueField::Atom {
                value: u32::from(payload[at] - 0x80),
                width: 1,
                offset,
            });
            at += 1;
        } else if payload
            .get(at)
            .is_some_and(|byte| (0xd1..=0xe4).contains(byte))
            && at + 2 <= payload.len()
        {
            fields.push(ValueField::Atom {
                value: u32::from(payload[at] - 0xd1) * 256 + u32::from(payload[at + 1]) + 1,
                width: 2,
                offset,
            });
            at += 2;
        } else if payload[at] == 0xfe {
            fields.push(ValueField::Terminator { offset });
            at += 1;
        } else {
            fields.push(ValueField::Literal {
                value: payload[at],
                offset,
            });
            at += 1;
        }
    }
    fields
}

#[cfg(test)]
mod tests;
