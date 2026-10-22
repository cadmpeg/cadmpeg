// SPDX-License-Identifier: Apache-2.0
//! Source ownership for compact edge-selection vectors.

use super::body_sources::BodySources;
use crate::records::{FeatureHistory, FeatureInputLane};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
    lane: &FeatureInputLane,
    sources: &mut BodySources<'_>,
) -> Result<(), CodecError> {
    if lane.edge_selections.is_empty() {
        return Ok(());
    }
    let source = sources.view(lane)?;
    let names = super::names::object_names(ctx, source.window(), &lane.id)?;
    let classes = super::names::class_declarations(source.window(), &lane.id)?;
    if names.len() != lane.names.len()
        || names.iter().zip(&lane.names).any(|(actual, staged)| {
            actual.id != staged.id
                || actual.parent != staged.parent
                || actual.ordinal != staged.ordinal
                || actual.offset != staged.offset
                || actual.object_id != staged.object_id
                || actual.value != staged.value
        })
        || classes.len() != lane.classes.len()
        || classes.iter().zip(&lane.classes).any(|(actual, staged)| {
            actual.id != staged.id
                || actual.parent != staged.parent
                || actual.ordinal != staged.ordinal
                || actual.offset != staged.offset
                || actual.name != staged.name
        })
    {
        return Err(CodecError::malformed(
            "edge-selection declarations differ from source",
        ));
    }
    let parsed = super::selections::compact_edge_selections(ctx, histories, lane)?;
    if parsed.records.len() != lane.edge_selections.len()
        || parsed
            .records
            .iter()
            .zip(&lane.edge_selections)
            .any(|(actual, staged)| {
                let mut actual = actual.clone();
                actual.sequence = staged.sequence;
                actual != *staged
            })
    {
        return Err(CodecError::malformed(
            "edge-selection content differs from source",
        ));
    }
    for record in &parsed.records {
        let frame = parsed
            .frames
            .get(&record.id)
            .ok_or_else(|| CodecError::malformed("edge-selection source frame is missing"))?;
        let start = source
            .start()
            .checked_add(frame.start)
            .ok_or_else(|| CodecError::malformed("edge-selection start overflow"))?;
        let end = source
            .start()
            .checked_add(frame.end)
            .ok_or_else(|| CodecError::malformed("edge-selection end overflow"))?;
        let extent = source
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("edge-selection frame exceeds source"))?;
        // The old locator addresses the marker; the owner starts at its count.
        let pointer = u64_from_index(
            frame
                .start
                .checked_add(12)
                .ok_or_else(|| CodecError::malformed("edge-selection marker overflow"))?,
        );
        if let Some(existing) = sources.pointer(&record.id) {
            if existing != pointer {
                return Err(CodecError::malformed(
                    "edge-selection locator changed during binding",
                ));
            }
            continue;
        }
        let feature = histories
            .iter()
            .flat_map(|history| &history.features)
            .find(|feature| feature.id == record.feature_ref)
            .ok_or_else(|| CodecError::malformed("edge-selection owner is absent"))?;
        ctx.record(&record.id, extent, || {
            let mut reader = extent;
            let bytes = reader
                .read_remaining()
                .ok_or_else(|| CodecError::malformed("edge-selection source is unavailable"))?;
            let ids = super::selections::compact_edge_selection_at(ctx, bytes, 12)?
                .ok_or_else(|| CodecError::malformed("edge-selection vector is invalid"))?;
            let components = super::selections::compact_edge_component_path_at(ctx, bytes, 12)?
                .unwrap_or_default();
            let references = super::selections::compact_edge_reference_list_for_feature(
                bytes,
                12,
                &feature.kind,
            )
            .unwrap_or_default();
            if ids != record.local_edge_ids
                || components != record.components
                || references != record.references
            {
                return Err(CodecError::malformed(
                    "tracked edge-selection values differ from staged content",
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
