// SPDX-License-Identifier: Apache-2.0
//! Exact restatements that let the writer encode a document in IGES entities.

use super::rational_support::{LoopSpan, Revolved};
use super::{
    edge_span, effective_topology_tolerance, flatten_curve, lifted_pcurve, polyline_knots,
    polyline_parameters, reverse_nurbs, solved_curve, topology_edge_explicit_tolerance,
    TopologyIndex,
};
use crate::entities::curve_conversion::ANGULAR_TOLERANCE;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CoedgeId, PcurveId, SurfaceId};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{Coedge, LoopBoundaryRole, PcurveUse, Sense};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

/// The document with a parameter curve for each coedge on an `admitted`
/// support that Type 144, a Type 143 whose loops already carry parameter
/// curves, or a rational restatement of the support requires. On a plane
/// each computed curve is the image of the edge curve under the plane's
/// inverse frame. On a cylinder, cone, sphere or torus face whose coedges
/// carry none, each computed curve is the parameter line of an edge line or
/// circle that lies on one parameter line of the support, followed, where
/// the next coedge starts at another parameter of the same pole or apex, by
/// the parameter line along that pole. Each is exact.
pub(super) fn state_pcurves<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    admitted: impl Fn(&str) -> bool,
) -> Result<Cow<'ir, CadIr>, CodecError> {
    if ir
        .geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty())
    {
        return Ok(Cow::Borrowed(ir));
    }
    let index = TopologyIndex::new(ir);
    let mut computed = Vec::new();
    for face in ir
        .geometry
        .faces
        .iter()
        .filter(|face| admitted(face.surface.as_str()))
    {
        let Some(surface) = index.surfaces.get(face.surface.as_str()).copied() else {
            continue;
        };
        let Some(loops) = face
            .loops
            .stated()
            .map(|loop_id| index.loops.get(loop_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(coedges) = loops
            .iter()
            .flat_map(|loop_| loop_.coedges())
            .map(|coedge_id| index.coedges.get(coedge_id.as_str()).copied())
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let trimmed = loops
            .iter()
            .any(|loop_| face.loop_role(&loop_.id) != LoopBoundaryRole::Unspecified);
        let unstated = coedges.iter().all(|coedge| coedge.pcurves.is_empty());
        let plane = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => plane,
            SurfaceGeometry::Solved(geometry) => {
                if let Some(form) = Revolved::of(geometry).filter(|_| unstated) {
                    if let Some(segments) =
                        revolved_loop_segments(ctx, &index, surface, &form, &loops)?
                    {
                        if let Some(pcurves) =
                            revolved_pcurves(&index, surface, &form, face.sense, segments)?
                        {
                            computed.extend(pcurves);
                        }
                    }
                }
                continue;
            }
            SurfaceGeometry::Procedural { .. } => continue,
        };
        if !trimmed && unstated {
            continue;
        }
        for coedge in coedges {
            if !coedge.pcurves.is_empty() {
                continue;
            }
            if let Some(pcurve) = plane_pcurve(ctx, &index, plane, coedge) {
                computed.push((coedge.id.clone(), vec![pcurve]));
            }
        }
    }
    if computed.is_empty() {
        return Ok(Cow::Borrowed(ir));
    }
    let mut stated = ir.clone();
    for (coedge_id, pcurves) in computed {
        if let Some(coedge) = stated
            .geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id)
        {
            coedge.pcurves = pcurves
                .iter()
                .map(|pcurve| PcurveUse {
                    pcurve: pcurve.id.clone(),
                    isoparametric: None,
                    parameter_range: None,
                })
                .collect();
            stated.geometry.pcurves.extend(pcurves);
        }
    }
    Ok(Cow::Owned(stated))
}

