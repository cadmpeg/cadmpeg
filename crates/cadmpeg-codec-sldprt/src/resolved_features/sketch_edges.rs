//! Sketch entity projection from B-rep edges.

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinitionInput, SketchConstraintId, SketchEntity,
    SketchGeometry, SketchGeometryDefinition, SketchId, SketchLocus,
};
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap, HashSet};

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
    sketch: &SketchId,
    entities: &[SketchEntity],
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
    stream: &cadmpeg_ir::StreamName,
    annotations: &mut Annotations,
    constraints: &mut Vec<SketchConstraint>,
) {
    let mut loci_by_endpoint = BTreeMap::<&str, Vec<SketchLocus>>::new();
    for entity in entities {
        if entity.endpoint_refs.len() != 2 {
            continue;
        }
        for (index, endpoint) in entity.endpoint_refs.iter().enumerate() {
            let locus = if index == 0 {
                SketchLocus::Start(entity.id().clone())
            } else {
                SketchLocus::End(entity.id().clone())
            };
            loci_by_endpoint.entry(endpoint).or_default().push(locus);
        }
    }
    for (_endpoint, loci) in loci_by_endpoint {
        let distinct_entities = loci
            .iter()
            .map(|locus| match locus {
                SketchLocus::Start(entity)
                | SketchLocus::End(entity)
                | SketchLocus::Center(entity)
                | SketchLocus::Entity(entity) => entity,
            })
            .collect::<HashSet<_>>();
        if distinct_entities.len() < 2 {
            continue;
        }
        let Ok(id) = SketchConstraintId::mint(format!(
            "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
            constraints.len()
        )) else {
            continue;
        };
        let Ok(definition) = cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::CoincidentLoci { loci },
        ) else {
            continue;
        };
        crate::annotations::note(
            annotations,
            id.as_str().to_owned(),
            stream,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        );
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
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    frame: SketchPlaneFrame,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<SketchGeometry> {
    let SketchPlaneFrame {
        origin,
        u_axis,
        v_axis,
    } = frame;
    let start = project_point(
        *points.get(vertices.get(&edge.start)?)?,
        origin,
        u_axis,
        v_axis,
    );
    let end = project_point(
        *points.get(vertices.get(&edge.end)?)?,
        origin,
        u_axis,
        v_axis,
    );
    let line = || SketchGeometry::try_from(SketchGeometryDefinition::Line { start, end }).ok();
    let tolerance = EdgeProjectionTolerance::of(edge)?.get();
    match edge.curve().and_then(|id| curves.get(id).copied()) {
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
                    major_radius: cadmpeg_ir::scalar::Length::from(ellipse_curve.major_radius()),
                    minor_radius: cadmpeg_ir::scalar::Length::from(ellipse_curve.minor_radius()),
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
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) => {
            match cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_checked_lanes(
                nurbs.degree(),
                nurbs.knots().to_vec(),
                nurbs
                    .control_points()
                    .iter()
                    .map(|point| project_point(point.get(), origin, u_axis, v_axis))
                    .collect::<Vec<_>>(),
                nurbs.weights(),
                nurbs.periodic(),
            ) {
                Ok(nurbs) => Some(SketchGeometry::nurbs(nurbs)),
                Err(error) => {
                    refusal.note(
                        format_args!("sldprt projected sketch edge {}", edge.id),
                        &error,
                    );
                    None
                }
            }
        }
        None if edge.start == edge.end => Some(
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position: start }).ok()?,
        ),
        Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(_))) | None => line(),
        Some(other) => Some(SketchGeometry::native(
            cadmpeg_core::text::NonBlankString::new(format!("{other:?}"))?,
        )),
    }
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
