// SPDX-License-Identifier: Apache-2.0
//! Whole circular sections and complete selected path wires.

use cadmpeg_ir::features::profiles::{PathRef, PlanarProfileRef};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchGeometryDefinition};

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
    /// The edges in order from `endpoints[0]`, each with its model-space curve
    /// and ends. Empty when `curves` is.
    pub(crate) spans: Vec<PathSpan>,
    /// Whether consecutive edges meet with the same tangent line and run
    /// onward on opposite sides of the joint.
    pub(crate) tangent: bool,
}

/// One edge of a sweep path, run from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PathSpan {
    pub(crate) name: i64,
    pub(crate) curve: SweepCurve,
    pub(crate) from: Point3,
    pub(crate) to: Point3,
    /// The curve point halfway between the ends of an arc.
    pub(crate) middle: Option<Point3>,
}

/// The closed wire of a sweep section.
pub(crate) struct SectionWire {
    /// Every edge of the wire in order, with the associative name of its
    /// sketch curve. An edge cut by other curves repeats its name.
    pub(crate) curves: Vec<(i64, SweepCurve)>,
    /// A point of the sketch plane and the plane normal.
    pub(crate) plane: (Point3, Vector3),
}

/// A line or arc of a sweep section or path in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SweepCurve {
    Line {
        start: Point3,
        end: Point3,
    },
    /// An arc or full circle about `center`, in the plane with `normal`.
    Arc {
        center: Point3,
        radius: f64,
        normal: Vector3,
    },
}

/// Largest norm of the sum of the two unit tangents that meet at a joint:
/// the sine of the corner angle for a tangent-continuous pair.
const EPS_PATH_TANGENT: f64 = 1.0e-6;

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
            vec![cadmpeg_ir::features::profiles::SketchProfileRegion::loops(0, Vec::new()).ok()?],
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

    /// The whole sketch curves that the named edges of the wire `ordinal`
    /// run along, in wire order, when they form one open path of `sketch`.
    /// An unnamed edge between two named edges selects no sketch curve. The
    /// first and last edges are named.
    pub(in crate::feature) fn named_open_path(
        &self,
        token: &str,
        ordinal: u32,
        sketch_ordinal: u32,
        sketch: &Sketch,
    ) -> Option<Vec<cadmpeg_ir::sketches::SketchEntityId>> {
        if self
            .incomplete_sketches
            .contains_key(&(token, sketch_ordinal))
        {
            return None;
        }
        let (link, wire) = self.wire(token, ordinal)?;
        if wire.edges.first()?.name.is_none() || wire.edges.last()?.name.is_none() {
            return None;
        }
        let mut names: Vec<i64> = Vec::new();
        for name in wire.edges.iter().filter_map(|edge| edge.name) {
            if name.naming_type != i64::from(link.entity_type) {
                return None;
            }
            if names.last() != Some(&name.associative_id) {
                names.push(name.associative_id);
            }
        }
        let curves = names
            .iter()
            .map(|name| {
                let id = self
                    .curves
                    .get(&(token, sketch_ordinal, *name))?
                    .as_deref()?;
                let entity = *self.by_id.get(id)?;
                (entity.sketch == sketch.id && !entity.construction).then_some(entity)
            })
            .collect::<Option<Vec<_>>>()?;
        let distinct = curves
            .iter()
            .map(|entity| entity.id())
            .collect::<std::collections::HashSet<_>>();
        if distinct.len() != curves.len() || !super::open_chain(&curves) {
            return None;
        }
        Some(curves.iter().map(|entity| entity.id().clone()).collect())
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
        let mut oriented_frames = Vec::new();
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
                        return Some((
                            piece.entity.id().clone(),
                            path_curve(&frame, &piece, edge),
                            sweep_curve(&frame, &piece, edge),
                        ));
                    }
                    let id = self
                        .curves
                        .get(&(token, *sketch_ordinal, *name))?
                        .as_deref()?;
                    let entity = self.by_id.get(id)?;
                    let piece = Piece::new(&frame, entity, edge)?;
                    (entity.sketch == sketch.id && !entity.construction && piece.whole).then(|| {
                        (
                            entity.id().clone(),
                            path_curve(&frame, &piece, edge),
                            sweep_curve(&frame, &piece, edge),
                        )
                    })
                })
                .collect::<Option<Vec<_>>>();
            if let Some(found) = curves {
                if resolved {
                    return None;
                }
                let (ids, geometry, frames): (Vec<_>, Vec<_>, Vec<_>) =
                    found
                        .into_iter()
                        .fold(Default::default(), |mut acc, (a, b, c)| {
                            acc.0.push(a);
                            acc.1.push(b);
                            acc.2.push(c);
                            acc
                        });
                oriented_frames = frames;
                reference = PathRef::SketchCurves {
                    sketch: sketch.id.clone(),
                    curves: ids.try_into().ok()?,
                };
                spans = geometry;
                resolved = true;
            }
        }
        let oriented = oriented_frames;
        let tangent = !oriented.is_empty() && tangent_continuous(&wire.edges, &oriented);
        let mut path_spans = Vec::new();
        let mut at = endpoints[0];
        if !oriented.is_empty() {
            for ((edge, name), curve) in wire.edges.iter().zip(&names).zip(&oriented) {
                let (from, to) = if near3(at, edge.start) {
                    (edge.start, edge.end)
                } else {
                    (edge.end, edge.start)
                };
                path_spans.push(PathSpan {
                    name: *name,
                    curve: *curve,
                    from,
                    to,
                    middle: edge.middle,
                });
                at = to;
            }
        }
        Some(SelectedPath {
            reference,
            names,
            curves: spans,
            endpoints,
            spans: path_spans,
            tangent,
        })
    }

    /// The sketch curves that the named edges of the closed wire `ordinal`
    /// run along, in wire order, with each edge's associative name and
    /// model-space geometry, and the sketch plane. Edges of one name lie on one
    /// sketch curve, and each sketch curve has one name. The edges form one
    /// closed chain.
    pub(in crate::feature) fn section_curves(
        &self,
        token: &str,
        ordinal: u32,
        sketch_ordinal: u32,
        sketch: &Sketch,
    ) -> Option<SectionWire> {
        if self
            .incomplete_sketches
            .contains_key(&(token, sketch_ordinal))
        {
            return None;
        }
        let (link, wire) = self.wire(token, ordinal)?;
        let frame = SketchFrame::new(sketch)?;
        if !closed_chain(&wire.edges) {
            return None;
        }
        let names = wire
            .edges
            .iter()
            .map(|edge| {
                let name = edge.name?;
                (name.naming_type == i64::from(link.entity_type)).then_some(name.associative_id)
            })
            .collect::<Option<Vec<_>>>()?;
        // When no edge name selects a curve link, the names are those of the
        // generated faces and each edge selects the whole curve it runs along.
        let named = names
            .iter()
            .any(|name| self.curves.contains_key(&(token, sketch_ordinal, *name)));
        let mut curves: Vec<(i64, SweepCurve)> = Vec::new();
        let mut owners: Vec<(i64, &SketchEntityId)> = Vec::new();
        for (edge, name) in wire.edges.iter().zip(&names) {
            let piece = if named {
                let id = self
                    .curves
                    .get(&(token, sketch_ordinal, *name))?
                    .as_deref()?;
                Piece::new(&frame, self.by_id.get(id)?, edge)?
            } else {
                self.whole_piece(&frame, sketch, edge)?
            };
            let entity = piece.entity;
            if entity.sketch != sketch.id || entity.construction {
                return None;
            }
            // An edge can be a part of its sketch curve: the curve is split
            // where other curves cross it. The edges of one name lie on one
            // sketch curve, and the curve generates the same surface.
            match owners.iter().find(|(seen, _)| seen == name) {
                Some((_, owner)) => {
                    if *owner != entity.id() {
                        return None;
                    }
                }
                None => {
                    if owners.iter().any(|(_, owner)| *owner == entity.id()) {
                        return None;
                    }
                    owners.push((*name, entity.id()));
                }
            }
            curves.push((*name, sweep_curve(&frame, &piece, edge)));
        }
        Some(SectionWire {
            curves,
            plane: (frame.origin, frame.normal),
        })
    }
}