/// The parameter curve of `coedge` on `plane`, in the coedge's traversal.
fn plane_pcurve(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'_>,
    plane: &PlaneSurface,
    coedge: &Coedge,
) -> Option<Pcurve> {
    let id = computed_pcurve_id(index, coedge)?;
    let edge = index.edges.get(coedge.edge.as_str()).copied()?;
    let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
    let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
    let span = edge_span(ctx, index, edge, &geometry).ok()?;
    let nurbs = nurbs_form(&geometry, span.range)?;
    let (nurbs, range) = if coedge.sense == Sense::Reversed {
        reverse_nurbs(&nurbs, span.range).ok()?
    } else {
        (nurbs, span.range)
    };
    let tolerance = effective_topology_tolerance(topology_edge_explicit_tolerance(index, edge));
    let origin = plane.origin().get();
    let normal = plane.frame().axis().as_raw();
    let u_axis = plane.frame().reference().as_raw();
    let v_axis = plane.frame().binormal();
    let v_axis = v_axis.as_raw();
    let points = nurbs
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            let offset = cadmpeg_ir::math::Vector3::new(
                point.x - origin.x,
                point.y - origin.y,
                point.z - origin.z,
            );
            (offset.dot(*normal).abs() <= tolerance)
                .then(|| Point2::new(offset.dot(*u_axis), offset.dot(*v_axis)))
        })
        .collect::<Option<Vec<_>>>()?;
    let weights = nurbs.weights().map(|weights| {
        weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect()
    });
    let knots = nurbs
        .knots()
        .finite_knots()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    let nurbs =
        PcurveNurbs::from_lanes(nurbs.degree(), knots, points, weights, nurbs.periodic()).ok()?;
    Some(Pcurve {
        id,
        geometry: PcurveGeometry::Nurbs { nurbs },
        metadata: PcurveMetadata::general(None, Some(range), None),
        native_ref: None,
    })
}

/// The identity of the parameter curve that the writer computes for
/// `coedge`, absent where the document already holds a record of that name.
fn computed_pcurve_id(index: &TopologyIndex<'_>, coedge: &Coedge) -> Option<PcurveId> {
    named_pcurve_id(index, "pcurve", coedge)
}

fn named_pcurve_id(index: &TopologyIndex<'_>, kind: &str, coedge: &Coedge) -> Option<PcurveId> {
    let id = PcurveId::try_from(format!(
        "iges:writer:{kind}#{}",
        coedge.id.as_str().replace('#', "/")
    ))
    .ok()?;
    (!index.pcurves.contains_key(id.as_str())).then_some(id)
}

/// An edge line or circle on a revolved support, stated as the parameter
/// line `uv(t) = mid_uv + rate (t - mid)` over its edge range.
struct Segment<'ir> {
    coedge: &'ir Coedge,
    range: [f64; 2],
    mid: f64,
    mid_uv: Point2,
    rate: Point2,
    tolerance: f64,
}

impl Segment<'_> {
    fn at(&self, parameter: f64) -> Point2 {
        Point2::new(
            self.rate.u.mul_add(parameter - self.mid, self.mid_uv.u),
            self.rate.v.mul_add(parameter - self.mid, self.mid_uv.v),
        )
    }

    /// The parameters at the coedge's start and end.
    fn ends(&self) -> [Point2; 2] {
        let [start, end] = self.range.map(|parameter| self.at(parameter));
        if self.coedge.sense == Sense::Reversed {
            [end, start]
        } else {
            [start, end]
        }
    }
}

/// The parameter lines of `form` that an edge line or circle can lie on,
/// with the edge point at each edge parameter and the edge tolerance.
struct Candidates<'ir> {
    segments: Vec<Segment<'ir>>,
    edge_point: Box<dyn Fn(f64) -> Point3>,
    tolerance: f64,
}

