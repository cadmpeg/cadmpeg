// SPDX-License-Identifier: Apache-2.0
//! Original part and expanded member Views at the source producer.

use cadmpeg_container::compression::{inflate_zlib_member, inflate_zlib_member_if};
use cadmpeg_core::decode::{u64_from_index, ByteRange, DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

pub(super) fn part<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
    start: usize,
    end: usize,
) -> Result<View<'a>, CodecError> {
    let start = root
        .start()
        .checked_add(start)
        .ok_or_else(|| CodecError::malformed("NX part start overflows"))?;
    let end = root
        .start()
        .checked_add(end)
        .ok_or_else(|| CodecError::malformed("NX part end overflows"))?;
    let range = ByteRange {
        start: u64_from_index(start),
        end: u64_from_index(end),
    };
    if !root.has_source_tracking() {
        return ctx.register_slice(root, range);
    }
    let view = root
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("NX part leaves its actual root"))?;
    let extent = ctx.capture_part_extent(view)?;
    let name = format!(
        "nx:part:{}:{}:{}:{}",
        extent.stream().len(),
        extent.stream(),
        extent.offset(),
        extent.byte_len()
    );
    ctx.register_named_slice(root, range, &name)
}

pub(super) enum Candidate<'a> {
    Accepted { source: View<'a>, consumed: u64 },
    Rejected { consumed: u64 },
}

pub(super) fn inflate<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    admit: impl FnOnce(&[u8], u64) -> Result<bool, CodecError>,
) -> Result<Option<Candidate<'a>>, CodecError> {
    if source.has_source_tracking() {
        ctx.require_source(source)?;
    }
    let mut decoded = false;
    let mut completed = None;
    let result = if source.has_source_tracking() {
        inflate_zlib_member_if(ctx, source, ExpandSpec::Unknown, |bytes, consumed| {
            decoded = true;
            completed = Some(u64_from_index(consumed));
            admit(bytes, u64_from_index(consumed))
        })
    } else {
        inflate_zlib_member(ctx, source, ExpandSpec::Unknown).and_then(|(view, consumed)| {
            decoded = true;
            completed = Some(u64_from_index(consumed));
            Ok(admit(view.window(), u64_from_index(consumed))?.then_some((view, consumed)))
        })
    };
    match result {
        Ok(Some((source, consumed))) => Ok(Some(Candidate::Accepted {
            source,
            consumed: u64_from_index(consumed),
        })),
        Ok(None) => Ok(completed.map(|consumed| Candidate::Rejected { consumed })),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) if decoded => Err(error),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests;
