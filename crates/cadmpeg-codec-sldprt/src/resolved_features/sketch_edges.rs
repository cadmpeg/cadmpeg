//! Sketch entity projection from B-rep edges.

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

struct FormattedByteCount(usize);

impl fmt::Write for FormattedByteCount {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0 = self.0.checked_add(text.len()).ok_or(fmt::Error)?;
        Ok(())
    }
}

fn retained_curve_debug(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &CurveGeometry,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut count = FormattedByteCount(0);
    fmt::write(&mut count, format_args!("{curve:?}")).map_err(|_| {
        ctx.refuse_codec_limit("retain SLDPRT opaque sketch curve", u64::MAX - 1, u64::MAX)
    })?;
    let mut text = String::new();
    ctx.try_reserve_retained_text(&mut text, count.0, "retain SLDPRT opaque sketch curve")?;
    fmt::write(&mut text, format_args!("{curve:?}")).map_err(|_| {
        cadmpeg_core::CodecError::malformed("cannot format SLDPRT opaque sketch curve")
    })?;
    Ok(text)
}

fn retained_id_text(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &str,
    operation: &'static str,
) -> Result<String, cadmpeg_core::CodecError> {
    let mut text = String::new();
    ctx.try_reserve_retained_text(&mut text, value.len(), operation)?;
    text.push_str(value);
    Ok(text)
}

const EPS_SKETCH_EDGES_PROJECT_EDGE_E9: f64 = 1.0e-9;
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