fn segment_candidates<'ir>(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'ir>,
    form: &Revolved,
    coedge: &'ir Coedge,
) -> Option<Candidates<'ir>> {
    let edge = index.edges.get(coedge.edge.as_str()).copied()?;
    let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
    let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
    let span = edge_span(ctx, index, edge, &geometry).ok()?;
    let range = span.range.finite_components().map(FiniteReal::get);
    let mid = 0.5 * (range[0] + range[1]);
    let tolerance = effective_topology_tolerance(topology_edge_explicit_tolerance(index, edge));
    let (at, rates): (Box<dyn Fn(f64) -> Point3>, Vec<Point2>) = match geometry.solved()? {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            (
                Box::new(move |parameter| {
                    Point3::new(
                        direction.x.mul_add(parameter, origin.x),
                        direction.y.mul_add(parameter, origin.y),
                        direction.z.mul_add(parameter, origin.z),
                    )
                }),
                vec![Point2::new(0.0, direction.dot(form.z_axis))],
            )
        }
        SolvedCurveGeometry::Circle(circle) => {
            let center = circle.center().get();
            let x_axis = *circle.frame().reference().as_raw();
            let y_axis = *circle.frame().binormal().as_raw();
            let radius = circle.radius().get();
            let turn = circle.frame().axis().as_raw().dot(form.z_axis).signum();
            (
                Box::new(move |parameter: f64| {
                    let (sin, cos) = parameter.sin_cos();
                    Point3::new(
                        center.x + radius * cos.mul_add(x_axis.x, sin * y_axis.x),
                        center.y + radius * cos.mul_add(x_axis.y, sin * y_axis.y),
                        center.z + radius * cos.mul_add(x_axis.z, sin * y_axis.z),
                    )
                }),
                vec![
                    Point2::new(turn, 0.0),
                    Point2::new(0.0, 1.0),
                    Point2::new(0.0, -1.0),
                ],
            )
        }
        _ => return None,
    };
    let mid_uv = form.parameters(at(mid))?;
    Some(Candidates {
        segments: rates
            .into_iter()
            .map(|rate| Segment {
                coedge,
                range,
                mid,
                mid_uv,
                rate,
                tolerance,
            })
            .collect(),
        edge_point: at,
        tolerance,
    })
}

/// The edge line or circle of `coedge` stated on one parameter line of
/// `form`, checked by evaluating the support at samples of the edge.
fn revolved_segment<'ir>(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'ir>,
    surface: &Surface,
    form: &Revolved,
    coedge: &'ir Coedge,
) -> Result<Option<Segment<'ir>>, CodecError> {
    let Some(candidates) = segment_candidates(ctx, index, form, coedge) else {
        return Ok(None);
    };
    'candidates: for segment in candidates.segments {
        let [start, end] = segment.range;
        for step in 0..=SEGMENT_SAMPLES {
            let parameter =
                (end - start).mul_add(f64::from(step) / f64::from(SEGMENT_SAMPLES), start);
            let uv = segment.at(parameter);
            let point = finite_or_refusal(cadmpeg_ir::eval::model_surface_point(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &index.model,
                &surface.geometry,
                uv.u,
                uv.v,
            ))?;
            if !point.is_some_and(|point| {
                point.get().distance((candidates.edge_point)(parameter)) <= candidates.tolerance
            }) {
                continue 'candidates;
            }
        }
        return Ok(Some(segment));
    }
    Ok(None)
}

/// The checked parameter line of each coedge in `loops`, by loop, or none
/// where an edge is not a parameter line of the support.
fn revolved_loop_segments<'ir>(
    ctx: &DecodeContext<'_>,
    index: &TopologyIndex<'ir>,
    surface: &Surface,
    form: &Revolved,
    loops: &[&cadmpeg_ir::topology::Loop],
) -> Result<Option<Vec<Vec<Segment<'ir>>>>, CodecError> {
    let mut segments = Vec::new();
    for loop_ in loops {
        let mut loop_segments = Vec::new();
        for coedge_id in loop_.coedges() {
            let Some(coedge) = index.coedges.get(coedge_id.as_str()).copied() else {
                return Ok(None);
            };
            let Some(segment) = revolved_segment(ctx, index, surface, form, coedge)? else {
                return Ok(None);
            };
            loop_segments.push(segment);
        }
        segments.push(loop_segments);
    }
    Ok(Some(segments))
}

/// Samples per edge at which the writer checks a computed parameter line.
const SEGMENT_SAMPLES: u32 = 8;

/// One coedge of a loop on a revolved support: its parameter line and, where
/// the next coedge starts at another parameter of the same pole or apex, the
/// parameter line along that pole to the next start.
struct Piece<'ir> {
    segment: Segment<'ir>,
    pole: Option<[Point2; 2]>,
}

/// A loop of parameter lines that closes in the parameter plane, and the
/// number of whole turns its traversal makes along `u` and along `v`.
struct LoopChain<'ir> {
    pieces: Vec<Piece<'ir>>,
    winding: [i32; 2],
}

