// SPDX-License-Identifier: Apache-2.0
//! Detect Extrude selection groups whose picks lie in sketch text glyphs.

use std::collections::HashSet;

use super::ScopedExtrudeProfileResolution;
use crate::design::geometry::atomic_sketch_regions;
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::topology::extrude_selection::{
    DesignExtrudeSelectionGroup, DesignExtrudeSelectionMember,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::PlanarProfileRef;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::{Sketch, SketchGeometryDefinition};

/// The profile of an Extrude whose one selection group picks more distinct
/// regions than the carried curves of `sketch` bound, in a Sketch that holds
/// non-construction text. Each member of a group names a distinct profile
/// region, so the picks that exceed the carried regions lie in the text
/// glyphs, which the neutral sketch carries as text entities.
///
/// When the member count equals the glyph count of the Sketch text, the
/// profile names the text entities. Otherwise a Sketch with one profile loop
/// selects that loop, and any other Sketch keeps the group as its selection.
/// `None` when a member resolves to a Sketch point, a Sketch curve, or a
/// history binding, the Sketch holds a curve outside the line, arc, and
/// circle families, or the carried regions do not resolve.
pub(super) fn text_glyph_selection(
    feature: &str,
    sketch: &Sketch,
    groups: &[&DesignExtrudeSelectionGroup],
    members: &[DesignExtrudeSelectionMember],
    scoped: ScopedExtrudeProfileResolution<'_>,
) -> Result<Option<(PlanarProfileRef, LossNote)>, CodecError> {
    let [group] = groups else {
        return Ok(None);
    };
    let mut texts = Vec::new();
    let mut glyphs = 0_usize;
    let mut curves = false;
    for entity in scoped
        .entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && !entity.construction)
    {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Text { text, .. } => {
                texts.push(entity.id().clone());
                let count = text
                    .as_str()
                    .chars()
                    .filter(|character| !character.is_whitespace())
                    .count();
                glyphs = glyphs.checked_add(count).ok_or_else(|| {
                    CodecError::malformed("sketch text glyph count overflows usize")
                })?;
            }
            SketchGeometryDefinition::Point { .. } => {}
            SketchGeometryDefinition::Line { .. }
            | SketchGeometryDefinition::Arc { .. }
            | SketchGeometryDefinition::Circle { .. } => curves = true,
            _ => return Ok(None),
        }
    }
    if texts.is_empty() {
        return Ok(None);
    }
    let picks = members
        .iter()
        .filter(|member| {
            native_stream(&member.id) == native_stream(&group.id)
                && member.group_record_index == group.record_index
        })
        .collect::<Vec<_>>();
    if picks.len() != group.members().len()
        || picks
            .iter()
            .any(|member| member.resolved_geometry.is_some() || member.historical.is_some())
    {
        return Ok(None);
    }
    let distinct = picks
        .iter()
        .map(|member| member.local_id)
        .collect::<HashSet<_>>()
        .len();
    let carried = if curves {
        let Some(regions) = atomic_sketch_regions(
            sketch,
            scoped.entities,
            scoped.linear_tolerance,
            scoped.profile_work,
            scoped.ctx,
        )?
        else {
            return Ok(None);
        };
        regions.len()
    } else {
        0
    };
    if distinct <= carried {
        return Ok(None);
    }
    if distinct == glyphs {
        if let Ok(profile) = PlanarProfileRef::sketch_entities(sketch.id.clone(), texts) {
            let note = F3dLossCode::FeatureProfileTextGlyphsAssumed.note(format!(
                "Feature {feature} picks {distinct} profile regions in a Sketch whose curves bound {carried}; its profile names the Sketch text on the reading that each pick is one glyph."
            ));
            return Ok(Some((profile, note)));
        }
    }
    let profile = if sketch.profiles.len() == 1 {
        PlanarProfileRef::sketch_profiles(sketch.id.clone(), vec![0])
    } else {
        PlanarProfileRef::sketch_selection(sketch.id.clone(), vec![group.id.clone()])
    }
    .unwrap_or_else(|_| PlanarProfileRef::Native(group.id.clone()));
    let note = F3dLossCode::FeatureProfileTextGlyphsNative.note(format!(
        "Feature {feature} picks {distinct} profile regions in a Sketch whose curves bound {carried} and whose text has {glyphs} glyphs; its glyph picks stay native."
    ));
    Ok(Some((profile, note)))
}

#[cfg(test)]
mod tests;
