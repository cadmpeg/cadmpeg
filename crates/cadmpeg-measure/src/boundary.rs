// SPDX-License-Identifier: Apache-2.0
//! The boundary resolver: each loop of a face as a closed chain of pieces in
//! the carrier's parameter plane.
//!
//! A ring is traversed by vertex identity. Each coedge becomes a piece on a
//! parameter interval of its 3D curve, inverted onto the carrier. Pieces are
//! lifted across periods so that each starts where the previous one ends; a
//! remaining parameter jump that maps to one surface point becomes a
//! synthesized segment. The closing jump gives the loop's net shift.

pub(crate) mod pcurve;
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
use crate::orientation::declared_normal;
use crate::owners::Owners;
use crate::reason::UnavailableReason;
use crate::report::{DeclaredRole, ListedOrder, NormalReading, UvSource};
use crate::traversal::{traverse, TraversalError};
use pcurve::UseFrame;
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
    /// Each edge's largest measured deviation: the gaps at its vertices and
    /// at the junctions it meets, and its pieces' distance from the surface.
    pub(crate) deviations: Vec<(&'ir str, f64)>,
    /// Whether the traversal direction is known for every coedge.
    pub(crate) directed: bool,
    /// The coedges whose pcurve uses are listed from their end to their
    /// start, in traversal order.
    pub(crate) pcurve_order: Vec<&'ir Coedge>,
}

/// A coedge's 3D curve, its interval in traversal order, and the source
/// its inverted image reports.
type ThreeD<'ir> = (Rc<CurveChart<'ir>>, [f64; 2], UvSource);

/// Two consecutive boundary points and their parameters.
#[derive(Clone, Copy)]
struct Join {
    from_point: Point3,
    to_point: Point3,
    from: [f64; 2],
    to: [f64; 2],
    /// The model distance between the points.
    gap: f64,
    /// The largest gap the two pieces may leave.
    allowance: f64,
}

/// The pieces of one coedge.
enum CoedgePieces<'ir> {
    /// Pieces in traversal order; `directed` is false when the direction of
    /// a closed pcurve-only coedge is not known, and `listed_backward` is
    /// true when its pcurve uses are listed from its end to its start.
    Pieces {
        pieces: Vec<Piece<'ir>>,
        directed: bool,
        listed_backward: bool,
    },
    /// A degenerate coedge at a singular point: a segment along the singular
    /// line between its neighbours.
    Pole,
}

