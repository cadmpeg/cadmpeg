//! Sketch entity projection from B-rep edges.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};

const EPS_SKETCH_EDGES_PROJECT_EDGE_E9: f64 = 1.0e-9;
/// Largest distance at which a projected NURBS endpoint snaps to its edge vertex.
const EPS_SKETCH_EDGES_NURBS_ENDPOINT_SNAP_MM: f64 = 1.0e-2;
/// Smallest distance within which an edge vertex inverts onto its NURBS carrier.
const EPS_SKETCH_EDGES_NURBS_VERTEX_INVERSION_MM: f64 = 1.0e-2;
const EPS_SKETCH_EDGES_CIRCLE_CONTAINS_POINT_E9: f64 = 1.0e-9;
const EPS_SKETCH_EDGES_ELLIPSE_CONTAINS_POINT_E9: f64 = 1.0e-9;

/// The projection tolerance of one edge projected onto a sketch plane.
///
/// The projection resolves to [`EPS_SKETCH_EDGES_PROJECT_EDGE_E9`], so an edge
/// stating a finer tolerance states a resolution the projection does not
/// carry. [`EdgeProjectionTolerance::of`] is the only constructor: a stated
/// tolerance under that bound is refused, so no stated value is floored, and an
/// edge that states no tolerance projects at the bound.
#[derive(Debug, Clone, Copy)]
struct EdgeProjectionTolerance(f64);

impl EdgeProjectionTolerance {
    /// The projection tolerance of `edge`, or `None` when the edge states a
    /// tolerance finer than the projection resolves.
    fn of(edge: &cadmpeg_ir::topology::Edge) -> Option<Self> {
        let Some(stated) = edge.tolerance else {
            return Some(Self(EPS_SKETCH_EDGES_PROJECT_EDGE_E9));
        };
        (stated.get() >= EPS_SKETCH_EDGES_PROJECT_EDGE_E9).then_some(Self(stated.get()))
    }

    /// The tolerance value.
    fn get(self) -> f64 {
        self.0
    }
}

/// The sketch and the source position whose shared endpoints become constraints.
#[derive(Clone, Copy)]
pub(super) struct EndpointConstraintSource<'a> {
    pub(super) sketch: &'a SketchId,
    pub(super) entities: &'a [SketchEntity],
    pub(super) block_offset: usize,
    pub(super) stream_ordinal: usize,
    pub(super) face_ordinal: usize,
    pub(super) stream: &'a cadmpeg_ir::StreamName,
}

pub(super) fn project_endpoint_constraints(
    source: EndpointConstraintSource<'_>,
    annotations: &mut Annotations,
    constraints: &mut Vec<SketchConstraint>,
) -> Result<(), cadmpeg_core::CodecError> {
    let EndpointConstraintSource {
        sketch,
        entities,
        block_offset,
        stream_ordinal,
        face_ordinal,
        stream,
    } = source;
    let mut loci_by_endpoint =
        BTreeMap::<&str, Vec<(bool, &cadmpeg_ir::sketches::SketchEntityId)>>::new();
    for entity in entities {
        if entity.endpoint_refs.len() != 2 {
            continue;
        }
        for (index, endpoint) in entity.endpoint_refs.iter().enumerate() {
            let loci = loci_by_endpoint.entry(endpoint).or_default();
            loci.push((index == 0, entity.id()));
        }
    }
    for (_endpoint, sources) in loci_by_endpoint {
        let Some((_, first)) = sources.first() else {
            continue;
        };
        if !sources.iter().any(|(_, entity)| entity != first) {
            continue;
        }
        let Ok(id) = SketchConstraintId::mint(format!(
            "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
            constraints.len()
        )) else {
            continue;
        };
        let loci = sources
            .into_iter()
            .map(|(start, source)| {
                if start {
                    SketchLocus::Start(source.clone())
                } else {
                    SketchLocus::End(source.clone())
                }
            })
            .collect();
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci { loci },
        ) else {
            continue;
        };
        crate::annotations::note(
            annotations,
            id.as_str(),
            stream,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        )?;
        constraints.push(SketchConstraint {
            id,
            sketch: sketch.clone(),
            definition,
            name: None,
            driving: None,
            active: None,
            virtual_space: None,
            visible: None,
            orientation: None,
            label_distance: None,
            label_position: None,
            metadata: None,
            native_ref: None,
        });
    }
    Ok(())
}

