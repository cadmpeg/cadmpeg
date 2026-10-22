// SPDX-License-Identifier: Apache-2.0
//! The relation binding's declaration locator belongs to source fidelity.
//!
//! A binding is derived: its encoded owner is the exact class declaration.
//! This slice reads that declaration's family, not copies of scalar signatures.

use super::body_sources::BodySources;
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{FeatureInputLane, FeatureInputRelationFamily};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn capture(
    ctx: &DecodeContext<'_>,
    lane: &FeatureInputLane,
    sources: &mut BodySources<'_>,
) -> Result<(), CodecError> {
    if lane.relation_bindings.is_empty() {
        return Ok(());
    }
    let source = sources.view(lane)?;
    // Framing probe over the genuine source, with the parser's complete stable
    // class ID. A repeated physical name is not a locator or a unique owner.
    let declarations = super::names::class_declarations(source.window(), &lane.id)?;
    for binding in &lane.relation_bindings {
        let actual = declarations
            .iter()
            .find(|class| class.id == binding.class_ref)
            .ok_or_else(|| {
                CodecError::malformed("relation binding declaration is absent from source")
            })?;
        let position = actual
            .offset
            .and_then(|offset| usize::try_from(offset).ok())
            .ok_or_else(|| CodecError::malformed("relation declaration has no source locator"))?;
        let start = source
            .start()
            .checked_add(position)
            .ok_or_else(|| CodecError::malformed("relation declaration start overflow"))?;
        let len = 6usize
            .checked_add(actual.name.len())
            .ok_or_else(|| CodecError::malformed("relation declaration length overflow"))?;
        let extent = source
            .child(
                start,
                start
                    .checked_add(len)
                    .ok_or_else(|| CodecError::malformed("relation declaration end overflow"))?,
            )
            .ok_or_else(|| CodecError::malformed("relation declaration exceeds source"))?;
        let pointer = u64_from_index(
            extent
                .start()
                .checked_sub(source.start())
                .ok_or_else(|| CodecError::malformed("relation declaration precedes lane"))?,
        );
        if let Some(fields) = sources.pointers().get(&binding.id) {
            // Supplemental re-finalization changes derived feature ownership,
            // not the already captured immutable declaration or its locator.
            if fields.get("offset") != Some(&pointer)
                || family(&actual.name) != Some(binding.family)
            {
                return Err(CodecError::malformed(
                    "relation declaration changed during binding",
                ));
            }
            continue;
        }
        ctx.record(&binding.id, extent, || {
            let actual_family = read_family(extent)
                .ok_or_else(|| CodecError::malformed("relation declaration source is invalid"))?;
            if actual_family != binding.family {
                return Err(CodecError::malformed(
                    "relation declaration family differs from binding",
                ));
            }
            Ok(())
        })?;
        sources.insert(binding.id.clone(), pointer)?;
    }
    Ok(())
}

fn family(name: &str) -> Option<FeatureInputRelationFamily> {
    match native_object_class(name) {
        NativeClassKind::SketchRelation(family) => Some(family),
        _ => None,
    }
}

fn read_family(mut source: View<'_>) -> Option<FeatureInputRelationFamily> {
    if source.take(4)? != super::CLASS_MARKER {
        return None;
    }
    let len = usize::from(source.u16_le()?);
    if !(1..=128).contains(&len) {
        return None;
    }
    let name = source.take(len)?;
    if !source.is_empty()
        || !name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return None;
    }
    family(std::str::from_utf8(name).ok()?)
}

#[cfg(test)]
mod tests;
