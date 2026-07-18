// SPDX-License-Identifier: Apache-2.0
//! Framed CATIA `7C0B` value blocks.
#![deny(clippy::disallowed_methods)]

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::decode::{DecodeContext, View};
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
///
/// The whole-image marker scan is charged once as `work`; each admitted payload
/// copy is charged against the `retained_bytes` budget before it is taken.
pub fn parse<'a>(ctx: &DecodeContext<'a>, view: View<'a>) -> Result<Vec<ValueBlock>, CodecError> {
    let bytes = view.window();
    ctx.charge_work(
        bytes.len() as u64,
        "catia_value_block_scan",
        Some(view.location()),
    )?;
    let mut blocks = ctx.grow_vec::<ValueBlock>();
    for pos in 0..bytes.len().saturating_sub(1) {
        if bytes[pos..pos + 2] != [0x7c, 0x0b] {
            continue;
        }
        if let Some(block) = parse_candidate(ctx, view, bytes, pos)? {
            blocks.try_push(block)?;
        }
    }
    Ok(blocks.finish())
}

fn parse_candidate<'a>(
    ctx: &DecodeContext<'a>,
    view: View<'a>,
    bytes: &[u8],
    pos: usize,
) -> Result<Option<ValueBlock>, CodecError> {
    let Some(declared_len) = u32_le(bytes, pos + 2).and_then(|len| usize::try_from(len).ok())
    else {
        return Ok(None);
    };
    if declared_len < 6 {
        return Ok(None);
    }
    let Some(terminator) = pos.checked_add(declared_len) else {
        return Ok(None);
    };
    let Some(next) = terminator.checked_add(1) else {
        return Ok(None);
    };
    if bytes.get(terminator) != Some(&0xfe) || bytes.get(next..next + 2) != Some(&[0x7c, 0x02][..])
    {
        return Ok(None);
    }
    // `declared_len >= 6` guarantees `terminator >= pos + 6`, so the payload
    // range is non-descending. Charge the retained copy before taking it.
    let payload_len = terminator - (pos + 6);
    ctx.charge_retained(
        payload_len as u64,
        "catia_value_block_payload",
        Some(view.location()),
    )?;
    Ok(Some(ValueBlock {
        pos,
        declared_len,
        total_len: declared_len + 1,
        payload: bytes[pos + 6..terminator].to_vec(),
    }))
}