/// The sketch plane a projection maps model space onto.
///
/// The origin and the two in-plane axes are one frame: a projection reads
/// all three together or none of them.
#[derive(Debug, Clone, Copy)]
pub(super) struct SketchPlaneFrame {
    /// Plane origin in model space.
    pub(super) origin: Point3,
    /// In-plane u direction.
    pub(super) u_axis: Vector3,
    /// In-plane v direction.
    pub(super) v_axis: Vector3,
}

/// The edge parameter interval on its NURBS carrier, as
/// `[start vertex parameter, end vertex parameter]`.
pub(super) fn nurbs_edge_trim_range(
    ctx: &DecodeContext<'_>,
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    vertex_tolerances: &HashMap<&cadmpeg_ir::ids::VertexId, Option<f64>>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
) -> Result<Option<[f64; 2]>, CodecError> {
    if let Some(range) = edge
        .param_range()
        .map(cadmpeg_ir::units::FiniteVector::get)
        .filter(|[start, end]| start != end)
    {
        return Ok(Some(range));
    }
    let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) =
        edge.curve().and_then(|id| curves.get(id).copied())
    else {
        return Ok(None);
    };
    if nurbs.periodic() {
        return Ok(None);
    }
    let (Some(start_vertex), Some(end_vertex)) =
        (vertices.get(&edge.start), vertices.get(&edge.end))
    else {
        return Ok(None);
    };
    let (Some(start_point), Some(end_point)) = (points.get(start_vertex), points.get(end_vertex))
    else {
        return Ok(None);
    };
    let vertex_tolerance = |vertex: &cadmpeg_ir::ids::VertexId| {
        vertex_tolerances
            .get(vertex)
            .copied()
            .flatten()
            .unwrap_or(0.0)
    };
    let tolerance = EdgeProjectionTolerance::of(edge)
        .map_or(0.0, EdgeProjectionTolerance::get)
        .max(vertex_tolerance(&edge.start))
        .max(vertex_tolerance(&edge.end))
        .max(EPS_SKETCH_EDGES_NURBS_VERTEX_INVERSION_MM);
    if nurbs.degree() == 1 {
        let (Some(start), Some(end)) = (
            degree_one_nurbs_parameter(nurbs, *start_point, tolerance),
            degree_one_nurbs_parameter(nurbs, *end_point, tolerance),
        ) else {
            return Ok(None);
        };
        return Ok((start != end).then_some([start, end]));
    }
    let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) else {
        return Ok(None);
    };
    let [lower, upper] = domain.endpoints();
    for (start_seed, end_seed) in [(lower, upper), (upper, lower)] {
        let start = invert_nurbs_vertex(ctx, nurbs, *start_point, tolerance, start_seed)?;
        let end = invert_nurbs_vertex(ctx, nurbs, *end_point, tolerance, end_seed)?;
        let (Some(start), Some(end)) = (start, end) else {
            continue;
        };
        if start != end {
            return Ok(Some([start, end]));
        }
    }
    Ok(None)
}

