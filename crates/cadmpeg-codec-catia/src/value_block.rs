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
}

/// Parse every exact `7C0B` value block immediately followed by `7C02`.
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<ValueBlock> {
    bytes
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x0b])
        .filter_map(|(pos, _)| parse_candidate(bytes, pos))
        .collect()
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
    })
}
