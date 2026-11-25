// SPDX-License-Identifier: Apache-2.0
//! Exact restatements that let the writer encode a document in IGES entities.

use super::rational_support::Revolved;
use super::{
    edge_span, effective_topology_tolerance, flatten_curve, lifted_pcurve, polyline_knots,
    polyline_parameters, reverse_nurbs, solved_curve, topology_edge_explicit_tolerance,
    TopologyIndex,
};
use cadmpeg_ir::geometry::analytic::PlaneSurface;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{PcurveId, SurfaceId};
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
/// circle that lies on one parameter line of the support. Each is exact.
pub(super) fn state_pcurves(ir: &CadIr, admitted: impl Fn(&str) -> bool) -> Cow<'_, CadIr> {
    if ir
        .geometry
        .coedges
        .iter()
        .all(|coedge| !coedge.pcurves.is_empty())
    {
        return Cow::Borrowed(ir);
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
        if !trimmed && coedges.iter().all(|coedge| coedge.pcurves.is_empty()) {
            continue;
        }
        let plane = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => plane,
            SurfaceGeometry::Solved(geometry) => {
                if let Some(form) = Revolved::of(geometry) {
                    if coedges.iter().all(|coedge| coedge.pcurves.is_empty()) {
                        computed.extend(
                            revolved_pcurves(&index, surface, &form, &loops).unwrap_or_default(),
                        );
                    }
                }
                continue;
            }
            SurfaceGeometry::Procedural { .. } => continue,
        };
        for coedge in coedges {
            if !coedge.pcurves.is_empty() {
                continue;
            }
            if let Some(pcurve) = plane_pcurve(&index, plane, coedge) {
                computed.push((coedge.id.clone(), pcurve));
            }
        }
    }
    if computed.is_empty() {
        return Cow::Borrowed(ir);
    }
    let mut stated = ir.clone();
    for (coedge_id, pcurve) in computed {
        if let Some(coedge) = stated
            .geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == coedge_id)
        {
            coedge.pcurves = vec![PcurveUse {
                pcurve: pcurve.id.clone(),
                isoparametric: None,
                parameter_range: None,
            }];
            stated.geometry.pcurves.push(pcurve);
        }
    }
    Cow::Owned(stated)
}

