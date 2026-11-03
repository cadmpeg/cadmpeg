// SPDX-License-Identifier: Apache-2.0
//! Selected profile membership for persistent curve names.

use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::sketches::Sketch;

use super::ProfileIndex;

impl ProfileIndex<'_> {
    pub(in crate::feature) fn contains_curve_name(
        &self,
        token: &str,
        ordinal: u32,
        sketch: &Sketch,
        profile: &PlanarProfileRef,
        name: i64,
    ) -> bool {
        let Some(id) = self
            .curves
            .get(&(token, ordinal, name))
            .and_then(Option::as_deref)
        else {
            return false;
        };
        let Some(entity) = self.by_id.get(id) else {
            return false;
        };
        if entity.sketch != sketch.id || entity.construction {
            return false;
        }
        let added = self.added.borrow();
        let added = added.get(sketch.id.as_str()).map_or(&[][..], Vec::as_slice);
        contains_entity(profile, &sketch.id, id, |index| {
            let Ok(index) = usize::try_from(index) else {
                return false;
            };
            sketch
                .profiles
                .get(index)
                .or_else(|| added.get(index.checked_sub(sketch.profiles.len())?))
                .is_some_and(|ring| ring.iter().any(|use_| use_.entity.as_str() == id))
        })
    }
}

fn contains_entity(
    profile: &PlanarProfileRef,
    sketch_id: &cadmpeg_ir::sketches::SketchId,
    entity: &str,
    in_loop: impl Fn(u32) -> bool,
) -> bool {
    let PlanarProfileRef::SketchRegions { sketch, regions } = profile else {
        return false;
    };
    sketch == sketch_id
        && regions.iter().any(|region| match region {
            SketchProfileRegion::Loops { loops } => {
                in_loop(loops.outer()) || loops.holes().iter().any(|index| in_loop(*index))
            }
            SketchProfileRegion::Trimmed {
                outer_boundary,
                hole_boundaries,
            } => {
                outer_boundary
                    .iter()
                    .any(|use_| use_.entity.as_str() == entity)
                    || hole_boundaries
                        .iter()
                        .any(|ring| ring.iter().any(|use_| use_.entity.as_str() == entity))
            }
        })
}

#[cfg(test)]
mod tests;
