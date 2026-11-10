// SPDX-License-Identifier: Apache-2.0
//! Whole circular sections and complete selected path wires.

use cadmpeg_ir::{
    features::{PathRef, PlanarProfileRef},
    math::Point3,
    sketches::{Sketch, SketchGeometryDefinition},
};

use super::{
    LoopCurve, Piece, ProfileEdge, ProfileIndex, ProfileWire, SketchFrame,
    EPS_PROFILE_WIRE_POINT_MM,
};
use crate::feature::PmDcKernelEntityLink;

pub(crate) struct SelectedPath {
    pub(crate) reference: PathRef,
    pub(crate) names: Vec<i64>,
    /// The sketch curve each edge of `names` runs along in full, in model
    /// space. Empty when the path stays a native reference.
    pub(crate) curves: Vec<PathCurve>,
    pub(crate) endpoints: [Point3; 2],
}

/// The model-space geometry of the sketch curve a path edge runs along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PathCurve {
    Line { start: Point3, end: Point3 },
    Arc { center: Point3, radius: f64 },
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

    /// The unique non-construction curve of `sketch` whose full extent
    /// `edge` covers, as the piece of that curve.
    fn whole_piece<'p>(
        &'p self,
        frame: &SketchFrame,
        sketch: &Sketch,
        edge: &ProfileEdge,
    ) -> Option<Piece<'p>> {
        let mut matches = self.by_id.values().filter_map(|entity| {
            (entity.sketch == sketch.id
                && !entity.construction
                && !matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Point { .. }
                ))
            .then(|| Piece::new(frame, entity, edge))
            .flatten()
            .filter(|piece| piece.whole)
        });
        let piece = matches.next()?;
        matches.next().is_none().then_some(piece)
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
        let mut spans = Vec::new();
        for (sketch_ordinal, sketch) in sketches {
            let frame = SketchFrame::new(sketch)?;
            let named = names
                .iter()
                .any(|name| self.curves.contains_key(&(token, *sketch_ordinal, *name)));
            let curves = wire
                .edges
                .iter()
                .zip(&names)
                .map(|(edge, name)| {
                    if !named {
                        let piece = self.whole_piece(&frame, sketch, edge)?;
                        return Some((piece.entity.id().clone(), path_curve(&frame, &piece, edge)));
                    }
                    let id = self
                        .curves
                        .get(&(token, *sketch_ordinal, *name))?
                        .as_deref()?;
                    let entity = self.by_id.get(id)?;
                    let piece = Piece::new(&frame, entity, edge)?;
                    (entity.sketch == sketch.id && !entity.construction && piece.whole)
                        .then(|| (entity.id().clone(), path_curve(&frame, &piece, edge)))
                })
                .collect::<Option<Vec<_>>>();
            if let Some(found) = curves {
                if resolved {
                    return None;
                }
                let (ids, geometry): (Vec<_>, Vec<_>) = found.into_iter().unzip();
                reference = PathRef::SketchCurves {
                    sketch: sketch.id.clone(),
                    curves: ids.try_into().ok()?,
                };
                spans = geometry;
                resolved = true;
            }
        }
        Some(SelectedPath {
            reference,
            names,
            curves: spans,
            endpoints,
        })
    }
}

/// The model-space curve that `piece`, a whole sketch curve, lies on.
fn path_curve(frame: &SketchFrame, piece: &Piece<'_>, edge: &ProfileEdge) -> PathCurve {
    match piece.curve {
        LoopCurve::Line { .. } => PathCurve::Line {
            start: edge.start,
            end: edge.end,
        },
        LoopCurve::Arc { center, radius, .. } | LoopCurve::Circle { center, radius } => {
            PathCurve::Arc {
                center: frame.point(center),
                radius,
            }
        }
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