/// The parameter curve of `coedge` on `plane`, in the coedge's traversal.
fn plane_pcurve(
    index: &TopologyIndex<'_>,
    plane: &PlaneSurface,
    coedge: &Coedge,
) -> Option<Pcurve> {
    let id = computed_pcurve_id(index, coedge)?;
    let edge = index.edges.get(coedge.edge.as_str()).copied()?;
    let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
    let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
    let span = edge_span(index, edge, &geometry).ok()?;
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
    let id = PcurveId::try_from(format!(
        "iges:writer:pcurve#{}",
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

/// The edge line or circle of `coedge` stated on one parameter line of
/// `form`, checked by evaluating the support at samples of the edge.
fn revolved_segment<'ir>(
    index: &TopologyIndex<'ir>,
    surface: &Surface,
    form: &Revolved,
    coedge: &'ir Coedge,
) -> Option<Segment<'ir>> {
    let edge = index.edges.get(coedge.edge.as_str()).copied()?;
    let curve = index.curves.get(edge.curve()?.as_str()).copied()?;
    let geometry = flatten_curve(solved_curve(curve).ok()?).ok()?;
    let span = edge_span(index, edge, &geometry).ok()?;
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
    rates.into_iter().find_map(|rate| {
        let segment = Segment {
            coedge,
            range,
            mid,
            mid_uv,
            rate,
        };
        (0..=SEGMENT_SAMPLES)
            .all(|step| {
                let parameter = (range[1] - range[0])
                    .mul_add(f64::from(step) / f64::from(SEGMENT_SAMPLES), range[0]);
                let uv = segment.at(parameter);
                cadmpeg_ir::eval::model_surface_point(
                    cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                    index.ir,
                    &surface.geometry,
                    uv.u,
                    uv.v,
                )
                .is_ok_and(|point| point.get().distance(at(parameter)) <= tolerance)
            })
            .then_some(segment)
    })
}

/// Samples per edge at which the writer checks a computed parameter line.
const SEGMENT_SAMPLES: u32 = 8;

/// The largest parameter step between consecutive coedges of a loop after
/// whole periods are removed. A larger step crosses a pole or a seam.
const JOIN_LIMIT: f64 = std::f64::consts::FRAC_PI_4;

/// The parameter curve of each coedge in `loops` on the revolved support,
/// or none where an edge is not a parameter line of the support or a loop
/// does not close in the parameter plane.
fn revolved_pcurves(
    index: &TopologyIndex<'_>,
    surface: &Surface,
    form: &Revolved,
    loops: &[&cadmpeg_ir::topology::Loop],
) -> Option<Vec<(cadmpeg_ir::ids::CoedgeId, Pcurve)>> {
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
    let mut computed = Vec::new();
    let mut first_centre = None;
    for loop_ in loops {
        let mut segments = Vec::new();
        for coedge_id in loop_.coedges() {
            let coedge = index.coedges.get(coedge_id.as_str()).copied()?;
            let mut segment = revolved_segment(index, surface, form, coedge)?;
            if let Some(previous) = segments.last().map(Segment::ends) {
                let [start, _] = segment.ends();
                let shift = periods(Point2::new(
                    previous[1].u - start.u,
                    previous[1].v - start.v,
                ));
                segment.mid_uv =
                    Point2::new(segment.mid_uv.u + shift.u, segment.mid_uv.v + shift.v);
                let [start, _] = segment.ends();
                if (previous[1].u - start.u).abs() > JOIN_LIMIT
                    || (previous[1].v - start.v).abs() > JOIN_LIMIT
                {
                    return None;
                }
            }
            segments.push(segment);
        }
        let [start, _] = segments.first()?.ends();
        let [_, end] = segments.last()?.ends();
        if (end.u - start.u).abs() > JOIN_LIMIT || (end.v - start.v).abs() > JOIN_LIMIT {
            return None;
        }
        let centre = segments.iter().map(|segment| segment.mid_uv.u).sum::<f64>()
            / cadmpeg_core::convert::f64_from_index(segments.len())?;
        let shift = match first_centre {
            Some(first) => periods(Point2::new(first - centre, 0.0)).u,
            None => {
                first_centre = Some(centre);
                0.0
            }
        };
        for mut segment in segments {
            segment.mid_uv = Point2::new(segment.mid_uv.u + shift, segment.mid_uv.v);
            let id = computed_pcurve_id(index, segment.coedge)?;
            let [start, end] = segment.range;
            let points = [start, end].map(|parameter| {
                let uv = segment.at(parameter);
                Point3::new(uv.u, uv.v, 0.0)
            });
            let nurbs = NurbsCurve::from_lanes(
                1,
                vec![start, start, end, end],
                points.to_vec(),
                None,
                false,
            )
            .ok()?;
            let range = FiniteVector::from([FiniteReal::new(start)?, FiniteReal::new(end)?]);
            let (nurbs, range) = if segment.coedge.sense == Sense::Reversed {
                reverse_nurbs(&nurbs, range).ok()?
            } else {
                (nurbs, range)
            };
            computed.push((
                segment.coedge.id.clone(),
                Pcurve {
                    id,
                    geometry: PcurveGeometry::Nurbs {
                        nurbs: planar_lanes(&nurbs)?,
                    },
                    metadata: PcurveMetadata::general(None, Some(range), None),
                    native_ref: None,
                },
            ));
        }
    }
    Some(computed)
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
fn nurbs_form(geometry: &CurveGeometry, range: FiniteVector<2>) -> Option<NurbsCurve> {
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

/// The document with each face whose sense runs against its plane or
/// B-spline support moved to the support with its parameters exchanged. The
/// exchange reverses the support normal, so the face is forward, as Type 144
/// states every face. A B-spline support's reversed-normal flag is folded
/// into the senses of its faces.
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
        let normal_reversed = match &surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => false,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) => nurbs.normal_reversed(),
            _ => continue,
        };
        let exchanged = (face.sense == Sense::Reversed) != normal_reversed;
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
    changed.retain(|_, (surface, faces)| {
        faces.iter().any(|face| face.exchanged && face.admitted)
            || matches!(
                &surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs))
                    if nurbs.normal_reversed()
            )
    });
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
            SolvedSurfaceGeometry::Nurbs(
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
                .ok()?,
            )
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
