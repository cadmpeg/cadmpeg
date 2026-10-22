// SPDX-License-Identifier: Apache-2.0
//! Given-up optional parts agree with their original physical archive.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::SourceFidelity;

pub(super) fn validate<'source>(
    ctx: &DecodeContext<'source>,
    root: View<'source>,
    archive: &ArchiveSnapshot<'source>,
    fidelity: &SourceFidelity,
) -> Result<(), CodecError> {
    if !fidelity.parts().values().any(|part| {
        matches!(part.state(), PartState::GivenUp {})
            && matches!(part.level().as_str(), "zip.entry" | "zip.local_header")
    }) {
        return Ok(());
    }
    let frames = archive.member_frames(ctx)?;
    let count = frames
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::malformed("ZIP part index size overflows"))?;
    let bytes = count
        .checked_mul(size_of::<(&str, u64, u64, View<'_>)>())
        .ok_or_else(|| CodecError::malformed("ZIP part index storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), "FCStd physical part index")?;
    let mut physical = ctx.alloc_filled(count, ("", 0, 0, root), "FCStd physical part index")?;
    let mut actual = 0;
    for frame in frames.into_iter().flatten() {
        for (level, view) in [
            ("zip.local_header", frame.header()),
            ("zip.entry", frame.payload()),
        ] {
            physical[actual] = (
                level,
                u64_from_index(view.start()),
                u64_from_index(view.window().len()),
                view,
            );
            actual += 1;
        }
    }
    physical.truncate(actual);
    physical
        .sort_unstable_by(|left, right| (left.0, left.1, left.2).cmp(&(right.0, right.1, right.2)));
    for (id, part) in fidelity.parts().iter().filter(|(_, part)| {
        part.state() == &PartState::GivenUp {}
            && matches!(part.level().as_str(), "zip.entry" | "zip.local_header")
    }) {
        let key = (part.level().as_str(), part.offset(), part.byte_len());
        let index = physical
            .binary_search_by(|item| (item.0, item.1, item.2).cmp(&key))
            .map_err(|_| {
                CodecError::malformed("given-up part extent disagrees with its physical ZIP entry")
            })?;
        checked(ctx, fidelity, id.as_str(), physical[index].3)?;
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn named<'a, 'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    fidelity: &'a SourceFidelity,
    id: &str,
    name: &str,
) -> Result<&'a [u8], CodecError> {
    let index = archive
        .entries()
        .iter()
        .position(|entry| entry.name == name)
        .ok_or_else(|| {
            CodecError::malformed(format_args!("original ZIP member {name} is absent"))
        })?;
    let frames = archive.member_frames(ctx)?;
    let frame = frames[index]
        .ok_or_else(|| CodecError::malformed("ZIP source part has no physical frame"))?;
    let part = fidelity
        .parts()
        .get(id)
        .ok_or_else(|| CodecError::malformed("ZIP source part is absent"))?;
    let extent = match part.level().as_str() {
        "zip.local_header" => frame.header(),
        "zip.entry" => frame.payload(),
        _ => {
            return Err(CodecError::malformed(
                "source part is not an optional ZIP member",
            ))
        }
    };
    checked(ctx, fidelity, id, extent)
}

fn checked<'a>(
    ctx: &DecodeContext<'_>,
    fidelity: &'a SourceFidelity,
    id: &str,
    extent: View<'_>,
) -> Result<&'a [u8], CodecError> {
    let part = fidelity
        .parts()
        .get(id)
        .ok_or_else(|| CodecError::malformed("ZIP source part is absent"))?;
    let actual = ctx.capture_part_extent(extent)?;
    let layout = fidelity
        .record_layout()
        .get(id)
        .ok_or_else(|| CodecError::malformed("ZIP source part has no layout"))?;
    let retained = fidelity
        .retained_record(id)
        .ok_or_else(|| CodecError::malformed("ZIP source part has no retained bytes"))?;
    if !matches!(part.state(), PartState::GivenUp {})
        || part.stream() != actual.stream()
        || part.offset() != actual.offset()
        || part.byte_len() != actual.byte_len()
        || layout.stream() != actual.stream()
        || layout.offset() != actual.offset()
        || layout.byte_len() != actual.byte_len()
        || retained.stream() != actual.stream()
        || retained.offset() != actual.offset()
        || retained.byte_len() != actual.byte_len()
    {
        return Err(CodecError::malformed(
            "given-up part extent disagrees with its physical ZIP entry",
        ));
    }
    let bytes = retained
        .data()
        .ok_or_else(|| CodecError::malformed("ZIP source part has no retained image"))?;
    if bytes != extent.window() {
        return Err(CodecError::malformed(
            "given-up part bytes disagree with its physical ZIP entry",
        ));
    }
    Ok(bytes)
}