impl LoopChain<'_> {
    fn shift(&mut self, shift: Point2) {
        for piece in &mut self.pieces {
            piece.segment.mid_uv = Point2::new(
                piece.segment.mid_uv.u + shift.u,
                piece.segment.mid_uv.v + shift.v,
            );
            if let Some(pole) = &mut piece.pole {
                *pole = pole.map(|point| Point2::new(point.u + shift.u, point.v + shift.v));
            }
        }
    }

    /// The least and greatest value of parameter `axis` on the loop.
    fn extent(&self, axis: usize) -> [f64; 2] {
        self.pieces
            .iter()
            .flat_map(|piece| piece.segment.ends())
            .map(|point| if axis == 0 { point.u } else { point.v })
            .fold([f64::INFINITY, f64::NEG_INFINITY], |[low, high], value| {
                [low.min(value), high.max(value)]
            })
    }
}

/// How one parameter line ends where the next starts.
enum Join {
    Coincident,
    /// The straight parameter line between the two lies on one pole or apex.
    Pole,
    Open,
}

/// The join from `from` to `to` on `surface`, checked by evaluating the
/// straight parameter line between them against the point at `from`.
fn join(
    index: &TopologyIndex<'_>,
    surface: &Surface,
    from: Point2,
    to: Point2,
    tolerance: f64,
) -> Result<Join, CodecError> {
    if (to.u - from.u).abs() <= ANGULAR_TOLERANCE && (to.v - from.v).abs() <= ANGULAR_TOLERANCE {
        return Ok(Join::Coincident);
    }
    let at = |uv: Point2| {
        finite_or_refusal(cadmpeg_ir::eval::model_surface_point(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &index.model,
            &surface.geometry,
            uv.u,
            uv.v,
        ))
    };
    let Some(anchor) = at(from)? else {
        return Ok(Join::Open);
    };
    for step in 1..=SEGMENT_SAMPLES {
        let fraction = f64::from(step) / f64::from(SEGMENT_SAMPLES);
        let uv = Point2::new(
            (to.u - from.u).mul_add(fraction, from.u),
            (to.v - from.v).mul_add(fraction, from.v),
        );
        if !at(uv)?.is_some_and(|point| point.get().distance(anchor.get()) <= tolerance) {
            return Ok(Join::Open);
        }
    }
    Ok(Join::Pole)
}

/// `segments` moved by whole periods so that each starts where the previous
/// ends, joined along a pole or apex where they meet there, or none where
/// the loop does not close in the parameter plane.
fn closed_chain<'ir>(
    index: &TopologyIndex<'_>,
    surface: &Surface,
    periods: impl Fn(Point2) -> Point2,
    segments: Vec<Segment<'ir>>,
) -> Result<Option<LoopChain<'ir>>, CodecError> {
    let mut pieces: Vec<Piece<'ir>> = Vec::new();
    for mut segment in segments {
        if let Some(previous) = pieces.last_mut() {
            let [_, end] = previous.segment.ends();
            let [start, _] = segment.ends();
            let shift = periods(Point2::new(end.u - start.u, end.v - start.v));
            segment.mid_uv = Point2::new(segment.mid_uv.u + shift.u, segment.mid_uv.v + shift.v);
            let [start, _] = segment.ends();
            match join(index, surface, end, start, previous.segment.tolerance)? {
                Join::Coincident => {}
                Join::Pole => previous.pole = Some([end, start]),
                Join::Open => return Ok(None),
            }
        }
        pieces.push(Piece {
            segment,
            pole: None,
        });
    }
    let (Some(first), Some(last)) = (pieces.first(), pieces.last()) else {
        return Ok(None);
    };
    let [start, _] = first.segment.ends();
    let [_, end] = last.segment.ends();
    let turns = periods(Point2::new(start.u - end.u, start.v - end.v));
    let target = Point2::new(start.u - turns.u, start.v - turns.v);
    let pole = match join(index, surface, end, target, last.segment.tolerance)? {
        Join::Coincident => None,
        Join::Pole => Some([end, target]),
        Join::Open => return Ok(None),
    };
    let winding = [turns.u, turns.v].map(|turn| {
        let whole = (-turn / TAU).round();
        if whole >= 1.0 {
            1
        } else if whole <= -1.0 {
            -1
        } else {
            0
        }
    });
    if let Some(last) = pieces.last_mut() {
        last.pole = pole;
    }
    Ok(Some(LoopChain { pieces, winding }))
}

