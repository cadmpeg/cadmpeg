// SPDX-License-Identifier: Apache-2.0
//! Actual scalar operand cells own their native reference source layouts.
//!
//! Scalar value/role source adoption remains separate. This boundary frames
//! cells once in the scalar parser and reads only their actual typed fields.

use super::body_sources::BodySources;
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    sources: &mut BodySources<'_>,
) -> Result<(), CodecError> {
    if lane.references.is_empty() {
        return Ok(());
    }
    let source = sources.view(lane)?;
    let names = super::names::object_names(ctx, source.window(), &lane.id)?;
    let classes = super::names::class_declarations(source.window(), &lane.id)?;
    let class_sources = super::class_sources::ClassSources::from_source(ctx, source, &classes)?;
    let scalars = super::scalars::named_scalars(source.window(), &lane.id, &names)?;
    let references = super::markers::reference_cells(&class_sources, &scalars.records, &classes)?;
    // feature_ref and sequence are derived by the binding/order passes, not
    // encoded in a cell. Compare every source-encoded fact and stable identity;
    // those later passes use this same proven cell locator for ownership.
    if references.records.len() != lane.references.len()
        || references
            .records
            .iter()
            .zip(&lane.references)
            .any(|(actual, staged)| {
                actual.id != staged.id
                    || actual.parent != staged.parent
                    || actual.ordinal != staged.ordinal
                    || actual.kind != staged.kind
                    || actual.class_ref != staged.class_ref
                    || actual.object_index != staged.object_index
            })
    {
        return Err(CodecError::malformed(
            "reference-cell content differs from source",
        ));
    }
    for record in &references.records {
        let frame = scalars
            .cell_frames
            .get(&record.id)
            .ok_or_else(|| CodecError::malformed("reference cell lacks its scalar parser frame"))?;
        let start = source
            .start()
            .checked_add(frame.start)
            .ok_or_else(|| CodecError::malformed("reference cell start overflow"))?;
        let end = source
            .start()
            .checked_add(frame.end)
            .ok_or_else(|| CodecError::malformed("reference cell end overflow"))?;
        let extent = source
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("reference cell exceeds source"))?;
        let pointer = u64_from_index(
            extent
                .start()
                .checked_sub(source.start())
                .ok_or_else(|| CodecError::malformed("reference cell precedes lane"))?,
        );
        if references.positions.get(&record.id) != Some(&pointer) {
            return Err(CodecError::malformed(
                "reference projection differs from actual scalar cell frame",
            ));
        }
        if let Some(fields) = sources.pointers().get(&record.id) {
            if fields.get("offset") != Some(&pointer) {
                return Err(CodecError::malformed(
                    "reference-cell locator changed during binding",
                ));
            }
            continue;
        }
        ctx.record(&record.id, extent, || {
            let mut cell = extent;
            let tag = cell
                .array::<2>()
                .ok_or_else(|| CodecError::malformed("reference family is truncated"))?;
            let kind = super::scalars::operand_kind(tag)
                .ok_or_else(|| CodecError::malformed("reference family is invalid"))?;
            let object_index = cell
                .u16_le()
                .ok_or_else(|| CodecError::malformed("reference index is truncated"))?;
            if cell.array::<4>() != Some([0xff; 4]) {
                return Err(CodecError::malformed("reference sentinel is invalid"));
            }
            if !cell.is_empty() && cell.array::<4>() != Some([0; 4]) {
                return Err(CodecError::malformed("reference trailer is invalid"));
            }
            if !cell.is_empty() || kind != record.kind || object_index != record.object_index {
                return Err(CodecError::malformed(
                    "tracked reference values differ from staged content",
                ));
            }
            Ok(())
        })?;
        sources.insert(record.id.clone(), pointer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
