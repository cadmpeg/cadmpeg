// SPDX-License-Identifier: Apache-2.0
//! Preserve a complete exterior loop across subdivisions by other sketch lines.

use super::{
    bounds_meet, curve_arrangement_faces, CodecError, DecodeContext, ExcludedIsland, Point2,
    ProfileGeometryWork, SketchArrangementFace,
};
use std::collections::HashSet;

mod incidence;

pub(in crate::design) fn resolve(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    program: &[(cadmpeg_ir::sketches::SketchEntityUse, [u32; 2])],
    tolerance: f64,
    work: &ProfileGeometryWork,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<cadmpeg_ir::features::SketchProfileRegion>>, CodecError> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    if entities.iter().any(|entity| {
        entity.sketch == sketch.id
            && !entity.construction
            && !matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. } | SketchGeometryDefinition::Point { .. }
            )
    }) {
        return Ok(None);
    }
    let members = program
        .iter()
        .map(|(use_, _)| use_.clone())
        .collect::<Vec<_>>();
    let named = members
        .iter()
        .map(|use_| &use_.entity)
        .collect::<HashSet<_>>();
    let selected = entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && named.contains(entity.id()))
        .cloned()
        .collect::<Vec<_>>();
    if selected.len() != named.len() {
        return Ok(None);
    }
    let selected_faces = geometric!(curve_arrangement_faces(
        sketch, &selected, tolerance, work, ctx, true
    )?);
    let mut matching = selected_faces
        .iter()
        .filter(|face| face.ring.has_curves(&members));
    let selection = geometric!(matching.next());
    if matching.next().is_some()
        || !selection.ring.matches_members(&members)
        || selection
            .holes
            .as_ref()
            .is_none_or(|holes| !holes.is_empty())
        || selection.excluded_island != ExcludedIsland::Absent
    {
        return Ok(None);
    }
    if !incidence::validate(
        &selected,
        program,
        &selection.ring.boundary,
        tolerance,
        work,
        ctx,
    )? {
        return Ok(None);
    }
    let faces = geometric!(curve_arrangement_faces(
        sketch, entities, tolerance, work, ctx, true
    )?);
    let mut regions = Vec::new();
    for face in &faces {
        if !bounds_meet(face.bounds, selection.bounds, tolerance) {
            continue;
        }
        if face.holes.as_ref().is_none_or(|holes| !holes.is_empty())
            || face.excluded_island != ExcludedIsland::Absent
        {
            return Ok(None);
        }
        let Some(inside) = interior_side(face, selection, tolerance, work) else {
            return Ok(None);
        };
        if inside {
            regions.push(geometric!(
                cadmpeg_ir::features::SketchProfileRegion::trimmed(
                    face.ring.boundary.clone(),
                    Vec::new(),
                )
                .ok()
            ));
        }
    }
    Ok((!regions.is_empty()).then_some(regions))
}

fn interior_side(
    face: &SketchArrangementFace,
    selection: &SketchArrangementFace,
    tolerance: f64,
    work: &ProfileGeometryWork,
) -> Option<bool> {
    let n = face.polyline.len();
    if n < 3 {
        return None;
    }
    for index in 0..n {
        if !face
            .pieces
            .len()
            .checked_add(selection.pieces.len())
            .is_some_and(|count| work.arrangement.charge_by(count))
        {
            work.arrangement_skipped.set(true);
            return None;
        }
        let [a, b, c] = [
            face.polyline[index],
            face.polyline[(index + 1) % n],
            face.polyline[(index + 2) % n],
        ];
        let point = Point2::new(
            a.u / 3.0 + b.u / 3.0 + c.u / 3.0,
            a.v / 3.0 + b.v / 3.0 + c.v / 3.0,
        );
        if point.u.is_finite() && point.v.is_finite() && face.holds(point, tolerance) == Some(true)
        {
            if let Some(side) = selection.holds(point, tolerance) {
                return Some(side);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