/// The parameter curves of each coedge in `loops` on the revolved support,
/// moved by whole periods so that each loop closes and the loops bound the
/// face. None where a loop does not close in the parameter plane or the
/// loops do not bound the face.
fn revolved_pcurves(
    index: &TopologyIndex<'_>,
    surface: &Surface,
    form: &Revolved,
    sense: Sense,
    loops: Vec<Vec<Segment<'_>>>,
) -> Result<Option<ComputedPcurves>, CodecError> {
    let periods = |delta: Point2| {
        let turns = |value: f64| (value / TAU).round() * TAU;
        Point2::new(
            turns(delta.u),
            if form.v_periodic() {
                turns(delta.v)
            } else {
                0.0
            },
        )
    };
    let mut chains = Vec::new();
    for segments in loops.into_iter().filter(|segments| !segments.is_empty()) {
        let Some(chain) = closed_chain(index, surface, periods, segments)? else {
            return Ok(None);
        };
        chains.push(chain);
    }
    let mut first_centre = None;
    for chain in &mut chains {
        let [low, high] = chain.extent(0);
        let centre = 0.5 * (low + high);
        let first = *first_centre.get_or_insert(centre);
        chain.shift(Point2::new(
            periods(Point2::new(first - centre, 0.0)).u,
            0.0,
        ));
    }
    let spans = chains
        .iter()
        .map(|chain| LoopSpan {
            winding: chain.winding,
            extent: [chain.extent(0), chain.extent(1)],
        })
        .collect::<Vec<_>>();
    let Some((shifts, _)) = form.face_sides(sense, &spans) else {
        return Ok(None);
    };
    for (chain, shift) in chains.iter_mut().zip(shifts) {
        chain.shift(shift);
    }
    let mut computed = Vec::new();
    for piece in chains.into_iter().flat_map(|chain| chain.pieces) {
        let Some(pcurves) = piece_pcurves(index, &piece) else {
            return Ok(None);
        };
        computed.push((piece.segment.coedge.id.clone(), pcurves));
    }
    Ok(Some(computed))
}

/// Computed parameter curves by coedge.
type ComputedPcurves = Vec<(CoedgeId, Vec<Pcurve>)>;

/// The parameter curve of `piece` in the coedge's traversal, followed by its
/// pole line.
fn piece_pcurves(index: &TopologyIndex<'_>, piece: &Piece<'_>) -> Option<Vec<Pcurve>> {
    let segment = &piece.segment;
    let line = |start: f64, end: f64, points: [Point2; 2]| {
        let nurbs = NurbsCurve::from_lanes(
            1,
            vec![start, start, end, end],
            points.map(|uv| Point3::new(uv.u, uv.v, 0.0)).to_vec(),
            None,
            false,
        )
        .ok()?;
        let range = FiniteVector::from([FiniteReal::new(start)?, FiniteReal::new(end)?]);
        Some((nurbs, range))
    };
    let [start, end] = segment.range;
    let (nurbs, range) = line(
        start,
        end,
        [start, end].map(|parameter| segment.at(parameter)),
    )?;
    let (nurbs, range) = if segment.coedge.sense == Sense::Reversed {
        reverse_nurbs(&nurbs, range).ok()?
    } else {
        (nurbs, range)
    };
    let pcurve = |id, nurbs: &NurbsCurve, range| {
        Some(Pcurve {
            id,
            geometry: PcurveGeometry::Nurbs {
                nurbs: planar_lanes(nurbs)?,
            },
            metadata: PcurveMetadata::general(None, Some(range), None),
            native_ref: None,
        })
    };
    let mut pcurves = vec![pcurve(
        computed_pcurve_id(index, segment.coedge)?,
        &nurbs,
        range,
    )?];
    if let Some(pole) = piece.pole {
        let (nurbs, range) = line(0.0, 1.0, pole)?;
        pcurves.push(pcurve(
            named_pcurve_id(index, "pole-pcurve", segment.coedge)?,
            &nurbs,
            range,
        )?);
    }
    Some(pcurves)
}