fn invert_nurbs_vertex(
    ctx: &DecodeContext<'_>,
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Result<Option<f64>, CodecError> {
    Ok(
        cadmpeg_ir::eval::nurbs_curve_parameter_near_point(ctx, curve, point, tolerance, seed)?
            .map(cadmpeg_ir::scalar::FiniteReal::get),
    )
}

/// The parameter of `point` on a degree-one NURBS curve within `tolerance`.
fn degree_one_nurbs_parameter(
    curve: &cadmpeg_ir::geometry::nurbs::NurbsCurve,
    point: Point3,
    tolerance: f64,
) -> Option<f64> {
    if curve.degree() != 1 || curve.periodic() {
        return None;
    }
    let knots = curve.knots();
    let (poles, weights): (Vec<Point3>, Option<Vec<f64>>) = match curve.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            (points.iter().map(|point| point.get()).collect(), None)
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => (
            points.iter().map(|point| point.point.get()).collect(),
            Some(points.iter().map(|point| point.weight.get()).collect()),
        ),
    };
    if poles.len() < 2 || knots.len() != poles.len().checked_add(2)? {
        return None;
    }
    let mut best: Option<(f64, f64)> = None;
    for index in 0..poles.len() - 1 {
        let [start, end] = [knots.get(index + 1)?, knots.get(index + 2)?];
        if !start.is_finite() || !end.is_finite() || start >= end {
            continue;
        }
        let origin = poles[index];
        let direction = cadmpeg_ir::math::Vector3::new(
            poles[index + 1].x - origin.x,
            poles[index + 1].y - origin.y,
            poles[index + 1].z - origin.z,
        );
        let length_squared =
            direction.x * direction.x + direction.y * direction.y + direction.z * direction.z;
        if !length_squared.is_finite() || length_squared <= tolerance * tolerance {
            continue;
        }
        let offset = cadmpeg_ir::math::Vector3::new(
            point.x - origin.x,
            point.y - origin.y,
            point.z - origin.z,
        );
        let fraction = (offset.x * direction.x + offset.y * direction.y + offset.z * direction.z)
            / length_squared;
        if !fraction.is_finite() {
            continue;
        }
        let fraction = fraction.clamp(0.0, 1.0);
        let foot = cadmpeg_ir::math::Point3::new(
            origin.x + fraction * direction.x,
            origin.y + fraction * direction.y,
            origin.z + fraction * direction.z,
        );
        let distance = (point.x - foot.x)
            .hypot(point.y - foot.y)
            .hypot(point.z - foot.z);
        if distance > tolerance {
            continue;
        }
        let local = match &weights {
            None => fraction,
            Some(weights) => {
                let first = weights[index];
                let second = weights[index + 1];
                let denominator = second * (1.0 - fraction) + fraction * first;
                if !denominator.is_finite() || denominator == 0.0 {
                    continue;
                }
                fraction * first / denominator
            }
        };
        let parameter = start + local * (end - start);
        if !parameter.is_finite() {
            continue;
        }
        if best.is_none_or(|(best_distance, _)| distance < best_distance) {
            best = Some((distance, parameter));
        }
    }
    best.map(|(_, parameter)| parameter)
}