#[allow(clippy::too_many_arguments)]
pub(super) fn project_endpoint_constraints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sketch: &SketchId,
    entities: &[SketchEntity],
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
    stream: &cadmpeg_ir::StreamName,
    annotations: &mut Annotations,
    constraints: &mut Vec<SketchConstraint>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut loci_by_endpoint =
        BTreeMap::<&str, Vec<(bool, &cadmpeg_ir::sketches::SketchEntityId)>>::new();
    for entity in entities {
        if entity.endpoint_refs.len() != 2 {
            continue;
        }
        for (index, endpoint) in entity.endpoint_refs.iter().enumerate() {
            if !loci_by_endpoint.contains_key(endpoint.as_str()) {
                ctx.charge_collection_items(1, "index SLDPRT shared sketch endpoints")?;
            }
            let loci = loci_by_endpoint.entry(endpoint).or_default();
            ctx.reserve_vec(loci, 1, "collect SLDPRT shared sketch endpoint loci")?;
            loci.push((index == 0, entity.id()));
        }
    }
    for (_endpoint, sources) in loci_by_endpoint {
        let Some((_, first)) = sources.first() else {
            continue;
        };
        ctx.charge_work(
            u64::try_from(sources.len()).map_err(|_| {
                ctx.refuse_codec_limit(
                    "compare SLDPRT shared sketch endpoints",
                    u64::MAX - 1,
                    u64::MAX,
                )
            })?,
            "compare SLDPRT shared sketch endpoints",
        )?;
        if !sources.iter().any(|(_, entity)| entity != first) {
            continue;
        }
        let digits = |value: usize| {
            if value == 0 {
                1
            } else {
                value.ilog10() as usize + 1
            }
        };
        let id_length = "sldprt:model:sketch-constraint#:::".len()
            + digits(block_offset)
            + digits(stream_ordinal)
            + digits(face_ordinal)
            + digits(constraints.len());
        let mut id_text = String::new();
        ctx.try_reserve_retained_text(
            &mut id_text,
            id_length,
            "retain SLDPRT shared endpoint constraint ID",
        )?;
        std::fmt::Write::write_fmt(
            &mut id_text,
            format_args!(
                "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
                constraints.len()
            ),
        )
        .map_err(|_| {
            cadmpeg_core::CodecError::malformed(
                "cannot format SLDPRT shared endpoint constraint ID",
            )
        })?;
        let Ok(id) = SketchConstraintId::mint(id_text) else {
            continue;
        };
        let mut loci = Vec::new();
        ctx.reserve_vec(
            &mut loci,
            sources.len(),
            "collect SLDPRT shared endpoint constraint loci",
        )?;
        for (start, source) in sources {
            let source = cadmpeg_ir::sketches::SketchEntityId::mint(retained_id_text(
                ctx,
                source.as_str(),
                "retain SLDPRT shared endpoint entity ID",
            )?)
            .map_err(|_| {
                cadmpeg_core::CodecError::malformed("invalid admitted SLDPRT sketch entity ID")
            })?;
            loci.push(if start {
                SketchLocus::Start(source)
            } else {
                SketchLocus::End(source)
            });
        }
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci { loci },
        ) else {
            continue;
        };
        crate::annotations::note(
            ctx,
            annotations,
            retained_id_text(
                ctx,
                id.as_str(),
                "retain SLDPRT shared endpoint annotation ID",
            )?,
            stream,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        )?;
        let sketch = SketchId::mint(retained_id_text(
            ctx,
            sketch.as_str(),
            "retain SLDPRT constraint sketch ID",
        )?)
        .map_err(|_| cadmpeg_core::CodecError::malformed("invalid admitted SLDPRT sketch ID"))?;
        ctx.reserve_vec(constraints, 1, "collect SLDPRT shared endpoint constraints")?;
        constraints.push(SketchConstraint {
            id,
            sketch,
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

pub(super) fn project_edge(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    frame: SketchPlaneFrame,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<SketchGeometry>, cadmpeg_core::CodecError> {
    let SketchPlaneFrame {
        origin,
        u_axis,
        v_axis,
    } = frame;
    let Some(start_id) = vertices.get(&edge.start) else {
        return Ok(None);
    };
    let Some(start_point) = points.get(start_id) else {
        return Ok(None);
    };
    let start = project_point(*start_point, origin, u_axis, v_axis);
    let Some(end_id) = vertices.get(&edge.end) else {
        return Ok(None);
    };
    let Some(end_point) = points.get(end_id) else {
        return Ok(None);
    };
    let end = project_point(*end_point, origin, u_axis, v_axis);
    let line = || SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).ok();
    let Some(tolerance) = EdgeProjectionTolerance::of(edge) else {
        return Ok(None);
    };
    let tolerance = tolerance.get();
    if let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) =
        edge.curve().and_then(|id| curves.get(id).copied())
    {
        let count = nurbs.pole_count();
        let operation = "project SLDPRT sketch NURBS edge";
        let knots = nurbs.knots().try_clone_for_decode(ctx, operation)?;
        let mut projected = Vec::new();
        ctx.reserve_vec(&mut projected, count, operation)?;
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
                let mut weights = Vec::new();
                ctx.reserve_vec(&mut weights, count, operation)?;
                weights.extend(points.iter().map(|point| point.weight));
                Some(weights)
            }
        };
        let passes = if weights.is_some() { 2usize } else { 1usize };
        let admitted = count
            .checked_mul(passes)
            .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?;
        ctx.charge_collection_items(
            u64::try_from(admitted)
                .map_err(|_| ctx.refuse_codec_limit(operation, u64::MAX - 1, u64::MAX))?,
            operation,
        )?;
        return match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
            nurbs.degree(),
            knots,
            projected,
            weights,
            nurbs.periodic(),
        ) {
            Ok(nurbs) => Ok(Some(SketchGeometry::nurbs(nurbs))),
            Err(error) => {
                refusal.note(
                    ctx,
                    format_args!("sldprt projected sketch edge {}", edge.id),
                    &error,
                )?;
                Ok(None)
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
        Some(other) => cadmpeg_core::text::NonBlankString::new(retained_curve_debug(ctx, other)?)
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
                let parameters = edge
                    .param_range()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .filter(|[start, end]| start != end);
                Some(
                    SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                        center,
                        radius: cadmpeg_ir::scalar::Length::from(circle_curve.radius()),
                        start_angle: cadmpeg_ir::scalar::Angle::new(parameters.map_or_else(
                            || (start.v - center.v).atan2(start.u - center.u),
                            |range| range[0],
                        ))?,
                        end_angle: cadmpeg_ir::scalar::Angle::new(parameters.map_or_else(
                            || (end.v - center.v).atan2(end.u - center.u),
                            |range| range[1],
                        ))?,
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
                                parameters.map_or_else(|| parameter(start), |range| range[0]),
                            )?,
                            cadmpeg_ir::scalar::Angle::new(
                                parameters.map_or_else(|| parameter(end), |range| range[1]),
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
    Ok(projected)
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