/// The parameter curve form whose `(u, v)` control points are the `x`, `y`
/// coordinates of `nurbs`.
fn planar_lanes(nurbs: &NurbsCurve) -> Option<PcurveNurbs> {
    let points = nurbs
        .control_points()
        .into_iter()
        .map(|point| {
            let point = point.get();
            Point2::new(point.x, point.y)
        })
        .collect();
    let weights = nurbs.weights().map(|weights| {
        weights
            .into_iter()
            .map(cadmpeg_ir::scalar::NonZeroReal::get)
            .collect()
    });
    let knots = nurbs
        .knots()
        .finite_knots()
        .map(FiniteReal::get)
        .collect::<Vec<_>>();
    PcurveNurbs::from_lanes(nurbs.degree(), knots, points, weights, nurbs.periodic()).ok()
}

/// The exact NURBS form of `geometry` whose domain contains `range`.
pub(super) fn nurbs_form(geometry: &CurveGeometry, range: FiniteVector<2>) -> Option<NurbsCurve> {
    let interval = range.finite_components().map(FiniteReal::get);
    match geometry.solved()? {
        SolvedCurveGeometry::Line(line) => {
            let origin = line.origin().get();
            let direction = *line.direction().as_raw();
            let at = |parameter: f64| {
                Point3::new(
                    direction.x.mul_add(parameter, origin.x),
                    direction.y.mul_add(parameter, origin.y),
                    direction.z.mul_add(parameter, origin.z),
                )
            };
            let [start, end] = interval;
            NurbsCurve::from_lanes(
                1,
                vec![start, start, end, end],
                vec![at(start), at(end)],
                None,
                false,
            )
            .ok()
        }
        SolvedCurveGeometry::Circle(circle) => {
            crate::entities::curve_conversion::circular_arc_nurbs(
                circle.center().get(),
                *circle.frame().axis().as_raw(),
                *circle.frame().reference().as_raw(),
                circle.radius(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Ellipse(ellipse) => {
            crate::entities::curve_conversion::elliptical_arc_nurbs(
                ellipse.center().get(),
                *ellipse.frame().axis().as_raw(),
                *ellipse.frame().reference().as_raw(),
                ellipse.major_radius(),
                ellipse.minor_radius(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Parabola(parabola) => {
            crate::entities::curve_conversion::parabolic_arc_nurbs(
                parabola.vertex().get(),
                *parabola.frame().axis().as_raw(),
                *parabola.frame().reference().as_raw(),
                parabola.focal_distance(),
                interval,
            )
            .ok()?
        }
        SolvedCurveGeometry::Nurbs(nurbs) => Some(nurbs.clone()),
        SolvedCurveGeometry::Polyline(polyline) => {
            let parameters = polyline_parameters(polyline).ok()?;
            NurbsCurve::from_lanes(
                1,
                polyline_knots(&parameters),
                polyline.points().collect(),
                None,
                false,
            )
            .ok()
        }
        _ => None,
    }
}

/// The document with the reversed-normal flag of each B-spline support
/// folded into the senses of its faces. A Type 128 normal is the cross
/// product of its `u` and `v` derivatives, so each face on a support whose
/// flag is set takes the opposite sense and keeps its oriented normal.
pub(super) fn forward_normals(ir: &CadIr) -> Cow<'_, CadIr> {
    let reversed = ir
        .geometry
        .surfaces
        .iter()
        .filter(|surface| {
            matches!(
                surface.geometry.solved(),
                Some(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.normal_reversed()
            )
        })
        .map(|surface| surface.id.clone())
        .collect::<BTreeSet<_>>();
    if reversed.is_empty() {
        return Cow::Borrowed(ir);
    }
    let mut restated = ir.clone();
    let mut folded = BTreeSet::new();
    for surface in &mut restated.geometry.surfaces {
        if !reversed.contains(&surface.id) {
            continue;
        }
        let (SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
        | SurfaceGeometry::Procedural {
            cache: Some(SolvedSurfaceGeometry::Nurbs(nurbs)),
            ..
        }) = &mut surface.geometry
        else {
            continue;
        };
        if let Some(forward) = forward_nurbs(nurbs) {
            *nurbs = forward;
            folded.insert(surface.id.clone());
        }
    }
    for face in &mut restated.geometry.faces {
        if folded.contains(&face.surface) {
            face.sense = super::placement::opposite(face.sense);
        }
    }
    Cow::Owned(restated)
}

/// `nurbs` with its reversed-normal flag cleared.
fn forward_nurbs(nurbs: &NurbsSurface) -> Option<NurbsSurface> {
    NurbsSurface::new(
        NurbsSurfaceAxis::new(
            nurbs.u_degree(),
            nurbs.u_knots().clone(),
            nurbs.u_periodic(),
        ),
        NurbsSurfaceAxis::new(
            nurbs.v_degree(),
            nurbs.v_knots().clone(),
            nurbs.v_periodic(),
        ),
        nurbs.pole_grid().clone(),
        false,
    )
    .ok()
}

/// The document with each face whose sense runs against its plane or
/// B-spline support moved to the support with its parameters exchanged. The
/// exchange reverses the support normal, so the face is forward, as Type 144
/// states every face. The B-spline supports carry no reversed-normal flag
/// ([`forward_normals`]).
pub(super) fn forward_supports(ir: &CadIr) -> Cow<'_, CadIr> {
    let geometry = &ir.geometry;
    let mut changed = BTreeMap::<&str, (&Surface, Vec<Exchange<'_>>)>::new();
    for face in &geometry.faces {
        let Some(surface) = geometry
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
        else {
            continue;
        };
        if !matches!(
            &surface.geometry,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Plane(_) | SolvedSurfaceGeometry::Nurbs(_)
            )
        ) {
            continue;
        }
        let exchanged = face.sense == Sense::Reversed;
        let admitted =
            !exchanged || face_pcurves(ir, face).all(|pcurve| exchanged_pcurve(pcurve).is_some());
        changed
            .entry(surface.id.as_str())
            .or_insert_with(|| (surface, Vec::new()))
            .1
            .push(Exchange {
                face: face.id.as_str(),
                exchanged,
                admitted,
            });
    }
    changed.retain(|_, (_, faces)| faces.iter().any(|face| face.exchanged && face.admitted));
    if changed.is_empty() {
        return Cow::Borrowed(ir);
    }
    let mut restated = ir.clone();
    let mut moved = BTreeMap::new();
    let mut senses = BTreeMap::new();
    for (id, (surface, faces)) in &changed {
        let all = faces.iter().all(|face| face.exchanged && face.admitted);
        let any = faces.iter().any(|face| face.exchanged && face.admitted);
        let (Some(kept), Some(exchanged)) = (
            support_with(&surface.geometry, false),
            support_with(&surface.geometry, true),
        ) else {
            continue;
        };
        let Some(record) = restated
            .geometry
            .surfaces
            .iter_mut()
            .find(|record| record.id.as_str() == *id)
        else {
            continue;
        };
        if all {
            record.geometry = exchanged;
        } else {
            record.geometry = kept;
            if any {
                let Ok(clone_id) = SurfaceId::try_from(format!("{id}:transposed")) else {
                    continue;
                };
                let mut clone = (*surface).clone();
                clone.id = clone_id.clone();
                clone.geometry = exchanged;
                restated.geometry.surfaces.push(clone);
                for face in faces.iter().filter(|face| face.exchanged && face.admitted) {
                    moved.insert(face.face, clone_id.clone());
                }
            }
        }
        for face in faces {
            // A face whose curves admit no exchange keeps its orientation
            // and stays reversed on the folded support.
            let sense = if face.exchanged && !face.admitted {
                Sense::Reversed
            } else {
                Sense::Forward
            };
            senses.insert(face.face, (sense, face.exchanged && face.admitted));
        }
    }
    let exchanged_pcurves = geometry
        .faces
        .iter()
        .filter(|face| {
            senses
                .get(face.id.as_str())
                .is_some_and(|(_, exchanged)| *exchanged)
        })
        .flat_map(|face| face_pcurves(ir, face))
        .map(|pcurve| pcurve.id.as_str())
        .collect::<BTreeSet<_>>();
    for face in &mut restated.geometry.faces {
        if let Some((sense, _)) = senses.get(face.id.as_str()) {
            face.sense = *sense;
        }
        if let Some(surface) = moved.get(face.id.as_str()) {
            face.surface = surface.clone();
        }
    }
    for pcurve in &mut restated.geometry.pcurves {
        if exchanged_pcurves.contains(pcurve.id.as_str()) {
            if let Some(exchanged) = exchanged_pcurve(pcurve) {
                *pcurve = exchanged;
            }
        }
    }
    Cow::Owned(restated)
}

/// A face on a plane or B-spline support: whether its sense runs against the
/// support normal, and whether its parameter curves admit the exchange.
struct Exchange<'ir> {
    face: &'ir str,
    exchanged: bool,
    admitted: bool,
}

/// The parameter curves on the loops of `face`.
fn face_pcurves<'ir>(
    ir: &'ir CadIr,
    face: &'ir cadmpeg_ir::topology::Face,
) -> impl Iterator<Item = &'ir Pcurve> + 'ir {
    let geometry = &ir.geometry;
    face.loops
        .stated()
        .filter_map(|id| geometry.loops.iter().find(|loop_| loop_.id == *id))
        .flat_map(|loop_| {
            let vertex_uses = match &loop_.boundary {
                cadmpeg_ir::topology::LoopBoundary::Vertex { pcurves, .. } => pcurves.clone(),
                cadmpeg_ir::topology::LoopBoundary::Ring(ring) => ring
                    .vertex_uses()
                    .iter()
                    .flat_map(|use_| use_.pcurves.clone())
                    .collect(),
            };
            loop_
                .coedges()
                .iter()
                .filter_map(|id| geometry.coedges.iter().find(|coedge| coedge.id == *id))
                .flat_map(|coedge| coedge.pcurves.clone())
                .chain(vertex_uses)
        })
        .filter_map(|use_| {
            geometry
                .pcurves
                .iter()
                .find(|pcurve| pcurve.id == use_.pcurve)
        })
}

/// The plane or B-spline support with its normal flag folded, and with its
/// parameters exchanged when `exchanged`.
fn support_with(geometry: &SurfaceGeometry, exchanged: bool) -> Option<SurfaceGeometry> {
    let solved = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) if exchanged => {
            let frame = plane.frame();
            let axis = frame.axis().as_raw();
            SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(
                    plane.origin().get(),
                    cadmpeg_ir::math::Vector3::new(-axis.x, -axis.y, -axis.z),
                    *frame.binormal().as_raw(),
                )
                .ok()?,
            )
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
            SolvedSurfaceGeometry::Plane(*plane)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => {
            let mut nurbs = nurbs.clone();
            if exchanged {
                nurbs.transpose_parameter_axes();
            }
            SolvedSurfaceGeometry::Nurbs(nurbs)
        }
        _ => return None,
    };
    Some(SurfaceGeometry::Solved(solved))
}

/// `pcurve` with its `u` and `v` coordinates exchanged.
fn exchanged_pcurve(pcurve: &Pcurve) -> Option<Pcurve> {
    if pcurve.uv_offset().is_some() {
        return None;
    }
    let lifted = match &pcurve.geometry {
        PcurveGeometry::Nurbs { nurbs } => nurbs
            .lift(|point| Point3::new(point.v, point.u, 0.0))
            .ok()?,
        PcurveGeometry::Line(_) => {
            let mut lifted = lifted_pcurve(pcurve, pcurve.parameter_range()?).ok()?;
            lifted
                .try_map_control_points(|_, point| {
                    let point = point.get();
                    cadmpeg_ir::features::geometry::FinitePoint3::new(Point3::new(
                        point.y, point.x, 0.0,
                    ))
                    .ok_or(())
                })
                .ok()?;
            lifted
        }
        _ => return None,
    };
    Some(Pcurve {
        geometry: PcurveGeometry::Nurbs {
            nurbs: planar_lanes(&lifted)?,
        },
        ..pcurve.clone()
    })
}