/// What the resolver needs to know about the face.
pub(crate) struct FaceFrame<'a, 'ir> {
    pub(crate) owners: &'a Owners<'ir>,
    pub(crate) edges: &'a HashMap<&'ir str, EdgeGeometry<'ir>>,
    pub(crate) face: &'ir Face,
    pub(crate) chart: &'a Chart<'ir>,
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
                        deviations: Vec::new(),
                        directed: true,
                        pcurve_order: Vec::new(),
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
            .map(|coedge| self.owners.oriented_ends(coedge))
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
                .map(|coedge| self.owners.oriented_ends(coedge).1);
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
        let mut deviations: Vec<(&'ir str, f64)> = Vec::new();
        let mut deviate = |edge: Option<&'ir Coedge>, amount: f64| {
            let Some(edge) = edge.map(|coedge| coedge.edge.as_str()) else {
                return;
            };
            match deviations.iter_mut().find(|(id, _)| *id == edge) {
                Some((_, largest)) => *largest = largest.max(amount),
                None => deviations.push((edge, amount)),
            }
        };
        let mut directed = true;
        let mut pcurve_order = Vec::new();
        let mut pending_pole: Option<&'ir Coedge> = None;
        let mut leading_pole: Option<&'ir Coedge> = None;
        let alone = coedges.len() == 1;
        for coedge in coedges {
            let new_pieces = match self.coedge_pieces(eval, coedge, alone)? {
                CoedgePieces::Pole => {
                    if let Some(earlier) = pending_pole.replace(coedge) {
                        pending_pole = Some(earlier);
                    }
                    continue;
                }
                CoedgePieces::Pieces {
                    pieces: new_pieces,
                    directed: known,
                    listed_backward,
                } => {
                    directed &= known;
                    if listed_backward {
                        pcurve_order.push(*coedge);
                    }
                    new_pieces
                }
            };
            let (start_vertex, end_vertex) = self.owners.oriented_ends(coedge);
            if let (Some(first), Some(position)) = (
                new_pieces.first(),
                self.owners.vertex_position(start_vertex),
            ) {
                let gap = self.body.point(position).distance(first.start_point(eval)?);
                max_gap = max_gap.max(gap);
                deviate(Some(coedge), gap);
            }
            if let (Some(last), Some(position)) =
                (new_pieces.last(), self.owners.vertex_position(end_vertex))
            {
                let gap = self.body.point(position).distance(last.end_point(eval)?);
                max_gap = max_gap.max(gap);
                deviate(Some(coedge), gap);
            }
            for mut piece in new_pieces {
                off_surface = off_surface.max(piece.off_surface);
                deviate(Some(coedge), piece.off_surface);
                let start = piece.start_point(eval)?;
                if let Some(previous) = pieces.last() {
                    let from = previous.end_uv(eval, self.chart)?;
                    let to = piece.start_uv(eval, self.chart)?;
                    piece.shift = lift(periods, from, to);
                    let to = piece.start_uv(eval, self.chart)?;
                    let end = previous.end_point(eval)?;
                    if let Some(turn) = self.pole_turn(from, [end, start], to[0] - from[0]) {
                        piece.shift[0] += turn;
                    }
                    let to = piece.start_uv(eval, self.chart)?;
                    let gap = end.distance(start);
                    max_gap = max_gap.max(gap);
                    deviate(previous.coedge, gap);
                    deviate(Some(coedge), gap);
                    let join = Join {
                        from_point: end,
                        to_point: start,
                        from,
                        to,
                        gap,
                        allowance: self.join_allowance(previous, &piece),
                    };
                    if let Some(junction) = self.junction(eval, join, pending_pole.take())? {
                        pieces.push(junction);
                    }
                } else if let Some(pole) = pending_pole.take() {
                    leading_pole = Some(pole);
                }
                pieces.push(piece);
            }
        }
        let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryCoedgeUnresolved,
                None,
                "the ring has no coedge with a curve",
            ));
        };
        let start = first.start_uv(eval, self.chart)?;
        let end = last.end_uv(eval, self.chart)?;
        let start_point = first.start_point(eval)?;
        let end_point = last.end_point(eval)?;
        let gap = end_point.distance(start_point);
        let closing_allowance = self.join_allowance(last, first);
        if gap > closing_allowance {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                format!("the loop's ends are {gap} mm apart"),
            ));
        }
        max_gap = max_gap.max(gap);
        deviate(first.coedge, gap);
        deviate(last.coedge, gap);
        let mut net_shift = [0_i32; 2];
        let mut target = start;
        for direction in 0..2 {
            if let Some(period) = periods[direction] {
                let mut turns = ((end[direction] - start[direction]) / period).round();
                if direction == 0 {
                    let jump = start[0] + turns * period - end[0];
                    if let Some(turn) = self.pole_turn(end, [end_point, start_point], jump) {
                        turns += (turn / period).round();
                    }
                }
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
        let closing_pole = pending_pole.or(leading_pole);
        let join = Join {
            from_point: end_point,
            to_point: start_point,
            from: end,
            to: target,
            gap,
            allowance: closing_allowance,
        };
        match self.junction(eval, join, closing_pole) {
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
            deviations,
            directed,
            pcurve_order,
        })
    }

    /// The whole periods to add to the `u` jump `jump` of a junction at a
    /// sphere pole, so that the segment along the pole keeps the region on
    /// its left: `−u` at the north pole and `+u` at the south pole for loops
    /// oriented with the region on their left, reversed for a face whose
    /// declared normal is reversed, by an amount in `[0, 2π)`. `None` when
    /// the junction is not at a sphere pole, or when the amount lies within
    /// the angular allowance of 0 or a full turn, where the nearest lift
    /// stands.
    fn pole_turn(&self, from: [f64; 2], points: [Point3; 2], jump: f64) -> Option<f64> {
        let Chart::Sphere(sphere) = self.chart else {
            return None;
        };
        if !points
            .iter()
            .all(|point| self.chart.singular_at(*point, self.alpha))
        {
            return None;
        }
        let period = std::f64::consts::TAU;
        let keeping = match declared_normal(self.face, Some(self.chart)) {
            NormalReading::Reversed => -1.0,
            _ => 1.0,
        };
        let direction = if from[1] > 0.0 { -keeping } else { keeping };
        let amount = (direction * jump).rem_euclid(period);
        let allowance = self
            .owners
            .ir
            .tolerances
            .angular
            .get()
            .max(self.alpha / sphere.radius.abs());
        if amount <= allowance || period - amount <= allowance {
            return None;
        }
        let turn = direction * amount - jump;
        (turn != 0.0).then_some(turn)
    }

    /// The gap two consecutive pieces may leave: the sum of their edges'
    /// tolerances, at least the face's allowance.
    fn join_allowance(&self, from: &Piece<'ir>, to: &Piece<'ir>) -> f64 {
        let tau = |piece: &Piece<'ir>| {
            piece
                .coedge
                .and_then(|coedge| self.edges.get(coedge.edge.as_str()))
                .map_or(0.0, |edge| edge.tau)
        };
        self.alpha.max(tau(from) + tau(to))
    }

    /// A synthesized segment that bridges a parameter jump between two
    /// boundary points that coincide within the allowance. `None` when there
    /// is no jump.
    fn junction(
        &self,
        eval: &Eval<'_, '_>,
        join: Join,
        pole: Option<&'ir Coedge>,
    ) -> Measure<Option<Piece<'ir>>> {
        let Join {
            from_point,
            to_point,
            from,
            to,
            gap,
            allowance,
        } = join;
        if gap > allowance {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                format!("consecutive pieces are {gap} mm apart"),
            ));
        }
        if let Some(pole) = pole {
            if from != to {
                self.check_segment(eval, from_point, to_point, from, to)?;
            }
            return Ok(Some(Piece::pole(pole, from, to, from_point)));
        }
        if from == to {
            return Ok(None);
        }
        self.check_segment(eval, from_point, to_point, from, to)?;
        let singular = self.chart.singular_at(from_point, self.alpha)
            && self.chart.singular_at(to_point, self.alpha);
        Ok(Some(Piece::segment(from, to, from_point, singular)))
    }

    /// Require the straight parameter segment between two junction points to
    /// map within the allowance of both.
    fn check_segment(
        &self,
        eval: &Eval<'_, '_>,
        from_point: Point3,
        to_point: Point3,
        from: [f64; 2],
        to: [f64; 2],
    ) -> Measure<()> {
        let middle = [from[0].midpoint(to[0]), from[1].midpoint(to[1])];
        let reached = self.chart.partials(eval, middle[0], middle[1])?.point;
        if reached.distance(from_point) > self.alpha || reached.distance(to_point) > self.alpha {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryNotClosed,
                None,
                "a parameter jump between pieces does not map to one surface point",
            ));
        }
        Ok(())
    }

    /// The pieces of one coedge: a pole, its pcurve uses, or its 3D curve
    /// interval inverted onto the carrier.
    fn coedge_pieces(
        &self,
        eval: &Eval<'_, '_>,
        coedge: &'ir Coedge,
        alone: bool,
    ) -> Measure<CoedgePieces<'ir>> {
        let entity = coedge.id.as_str();
        let Some(edge) = self.edges.get(coedge.edge.as_str()) else {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryCoedgeUnresolved,
                Some(entity),
                "the coedge's edge does not resolve",
            ));
        };
        let (start_vertex, end_vertex) = self.owners.oriented_ends(coedge);
        let start = self
            .owners
            .vertex_position(start_vertex)
            .map(|point| self.body.point(point));
        let end = self
            .owners
            .vertex_position(end_vertex)
            .map(|point| self.body.point(point));
        if edge.closed() && (edge.degenerate || edge.curve.is_err()) {
            if let Some(point) = start {
                if self.chart.singular_at(point, self.alpha) {
                    return Ok(CoedgePieces::Pole);
                }
            }
        }
        let three_d = self.three_d(coedge, edge)?;
        if !coedge.pcurves.is_empty() {
            let (Some(start), Some(end)) = (start, end) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::BoundaryCoedgeUnresolved,
                    Some(entity),
                    "the coedge's vertices have no points",
                ));
            };
            let frame = UseFrame {
                owners: self.owners,
                chart: self.chart,
                alpha: self.alpha,
                start,
                end,
                closed: edge.closed(),
                curve: three_d
                    .as_ref()
                    .map(|(curve, interval, _)| (&**curve, *interval)),
                edge_range: edge
                    .edge
                    .param_range()
                    .map(cadmpeg_ir::units::FiniteVector::get),
            };
            let resolved = frame
                .resolve(
                    eval,
                    &coedge.pcurves,
                    alone && pcurve::nesting_domain(self.chart),
                )
                .map_err(|failure| failure.at(entity))?;
            let farthest = frame
                .witness(eval, &resolved.spans)
                .map_err(|failure| failure.at(entity))?;
            let uv_source = if resolved.spans.len() > 1 {
                UvSource::PcurveChain
            } else {
                UvSource::Pcurve
            };
            let pieces = resolved
                .spans
                .into_iter()
                .map(|span| {
                    Piece::pcurve(
                        span,
                        self.chart,
                        CoedgeUse {
                            coedge,
                            uv_source,
                            alpha: self.alpha,
                        },
                        farthest,
                    )
                    .unwrapped(eval)
                })
                .collect::<Measure<Vec<_>>>()?;
            return Ok(CoedgePieces::Pieces {
                pieces,
                directed: resolved.directed,
                listed_backward: resolved.listed_backward,
            });
        }
        let Some((curve, interval, uv_source)) = three_d else {
            return Err(match &edge.curve {
                Err(missing) => missing.failure().at(entity),
                Ok(_) => MeasureFailure::unavailable(
                    UnavailableReason::BoundaryCoedgeUnresolved,
                    Some(entity),
                    "the coedge has no pcurve and no 3D interval",
                ),
            });
        };
        let piece = Piece::inverted(
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
        .map_err(|failure| failure.at(entity))?;
        Ok(CoedgePieces::Pieces {
            pieces: vec![piece],
            directed: true,
            listed_backward: false,
        })
    }

    /// The coedge's 3D curve and its interval in traversal order: the
    /// use-curve, else the edge curve on its interval by sense. `None` when
    /// the edge has no usable curve interval.
    fn three_d(
        &self,
        coedge: &'ir Coedge,
        edge: &EdgeGeometry<'ir>,
    ) -> Measure<Option<ThreeD<'ir>>> {
        if let Some(use_curve) = &coedge.use_curve {
            let Some(record) = self.owners.curve(use_curve.curve.as_str()) else {
                return Err(MeasureFailure::unavailable(
                    UnavailableReason::CarrierUnresolved,
                    Some(coedge.id.as_str()),
                    "the coedge's use-curve does not resolve",
                ));
            };
            return Ok(Some((
                Rc::new(CurveChart::new(record, self.body)?),
                use_curve.parameter_range.endpoints(),
                UvSource::InvertedUseCurve,
            )));
        }
        let (Ok(curve), Ok([t0, t1])) = (&edge.curve, &edge.interval) else {
            return Ok(None);
        };
        let interval = match coedge.sense {
            Sense::Forward => [*t0, *t1],
            Sense::Reversed => [*t1, *t0],
        };
        Ok(Some((curve.clone(), interval, UvSource::InvertedEdgeCurve)))
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
        piece::Source::Curve(_) | piece::Source::Pcurve { .. } => return,
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
