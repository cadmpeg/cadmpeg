// SPDX-License-Identifier: Apache-2.0
//! Exact source ownership for feature-input class declarations.

use super::body_sources::BodySources;
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    sources: &mut BodySources<'_>,
) -> Result<(), CodecError> {
    let source = sources.view(lane)?;
    let parsed = super::names::class_declarations(source.window(), &lane.id)?;
    if parsed.len() != lane.classes.len()
        || parsed.iter().zip(&lane.classes).any(|(actual, staged)| {
            actual.id != staged.id
                || actual.parent != staged.parent
                || actual.ordinal != staged.ordinal
                || actual.name != staged.name
        })
    {
        return Err(CodecError::malformed(
            "class declaration content differs from source",
        ));
    }
    for (record, (relative, name)) in parsed
        .iter()
        .zip(super::names::payload_classes(source.window()))
    {
        if name != record.name {
            return Err(CodecError::malformed(
                "source class declaration order changed",
            ));
        }
        let start = source
            .start()
            .checked_add(relative)
            .ok_or_else(|| CodecError::malformed("class declaration start overflow"))?;
        let len = record
            .name
            .len()
            .checked_add(6)
            .ok_or_else(|| CodecError::malformed("class declaration length overflow"))?;
        let end = start
            .checked_add(len)
            .ok_or_else(|| CodecError::malformed("class declaration end overflow"))?;
        let extent = source
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("class declaration exceeds source"))?;
        let pointer = u64_from_index(relative);
        if let Some(existing) = sources.pointer(&record.id) {
            if existing != pointer {
                return Err(CodecError::malformed("class declaration locator changed"));
            }
            continue;
        }
        ctx.record(&record.id, extent, || {
            let mut reader = extent;
            if reader.take(4) != Some(super::CLASS_MARKER) {
                return Err(CodecError::malformed("class declaration marker is invalid"));
            }
            let len = reader
                .u16_le()
                .ok_or_else(|| CodecError::malformed("class declaration length is truncated"))?;
            let name = reader
                .take(usize::from(len))
                .ok_or_else(|| CodecError::malformed("class declaration name is truncated"))?;
            if !reader.is_empty() || name != record.name.as_bytes() {
                return Err(CodecError::malformed(
                    "tracked class name differs from staged content",
                ));
            }
            Ok(())
        })?;
        sources.insert_class(ctx, &record.id, pointer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