/// Positive when the carrier parameter turns counter-clockwise in the sketch plane.
fn projected_handedness(
    axis: Vector3,
    reference: Vector3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> f64 {
    let perpendicular = axis.cross(reference);
    let projected_reference = (reference.dot(u_axis), reference.dot(v_axis));
    let projected_perpendicular = (perpendicular.dot(u_axis), perpendicular.dot(v_axis));
    projected_reference.0 * projected_perpendicular.1
        - projected_reference.1 * projected_perpendicular.0
}

pub(super) fn project_edge(
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    frame: SketchPlaneFrame,
    nurbs_trim: Option<[f64; 2]>,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<(SketchGeometry, bool)> {
    let SketchPlaneFrame {
        origin,
        u_axis,
        v_axis,
    } = frame;
    let start_point = points.get(vertices.get(&edge.start)?)?;
    let start = project_point(*start_point, origin, u_axis, v_axis);
    let end_point = points.get(vertices.get(&edge.end)?)?;
    let end = project_point(*end_point, origin, u_axis, v_axis);
    let line = || SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).ok();
    let tolerance = EdgeProjectionTolerance::of(edge)?.get();
    if let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) =
        edge.curve().and_then(|id| curves.get(id).copied())
    {
        let clamped = nurbs_trim.and_then(|[start, end]| {
            crate::brep::graph::clamp_nurbs_curve_to_domain(nurbs, [start.min(end), start.max(end)])
                .ok()
                .flatten()
        });
        let nurbs_trim = nurbs_trim.filter(|_| clamped.is_some());
        let whole = nurbs.periodic() || edge.start == edge.end;
        let Some(nurbs) = clamped.as_ref().or_else(|| whole.then_some(nurbs)) else {
            return line().map(|geometry| (geometry, true));
        };
        let knots = nurbs.knots().clone();
        let mut projected = Vec::new();
        match nurbs.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
                projected.extend(
                    points
                        .iter()
                        .map(|point| project_point(point.get(), origin, u_axis, v_axis)),
                );
            }
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                projected.extend(
                    points
                        .iter()
                        .map(|point| project_point(point.point.get(), origin, u_axis, v_axis)),
                );
            }
        }
        let weights = match nurbs.pole_rows() {
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { .. } => None,
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
                Some(points.iter().map(|point| point.weight).collect())
            }
        };
        let distance = |left: Point2, right: Point2| (left.u - right.u).hypot(left.v - right.v);
        let start_aligned = nurbs_trim.map_or_else(
            || match (projected.first(), projected.last()) {
                (Some(first), Some(last)) => {
                    !(distance(*first, end) < distance(*first, start)
                        && distance(*last, start) < distance(*last, end))
                }
                _ => true,
            },
            |[start, end]| start <= end,
        );
        let (first_target, last_target) = if start_aligned {
            (start, end)
        } else {
            (end, start)
        };
        let snaps = |pole: Point2, target: Point2| {
            nurbs_trim.is_some()
                || (!nurbs.periodic()
                    && distance(pole, target) <= EPS_SKETCH_EDGES_NURBS_ENDPOINT_SNAP_MM)
        };
        if let Some(first) = projected.first_mut() {
            if snaps(*first, first_target) {
                *first = first_target;
            }
        }
        if let Some(last) = projected.last_mut() {
            if snaps(*last, last_target) {
                *last = last_target;
            }
        }
        return match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
            nurbs.degree(),
            knots,
            projected,
            weights,
            nurbs.periodic(),
        ) {
            Ok(nurbs) => Some((SketchGeometry::nurbs(nurbs), start_aligned)),
            Err(error) => {
                refusal.note(
                    format_args!("sldprt projected sketch edge {}", edge.id),
                    &error,
                );
                None
            }
        };
    }
    let curve = edge.curve().and_then(|id| curves.get(id).copied());
    let native = match curve {
        Some(CurveGeometry::Solved(
            SolvedCurveGeometry::Circle(_)
            | SolvedCurveGeometry::Ellipse(_)
            | SolvedCurveGeometry::Line(_)
            | SolvedCurveGeometry::Nurbs(_),
        ))
        | None => None,
        Some(other) => cadmpeg_core::text::NonBlankString::new(format!("{other:?}"))
            .map(SketchGeometry::native),
    };
    let projected = (|| match curve {
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))) => {
            let center = circle_curve.center().get();
            let radius = circle_curve.radius();
            let center = project_point(center, origin, u_axis, v_axis);
            if !circle_contains_point(center, radius, start, tolerance)
                || !circle_contains_point(center, radius, end, tolerance)
            {
                return line();
            }
            if (start.u - end.u).hypot(start.v - end.v) <= EPS_SKETCH_EDGES_PROJECT_EDGE_E9 {
                Some(
                    SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                        center,
                        radius: cadmpeg_ir::scalar::Length::from(circle_curve.radius()),
                    })
                    .ok()?,
                )
            } else {
                let start_angle = (start.v - center.v).atan2(start.u - center.u);
                let end_angle = match edge
                    .param_range()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .filter(|[start, end]| start != end)
                {
                    Some([start_parameter, end_parameter]) => {
                        let handedness = projected_handedness(
                            *circle_curve.frame().axis().as_raw(),
                            *circle_curve.frame().reference().as_raw(),
                            u_axis,
                            v_axis,
                        );
                        let sweep = end_parameter - start_parameter;
                        start_angle + if handedness < 0.0 { -sweep } else { sweep }
                    }
                    None => (end.v - center.v).atan2(end.u - center.u),
                };
                Some(
                    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                        center,
                        radius: cadmpeg_ir::scalar::Length::from(circle_curve.radius()),
                        start_angle: cadmpeg_ir::scalar::Angle::new(start_angle)?,
                        end_angle: cadmpeg_ir::scalar::Angle::new(end_angle)?,
                    })
                    .ok()?,
                )
            }
        }
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve))) => {
            let center = ellipse_curve.center().get();
            let major_direction = ellipse_curve.frame().reference().as_raw();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            let center = project_point(center, origin, u_axis, v_axis);
            let major_u = major_direction.dot(u_axis);
            let major_v = major_direction.dot(v_axis);
            let major_angle = major_v.atan2(major_u);
            if !ellipse_contains_point(
                center,
                major_angle,
                ellipse_curve.major_radius(),
                ellipse_curve.minor_radius(),
                start,
                tolerance,
            ) || !ellipse_contains_point(
                center,
                major_angle,
                ellipse_curve.major_radius(),
                ellipse_curve.minor_radius(),
                end,
                tolerance,
            ) {
                return line();
            }
            let full = (start.u - end.u).hypot(start.v - end.v) <= EPS_SKETCH_EDGES_PROJECT_EDGE_E9;
            let parameter = |point: Point2| {
                let du = point.u - center.u;
                let dv = point.v - center.v;
                let major_component = du * major_angle.cos() + dv * major_angle.sin();
                let minor_component = -du * major_angle.sin() + dv * major_angle.cos();
                (minor_component / minor_radius).atan2(major_component / major_radius)
            };
            let parameters = edge
                .param_range()
                .map(cadmpeg_ir::units::FiniteVector::get)
                .filter(|[start, end]| start != end);
            let handedness = projected_handedness(
                *ellipse_curve.frame().axis().as_raw(),
                *major_direction,
                u_axis,
                v_axis,
            );
            let hand = if handedness < 0.0 { -1.0 } else { 1.0 };
            Some(
                SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                    center,
                    major_angle: cadmpeg_ir::scalar::Angle::new(major_angle)?,
                    radii: cadmpeg_ir::sketches::EllipseRadii {
                        major_radius: cadmpeg_ir::scalar::Length::from(
                            ellipse_curve.major_radius(),
                        ),
                        minor_radius: cadmpeg_ir::scalar::Length::from(
                            ellipse_curve.minor_radius(),
                        ),
                    },
                    bounds: if full {
                        None
                    } else {
                        Some([
                            cadmpeg_ir::scalar::Angle::new(
                                parameters
                                    .map_or_else(|| parameter(start), |range| hand * range[0]),
                            )?,
                            cadmpeg_ir::scalar::Angle::new(
                                parameters.map_or_else(|| parameter(end), |range| hand * range[1]),
                            )?,
                        ])
                    },
                })
                .ok()?,
            )
        }
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))) => None,
        None if edge.start == edge.end => Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position: start }).ok()?,
        ),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_))) | None => line(),
        Some(_) => native,
    })();
    projected.map(|geometry| (geometry, true))
}

