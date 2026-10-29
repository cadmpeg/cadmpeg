// SPDX-License-Identifier: Apache-2.0
//! Framed entity revisions from the active carrier and its history archive.

use crate::kernel::{ActiveCarrier, KernelFamily};
use cadmpeg_asm::{asm_header, kernel_header::RefWidth, sab};
use cadmpeg_core::{
    decode::{DecodeContext, View},
    CodecError,
};

const DELTA_NAME: &[u8] = b"\x0d\x0bdelta_state";

fn tagged(view: &mut View<'_>, tag: u8, width: RefWidth) -> Result<i64, CodecError> {
    if view.req_u8()? != tag {
        return Err(CodecError::malformed("Inventor ASM history tag"));
    }
    Ok(match width {
        RefWidth::Four => i64::from(view.req_i32_le()?),
        RefWidth::Eight => view.req_i64_le()?,
    })
}

fn snapshot_start(mut view: View<'_>, width: RefWidth) -> Result<usize, CodecError> {
    loop {
        if view.req_take(DELTA_NAME.len())? != DELTA_NAME {
            return Err(CodecError::malformed("Inventor ASM delta name"));
        }
        tagged(&mut view, 4, width)?;
        if tagged(&mut view, 4, width)? != 1 || tagged(&mut view, 4, width)? != 0 {
            return Err(CodecError::malformed("Inventor ASM delta form"));
        }
        let previous = tagged(&mut view, 12, width)?;
        let next = tagged(&mut view, 12, width)?;
        let index = tagged(&mut view, 12, width)?;
        if previous < -1
            || next < -1
            || index < 0
            || tagged(&mut view, 12, width)? != -1
            || tagged(&mut view, 12, width)? != 0
            || view.req_u8()? != 11
        {
            return Err(CodecError::malformed("Inventor ASM delta links"));
        }
        while tagged(&mut view, 4, width)? != 0 {
            tagged(&mut view, 12, width)?;
            tagged(&mut view, 4, width)?;
            while tagged(&mut view, 4, width)? != 0 {
                let old = tagged(&mut view, 12, width)?;
                let new = tagged(&mut view, 12, width)?;
                if old < 0 && new < 0 {
                    return Err(CodecError::malformed("Inventor ASM empty change"));
                }
            }
        }
        if tagged(&mut view, 4, width)? != 0 || view.req_u8()? != 17 {
            return Err(CodecError::malformed("Inventor ASM delta end"));
        }
        if !view.unread().starts_with(DELTA_NAME) {
            return Ok(view.read_len());
        }
    }
}

pub(super) fn records(
    ctx: &DecodeContext<'_>,
    carrier: &ActiveCarrier<'_>,
) -> Result<(Vec<sab::Record>, bool), CodecError> {
    if carrier.family != KernelFamily::Asm {
        return Ok((Vec::new(), false));
    }
    let header = carrier.header.as_deref().map_err(CodecError::malformed)?;
    let bytes = carrier.bytes.window();
    let start = asm_header::record_stream_start_with_header(bytes, header)
        .ok_or_else(|| CodecError::malformed("Inventor ASM record start"))?;
    let mut records = sab::frame_history(ctx, bytes, start, bytes.len(), header.width, None)
        .map_err(|error| error.into_codec_error(ctx, CodecError::malformed))?;
    let Some(preamble) = records
        .last()
        .filter(|record| record.name == "Begin-of-ASM-History-Data")
    else {
        let complete = !header.metadata.has_history_partition()
            && records
                .last()
                .is_some_and(|record| record.offset.checked_add(record.len) == Some(bytes.len()));
        return Ok((records, complete));
    };
    let mut view = carrier.bytes;
    view.seek(view.start() + preamble.offset + preamble.len)
        .ok_or_else(|| CodecError::malformed("Inventor ASM history start"))?;
    let start = snapshot_start(view, header.width)?;
    records.pop();
    let base = records.len();
    let mut archive = sab::frame_history(ctx, bytes, start, bytes.len(), header.width, None)
        .map_err(|error| error.into_codec_error(ctx, CodecError::malformed))?;
    let starts_with_marker = archive.first().is_some_and(|record| {
        record.name == "End-of-ASM-History-Section"
            || (record.name == "edge"
                && matches!(record.tokens.first(), Some(sab::Token::Ident(name)) if name == "edge"))
    });
    let ends_with_marker = archive
        .last()
        .is_some_and(|record| record.name == "End-of-ASM-data");
    if !starts_with_marker || !ends_with_marker {
        return Err(CodecError::malformed("Inventor ASM archive bounds"));
    }
    archive.pop();
    for record in &mut archive {
        record.index += base;
    }
    records.extend(archive);
    Ok((records, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_state() -> Vec<u8> {
        let mut bytes = b"\x0d\x0bdelta_state".to_vec();
        for value in [9_i32, 1, 0] {
            bytes.push(4);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for reference in [-1_i32, -1, 0, -1, 0] {
            bytes.push(12);
            bytes.extend_from_slice(&reference.to_le_bytes());
        }
        bytes.push(11);
        for _ in 0..2 {
            bytes.push(4);
            bytes.extend_from_slice(&0_i32.to_le_bytes());
        }
        bytes.push(17);
        bytes
    }

    #[test]
    fn archive_starts_after_the_structured_tail_state() {
        let bytes = empty_state();
        assert_eq!(
            snapshot_start(View::over_retained(&bytes), RefWidth::Four)
                .expect("well-framed history fixture"),
            bytes.len()
        );
    }

    #[test]
    fn archive_positions_are_relative_to_the_carrier_window() {
        let state = empty_state();
        let mut bytes = vec![0; 5];
        bytes.extend_from_slice(&state);
        let mut root = View::over_retained(&bytes);
        root.req_take(5).expect("carrier prefix");
        let child = root.req_take_child(state.len()).expect("carrier window");
        assert_eq!(
            snapshot_start(child, RefWidth::Four).expect("well-framed history fixture"),
            state.len()
        );
    }

    #[test]
    fn truncated_and_unframed_history_cannot_supply_edge_names() {
        let bytes = empty_state();
        assert!(snapshot_start(
            View::over_retained(&bytes[..bytes.len() - 1]),
            RefWidth::Four
        )
        .is_err());
        assert!(snapshot_start(
            View::over_retained(b"INV_NMX_MATCHED_ATTRIB"),
            RefWidth::Four
        )
        .is_err());
    }
}
