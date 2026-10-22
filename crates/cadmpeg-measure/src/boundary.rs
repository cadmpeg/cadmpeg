// SPDX-License-Identifier: Apache-2.0
//! The boundary resolver: each loop of a face as a closed chain of pieces in
//! the carrier's parameter plane.
//!
//! A ring is traversed by vertex identity. Each coedge becomes a piece on a
//! parameter interval of its 3D curve, inverted onto the carrier. Pieces are
//! lifted across periods so that each starts where the previous one ends; a
//! remaining parameter jump that maps to one surface point becomes a
//! synthesized segment. The closing jump gives the loop's net shift.

pub(crate) mod piece;

use std::collections::HashMap;
use std::rc::Rc;

use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Coedge, Face, Loop, LoopBoundary, Sense};

use crate::canonical::curve::CurveChart;
use crate::canonical::placement::Placement;
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::edges::EdgeGeometry;
use crate::failure::{Measure, MeasureFailure};
use crate::owners::Owners;
use crate::reason::UnavailableReason;
use crate::report::{DeclaredRole, ListedOrder, UvSource};
use crate::traversal::{traverse, TraversalError};
use piece::{CoedgeUse, Piece};

/// The boundary of one face.
pub(crate) struct ResolvedFace<'ir> {
    pub(crate) loops: Vec<LoopResolution<'ir>>,
}

/// One loop of a face: its stored facts and its resolution.
pub(crate) struct LoopResolution<'ir> {
    pub(crate) face_loop: &'ir Loop,
    pub(crate) declared_role: DeclaredRole,
    /// Coedges in traversal order when the ring chains, else in listed order.
    pub(crate) coedges: Vec<&'ir Coedge>,
    pub(crate) listed: Option<ListedOrder>,
    pub(crate) outcome: Measure<ResolvedLoop<'ir>>,
}

/// What a loop is.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LoopKind {
    /// A coedge ring.
    Ring,
    /// One vertex at a surface singularity, at this model point.
    Vertex(Point3),
}

/// A loop as a closed chain of pieces.
pub(crate) struct ResolvedLoop<'ir> {
    pub(crate) kind: LoopKind,
    pub(crate) pieces: Vec<Piece<'ir>>,
    pub(crate) net_shift: [i32; 2],
    /// The largest accepted junction, closure or vertex-to-curve gap.
    pub(crate) max_gap: f64,
    /// The largest distance between a boundary point and its inverted
    /// surface point.
    pub(crate) off_surface: f64,
}

/// What the resolver needs to know about the face.
pub(crate) struct FaceFrame<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    pub(crate) face: &'ir Face,
    pub(crate) chart: &'a Chart,
    pub(crate) body: Placement,
    pub(crate) alpha: f64,
}