/// The model-space curve of `edge` on the sketch curve of `piece`.
fn sweep_curve(frame: &SketchFrame, piece: &Piece<'_>, edge: &ProfileEdge) -> SweepCurve {
    match piece.curve {
        LoopCurve::Line { .. } => SweepCurve::Line {
            start: edge.start,
            end: edge.end,
        },
        LoopCurve::Arc { center, radius, .. } | LoopCurve::Circle { center, radius } => {
            SweepCurve::Arc {
                center: frame.point(center),
                radius,
                normal: frame.normal,
            }
        }
    }
}

/// Whether the edges, in any direction each, form one closed chain in order.
fn closed_chain(edges: &[ProfileEdge]) -> bool {
    let Some(first) = edges.first() else {
        return false;
    };
    [false, true].into_iter().any(|reversed| {
        let (start, mut at) = if reversed {
            (first.end, first.start)
        } else {
            (first.start, first.end)
        };
        edges[1..].iter().all(|edge| {
            if near3(at, edge.start) {
                at = edge.end;
                true
            } else if near3(at, edge.end) {
                at = edge.start;
                true
            } else {
                false
            }
        }) && near3(at, start)
    })
}

/// The unit tangent of `edge` at its end `at`, pointing into the edge.
fn inward_tangent(edge: &ProfileEdge, curve: &SweepCurve, at: Point3) -> Option<Vector3> {
    let direction = match *curve {
        SweepCurve::Line { .. } => {
            let other = if near3(at, edge.start) {
                edge.end
            } else {
                edge.start
            };
            other.vector_from(at)
        }
        SweepCurve::Arc { center, normal, .. } => {
            let tangent = normal.cross(at.vector_from(center));
            let toward = edge.middle?.vector_from(at);
            if tangent.dot(toward) < 0.0 {
                tangent.scale(-1.0)
            } else {
                tangent
            }
        }
    };
    direction.unit()
}

/// Whether every joint of the open chain `edges` is tangent-continuous.
fn tangent_continuous(edges: &[ProfileEdge], curves: &[SweepCurve]) -> bool {
    edges
        .windows(2)
        .zip(curves.windows(2))
        .all(|(pair, curves)| {
            let joint = [pair[0].start, pair[0].end]
                .into_iter()
                .find(|point| near3(*point, pair[1].start) || near3(*point, pair[1].end));
            let Some(joint) = joint else {
                return false;
            };
            match (
                inward_tangent(&pair[0], &curves[0], joint),
                inward_tangent(&pair[1], &curves[1], joint),
            ) {
                (Some(before), Some(after)) => {
                    (before.x + after.x)
                        .hypot(before.y + after.y)
                        .hypot(before.z + after.z)
                        <= EPS_PATH_TANGENT
                }
                _ => false,
            }
        })
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
