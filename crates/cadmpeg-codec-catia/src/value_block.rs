// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C0B` value blocks.

use cadmpeg_ir::le::u32_at as u32_le;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One exact `7C0B` value block immediately preceding a schema catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ValueBlock {
    /// Byte offset of the `7C0B` marker.
    pub pos: usize,
    /// Stored length from the marker through the byte before the terminator.
    pub declared_len: usize,
    /// Complete extent including the trailing `0xFE` terminator.
    pub total_len: usize,
    /// Value payload between the six-byte header and terminator.
    pub payload: Vec<u8>,
    /// Lossless tokenization of the value payload.
    pub fields: Vec<ValueField>,
}

/// One token in a `7C0B` value payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
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
        /// Length code; the payload length is `code - E7`.
        code: u8,
        /// Exact inline bytes.
        #[serde(with = "cadmpeg_ir::bytes")]
        #[schemars(with = "String")]
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
    let candidates = bytes
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x0b])
        .filter_map(|(pos, _)| parse_candidate(bytes, pos))
        .collect::<Vec<_>>();
    let mut blocks = Vec::<ValueBlock>::new();
    for block in candidates {
        let block_end = block.pos + block.total_len;
        if blocks
            .iter()
            .any(|outer| outer.pos < block.pos && outer.pos + outer.total_len >= block_end)
        {
            continue;
        }
        blocks.push(block);
    }
    blocks
}

fn parse_candidate(bytes: &[u8], pos: usize) -> Option<ValueBlock> {
    let declared_len = usize::try_from(u32_le(bytes, pos + 2)?).ok()?;
    if declared_len < 6 {
        return None;
    }
    let terminator = pos.checked_add(declared_len)?;
    let next = terminator.checked_add(1)?;
    if bytes.get(terminator) != Some(&0xfe) || bytes.get(next..next + 2) != Some(&[0x7c, 0x02]) {
        return None;
    }
    Some(ValueBlock {
        pos,
        declared_len,
        total_len: declared_len + 1,
        payload: bytes[pos + 6..terminator].to_vec(),
        fields: tokenize(&bytes[pos + 6..terminator]),
    })
}

pub(crate) fn tokenize(payload: &[u8]) -> Vec<ValueField> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < payload.len() {
        let offset = at;
        if payload.get(at..at + 2) == Some(&[0x87, 0xe6]) && at + 10 <= payload.len() {
            let bits = u64::from_le_bytes(
                payload[at + 2..at + 10]
                    .try_into()
                    .expect("checked binary64 extent"),
            );
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
                    code,
                    bytes: payload[at + 3..end].to_vec(),
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
                ordinal: u32::from_le_bytes(
                    payload[at + 1..at + 5]
                        .try_into()
                        .expect("checked schema-reference extent"),
                ),
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
mod tests {
    use super::*;

    #[test]
    fn typed_payloads_hide_embedded_schema_marker_bytes() {
        let payload = [
            0x32, 5, 0, 0, 0, 0x87, 0xe6, 0, 0, 0, 0, 0, 0x32, 0, 0, 0x8e, 0xea, 0x84, 0x32, 1, 2,
            0x87, 0xe8,
        ];
        assert_eq!(
            tokenize(&payload),
            vec![
                ValueField::SchemaSelector {
                    ordinal: 5,
                    offset: 0,
                },
                ValueField::Binary64 {
                    bits: 0x0000_3200_0000_0000,
                    offset: 5,
                },
                ValueField::Inline {
                    code: 0xea,
                    bytes: vec![0x32, 1, 2],
                    offset: 15,
                },
                ValueField::Marker {
                    code: 0xe8,
                    offset: 21,
                },
            ]
        );
    }

    #[test]
    fn truncated_multi_byte_forms_remain_literal() {
        assert_eq!(
            tokenize(&[0x8e, 0xef, 0x84, 1]),
            vec![
                ValueField::Literal {
                    value: 0x8e,
                    offset: 0,
                },
                ValueField::Literal {
                    value: 0xef,
                    offset: 1,
                },
                ValueField::Atom {
                    value: 4,
                    width: 1,
                    offset: 2,
                },
                ValueField::Literal {
                    value: 1,
                    offset: 3,
                },
            ]
        );
    }

    #[test]
    fn untagged_value_opcodes_and_terminators_remain_distinct() {
        assert_eq!(
            tokenize(&[0xe6, 0xe7, 0xe8, 0xe9, 0xfe]),
            vec![
                ValueField::Opcode {
                    code: 0xe6,
                    offset: 0,
                },
                ValueField::Opcode {
                    code: 0xe7,
                    offset: 1,
                },
                ValueField::Opcode {
                    code: 0xe8,
                    offset: 2,
                },
                ValueField::Opcode {
                    code: 0xe9,
                    offset: 3,
                },
                ValueField::Terminator { offset: 4 },
            ]
        );
    }
}