impl<'ir> FaceFrame<'_, 'ir> {
    /// Resolve every loop of the face.
    pub(crate) fn resolve(&self, eval: &Eval<'_, '_>) -> Measure<ResolvedFace<'ir>> {
        let mut loops = Vec::new();
        for loop_id in &self.face.loops {
            let declared_role = declared(self.face.loop_role(loop_id));
            let Some(face_loop) = self.owners.loop_(loop_id.as_str()) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::BoundaryCoedgeUnresolved,
                    Some(loop_id.as_str()),
                    "the face names a loop that does not resolve",
                ));
            };
            let resolution = self.resolve_loop(eval, face_loop, declared_role)?;
            loops.push(resolution);
        }
        Ok(ResolvedFace { loops })
    }

    /// Resolve one loop. Unavailability is recorded in the outcome; only a
    /// session refusal returns an error.
    fn resolve_loop(
        &self,
        eval: &Eval<'_, '_>,
        face_loop: &'ir Loop,
        declared_role: DeclaredRole,
    ) -> Measure<LoopResolution<'ir>> {
        let ring = match &face_loop.boundary {
            LoopBoundary::Vertex { vertex, .. } => {
                let outcome = match self.owners.vertex_position(vertex.as_str()) {
                    Some(point) => Ok(ResolvedLoop {
                        kind: LoopKind::Vertex(self.body.point(point)),
                        pieces: Vec::new(),
                        net_shift: [0, 0],
                        max_gap: 0.0,
                        off_surface: 0.0,
                    }),
                    None => Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryCoedgeUnresolved,
                        Some(vertex.as_str()),
                        "the vertex loop's vertex has no point",
                    )),
                };
                return Ok(LoopResolution {
                    face_loop,
                    declared_role,
                    coedges: Vec::new(),
                    listed: None,
                    outcome,
                });
            }
            LoopBoundary::Ring(ring) => ring,
        };
        let mut listed_coedges = Vec::new();
        for id in ring.coedges() {
            match self.owners.coedge(id.as_str()) {
                Some(coedge) => listed_coedges.push(coedge),
                None => {
                    return Ok(LoopResolution {
                        face_loop,
                        declared_role,
                        coedges: listed_coedges,
                        listed: None,
                        outcome: Err(MeasureFailure::unavailable(
                            UnavailableReason::BoundaryCoedgeUnresolved,
                            Some(id.as_str()),
                            "the ring names a coedge that does not resolve",
                        )),
                    })
                }
            }
        }
        let ends: Vec<(&str, &str)> = listed_coedges
            .iter()
            .map(|coedge| self.oriented_ends(coedge))
            .collect();
        let traversal = match traverse(&ends) {
            Ok(traversal) => traversal,
            Err(error) => {
                let (reason, message) = match error {
                    TraversalError::Failed => (
                        UnavailableReason::BoundaryRingChainFailed,
                        "the ring's coedges form no closed traversal by vertex identity",
                    ),
                    TraversalError::Ambiguous => (
                        UnavailableReason::BoundaryRingChainAmbiguous,
                        "the ring's coedges form several closed traversals",
                    ),
                    TraversalError::Exhausted => (
                        UnavailableReason::EvaluationWorkExhausted,
                        "the ring traversal search ran out of steps",
                    ),
                };
                return Ok(LoopResolution {
                    face_loop,
                    declared_role,
                    coedges: listed_coedges,
                    listed: None,
                    outcome: Err(MeasureFailure::unavailable(
                        reason,
                        Some(face_loop.id.as_str()),
                        message,
                    )),
                });
            }
        };
        let coedges: Vec<&'ir Coedge> = traversal
            .order
            .iter()
            .map(|index| listed_coedges[*index])
            .collect();
        for vertex_use in ring.vertex_uses() {
            let after = coedges
                .iter()
                .find(|coedge| coedge.id == vertex_use.after)
                .map(|coedge| self.oriented_ends(coedge).1);
            if after != Some(vertex_use.vertex.as_str()) {
                return Ok(LoopResolution {
                    face_loop,
                    declared_role,
                    coedges,
                    listed: Some(traversal.listed),
                    outcome: Err(MeasureFailure::unavailable(
                        UnavailableReason::BoundaryRingChainFailed,
                        Some(vertex_use.vertex.as_str()),
                        "an anchored pole use is not at the end of its coedge",
                    )),
                });
            }
        }
        let outcome = match self.chain(eval, &coedges) {
            Ok(resolved) => Ok(resolved),
            Err(MeasureFailure::Refusal(limit)) => return Err(MeasureFailure::Refusal(limit)),
            Err(failure) => Err(failure),
        };
        Ok(LoopResolution {
            face_loop,
            declared_role,
            coedges,
            listed: Some(traversal.listed),
            outcome,
        })
    }

    /// The vertices a coedge starts and ends at, in traversal direction.
    fn oriented_ends(&self, coedge: &'ir Coedge) -> (&'ir str, &'ir str) {
        let Some(edge) = self.owners.edge(coedge.edge.as_str()) else {
            return (coedge.id.as_str(), coedge.edge.as_str());
        };
        match coedge.sense {
            Sense::Forward => (edge.start.as_str(), edge.end.as_str()),
            Sense::Reversed => (edge.end.as_str(), edge.start.as_str()),
        }
    }

    /// Whether another coedge of the same edge belongs to this face.
    pub(crate) fn is_seam(owners: &Owners<'_>, face: &Face, coedge: &Coedge) -> bool {
        owners
            .edge_coedges(coedge.edge.as_str())
            .iter()
            .any(|other| {
                other.id != coedge.id && owners.coedge_face(other) == Some(face.id.as_str())
            })
    }

    /// The traversed coedges as a closed, lifted chain of pieces.
    fn chain(&self, eval: &Eval<'_, '_>, coedges: &[&'ir Coedge]) -> Measure<ResolvedLoop<'ir>> {
        let periods = self.chart.periods();
        let mut pieces: Vec<Piece<'ir>> = Vec::new();
        let mut max_gap: f64 = 0.0;
        let mut off_surface: f64 = 0.0;
        for coedge in coedges {
            let mut piece = self.coedge_piece(eval, coedge)?;
            off_surface = off_surface.max(piece.off_surface);
            let (start_vertex, end_vertex) = self.oriented_ends(coedge);
            let start = piece.start_point(eval)?;
            let end = piece.end_point(eval)?;
            for (vertex, point) in [(start_vertex, start), (end_vertex, end)] {
                if let Some(position) = self.owners.vertex_position(vertex) {
                    max_gap = max_gap.max(self.body.point(position).distance(point));
                }
            }
            if let Some(previous) = pieces.last() {
                let from = previous.end_uv(eval, self.chart)?;
                let to = piece.start_uv(eval, self.chart)?;
                piece.shift = lift(periods, from, to);
                let to = piece.start_uv(eval, self.chart)?;
                let gap = previous.end_point(eval)?.distance(start);
                max_gap = max_gap.max(gap);
                if let Some(junction) =
                    self.junction(previous.end_point(eval)?, start, from, to, gap)?
                {
                    pieces.push(junction);
                }
            }
            pieces.push(piece);
        }
        let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryCoedgeUnresolved,
                None,
                "the ring has no coedge",
            ));
        };
        let start = first.start_uv(eval, self.chart)?;
        let end = last.end_uv(eval, self.chart)?;
        let start_point = first.start_point(eval)?;
        let end_point = last.end_point(eval)?;
        let gap = end_point.distance(start_point);
        if gap > self.alpha {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                format!("the loop's ends are {gap} mm apart"),
            ));
        }
        max_gap = max_gap.max(gap);
        let mut net_shift = [0_i32; 2];
        let mut target = start;
        for direction in 0..2 {
            if let Some(period) = periods[direction] {
                let turns = ((end[direction] - start[direction]) / period).round();
                net_shift[direction] = cadmpeg_core::convert::truncate_f64_to_i32(turns)
                    .ok_or_else(|| {
                        MeasureFailure::unavailable(
                            UnavailableReason::BoundaryPeriodMismatch,
                            None,
                            "the loop's closing shift is not a whole number of periods",
                        )
                    })?;
                target[direction] = start[direction] + turns * period;
            }
        }
        match self.junction(end_point, start_point, end, target, gap) {
            Ok(Some(closing)) => pieces.push(closing),
            Ok(None) => {}
            Err(MeasureFailure::Unavailable { .. }) => {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::BoundaryPeriodMismatch,
                    None,
                    "the loop's closing jump is not a whole number of periods",
                ))
            }
            Err(refusal) => return Err(refusal),
        }
        if let Some(period) = periods[0] {
            close_through_singular_line(&mut pieces, &mut net_shift, period);
        }
        Ok(ResolvedLoop {
            kind: LoopKind::Ring,
            pieces,
            net_shift,
            max_gap,
            off_surface,
        })
    }

    /// A synthesized segment that bridges a parameter jump between two
    /// boundary points that coincide within the allowance. `None` when there
    /// is no jump.
    fn junction(
        &self,
        from_point: Point3,
        to_point: Point3,
        from: [f64; 2],
        to: [f64; 2],
        gap: f64,
    ) -> Measure<Option<Piece<'ir>>> {
        if gap > self.alpha {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                format!("consecutive pieces are {gap} mm apart"),
            ));
        }
        if from == to {
            return Ok(None);
        }
        let middle = [from[0].midpoint(to[0]), from[1].midpoint(to[1])];
        let reached = self.chart.partials(middle[0], middle[1]).point;
        if reached.distance(from_point) > self.alpha || reached.distance(to_point) > self.alpha {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                "a parameter jump between pieces does not map to one surface point",
            ));
        }
        let singular = self.chart.singular_at(from_point, self.alpha)
            && self.chart.singular_at(to_point, self.alpha);
        Ok(Some(Piece::segment(from, to, from_point, singular)))
    }

    /// The piece of one coedge on the 3D interval of its curve.
    fn coedge_piece(&self, eval: &Eval<'_, '_>, coedge: &'ir Coedge) -> Measure<Piece<'ir>> {
        let entity = coedge.id.as_str();
        let (curve, interval, uv_source) = if let Some(use_curve) = &coedge.use_curve {
            let Some(record) = self.owners.curve(use_curve.curve.as_str()) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::CarrierUnresolved,
                    Some(entity),
                    "the coedge's use-curve does not resolve",
                ));
            };
            (
                Rc::new(CurveChart::new(record, self.body)?),
                use_curve.parameter_range.endpoints(),
                UvSource::InvertedUseCurve,
            )
        } else {
            let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::BoundaryCoedgeUnresolved,
                    Some(entity),
                    "the coedge's edge does not resolve",
                ));
            };
            let curve = edge.curve.clone().map_err(|missing| missing.failure())?;
            let [t0, t1] = edge.interval.clone().map_err(|missing| missing.failure())?;
            let interval = match coedge.sense {
                Sense::Forward => [t0, t1],
                Sense::Reversed => [t1, t0],
            };
            (curve, interval, UvSource::InvertedEdgeCurve)
        };
        Piece::inverted(
            eval,
            self.chart,
            curve,
            interval,
            CoedgeUse {
                coedge,
                uv_source,
                alpha: self.alpha,
            },
        )
        .map_err(|failure| failure.at(entity))
    }
}

