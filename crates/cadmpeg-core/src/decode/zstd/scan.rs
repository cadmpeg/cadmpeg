// SPDX-License-Identifier: Apache-2.0
//! Scan-budget admission around the shared Zstandard block reader.

use std::io::Read;

use super::{frame_header, ActiveFrame, FrameDecoder, BLOCK_BYTES, MAX_DECODER_WINDOW};
use crate::decode::{u64_from_index, View};
use crate::scan::{ScanBudget, StopReason};
use crate::CodecError;

const LITERAL_BYTES: u64 = 1024 * 1024;
const DECODER_STORAGE: u64 = 4 * 1024 * 1024;
const CHUNK: usize = 4096;

fn failure(error: &CodecError) -> StopReason {
    StopReason::Malformed(error.to_string())
}

pub(crate) fn decode(source: View<'_>, budget: &ScanBudget<'_>) -> Result<Vec<u8>, StopReason> {
    let mut input = source
        .child(source.start(), source.end())
        .ok_or_else(|| StopReason::Truncated("Zstandard input".into()))?;
    if input.is_empty() {
        return Err(StopReason::Truncated("Zstandard frame".into()));
    }
    let mut output = Vec::new();
    while !input.is_empty() {
        budget.check()?;
        let start = input.position();
        let whole = input
            .child(start, input.end())
            .ok_or_else(|| StopReason::Truncated("Zstandard frame".into()))?;
        let (mut body, facts) = frame_header(whole).map_err(|error| failure(&error))?;
        let Some(facts) = facts else {
            let size = usize::try_from(body.req_u32_le()?).map_err(|_| StopReason::SourceBytes)?;
            body.req_take(size)?;
            input.req_take(body.position() - start)?;
            continue;
        };
        if facts.window > MAX_DECODER_WINDOW {
            return Err(StopReason::Unsupported("Zstandard window".into()));
        }
        if facts
            .content_size
            .is_some_and(|size| size > budget.expansion_available())
        {
            return Err(budget.refuse(StopReason::ExpandedBytes));
        }
        let checksum = whole.window().get(4).is_some_and(|flags| flags & 4 != 0);
        loop {
            budget.check()?;
            let bytes = body.req_take(3)?;
            let header = View::u24_le_at(bytes, 0)
                .ok_or_else(|| StopReason::Truncated("Zstandard block header".into()))?;
            let kind = (header >> 1) & 3;
            let stored = match kind {
                1 => 1,
                3 => {
                    return Err(StopReason::Malformed(
                        "Zstandard reserved block type".into(),
                    ))
                }
                _ => usize::try_from(header >> 3).map_err(|_| StopReason::SourceBytes)?,
            };
            body.req_take(stored)?;
            if header & 1 != 0 {
                break;
            }
        }
        if checksum {
            body.req_take(4)?;
        }
        let frame = input.req_take_child(body.position() - start)?;
        let storage = facts
            .window
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(DECODER_STORAGE))
            .ok_or(StopReason::RetainedBytes)?;
        budget.retain(storage)?;
        let mut decoder = FrameDecoder::new();
        decoder.set_max_window_size(facts.window);
        let mut active = ActiveFrame {
            decoder,
            source: frame.window(),
            window: facts.window,
            block_maximum: facts.window.min(BLOCK_BYTES),
            content_size: facts.content_size,
            produced: 0,
            buffered: Some(0),
        };
        active
            .decoder
            .init(&mut active.source)
            .map_err(|error| failure(&super::zstd_error(&error, facts.window)))?;
        let mut reserved = 0_u64;
        while !active.decoder.is_finished() {
            budget.check()?;
            let header = View::u24_le_at(active.source, 0)
                .ok_or_else(|| StopReason::Truncated("Zstandard block header".into()))?;
            let kind = (header >> 1) & 3;
            let declared = u64::from(header >> 3);
            if declared > active.block_maximum {
                return Err(StopReason::Malformed("Zstandard block maximum".into()));
            }
            let allowance = if kind == 2 {
                LITERAL_BYTES
                    .checked_add(BLOCK_BYTES)
                    .ok_or(StopReason::ExpandedBytes)?
            } else {
                declared
            };
            budget.expand(allowance)?;
            reserved = reserved
                .checked_add(allowance)
                .ok_or(StopReason::ExpandedBytes)?;
            active.decode_block().map_err(|error| failure(&error))?;
            while active.decoder.can_collect() != 0 {
                budget.check()?;
                let capacity = active.decoder.can_collect().min(CHUNK);
                budget.retain(u64_from_index(capacity))?;
                output
                    .try_reserve_exact(capacity)
                    .map_err(|_| StopReason::RetainedBytes)?;
                let mut chunk = [0; CHUNK];
                let count = active
                    .decoder
                    .read(&mut chunk[..capacity])
                    .map_err(|error| StopReason::Malformed(error.to_string()))?;
                if count == 0 {
                    return Err(StopReason::Malformed("Zstandard output progress".into()));
                }
                active.produced = active
                    .produced
                    .checked_add(u64_from_index(count))
                    .ok_or(StopReason::ExpandedBytes)?;
                output.extend_from_slice(&chunk[..count]);
                if active
                    .content_size
                    .is_some_and(|declared| active.produced > declared)
                {
                    return Err(StopReason::Malformed("Zstandard content size".into()));
                }
            }
            // The decoder holds back at most one window of decoded output, so the
            // bytes decoded so far are at most `produced + window`. Return the
            // remainder of the block allowances now rather than at the end of the
            // frame.
            let decoded_bound = active.produced.saturating_add(active.window);
            if reserved > decoded_bound {
                budget.release_expansion(reserved - decoded_bound);
                reserved = decoded_bound;
            }
        }
        if let (Some(declared), Some(computed)) = (
            active.decoder.get_checksum_from_data(),
            active.decoder.get_calculated_checksum(),
        ) {
            if declared != computed {
                return Err(StopReason::Malformed("Zstandard checksum".into()));
            }
        }
        if active
            .content_size
            .is_some_and(|declared| declared != active.produced)
            || !active.source.is_empty()
        {
            return Err(StopReason::Malformed(
                "Zstandard frame extent or content size".into(),
            ));
        }
        budget.release_expansion(
            reserved
                .checked_sub(active.produced)
                .ok_or(StopReason::ExpandedBytes)?,
        );
    }
    Ok(output)
}