fn circle_contains_point(
    center: Point2,
    radius: cadmpeg_ir::scalar::PositiveLength,
    point: Point2,
    tolerance: f64,
) -> bool {
    let radius = radius.get();
    let distance = (point.u - center.u).hypot(point.v - center.v);
    distance.is_finite()
        && (distance - radius.abs()).abs()
            <= tolerance.max(radius.abs() * EPS_SKETCH_EDGES_CIRCLE_CONTAINS_POINT_E9)
}

fn ellipse_contains_point(
    center: Point2,
    major_angle: f64,
    major_radius: cadmpeg_ir::scalar::PositiveLength,
    minor_radius: cadmpeg_ir::scalar::PositiveLength,
    point: Point2,
    tolerance: f64,
) -> bool {
    let major_radius = major_radius.get();
    let minor_radius = minor_radius.get();
    if major_radius.abs() <= tolerance || minor_radius.abs() <= tolerance {
        return false;
    }
    let du = point.u - center.u;
    let dv = point.v - center.v;
    let major = du * major_angle.cos() + dv * major_angle.sin();
    let minor = -du * major_angle.sin() + dv * major_angle.cos();
    let parameter = (minor / minor_radius).atan2(major / major_radius);
    let reconstructed = Point2::new(
        center.u + major_radius * parameter.cos() * major_angle.cos()
            - minor_radius * parameter.sin() * major_angle.sin(),
        center.v
            + major_radius * parameter.cos() * major_angle.sin()
            + minor_radius * parameter.sin() * major_angle.cos(),
    );
    let distance = (point.u - reconstructed.u).hypot(point.v - reconstructed.v);
    distance.is_finite()
        && distance
            <= tolerance.max(
                major_radius.abs().max(minor_radius.abs())
                    * EPS_SKETCH_EDGES_ELLIPSE_CONTAINS_POINT_E9,
            )
}

pub(super) fn project_point(
    point: Point3,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Point2 {
    let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    Point2::new(delta.dot(u_axis), delta.dot(v_axis))
}

#[cfg(test)]
mod sketch_edges_tests;
