// SPDX-License-Identifier: Apache-2.0
//! Source layouts for persistent generated-surface identity paths.

use super::body_sources::BodySources;
use crate::records::{FeatureInputClass, FeatureInputGeneratedSurfaceIdentity, FeatureInputLane};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    sources: &mut BodySources<'_>,
) -> Result<(), CodecError> {
    if lane.generated_surface_identities.is_empty() {
        return Ok(());
    }
    let source = sources.view(lane)?;
    let classes = super::names::class_declarations(source.window(), &lane.id)?;
    let class_sources = super::class_sources::ClassSources::from_source(ctx, source, &classes)?;
    let parsed = super::selections::generated_surface_identities(
        &class_sources,
        &lane.id,
        &classes,
        source.window(),
    )?;
    // Independently changed native content cannot acquire source evidence by
    // reparsing unrelated original bytes. Final admission must agree exactly.
    if parsed.records != lane.generated_surface_identities {
        return Err(CodecError::malformed(
            "generated surface identity content differs from source",
        ));
    }
    for record in &parsed.records {
        let frame = parsed
            .frames
            .get(&record.id)
            .ok_or_else(|| CodecError::malformed("generated surface frame is missing"))?;
        let start = source
            .start()
            .checked_add(frame.extent.start)
            .ok_or_else(|| CodecError::malformed("generated surface start overflow"))?;
        let end = source
            .start()
            .checked_add(frame.extent.end)
            .ok_or_else(|| CodecError::malformed("generated surface end overflow"))?;
        let extent = source
            .child(start, end)
            .ok_or_else(|| CodecError::malformed("generated surface frame exceeds source"))?;
        let pointer = u64_from_index(
            extent
                .start()
                .checked_sub(source.start())
                .ok_or_else(|| CodecError::malformed("generated surface precedes lane"))?,
        );
        if let Some(fields) = sources.pointers().get(&record.id) {
            if fields.get("offset") != Some(&pointer) {
                return Err(CodecError::malformed(
                    "generated surface locator changed during binding",
                ));
            }
            continue;
        }
        ctx.record(&record.id, extent, || {
            let prefix =
                read_prefix(source, &classes, record.type_prefix.as_ref()).ok_or_else(|| {
                    CodecError::malformed("generated surface type declaration is invalid")
                })?;
            let components =
                super::selections::inline_surface_source_components_at(source, frame.extent.start)
                    .and_then(Iterator::collect::<Option<Vec<_>>>)
                    .ok_or_else(|| {
                        CodecError::malformed("generated surface source path is invalid")
                    })?;
            let identity = source
                .read_at(
                    source
                        .start()
                        .checked_add(frame.identity_at)
                        .and_then(|position| position.checked_add(4))
                        .ok_or_else(|| {
                            CodecError::malformed("generated surface producer position overflow")
                        })?,
                    12,
                )
                .ok_or_else(|| {
                    CodecError::malformed("generated surface producer fields are unavailable")
                })?;
            let feature_source_id = View::u32_le_at(identity, 0)
                .and_then(|id| id.try_into().ok())
                .ok_or_else(|| {
                    CodecError::malformed("generated surface producer source is invalid")
                })?;
            let local_identity = View::u32_le_at(identity, 8).ok_or_else(|| {
                CodecError::malformed("generated surface local identity is unavailable")
            })?;
            let actual = FeatureInputGeneratedSurfaceIdentity {
                id: record.id.clone(),
                parent: record.parent.clone(),
                ordinal: record.ordinal,
                sequence: record.sequence,
                type_prefix: prefix.into(),
                feature_source_id,
                local_identity,
                components,
            };
            if actual != *record {
                return Err(CodecError::malformed(
                    "generated surface tracked values differ from staged content",
                ));
            }
            Ok(())
        })?;
        sources.insert(record.id.clone(), pointer)?;
    }
    Ok(())
}

fn read_prefix(
    source: View<'_>,
    classes: &[FeatureInputClass],
    expected: &[u8],
) -> Option<[u8; 4]> {
    for (class, (offset, _)) in classes
        .iter()
        .zip(super::names::payload_classes(source.window()))
        .filter(|(class, _)| class.name.ends_with("SurfIdRep_c"))
    {
        let body = offset.checked_add(6)?.checked_add(class.name.len())?;
        // Probe exact declaration framing before opening its tracked fields.
        if source.window().get(body..body.checked_add(2)?) != Some(&[0, 0])
            || source
                .window()
                .get(body.checked_add(2)?..body.checked_add(6)?)
                != Some(expected)
        {
            continue;
        }
        let len = 6usize.checked_add(class.name.len())?.checked_add(6)?;
        let bytes = source.read_at(source.start().checked_add(offset)?, len)?;
        if bytes.get(..4)? != super::CLASS_MARKER {
            return None;
        }
        let name_len = usize::from(View::u16_le_at(bytes, 4)?);
        let name_end = 6usize.checked_add(name_len)?;
        let name = std::str::from_utf8(bytes.get(6..name_end)?).ok()?;
        if !name.ends_with("SurfIdRep_c")
            || bytes.get(name_end..name_end.checked_add(2)?) != Some(&[0, 0])
        {
            return None;
        }
        let prefix: [u8; 4] = bytes
            .get(name_end.checked_add(2)?..name_end.checked_add(6)?)?
            .try_into()
            .ok()?;
        return (prefix.as_slice() == expected).then_some(prefix);
    }
    None
}

#[cfg(test)]
mod tests;
