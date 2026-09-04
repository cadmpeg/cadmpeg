//! Sketch entity projection from B-rep edges.

use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity, SketchGeometry,
    SketchId, SketchLocus,
};
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap, HashSet};

const EPS_SKETCH_EDGES_PROJECT_EDGE_E9: f64 = 1.0e-9;
const EPS_SKETCH_EDGES_CIRCLE_CONTAINS_POINT_E9: f64 = 1.0e-9;
const EPS_SKETCH_EDGES_ELLIPSE_CONTAINS_POINT_E9: f64 = 1.0e-9;

#[allow(clippy::too_many_arguments)]
pub(super) fn project_endpoint_constraints(
    sketch: &SketchId,
    entities: &[SketchEntity],
    block_offset: usize,
    stream_ordinal: usize,
    face_ordinal: usize,
    section: &str,
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
        let id = SketchConstraintId(format!(
            "sldprt:model:sketch-constraint#{block_offset}:{stream_ordinal}:{face_ordinal}:{}",
            constraints.len()
        ));
        crate::annotations::note(
            annotations,
            id.0.clone(),
            section,
            0,
            "feature_input_shared_endpoint",
            Exactness::Derived,
        );
        constraints.push(SketchConstraint {
            id,
            sketch: sketch.clone(),
            definition: SketchConstraintDefinition::CoincidentLoci { loci },
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

pub(super) fn project_edge(
    edge: &cadmpeg_ir::topology::Edge,
    vertices: &HashMap<&cadmpeg_ir::ids::VertexId, &cadmpeg_ir::ids::PointId>,
    points: &HashMap<&cadmpeg_ir::ids::PointId, Point3>,
    curves: &HashMap<&cadmpeg_ir::ids::CurveId, &CurveGeometry>,
    origin: Point3,
    u_axis: Vector3,
    v_axis: Vector3,
) -> Option<SketchGeometry> {
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
    let line = || Some(SketchGeometry::Line { start, end });
    let tolerance = edge
        .tolerance
        .unwrap_or(EPS_SKETCH_EDGES_PROJECT_EDGE_E9)
        .max(EPS_SKETCH_EDGES_PROJECT_EDGE_E9);
    match edge.curve.as_ref().and_then(|id| curves.get(id).copied()) {
        Some(CurveGeometry::Circle { center, radius, .. }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            if !circle_contains_point(center, *radius, start, tolerance)
                || !circle_contains_point(center, *radius, end, tolerance)
            {
                return line();
            }
            if (start.u - end.u).hypot(start.v - end.v) <= EPS_SKETCH_EDGES_PROJECT_EDGE_E9 {
                Some(SketchGeometry::Circle {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                })
            } else {
                let parameters = edge
                    .param_range
                    .filter(|[start, end]| start.is_finite() && end.is_finite() && start != end);
                Some(SketchGeometry::Arc {
                    center,
                    radius: cadmpeg_ir::features::Length(*radius),
                    start_angle: cadmpeg_ir::features::Angle(parameters.map_or_else(
                        || (start.v - center.v).atan2(start.u - center.u),
                        |range| range[0],
                    )),
                    end_angle: cadmpeg_ir::features::Angle(parameters.map_or_else(
                        || (end.v - center.v).atan2(end.u - center.u),
                        |range| range[1],
                    )),
                })
            }
        }
        Some(CurveGeometry::Ellipse {
            center,
            major_direction,
            major_radius,
            minor_radius,
            ..
        }) => {
            let center = project_point(*center, origin, u_axis, v_axis);
            let major_u = major_direction.dot(u_axis);
            let major_v = major_direction.dot(v_axis);
            let major_angle = major_v.atan2(major_u);
            if !ellipse_contains_point(
                center,
                major_angle,
                *major_radius,
                *minor_radius,
                start,
                tolerance,
            ) || !ellipse_contains_point(
                center,
                major_angle,
                *major_radius,
                *minor_radius,
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
                (minor_component / *minor_radius).atan2(major_component / *major_radius)
            };
            let parameters = edge
                .param_range
                .filter(|[start, end]| start.is_finite() && end.is_finite() && start != end);
            Some(SketchGeometry::Ellipse {
                center,
                major_angle: cadmpeg_ir::features::Angle(major_angle),
                major_radius: cadmpeg_ir::features::Length(*major_radius),
                minor_radius: cadmpeg_ir::features::Length(*minor_radius),
                bounds: (!full).then(|| {
                    [
                        cadmpeg_ir::features::Angle(
                            parameters.map_or_else(|| parameter(start), |range| range[0]),
                        ),
                        cadmpeg_ir::features::Angle(
                            parameters.map_or_else(|| parameter(end), |range| range[1]),
                        ),
                    ]
                }),
            })
        }
        Some(CurveGeometry::Nurbs(nurbs)) => Some(SketchGeometry::Nurbs {
            degree: nurbs.degree(),
            knots: nurbs.knots().to_vec(),
            control_points: nurbs
                .control_points()
                .iter()
                .map(|point| project_point(*point, origin, u_axis, v_axis))
                .collect(),
            weights: nurbs.weights().map(<[f64]>::to_vec),
            periodic: nurbs.periodic(),
        }),
        None if edge.start == edge.end => Some(SketchGeometry::Point { position: start }),
        Some(CurveGeometry::Line { .. }) | None => line(),
        Some(other) => Some(SketchGeometry::Native {
            native_kind: format!("{other:?}"),
        }),
    }
}

pub(super) fn circle_contains_point(
    center: Point2,
    radius: f64,
    point: Point2,
    tolerance: f64,
) -> bool {
    let distance = (point.u - center.u).hypot(point.v - center.v);
    distance.is_finite()
        && radius.is_finite()
        && (distance - radius.abs()).abs()
            <= tolerance.max(radius.abs() * EPS_SKETCH_EDGES_CIRCLE_CONTAINS_POINT_E9)
}

pub(super) fn ellipse_contains_point(
    center: Point2,
    major_angle: f64,
    major_radius: f64,
    minor_radius: f64,
    point: Point2,
    tolerance: f64,
) -> bool {
    if !major_radius.is_finite()
        || !minor_radius.is_finite()
        || major_radius.abs() <= tolerance
        || minor_radius.abs() <= tolerance
    {
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
pub(super) fn dot(left: Vector3, right: Vector3) -> f64 {
    left.dot(right)
}

#[cfg(test)]
mod sketch_edges_tests;