/// The stored role of a loop as the report states it.
fn declared(role: cadmpeg_ir::topology::LoopBoundaryRole) -> DeclaredRole {
    match role {
        cadmpeg_ir::topology::LoopBoundaryRole::Outer => DeclaredRole::Outer,
        cadmpeg_ir::topology::LoopBoundaryRole::Inner => DeclaredRole::Inner,
        cadmpeg_ir::topology::LoopBoundaryRole::Unspecified => DeclaredRole::Unspecified,
    }
}

/// On a nesting domain a segment along a singular line may run either way
/// around it. When the loop has exactly one such segment and a nonzero net
/// shift in `u`, the segment takes the length that makes the loop
/// contractible, if that length is less than one period. Otherwise the loop
/// keeps its shift and the region rules read it as non-contractible.
fn close_through_singular_line(pieces: &mut [Piece<'_>], net_shift: &mut [i32; 2], period: f64) {
    if net_shift[0] == 0 {
        return;
    }
    let singular: Vec<usize> = pieces
        .iter()
        .enumerate()
        .filter(|(_, piece)| piece.singular_segment())
        .map(|(index, _)| index)
        .collect();
    let [index] = singular.as_slice() else {
        return;
    };
    let delta = -f64::from(net_shift[0]) * period;
    let Some(segment) = pieces.get(*index) else {
        return;
    };
    let length = match &segment.source {
        piece::Source::Segment { from, to, .. } => to[0] - from[0],
        piece::Source::Curve(_) => return,
    };
    if (length + delta).abs() >= period {
        return;
    }
    for (position, piece) in pieces.iter_mut().enumerate() {
        if position == *index {
            piece.extend_segment(delta);
        } else if position > *index {
            piece.shift[0] += delta;
        }
    }
    net_shift[0] = 0;
}

/// The period multiples that bring `to` nearest `from`.
fn lift(periods: [Option<f64>; 2], from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    std::array::from_fn(|direction| match periods[direction] {
        Some(period) => ((from[direction] - to[direction]) / period).round() * period,
        None => 0.0,
    })
}
