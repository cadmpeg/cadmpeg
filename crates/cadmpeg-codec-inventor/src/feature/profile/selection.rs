// SPDX-License-Identifier: Apache-2.0
//! Whole circular sections and complete selected path wires.

use cadmpeg_ir::{
    features::{PathRef, PlanarProfileRef},
    math::Point3,
    sketches::{Sketch, SketchGeometryDefinition},
};

use super::{
    Piece, ProfileEdge, ProfileIndex, ProfileWire, SketchFrame, EPS_PROFILE_WIRE_POINT_MM,
};
use crate::feature::PmDcKernelEntityLink;

pub(crate) struct SelectedPath {
    pub(crate) reference: PathRef,
    pub(crate) names: Vec<i64>,
    pub(crate) endpoints: [Point3; 2],
}

impl ProfileIndex<'_> {
    pub(in crate::feature) fn whole_sketch(
        &self,
        token: &str,
        ordinal: u32,
        sketch: &Sketch,
    ) -> Option<PlanarProfileRef> {
        if self.incomplete_sketches.contains_key(&(token, ordinal)) {
            return None;
        }
        SketchFrame::new(sketch)?;
        let [profile] = sketch.profiles.as_slice() else {
            return None;
        };
        let curves = profile
            .iter()
            .map(|use_| {
                let entity = *self.by_id.get(use_.entity.as_str())?;
                (entity.sketch == sketch.id && !entity.construction).then_some(entity)
            })
            .collect::<Option<Vec<_>>>()?;
        let count = self
            .by_id
            .values()
            .filter(|entity| {
                entity.sketch == sketch.id
                    && !entity.construction
                    && !matches!(
                        entity.geometry.definition(),
                        SketchGeometryDefinition::Point { .. }
                    )
            })
            .count();
        if count != curves.len() {
            return None;
        }
        super::chain(&curves)?;
        PlanarProfileRef::sketch_regions(
            sketch.id.clone(),
            vec![cadmpeg_ir::features::SketchProfileRegion::loops(0, Vec::new()).ok()?],
        )
        .ok()
    }

    fn wire(&self, token: &str, ordinal: u32) -> Option<(&PmDcKernelEntityLink, &ProfileWire)> {
        let link = *self.kernel_links.get(&(token, ordinal))?;
        let body = self
            .bodies
            .get(&(token, link.body.record_ordinal()?))?
            .as_ref()?;
        let mut wires = body
            .wires
            .iter()
            .filter(|wire| wire.key == u64::from(link.entity_id));
        let wire = wires.next()?;
        if wires.next().is_some() || wire.edges.is_empty() {
            return None;
        }
        Some((link, wire))
    }

    pub(in crate::feature) fn circle(
        &self,
        token: &str,
        sketch_ordinal: u32,
        sketch: &Sketch,
        ordinal: u32,
    ) -> Option<(PlanarProfileRef, f64, i64)> {
        let (link, wire) = self.wire(token, ordinal)?;
        let first = wire.edges.first()?.name?;
        if first.naming_type != i64::from(link.entity_type)
            || !wire.edges.iter().all(|edge| edge.name == Some(first))
        {
            return None;
        }
        let id = self
            .curves
            .get(&(token, sketch_ordinal, first.associative_id))?
            .as_deref()?;
        let entity = self.by_id.get(id)?;
        let SketchGeometryDefinition::Circle { radius, .. } = entity.geometry.definition() else {
            return None;
        };
        Some((
            self.regions(token, sketch_ordinal, sketch, &[ordinal])?,
            radius.get(),
            first.associative_id,
        ))
    }

    pub(in crate::feature) fn selected_path(
        &self,
        token: &str,
        ordinal: u32,
        native: String,
        sketches: &[(u32, &Sketch)],
    ) -> Option<SelectedPath> {
        let (link, wire) = self.wire(token, ordinal)?;
        let names = wire
            .edges
            .iter()
            .map(|edge| {
                let name = edge.name?;
                (name.naming_type == i64::from(link.entity_type)).then_some(name.associative_id)
            })
            .collect::<Option<Vec<_>>>()?;
        let endpoints = path_endpoints(&wire.edges)?;
        let mut reference = PathRef::Native(native);
        let mut resolved = false;
        for (sketch_ordinal, sketch) in sketches {
            let frame = SketchFrame::new(sketch)?;
            let curves = wire
                .edges
                .iter()
                .zip(&names)
                .map(|(edge, name)| {
                    let id = self
                        .curves
                        .get(&(token, *sketch_ordinal, *name))?
                        .as_deref()?;
                    let entity = self.by_id.get(id)?;
                    (entity.sketch == sketch.id
                        && !entity.construction
                        && Piece::new(&frame, entity, edge)?.whole)
                        .then(|| entity.id().clone())
                })
                .collect::<Option<Vec<_>>>();
            if let Some(curves) = curves {
                if resolved {
                    return None;
                }
                reference = PathRef::SketchCurves {
                    sketch: sketch.id.clone(),
                    curves: curves.try_into().ok()?,
                };
                resolved = true;
            }
        }
        Some(SelectedPath {
            reference,
            names,
            endpoints,
        })
    }
}

fn path_endpoints(edges: &[ProfileEdge]) -> Option<[Point3; 2]> {
    let first = edges.first()?;
    for reversed in [false, true] {
        let start = if reversed { first.end } else { first.start };
        let mut at = if reversed { first.start } else { first.end };
        let connected = edges[1..].iter().all(|edge| {
            if near3(at, edge.start) {
                at = edge.end;
                true
            } else if near3(at, edge.end) {
                at = edge.start;
                true
            } else {
                false
            }
        });
        if connected && !near3(start, at) {
            return Some([start, at]);
        }
    }
    None
}

fn near3(left: Point3, right: Point3) -> bool {
    (left.x - right.x)
        .hypot(left.y - right.y)
        .hypot(left.z - right.z)
        <= EPS_PROFILE_WIRE_POINT_MM
}

#[cfg(test)]
mod tests;
