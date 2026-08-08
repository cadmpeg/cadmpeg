// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    analytic_surface_parameters, nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point,
    nurbs_pcurve_parameter_domain, nurbs_pcurve_parameter_near_point, pcurve_uv,
};
use cadmpeg_ir::geometry::{
    derive_reference_direction, CompositeCurveSegment, CompositeCurveTransition, Curve,
    CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{LossKind, LossNote, Severity};
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::SourceObjectAssociation;

use crate::parse::{Exchange, RawRecord, Value};

use super::index::{step_instance_id, CarrierIndex};
use super::opaque_record_id;

const RANGE_INFERENCE_WORK_UNITS: u64 = 4_096;

pub(super) struct GeometryResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
    pub placements: BTreeMap<u64, (Point3, Vector3, Vector3)>,
    pub length_scale: f64,
    pub plane_angle_scale: f64,
}

/// Populate edge parameter ranges from the STEP edge endpoint witnesses when
/// the native `EDGE_CURVE` does not carry an explicit range.
///
/// Part 21 edges commonly trim an unbounded or periodic carrier by their two
/// `VERTEX_POINT` references. The neutral edge range is needed by pcurve
/// consistency checks; using the carrier's complete domain would compare a
/// trimmed edge with unrelated points on the same circle or spline.
pub(super) fn infer_edge_parameter_ranges(
    ir: &mut CadIr,
    ctx: Option<&cadmpeg_core::decode::DecodeContext<'_>>,
) -> Result<(), CodecError> {
    let points = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.0.as_str(), point.position))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            points
                .get(vertex.point.0.as_str())
                .copied()
                .map(|point| (vertex.id.0.as_str(), point))
        })
        .collect::<HashMap<_, _>>();
    let candidates = ir
        .model
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.param_range.is_none())
        .filter_map(|(index, edge)| {
            let curve = edge.curve.clone()?;
            let start = vertices.get(edge.start.0.as_str()).copied()?;
            let end = vertices.get(edge.end.0.as_str()).copied()?;
            Some((index, curve, start, end))
        })
        .collect::<Vec<_>>();
    charge_range_inference(ctx, candidates.len(), "step_edge_parameter_inference")?;

    let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
    let inferred = candidates
        .into_iter()
        .filter_map(|(edge_index, curve, start, end)| {
            let start_parameter = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                &model_index,
                &curve,
                start,
                0.0,
            )?;
            let end_parameter = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                &model_index,
                &curve,
                end,
                start_parameter,
            )?;
            let curve_geometry = model_index.curves(curve.0.as_str())?.geometry.clone();
            let [start_parameter, end_parameter] =
                edge_parameter_range(&curve_geometry, start_parameter, end_parameter)?;
            Some((edge_index, [start_parameter, end_parameter]))
        })
        .collect::<Vec<_>>();
    drop(model_index);

    for (index, range) in inferred {
        if let Some(edge) = ir.model.edges.get_mut(index) {
            edge.param_range = Some(range);
        }
    }
    Ok(())
}

fn edge_parameter_range(geometry: &CurveGeometry, start: f64, end: f64) -> Option<[f64; 2]> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let periodic_domain = match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => {
            Some([0.0, std::f64::consts::TAU])
        }
        CurveGeometry::Nurbs(nurbs) if nurbs.periodic => nurbs_curve_parameter_domain(nurbs),
        _ => None,
    };
    let Some([lower, upper]) = periodic_domain else {
        return (end > start).then_some([start, end]);
    };
    let period = upper - lower;
    if !period.is_finite() || period <= 0.0 {
        return None;
    }
    let mut sweep = end - start;
    while sweep < 0.0 {
        sweep += period;
    }
    let tolerance = 1.0e-9_f64.max(period.abs() * 1.0e-9);
    if sweep <= 0.0 || sweep > period + tolerance {
        return None;
    }
    let normalized_start = lower + (start - lower).rem_euclid(period);
    let normalized_start = if (normalized_start - upper).abs() <= tolerance {
        lower
    } else {
        normalized_start
    };
    Some([normalized_start, normalized_start + sweep.min(period)])
}

/// Recover pcurve-use intervals when a STEP file supplies only the two
/// topological endpoint witnesses. A pcurve and its model-space edge are
/// allowed to use different neutral parameters, so the edge interval is only
/// a search seed; the stored range belongs to the pcurve itself.
pub(super) fn infer_pcurve_parameter_ranges(
    ir: &mut CadIr,
    ctx: Option<&cadmpeg_core::decode::DecodeContext<'_>>,
) -> Result<(), CodecError> {
    let points = ir
        .model
        .points
        .iter()
        .map(|point| (point.id.0.as_str(), point.position))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| {
            points
                .get(vertex.point.0.as_str())
                .copied()
                .map(|point| (vertex.id.0.as_str(), point))
        })
        .collect::<HashMap<_, _>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (surface.id.0.as_str(), &surface.geometry))
        .collect::<HashMap<_, _>>();
    let faces = ir
        .model
        .faces
        .iter()
        .map(|face| (face.id.0.as_str(), face))
        .collect::<HashMap<_, _>>();
    let loops = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.0.as_str(), loop_))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.0.as_str(), edge))
        .collect::<HashMap<_, _>>();
    let pcurves = ir
        .model
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.0.as_str(), &pcurve.geometry))
        .collect::<HashMap<_, _>>();
    let candidates = ir
        .model
        .coedges
        .iter()
        .enumerate()
        .filter(|(_, coedge)| {
            coedge.pcurves.len() == 1 && coedge.pcurves[0].parameter_range.is_none()
        })
        .filter_map(|(coedge_index, coedge)| {
            let pcurve = pcurves.get(coedge.pcurves[0].pcurve.0.as_str())?;
            let face = loops
                .get(coedge.owner_loop.0.as_str())
                .and_then(|loop_| faces.get(loop_.face.0.as_str()))?;
            let surface = surfaces.get(face.surface.0.as_str())?;
            let edge = edges.get(coedge.edge.0.as_str())?;
            let start = vertices.get(edge.start.0.as_str()).copied()?;
            let end = vertices.get(edge.end.0.as_str()).copied()?;
            let seed_range = edge.param_range.or_else(|| pcurve_parameter_domain(pcurve));
            Some((
                coedge_index,
                start,
                end,
                (*surface).clone(),
                (*pcurve).clone(),
                seed_range.unwrap_or([0.0, 1.0]),
            ))
        })
        .collect::<Vec<_>>();
    charge_range_inference(ctx, candidates.len(), "step_pcurve_parameter_inference")?;
    let tolerance = ir.tolerances.linear.max(1.0e-9);

    for (coedge_index, start, end, surface, pcurve, seed_range) in candidates {
        let Some(start_uv) = analytic_surface_parameters(&surface, start) else {
            continue;
        };
        let Some(end_uv) = analytic_surface_parameters(&surface, end) else {
            continue;
        };
        let Some(start_parameter) =
            pcurve_parameter_near_uv(&pcurve, start_uv, seed_range[0], tolerance)
        else {
            continue;
        };
        let Some(end_parameter) =
            pcurve_parameter_near_uv(&pcurve, end_uv, seed_range[1], tolerance)
        else {
            continue;
        };
        if !start_parameter.is_finite()
            || !end_parameter.is_finite()
            || start_parameter == end_parameter
        {
            continue;
        }
        if let Some(use_) = ir
            .model
            .coedges
            .get_mut(coedge_index)
            .and_then(|coedge| coedge.pcurves.first_mut())
        {
            use_.parameter_range = Some([start_parameter, end_parameter]);
        }
    }
    Ok(())
}

fn charge_range_inference(
    ctx: Option<&cadmpeg_core::decode::DecodeContext<'_>>,
    candidate_count: usize,
    operation: &'static str,
) -> Result<(), CodecError> {
    let count = u64::try_from(candidate_count).unwrap_or(u64::MAX);
    let units = count.saturating_mul(RANGE_INFERENCE_WORK_UNITS);
    ctx.map_or(Ok(()), |ctx| ctx.charge_work(units, operation))
}

fn pcurve_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            ..
        } => nurbs_pcurve_parameter_domain(*degree, knots, control_points.len()),
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            ..
        } => nurbs_pcurve_parameter_domain(*degree, knots, radial_control_points.len()),
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        _ => None,
    }
}

fn pcurve_parameter_near_uv(
    geometry: &PcurveGeometry,
    target: Point2,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    let parameter = match geometry {
        PcurveGeometry::Line { origin, direction } => {
            let delta = Point2::new(target.u - origin.u, target.v - origin.v);
            let denominator = direction.u * direction.u + direction.v * direction.v;
            (denominator.is_finite() && denominator > 0.0)
                .then(|| (delta.u * direction.u + delta.v * direction.v) / denominator)?
        }
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let delta = Point2::new(target.u - center.u, target.v - center.v);
            let x = delta.u * x_axis.u + delta.v * x_axis.v;
            let y = delta.u * y_axis.u + delta.v * y_axis.v;
            let canonical = (y / radius).atan2(x / radius);
            nearest_periodic_parameter(canonical, seed)
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            if *major_radius == 0.0 || *minor_radius == 0.0 {
                return None;
            }
            let delta = Point2::new(target.u - center.u, target.v - center.v);
            let x = delta.u * x_axis.u + delta.v * x_axis.v;
            let y = delta.u * y_axis.u + delta.v * y_axis.v;
            let canonical = (y / minor_radius).atan2(x / major_radius);
            nearest_periodic_parameter(canonical, seed)
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => nurbs_pcurve_parameter_near_point(
            *degree,
            knots,
            control_points,
            weights.as_deref(),
            target,
            tolerance,
            seed,
        )?,
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
        } => {
            let parameter = pcurve_parameter_near_uv(basis, target, seed, tolerance)?;
            parameter_range.contains(&parameter).then_some(parameter)?
        }
        PcurveGeometry::Offset { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. } => return None,
    };
    let evaluated = pcurve_uv(geometry, parameter)?;
    ((evaluated.u - target.u).hypot(evaluated.v - target.v) <= tolerance).then_some(parameter)
}

fn nearest_periodic_parameter(canonical: f64, seed: f64) -> f64 {
    canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> GeometryResult {
    let mut losses = Vec::new();
    let scale = length_scale(exchange).unwrap_or_else(|| {
        losses.push(unresolved_unit_loss(
            "the document length unit did not resolve; coordinates are unscaled and reported as millimetres",
        ));
        1.0
    });
    let angle_scale = plane_angle_scale(exchange).unwrap_or_else(|| {
        losses.push(unresolved_unit_loss(
            "the document plane-angle unit did not resolve; angles are unscaled and reported as radians",
        ));
        1.0
    });
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut points = BTreeMap::new();
    let mut points2 = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut directions2 = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut vectors2 = BTreeMap::new();
    let mut placements = BTreeMap::new();
    let mut placements2 = BTreeMap::new();
    if let Some(uncertainty) = linear_uncertainty(exchange) {
        ir.tolerances.linear = uncertainty;
    }

    for (id, record) in exchange.entities_any(&["CARTESIAN_POINT", "DIRECTION"]) {
        match record.simple_name() {
            Some("CARTESIAN_POINT") => {
                if let Some(position) = coordinates(record, 1, scale) {
                    points.insert(id, position);
                    typed.insert(id);
                } else if let Some(position) = coordinates2(record, 1) {
                    points2.insert(id, position);
                    typed.insert(id);
                } else {
                    warnings.push(format!("CARTESIAN_POINT #{id} has invalid coordinates"));
                }
            }
            Some("DIRECTION") => {
                if let Some(direction) = vector3(record.parameter(1), 1.0).and_then(normalize) {
                    directions.insert(id, direction);
                    typed.insert(id);
                } else if let Some(direction) = vector2(record.parameter(1)).and_then(normalize2) {
                    directions2.insert(id, direction);
                    typed.insert(id);
                } else {
                    warnings.push(format!("DIRECTION #{id} is invalid or zero"));
                }
            }
            _ => {}
        }
    }

    let mut point_carriers = BTreeSet::new();
    for record in exchange.records.values() {
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "VERTEX_POINT")
        {
            if let Some(id) = vertex_point_reference(record) {
                point_carriers.insert(id);
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| partial.name.ends_with("REPRESENTATION"))
        {
            if let Some(items) = representation_items(record) {
                point_carriers.extend(items.into_iter().filter(|id| points.contains_key(id)));
            }
        }
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
            )
        }) {
            if let Some(items) = first_named_list(record, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"])
            {
                point_carriers.extend(items.into_iter().filter(|id| points.contains_key(id)));
            }
        }
        if record
            .partials
            .iter()
            .any(|partial| partial.name == "POLY_LOOP")
        {
            if let Some(items) = first_named_list(record, &["POLY_LOOP"]) {
                point_carriers.extend(items.into_iter().filter(|id| points.contains_key(id)));
            }
        }
        if matches!(
            record.simple_name(),
            Some("STYLED_ITEM" | "OVER_RIDING_STYLED_ITEM")
        ) {
            if let Some(id) = record.parameter(2).and_then(Value::reference) {
                if points.contains_key(&id) {
                    point_carriers.insert(id);
                }
            }
        }
    }
    ir.model
        .points
        .extend(point_carriers.into_iter().filter_map(|id| {
            points.get(&id).copied().map(|position| Point {
                source_object: None,
                id: PointId(format!("step:data:point#{id}")),
                position,
            })
        }));
    for (id, record) in exchange.entities("VECTOR") {
        if record.partial("VECTOR").is_some() {
            let value = named_parameter(record, "VECTOR", 1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(named_parameter(record, "VECTOR", 2).and_then(Value::number))
                .map(|(direction, magnitude)| scale_vector(direction, magnitude * scale));
            let value2 = named_parameter(record, "VECTOR", 1)
                .and_then(Value::reference)
                .and_then(|direction| directions2.get(&direction).copied())
                .zip(named_parameter(record, "VECTOR", 2).and_then(Value::number))
                .map(|(direction, magnitude)| {
                    Point2::new(direction.u * magnitude, direction.v * magnitude)
                });
            if let Some(value) = value {
                vectors.insert(id, value);
                typed.insert(id);
            } else if let Some(value) = value2 {
                vectors2.insert(id, value);
                typed.insert(id);
            } else {
                warnings.push(format!(
                    "VECTOR #{id} has an invalid direction or magnitude"
                ));
            }
        }
    }
    for (id, record) in exchange.entities_any(&["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"]) {
        let Some(placement_type) = entity_type(record, &["AXIS2_PLACEMENT_3D", "AXIS1_PLACEMENT"])
        else {
            continue;
        };
        {
            let placement = named_parameter(record, placement_type, 1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis = optional_direction(
                        named_parameter(record, placement_type, 2),
                        &directions,
                    )
                        .unwrap_or(Vector3::new(0.0, 0.0, 1.0));
                    let reference = match optional_direction(
                        named_parameter(record, placement_type, 3),
                        &directions,
                    ) {
                        Some(reference) => {
                            if let Some(reference) = orthogonal_reference(axis, reference) {
                                reference
                            } else {
                                losses.push(LossNote {
                                    code: LossKind::CarrierAxisInferred,
                                    severity: Severity::Warning,
                                    message: format!(
                                        "AXIS2_PLACEMENT_3D #{id} has a reference direction parallel to its axis; inferred an orthogonal reference"
                                    ),
                                    provenance: None,
                                });
                                derive_reference_direction(axis)
                            }
                        }
                        None => derive_reference_direction(axis),
                    };
                    (origin, axis, reference)
                });
            if let Some(placement) = placement {
                placements.insert(id, placement);
                typed.insert(id);
            } else {
                warnings.push(format!("AXIS2_PLACEMENT_3D #{id} has an invalid location"));
            }
        }
    }
    let mut transformation_operators = BTreeMap::new();
    for (id, record) in exchange.entities("CARTESIAN_TRANSFORMATION_OPERATOR_3D") {
        if let Some(transform) = cartesian_transformation_operator(record, &points, &directions) {
            transformation_operators.insert(id, transform);
            typed.insert(id);
        } else {
            warnings.push(format!(
                "CARTESIAN_TRANSFORMATION_OPERATOR_3D #{id} has invalid axes, origin, or scale"
            ));
        }
    }
    for (id, record) in exchange.entities("AXIS2_PLACEMENT_2D") {
        if record.partial("AXIS2_PLACEMENT_2D").is_none() {
            continue;
        }
        let placement = named_parameter(record, "AXIS2_PLACEMENT_2D", 1)
            .and_then(Value::reference)
            .and_then(|point| points2.get(&point).copied())
            .and_then(|origin| {
                let x_axis = match named_parameter(record, "AXIS2_PLACEMENT_2D", 2) {
                    Some(Value::Reference(direction)) => directions2.get(direction).copied()?,
                    Some(Value::Omitted) | None => Point2::new(1.0, 0.0),
                    _ => return None,
                };
                Some((origin, x_axis, Point2::new(-x_axis.v, x_axis.u)))
            });
        if let Some(placement) = placement {
            placements2.insert(id, placement);
            typed.insert(id);
        } else {
            warnings.push(format!("AXIS2_PLACEMENT_2D #{id} has an invalid location"));
        }
    }
    let mut pcurve_geometries = BTreeMap::<u64, (PcurveGeometry, BTreeSet<u64>)>::new();
    let mut pcurve_geometry_records = BTreeSet::new();
    for (_, record) in exchange.entities("PCURVE") {
        if record.partial("PCURVE").is_none() {
            continue;
        }
        let Some(items) = named_parameter(record, "PCURVE", 2)
            .and_then(Value::reference)
            .and_then(|representation| exchange.records.get(&representation))
            .and_then(|representation| representation.parameter(1))
            .and_then(Value::list)
        else {
            continue;
        };
        let decoded = items
            .iter()
            .filter_map(Value::reference)
            .filter_map(|curve| {
                decode_pcurve_geometry(
                    curve,
                    exchange,
                    &points2,
                    &vectors2,
                    &placements2,
                    angle_scale,
                    &mut warnings,
                    &mut BTreeSet::new(),
                    0,
                )
                .map(|decoded| (curve, decoded))
            })
            .collect::<Vec<_>>();
        if let [(curve, decoded)] = decoded.as_slice() {
            pcurve_geometry_records.extend(decoded.1.iter().copied());
            pcurve_geometries.insert(*curve, decoded.clone());
        }
    }
    for (id, record) in exchange.entities_any(&[
        "LINE",
        "CIRCLE",
        "ELLIPSE",
        "PARABOLA",
        "HYPERBOLA",
        "POLYLINE",
        "B_SPLINE_CURVE_WITH_KNOTS",
    ]) {
        let Some(curve_type) = entity_type(
            record,
            &[
                "LINE",
                "CIRCLE",
                "ELLIPSE",
                "PARABOLA",
                "HYPERBOLA",
                "POLYLINE",
                "B_SPLINE_CURVE_WITH_KNOTS",
            ],
        ) else {
            continue;
        };
        if pcurve_geometry_records.contains(&id) {
            continue;
        }
        if curve_type == "B_SPLINE_CURVE_WITH_KNOTS" && record.simple_name().is_none() {
            continue;
        }
        let geometry = match curve_type {
            "LINE" => named_parameter(record, "LINE", 1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    named_parameter(record, "LINE", 2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| CurveGeometry::Line { origin, direction }),
            "CIRCLE" => named_parameter(record, "CIRCLE", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "CIRCLE", 2).and_then(Value::number))
                .filter(|(_, radius)| radius.is_finite() && *radius > 0.0)
                .map(
                    |((center, axis, ref_direction), radius)| CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            "ELLIPSE" => named_parameter(record, "ELLIPSE", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "ELLIPSE", 2).and_then(Value::number))
                .zip(named_parameter(record, "ELLIPSE", 3).and_then(Value::number))
                .filter(|((_, major), minor)| {
                    major.is_finite() && minor.is_finite() && *major > 0.0 && *minor > 0.0
                })
                .map(
                    |(((center, axis, major_direction), major_radius), minor_radius)| {
                        CurveGeometry::Ellipse {
                            center,
                            axis,
                            major_direction,
                            major_radius: major_radius * scale,
                            minor_radius: minor_radius * scale,
                        }
                    },
                ),
            "PARABOLA" => named_parameter(record, "PARABOLA", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "PARABOLA", 2).and_then(Value::number))
                .filter(|(_, focal_distance)| focal_distance.is_finite() && *focal_distance > 0.0)
                .map(
                    |((vertex, axis, major_direction), focal_distance)| CurveGeometry::Parabola {
                        vertex,
                        axis,
                        major_direction,
                        focal_distance: focal_distance * scale,
                    },
                ),
            "HYPERBOLA" => named_parameter(record, "HYPERBOLA", 1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(named_parameter(record, "HYPERBOLA", 2).and_then(Value::number))
                .zip(named_parameter(record, "HYPERBOLA", 3).and_then(Value::number))
                .filter(|((_, major), minor)| {
                    major.is_finite() && minor.is_finite() && *major > 0.0 && *minor > 0.0
                })
                .map(
                    |(((center, axis, major_direction), major_radius), minor_radius)| {
                        CurveGeometry::Hyperbola {
                            center,
                            axis,
                            major_direction,
                            major_radius: major_radius * scale,
                            minor_radius: minor_radius * scale,
                        }
                    },
                ),
            "POLYLINE" => polyline(record, &points).map(CurveGeometry::Nurbs),
            "B_SPLINE_CURVE_WITH_KNOTS" => {
                nurbs_curve(record, &points, &mut warnings).map(CurveGeometry::Nurbs)
            }
            _ => unreachable!("curve type was selected from the dispatch list"),
        };
        if let Some(geometry) = geometry {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!("{curve_type} #{id} has invalid geometry"));
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_CURVE_WITH_KNOTS") {
        if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_CURVE_WITH_KNOTS")
            || pcurve_geometry_records.contains(&id)
        {
            continue;
        }
        if let Some(nurbs) = nurbs_curve(record, &points, &mut warnings) {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry: CurveGeometry::Nurbs(nurbs),
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "B_SPLINE_CURVE_WITH_KNOTS #{id} has invalid geometry"
            ));
        }
    }

    // STEP geometry is a graph, not an ordered stream. Resolve all deferred
    // curve constructors to a fixpoint so nested or forward references do not
    // disappear merely because their source record has a larger instance id.
    let mut carrier_index = CarrierIndex::from_ir(ir);
    let deferred_ids = exchange
        .entities_any(&[
            "TRIMMED_CURVE",
            "COMPOSITE_CURVE",
            "BOUNDARY_CURVE",
            "OUTER_BOUNDARY_CURVE",
            "OFFSET_CURVE_3D",
        ])
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    let mut deferred_queue = VecDeque::from(deferred_ids);
    let mut waiting_on = HashMap::<u64, Vec<u64>>::new();
    while let Some(id) = deferred_queue.pop_front() {
        if carrier_index.curves.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        if let Some(parameters) = entity_parameters(record, "TRIMMED_CURVE") {
            let Some((basis_step, sense, master_representation)) =
                trimmed_curve_attributes(parameters)
            else {
                continue;
            };
            if !carrier_index.curves.contains_key(&basis_step) {
                waiting_on.entry(basis_step).or_default().push(id);
                continue;
            }
            let curve = CurveId(format!("step:data:curve#{id}"));
            let basis = CurveId(format!("step:data:curve#{basis_step}"));
            let Some(geometry) = carrier_index
                .curves
                .get(&basis_step)
                .and_then(|index| ir.model.curves.get(*index))
                .map(|candidate| candidate.geometry.clone())
            else {
                continue;
            };
            let linear_parameter_scale =
                line_parameter_scale(exchange, basis_step, scale, &mut losses);
            let (start, end) = {
                let mut trim_context = TrimParameterContext {
                    points: &points,
                    geometry: &geometry,
                    angle_scale,
                    linear_parameter_scale,
                    tolerance: ir.tolerances.linear,
                    master_representation,
                    record_id: id,
                    warnings: &mut warnings,
                };
                (
                    parameters
                        .get(2)
                        .and_then(|value| trim_parameter(value, &mut trim_context)),
                    parameters
                        .get(3)
                        .and_then(|value| trim_parameter(value, &mut trim_context)),
                )
            };
            let Some((start, end)) = start.zip(end) else {
                continue;
            };
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: curve.clone(),
                geometry,
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: ProceduralCurveId(format!("step:construction:trimmed_curve#{id}")),
                curve: curve.clone(),
                definition: ProceduralCurveDefinition::Subset {
                    source: basis,
                    parameter_range: if sense { [start, end] } else { [end, start] },
                },
                cache_fit_tolerance: Some(0.0),
            });
            carrier_index.curves.insert(id, curve_index);
            typed.insert(id);
            wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
            continue;
        }
        if composite_curve_partial(record).is_some() {
            let missing = composite_curve_dependencies(record, exchange)
                .into_iter()
                .filter(|dependency| !carrier_index.curves.contains_key(dependency))
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                for dependency in missing {
                    waiting_on.entry(dependency).or_default().push(id);
                }
                continue;
            }
            let Some((segments, self_intersect)) =
                composite_curve(record, exchange, &carrier_index)
            else {
                continue;
            };
            let curve = CurveId(format!("step:data:curve#{id}"));
            typed.extend(segments.iter().map(|(segment, _)| *segment));
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Composite {
                    segments: segments.into_iter().map(|(_, segment)| segment).collect(),
                    self_intersect,
                },
                source_object: None,
            });
            carrier_index.curves.insert(id, curve_index);
            typed.insert(id);
            wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
            continue;
        }
        let Some(parameters) = entity_parameters(record, "OFFSET_CURVE_3D") else {
            continue;
        };
        let source_step = parameters.get(1).and_then(Value::reference);
        let source = source_step.map(|source| CurveId(format!("step:data:curve#{source}")));
        let distance = parameters.get(2).and_then(Value::number);
        let self_intersect = parameters
            .get(3)
            .and_then(logical_value)
            .map(StepLogical::into_option);
        let reference_direction = parameters
            .get(4)
            .and_then(Value::reference)
            .and_then(|direction| directions.get(&direction).copied());
        let Some((source, distance, self_intersect, reference_direction)) = source
            .zip(distance)
            .zip(self_intersect)
            .zip(reference_direction)
            .map(|(((source, distance), self_intersect), direction)| {
                (source, distance, self_intersect, direction)
            })
        else {
            continue;
        };
        let Some(source_step) = source_step else {
            continue;
        };
        if !carrier_index.curves.contains_key(&source_step) {
            waiting_on.entry(source_step).or_default().push(id);
            continue;
        }
        let Some(geometry) = carrier_index
            .curves
            .get(&source_step)
            .and_then(|index| ir.model.curves.get(*index))
            .map(|candidate| candidate.geometry.clone())
        else {
            continue;
        };
        let curve = CurveId(format!("step:data:curve#{id}"));
        let curve_index = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry,
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("step:construction:offset_curve#{id}")),
            curve: curve.clone(),
            definition: ProceduralCurveDefinition::SpatialOffset {
                source,
                distance: distance * scale,
                reference_direction,
                self_intersect,
            },
            cache_fit_tolerance: None,
        });
        carrier_index.curves.insert(id, curve_index);
        typed.insert(id);
        wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
    }
    let curve_replica_count = exchange.entities("CURVE_REPLICA").count();
    for _ in 0..=curve_replica_count {
        let mut progress = false;
        for (id, record) in exchange.entities("CURVE_REPLICA") {
            if carrier_index.curves.contains_key(&id) {
                continue;
            }
            let Some(parent_step) =
                named_parameter(record, "CURVE_REPLICA", 1).and_then(Value::reference)
            else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(record, "CURVE_REPLICA", 2).and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.curves.get(&parent_step).copied() else {
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .curves
                .get(parent_index)
                .map(|curve| curve.geometry.clone())
            else {
                continue;
            };
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry: CurveGeometry::Transformed {
                    basis: Box::new(basis),
                    transform,
                },
                source_object: None,
            });
            carrier_index.curves.insert(id, curve_index);
            typed.insert(id);
            typed.insert(operator_step);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    for (id, _) in exchange.entities("CURVE_REPLICA") {
        if let Entry::Vacant(entry) = carrier_index.curves.entry(id) {
            warnings.push(format!(
                "CURVE_REPLICA #{id} has invalid or unresolved parent/operator"
            ));
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry: CurveGeometry::Unknown {
                    record: exchange.records.get(&id).map(opaque_record_id),
                },
                source_object: None,
            });
            entry.insert(curve_index);
        }
    }
    for (id, _) in exchange
        .entities("TRIMMED_CURVE")
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "TRIMMED_CURVE #{id} has invalid or unresolved basis/trim selectors"
            ));
        }
    }
    for (id, _) in exchange.entities("COMPOSITE_CURVE") {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "COMPOSITE_CURVE #{id} has invalid, cyclic, or unresolved segments"
            ));
        }
    }
    for (id, record) in exchange.entities_any(&["BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"]) {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "{} #{id} has invalid, cyclic, or unresolved segments",
                record.simple_name().unwrap_or("BOUNDARY_CURVE")
            ));
        }
    }
    for (id, _) in exchange.entities("OFFSET_CURVE_3D") {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "OFFSET_CURVE_3D #{id} has invalid or unresolved basis parameters"
            ));
        }
    }
    for (id, _) in exchange
        .entities_any(&[
            "TRIMMED_CURVE",
            "COMPOSITE_CURVE",
            "BOUNDARY_CURVE",
            "OUTER_BOUNDARY_CURVE",
            "OFFSET_CURVE_3D",
        ])
        .filter(|(id, _)| !pcurve_geometry_records.contains(id))
    {
        if let Entry::Vacant(entry) = carrier_index.curves.entry(id) {
            let curve = CurveId(format!("step:data:curve#{id}"));
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Unknown {
                    record: exchange.records.get(&id).map(opaque_record_id),
                },
                source_object: None,
            });
            warnings.push(format!(
                "retained unresolved deferred curve #{id} as an unknown carrier"
            ));
            entry.insert(curve_index);
        }
    }
    for (id, record) in
        exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])
    {
        let Some(basis) = surface_curve_basis(record) else {
            warnings.push(format!(
                "{} #{id} has no decoded 3D curve",
                record.simple_name().unwrap_or("SURFACE_CURVE")
            ));
            continue;
        };
        if carrier_index.curves.contains_key(&basis) {
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has no decoded 3D curve",
                record.simple_name().unwrap_or("SURFACE_CURVE")
            ));
        }
    }

    for (id, record) in
        exchange.entities_any(&["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"])
    {
        let definition = match record.simple_name() {
            Some("SURFACE_OF_LINEAR_EXTRUSION") => record
                .parameter(1)
                .and_then(Value::reference)
                .filter(|curve| carrier_index.curves.contains_key(curve))
                .map(|curve| CurveId(format!("step:data:curve#{curve}")))
                .zip(
                    record
                        .parameter(2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied()),
                )
                .map(
                    |(directrix, direction)| ProceduralSurfaceDefinition::LinearSweep {
                        directrix,
                        direction,
                    },
                ),
            Some("SURFACE_OF_REVOLUTION") => record
                .parameter(1)
                .and_then(Value::reference)
                .filter(|curve| carrier_index.curves.contains_key(curve))
                .map(|curve| CurveId(format!("step:data:curve#{curve}")))
                .zip(
                    record
                        .parameter(2)
                        .and_then(Value::reference)
                        .and_then(|placement| placements.get(&placement).copied()),
                )
                .map(|(directrix, (axis_origin, axis_direction, _))| {
                    ProceduralSurfaceDefinition::AxisRevolution {
                        directrix,
                        axis_origin,
                        axis_direction,
                    }
                }),
            _ => continue,
        };
        let Some(definition) = definition else {
            warnings.push(format!(
                "{} #{id} has an unresolved directrix, vector, or axis",
                record.simple_name().expect("matched swept surface")
            ));
            continue;
        };
        let surface = SurfaceId(format!("step:data:surface#{id}"));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("step:construction:swept_surface#{id}")),
            surface,
            definition,
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        typed.insert(id);
    }

    for (id, record) in exchange.entities_any(&[
        "PLANE",
        "CYLINDRICAL_SURFACE",
        "CONICAL_SURFACE",
        "SPHERICAL_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
        "TOROIDAL_SURFACE",
        "B_SPLINE_SURFACE_WITH_KNOTS",
    ]) {
        let Some(surface_type) = entity_type(
            record,
            &[
                "PLANE",
                "CYLINDRICAL_SURFACE",
                "CONICAL_SURFACE",
                "SPHERICAL_SURFACE",
                "DEGENERATE_TOROIDAL_SURFACE",
                "TOROIDAL_SURFACE",
                "B_SPLINE_SURFACE_WITH_KNOTS",
            ],
        ) else {
            continue;
        };
        if surface_type == "B_SPLINE_SURFACE_WITH_KNOTS" && record.simple_name().is_none() {
            continue;
        }
        let placement = named_parameter(record, surface_type, 1)
            .and_then(Value::reference)
            .and_then(|placement| placements.get(&placement).copied());
        let geometry = match surface_type {
            "PLANE" => placement.map(|(origin, normal, u_axis)| SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            }),
            "CYLINDRICAL_SURFACE" => placement
                .zip(positive(named_parameter(record, "CYLINDRICAL_SURFACE", 2)))
                .map(
                    |((origin, axis, ref_direction), radius)| SurfaceGeometry::Cylinder {
                        origin,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            "CONICAL_SURFACE" => placement
                .zip(nonnegative(named_parameter(record, "CONICAL_SURFACE", 2)))
                .zip(named_parameter(record, "CONICAL_SURFACE", 3).and_then(Value::number))
                .filter(|(_, angle)| angle.is_finite())
                .map(|(((origin, axis, ref_direction), radius), half_angle)| {
                    SurfaceGeometry::Cone {
                        origin,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                        ratio: 1.0,
                        half_angle: half_angle * angle_scale,
                    }
                }),
            "SPHERICAL_SURFACE" => placement
                .zip(positive(named_parameter(record, "SPHERICAL_SURFACE", 2)))
                .map(
                    |((center, axis, ref_direction), radius)| SurfaceGeometry::Sphere {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            "TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => placement
                .zip(positive(named_parameter(record, surface_type, 2)))
                .zip(positive(named_parameter(record, surface_type, 3)))
                .map(
                    |(((center, axis, ref_direction), major_radius), minor_radius)| {
                        SurfaceGeometry::Torus {
                            center,
                            axis,
                            ref_direction,
                            major_radius: major_radius * scale,
                            minor_radius: minor_radius * scale,
                        }
                    },
                ),
            "B_SPLINE_SURFACE_WITH_KNOTS" => {
                nurbs_surface(record, &points, &mut warnings).map(SurfaceGeometry::Nurbs)
            }
            _ => unreachable!("surface type was selected from the dispatch list"),
        };
        if let Some(geometry) = geometry {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!("{surface_type} #{id} has invalid geometry"));
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_SURFACE_WITH_KNOTS") {
        if record.partial("B_SPLINE_SURFACE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_SURFACE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_surface(record, &points, &mut warnings) {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry: SurfaceGeometry::Nurbs(nurbs),
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "B_SPLINE_SURFACE_WITH_KNOTS #{id} has invalid geometry"
            ));
        }
    }

    carrier_index = CarrierIndex::from_ir(ir);
    let deferred_surface_count = exchange
        .entities_any(&["CURVE_BOUNDED_SURFACE", "OFFSET_SURFACE"])
        .count();
    for _ in 0..=deferred_surface_count {
        let mut progress = false;
        for (id, record) in exchange.entities("CURVE_BOUNDED_SURFACE") {
            let surface = SurfaceId(format!("step:data:surface#{id}"));
            if carrier_index.surfaces.contains_key(&id) {
                continue;
            }
            let Some(parameters) = entity_parameters(record, "CURVE_BOUNDED_SURFACE") else {
                continue;
            };
            let support_step = parameters.get(1).and_then(Value::reference);
            let support =
                support_step.map(|support| SurfaceId(format!("step:data:surface#{support}")));
            let boundary_steps = parameters.get(2).and_then(references);
            let boundaries = boundary_steps.as_ref().map(|boundaries| {
                boundaries
                    .iter()
                    .copied()
                    .map(|boundary| CurveId(format!("step:data:curve#{boundary}")))
                    .collect::<Vec<_>>()
            });
            let boundary_pcurves = boundary_steps
                .iter()
                .flatten()
                .flat_map(|boundary| {
                    support_step
                        .into_iter()
                        .flat_map(|support| boundary_pcurve_steps(*boundary, support, exchange))
                })
                .map(|pcurve| PcurveId(format!("step:data:pcurve#{pcurve}")))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let implicit_outer = parameters.get(3).and_then(Value::logical);
            let Some((support, boundaries, implicit_outer, geometry)) = support_step
                .and_then(|support| carrier_index.surfaces.get(&support))
                .and_then(|index| ir.model.surfaces.get(*index))
                .map(|surface| surface.geometry.clone())
                .zip(support)
                .zip(boundaries)
                .zip(implicit_outer)
                .map(|(((geometry, support), boundaries), implicit_outer)| {
                    (support, boundaries, implicit_outer, geometry)
                })
                .filter(|(_, boundaries, _, _)| {
                    !boundaries.is_empty()
                        && boundaries.iter().all(|curve| {
                            step_instance_id(&curve.0)
                                .is_some_and(|id| carrier_index.curves.contains_key(&id))
                        })
                })
            else {
                continue;
            };
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry,
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("step:construction:curve_bounded_surface#{id}")),
                surface,
                definition: ProceduralSurfaceDefinition::CurveBounded {
                    support,
                    boundaries,
                    boundary_pcurves,
                    implicit_outer,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            carrier_index.surfaces.insert(id, surface_index);
            typed.insert(id);
            progress = true;
        }
        for (id, record) in exchange.entities("OFFSET_SURFACE") {
            let surface = SurfaceId(format!("step:data:surface#{id}"));
            if carrier_index.surfaces.contains_key(&id) {
                continue;
            }
            let Some(parameters) = entity_parameters(record, "OFFSET_SURFACE") else {
                continue;
            };
            let support = parameters
                .get(1)
                .and_then(Value::reference)
                .filter(|support| carrier_index.surfaces.contains_key(support))
                .map(|support| SurfaceId(format!("step:data:surface#{support}")));
            let distance = parameters.get(2).and_then(Value::number);
            let self_intersect = parameters
                .get(3)
                .and_then(logical_value)
                .map(StepLogical::into_option);
            let Some((support, distance, self_intersect)) = support
                .zip(distance)
                .zip(self_intersect)
                .map(|((support, distance), self_intersect)| (support, distance, self_intersect))
            else {
                continue;
            };
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!("step:construction:offset_surface#{id}")),
                surface,
                definition: ProceduralSurfaceDefinition::ParallelOffset {
                    support,
                    distance: distance * scale,
                    self_intersect,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            carrier_index.surfaces.insert(id, surface_index);
            typed.insert(id);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    let surface_replica_count = exchange.entities("SURFACE_REPLICA").count();
    for _ in 0..=surface_replica_count {
        let mut progress = false;
        for (id, record) in exchange.entities("SURFACE_REPLICA") {
            if carrier_index.surfaces.contains_key(&id) {
                continue;
            }
            let Some(parent_step) =
                named_parameter(record, "SURFACE_REPLICA", 1).and_then(Value::reference)
            else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(record, "SURFACE_REPLICA", 2).and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.surfaces.get(&parent_step).copied() else {
                continue;
            };
            let Some(transform) = transformation_operators.get(&operator_step).copied() else {
                continue;
            };
            let Some(basis) = ir
                .model
                .surfaces
                .get(parent_index)
                .map(|surface| surface.geometry.clone())
            else {
                continue;
            };
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry: SurfaceGeometry::Transformed {
                    basis: Box::new(basis),
                    transform,
                },
                source_object: None,
            });
            carrier_index.surfaces.insert(id, surface_index);
            typed.insert(id);
            typed.insert(operator_step);
            progress = true;
        }
        if !progress {
            break;
        }
    }
    for (id, _) in exchange.entities("SURFACE_REPLICA") {
        if let Entry::Vacant(entry) = carrier_index.surfaces.entry(id) {
            warnings.push(format!(
                "SURFACE_REPLICA #{id} has invalid or unresolved parent/operator"
            ));
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry: SurfaceGeometry::Unknown {
                    record: exchange.records.get(&id).map(opaque_record_id),
                },
                source_object: None,
            });
            entry.insert(surface_index);
        }
    }
    for (id, _) in exchange.entities("CURVE_BOUNDED_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            warnings.push(format!(
                "CURVE_BOUNDED_SURFACE #{id} has invalid or unresolved support/boundaries"
            ));
        }
    }
    for (id, _) in exchange.entities("OFFSET_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            warnings.push(format!(
                "OFFSET_SURFACE #{id} has invalid or unresolved support parameters"
            ));
        }
    }

    for record in exchange.entities("EDGE_CURVE").map(|(_, record)| record) {
        let Some(curve_step) = edge_curve_geometry_reference(record)
            .and_then(|curve| curve_carrier_record(curve, exchange))
        else {
            continue;
        };
        if let Entry::Vacant(entry) = carrier_index.curves.entry(curve_step) {
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{curve_step}")),
                geometry: CurveGeometry::Unknown {
                    record: exchange.records.get(&curve_step).map(opaque_record_id),
                },
                source_object: None,
            });
            warnings.push(format!(
                "retained undecoded topology curve #{curve_step} as an unknown carrier"
            ));
            entry.insert(curve_index);
        }
    }
    for (id, _) in exchange.entities_any(&["CURVE_BOUNDED_SURFACE", "OFFSET_SURFACE"]) {
        let surface = SurfaceId(format!("step:data:surface#{id}"));
        if let Entry::Vacant(entry) = carrier_index.surfaces.entry(id) {
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: surface,
                geometry: SurfaceGeometry::Unknown {
                    record: exchange.records.get(&id).map(opaque_record_id),
                },
                source_object: None,
            });
            warnings.push(format!(
                "retained unresolved deferred surface #{id} as an unknown carrier"
            ));
            entry.insert(surface_index);
        }
    }
    for (&face_id, face) in &exchange.records {
        if !face
            .partials
            .iter()
            .any(|partial| matches!(partial.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE"))
        {
            continue;
        }
        let Some(surface_step) = face_surface_reference(face) else {
            continue;
        };
        if let Entry::Vacant(entry) = carrier_index.surfaces.entry(surface_step) {
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{surface_step}")),
                geometry: SurfaceGeometry::Unknown {
                    record: exchange.records.get(&surface_step).map(opaque_record_id),
                },
                source_object: None,
            });
            warnings.push(format!(
                "retained undecoded face surface #{surface_step} from face #{face_id} as an unknown carrier"
            ));
            entry.insert(surface_index);
        }
    }
    let planar_surface_ids = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| matches!(surface.geometry, SurfaceGeometry::Plane { .. }))
        .filter_map(|surface| step_instance_id(&surface.id.0))
        .collect::<BTreeSet<_>>();
    for (id, record) in exchange.entities("PCURVE") {
        if record.partial("PCURVE").is_none() {
            continue;
        }
        let surface_step = named_parameter(record, "PCURVE", 1).and_then(Value::reference);
        let representation = named_parameter(record, "PCURVE", 2)
            .and_then(Value::reference)
            .and_then(|representation| exchange.records.get(&representation));
        let curve_steps = representation
            .and_then(|representation| representation.parameter(1))
            .and_then(Value::list)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::reference)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let decoded = curve_steps
            .iter()
            .filter_map(|curve| {
                pcurve_geometries
                    .get(curve)
                    .map(|decoded| (*curve, decoded))
            })
            .collect::<Vec<_>>();
        let Some((curve_step, (geometry, geometry_records))) = surface_step
            .filter(|surface| carrier_index.surfaces.contains_key(surface))
            .and(match decoded.as_slice() {
                [decoded] => Some(*decoded),
                _ => None,
            })
        else {
            warnings.push(format!("PCURVE #{id} has no decoded surface or 2D curve"));
            continue;
        };
        let mut geometry = geometry.clone();
        if surface_step.is_some_and(|surface| planar_surface_ids.contains(&surface)) {
            scale_planar_pcurve_geometry(&mut geometry, scale);
        }
        ir.model.pcurves.push(Pcurve {
            id: PcurveId(format!("step:data:pcurve#{id}")),
            geometry,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        });
        typed.insert(id);
        if let Some(representation) =
            named_parameter(record, "PCURVE", 2).and_then(Value::reference)
        {
            typed.insert(representation);
        }
        typed.insert(curve_step);
        typed.extend(geometry_records.iter().copied());
    }

    // Curve-bounded surfaces are decoded before PCURVE records because their
    // boundary carriers can be resolved without parameter-space geometry.
    // Resolve the deferred pcurve references after the PCURVE pass so an
    // unresolved parameter-space carrier cannot become an IR reference.
    let decoded_pcurve_steps = ir
        .model
        .pcurves
        .iter()
        .filter_map(|pcurve| step_instance_id(&pcurve.id.0))
        .collect::<BTreeSet<_>>();
    for surface in &mut ir.model.procedural_surfaces {
        if let ProceduralSurfaceDefinition::CurveBounded {
            boundary_pcurves, ..
        } = &mut surface.definition
        {
            boundary_pcurves.retain(|pcurve| {
                step_instance_id(&pcurve.0).is_some_and(|id| decoded_pcurve_steps.contains(&id))
            });
        }
    }

    for (id, record) in exchange.entities("DEGENERATE_TOROIDAL_SURFACE") {
        let Some(select_outer) = named_parameter(record, "DEGENERATE_TOROIDAL_SURFACE", 4)
            .and_then(logical_value)
            .and_then(StepLogical::into_option)
        else {
            if carrier_index.surfaces.contains_key(&id) {
                warnings.push(format!(
                    "DEGENERATE_TOROIDAL_SURFACE #{id} has invalid sheet selection"
                ));
            }
            continue;
        };
        let surface = SurfaceId(format!("step:data:surface#{id}"));
        if !carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("step:construction:degenerate_torus#{id}")),
            surface,
            definition: ProceduralSurfaceDefinition::DegenerateTorus { select_outer },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
    }

    for (&id, record) in &exchange.records {
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "LENGTH_UNIT"
                    | "NAMED_UNIT"
                    | "SI_UNIT"
                    | "CONVERSION_BASED_UNIT"
                    | "MEASURE_WITH_UNIT"
                    | "LENGTH_MEASURE_WITH_UNIT"
                    | "PLANE_ANGLE_MEASURE_WITH_UNIT"
                    | "UNCERTAINTY_MEASURE_WITH_UNIT"
                    | "GEOMETRIC_REPRESENTATION_CONTEXT"
                    | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                    | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
                    | "REPRESENTATION_CONTEXT"
            )
        }) || record.simple_name() == Some("SHAPE_REPRESENTATION")
        {
            typed.insert(id);
        }
    }
    GeometryResult {
        typed_records: typed,
        warnings,
        losses,
        placements,
        length_scale: scale,
        plane_angle_scale: angle_scale,
    }
}

fn face_surface_reference(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1
        && matches!(record.simple_name(), Some("ADVANCED_FACE" | "FACE_SURFACE"))
    {
        return record.parameter(2).and_then(Value::reference);
    }
    record
        .partials
        .iter()
        .filter(|partial| matches!(partial.name.as_str(), "ADVANCED_FACE" | "FACE_SURFACE"))
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(Value::reference)
        .next_back()
}

pub(super) fn associate_free_geometric_set_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
) {
    for set in exchange.records.values() {
        let Some(set_type) = entity_type(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(members) = named_parameter(set, set_type, 1).and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            let name = exchange
                .records
                .get(&member)
                .and_then(|record| record.parameter(0))
                .and_then(|value| {
                    super::decode_text(
                        value,
                        losses,
                        member,
                        "geometric-set member name",
                        LossKind::MetadataNotTransferred,
                    )
                })
                .filter(|name| !name.is_empty());
            let association = || SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{member}"),
                name: name.clone(),
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            };
            if let Some(index) = index.curves.get(&member) {
                if owned.curves.contains(index) {
                    continue;
                }
                ir.model.curves[*index]
                    .source_object
                    .get_or_insert_with(association);
            }
            if let Some(index) = index.points.get(&member) {
                if owned.points.contains(index) {
                    continue;
                }
                ir.model.points[*index]
                    .source_object
                    .get_or_insert_with(association);
            }
            if let Some(index) = index.surfaces.get(&member) {
                if owned.surfaces.contains(index) {
                    continue;
                }
                ir.model.surfaces[*index]
                    .source_object
                    .get_or_insert_with(association);
            }
        }
    }
}

/// Associate carriers that are listed directly by a STEP representation but
/// are not owned by committed topology. A representation item is an explicit
/// source owner for free geometry; without this association the generic IR
/// reachability check would misclassify a valid standalone carrier as an
/// orphan.
pub(super) fn associate_free_representation_members(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
) {
    for representation in exchange.records.values().filter(|record| {
        record
            .partials
            .iter()
            .any(|partial| partial.name.ends_with("REPRESENTATION"))
    }) {
        let Some(items) = representation_items(representation) else {
            continue;
        };
        for member in items {
            let source_name = exchange
                .records
                .get(&member)
                .and_then(|record| record.parameter(0))
                .and_then(|value| {
                    super::decode_text(
                        value,
                        losses,
                        member,
                        "representation member name",
                        LossKind::MetadataNotTransferred,
                    )
                })
                .filter(|name| !name.is_empty());
            let association = || SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{member}"),
                name: source_name.clone(),
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            };
            if let Some(index) = index.curves.get(&member) {
                if !owned.curves.contains(index) {
                    ir.model.curves[*index]
                        .source_object
                        .get_or_insert_with(association);
                }
            }
            if let Some(index) = index.points.get(&member) {
                if !owned.points.contains(index) {
                    ir.model.points[*index]
                        .source_object
                        .get_or_insert_with(association);
                }
            }
            if let Some(index) = index.surfaces.get(&member) {
                if !owned.surfaces.contains(index) {
                    ir.model.surfaces[*index]
                        .source_object
                        .get_or_insert_with(association);
                }
            }
        }
    }
}

fn representation_items(record: &RawRecord) -> Option<Vec<u64>> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .find_map(Value::list)
        .map(|items| items.iter().filter_map(Value::reference).collect())
}

fn entity_parameters<'a>(record: &'a RawRecord, name: &str) -> Option<&'a [Value]> {
    record
        .partial(name)
        .map(|partial| partial.parameters.as_slice())
}

fn named_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record.partial(name)?.parameters.get(index)
}

fn entity_type<'a>(record: &RawRecord, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .copied()
        .find(|name| record.partial(name).is_some())
}

fn first_named_list(record: &RawRecord, names: &[&str]) -> Option<Vec<u64>> {
    record
        .partials
        .iter()
        .find(|partial| names.iter().any(|name| partial.name == *name))
        .and_then(|partial| {
            partial.parameters.iter().find_map(|value| {
                value.list().and_then(|items| {
                    items
                        .iter()
                        .map(Value::reference)
                        .collect::<Option<Vec<_>>>()
                })
            })
        })
}

fn vertex_point_reference(record: &RawRecord) -> Option<u64> {
    record
        .partial("VERTEX_POINT")
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

fn edge_curve_geometry_reference(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(3).and_then(Value::reference);
    }
    record
        .partial("EDGE_CURVE")
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

#[derive(Clone, Copy)]
enum TrimMasterRepresentation {
    Parameter,
    Cartesian,
    Unspecified,
}

struct TrimParameterContext<'a> {
    points: &'a BTreeMap<u64, Point3>,
    geometry: &'a CurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    tolerance: f64,
    master_representation: TrimMasterRepresentation,
    record_id: u64,
    warnings: &'a mut Vec<String>,
}

fn trimmed_curve_attributes(parameters: &[Value]) -> Option<(u64, bool, TrimMasterRepresentation)> {
    let basis = parameters.get(1).and_then(Value::reference)?;
    let sense = parameters.get(4).and_then(Value::logical)?;
    let master_representation = match parameters.get(5).and_then(Value::enumeration)? {
        "PARAMETER" => TrimMasterRepresentation::Parameter,
        "CARTESIAN" => TrimMasterRepresentation::Cartesian,
        "UNSPECIFIED" => TrimMasterRepresentation::Unspecified,
        _ => return None,
    };
    Some((basis, sense, master_representation))
}

fn surface_curve_basis(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(1).and_then(Value::reference);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
        .or_else(|| record.partial("INTERSECTION_CURVE"))
        .and_then(|partial| partial.parameters.iter().find_map(Value::reference))
}

pub(super) struct OwnedCarriers {
    pub(super) curves: HashSet<usize>,
    pub(super) surfaces: HashSet<usize>,
    pub(super) points: HashSet<usize>,
}

pub(super) fn topology_owned_carriers(ir: &CadIr, index: &CarrierIndex) -> OwnedCarriers {
    let curves = ir
        .model
        .edges
        .iter()
        .filter_map(|edge| edge.curve.as_ref())
        .chain(
            ir.model
                .coedges
                .iter()
                .filter_map(|coedge| coedge.use_curve.as_ref()),
        )
        .filter_map(|curve| step_instance_id(&curve.0))
        .filter_map(|id| index.curves.get(&id).copied())
        .collect();
    let surfaces = ir
        .model
        .faces
        .iter()
        .filter_map(|face| step_instance_id(&face.surface.0))
        .filter_map(|id| index.surfaces.get(&id).copied())
        .collect();
    let points = ir
        .model
        .vertices
        .iter()
        .filter_map(|vertex| step_instance_id(&vertex.point.0))
        .filter_map(|id| index.points.get(&id).copied())
        .collect();
    OwnedCarriers {
        curves,
        surfaces,
        points,
    }
}

pub(super) fn associate_topology_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
) {
    for (edge_id, edge) in exchange.entities("EDGE_CURVE") {
        let Some(curve_step) = edge_curve_geometry_reference(edge)
            .and_then(|curve| curve_carrier_record(curve, exchange))
        else {
            continue;
        };
        let Some(index) = index.curves.get(&curve_step) else {
            continue;
        };
        if owned.curves.contains(index) {
            continue;
        }
        ir.model.curves[*index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{edge_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
    for (face_id, face) in exchange.entities_any(&["ADVANCED_FACE", "FACE_SURFACE"]) {
        let Some(surface_step) = face_surface_reference(face) else {
            continue;
        };
        let Some(index) = index.surfaces.get(&surface_step) else {
            continue;
        };
        if owned.surfaces.contains(index) {
            continue;
        }
        ir.model.surfaces[*index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{face_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
    for (vertex_id, vertex) in exchange.entities("VERTEX_POINT") {
        let Some(point_step) = vertex_point_reference(vertex) else {
            continue;
        };
        let Some(index) = index.points.get(&point_step) else {
            continue;
        };
        if owned.points.contains(index) {
            continue;
        }
        ir.model.points[*index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{vertex_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
}

/// Associate the basis carrier owned by each valid replica with that replica.
///
/// A replica is the carrier used by topology, while its basis remains a
/// separate IR geometry entry because the transformed geometry stores the
/// basis inline. The basis is still a real STEP dependency and must not be
/// reported as an unowned carrier by generic IR validation.
pub(super) fn associate_replica_bases(exchange: &Exchange, ir: &mut CadIr, index: &CarrierIndex) {
    for (replica_id, record) in exchange.entities("CURVE_REPLICA") {
        let Some(parent_id) =
            named_parameter(record, "CURVE_REPLICA", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.curves.get(&parent_id).copied() else {
            continue;
        };
        ir.model.curves[parent_index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{replica_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
    for (replica_id, record) in exchange.entities("SURFACE_REPLICA") {
        let Some(parent_id) =
            named_parameter(record, "SURFACE_REPLICA", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(parent_index) = index.surfaces.get(&parent_id).copied() else {
            continue;
        };
        ir.model.surfaces[parent_index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{replica_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
}

/// Associate surfaces referenced only as PCURVE supports with their STEP
/// PCURVE records. The canonical pcurve stores its parameter-space geometry
/// inline, so this source association preserves reachability of the separate
/// support carrier.
pub(super) fn associate_pcurve_supports(exchange: &Exchange, ir: &mut CadIr, index: &CarrierIndex) {
    let owned_pcurves = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.as_str()))
        .chain(ir.model.loops.iter().flat_map(|loop_| {
            loop_
                .vertex_uses
                .iter()
                .flat_map(|use_| use_.pcurves.iter().map(|pcurve| pcurve.pcurve.0.as_str()))
        }))
        .collect::<BTreeSet<_>>();
    for (pcurve_id, record) in exchange.entities("PCURVE") {
        let pcurve_identity = format!("step:data:pcurve#{pcurve_id}");
        if !owned_pcurves.contains(pcurve_identity.as_str()) {
            continue;
        }
        let Some(surface_id) = named_parameter(record, "PCURVE", 1).and_then(Value::reference)
        else {
            continue;
        };
        let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
            continue;
        };
        ir.model.surfaces[surface_index]
            .source_object
            .get_or_insert_with(|| SourceObjectAssociation {
                format: "step".into(),
                object_id: format!("#{pcurve_id}"),
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            });
    }
}

fn length_scale(exchange: &Exchange) -> Option<f64> {
    let context_units = exchange.records.values().find_map(|record| {
        record
            .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")?
            .parameters
            .first()?
            .list()
    });
    let unit_id = context_units
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .find(|id| {
            exchange
                .records
                .get(id)
                .is_some_and(|record| record.partial("LENGTH_UNIT").is_some())
        })
        .or_else(|| {
            exchange
                .records
                .iter()
                .find(|(_, record)| record.partial("LENGTH_UNIT").is_some())
                .map(|(&id, _)| id)
        })?;
    unit_scale_mm(unit_id, exchange, &mut BTreeSet::new())
}

fn plane_angle_scale(exchange: &Exchange) -> Option<f64> {
    let context_units = exchange.records.values().find_map(|record| {
        record
            .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")?
            .parameters
            .first()?
            .list()
    });
    let unit_id = context_units
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .find(|id| {
            exchange
                .records
                .get(id)
                .is_some_and(|record| record.partial("PLANE_ANGLE_UNIT").is_some())
        })
        .or_else(|| {
            exchange
                .records
                .iter()
                .find(|(_, record)| record.partial("PLANE_ANGLE_UNIT").is_some())
                .map(|(&id, _)| id)
        })?;
    unit_scale_radians(unit_id, exchange, &mut BTreeSet::new())
}

pub(super) fn unit_scale_radians(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> Option<f64> {
    unit_scale_radians_inner(id, exchange, active, 0)
}

fn unit_scale_radians_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<f64> {
    if depth >= 256 {
        return None;
    }
    if !active.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    let result = if let Some(unit) = record.partial("SI_UNIT") {
        (unit.parameters.get(1)?.enumeration()? == "RADIAN").then_some(1.0)
    } else if let Some(unit) = record.partial("CONVERSION_BASED_UNIT") {
        let factor_id = unit.parameters.get(1)?.reference()?;
        let factor = exchange.records.get(&factor_id)?;
        let value = record_values(factor).find_map(measure_number)?;
        let base = record_values(factor)
            .find_map(Value::reference)
            .and_then(|base| unit_scale_radians_inner(base, exchange, active, depth + 1))?;
        Some(value * base)
    } else {
        None
    };
    active.remove(&id);
    result.filter(|scale| scale.is_finite() && *scale > 0.0)
}

pub(super) fn unit_scale_mm(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> Option<f64> {
    unit_scale_mm_inner(id, exchange, active, 0)
}

fn unit_scale_mm_inner(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<f64> {
    if depth >= 256 {
        return None;
    }
    if !active.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    let result = if let Some(unit) = record.partial("SI_UNIT") {
        if unit.parameters.get(1)?.enumeration()? == "METRE" {
            let prefix = match unit.parameters.first()? {
                Value::Omitted => 1.0,
                Value::Enumeration(prefix) => si_prefix(prefix)?,
                _ => return None,
            };
            Some(prefix * 1000.0)
        } else {
            None
        }
    } else if let Some(unit) = record.partial("CONVERSION_BASED_UNIT") {
        let factor_id = unit.parameters.get(1)?.reference()?;
        let factor = exchange.records.get(&factor_id)?;
        let value = record_values(factor).find_map(measure_number)?;
        let base = factor
            .partials
            .iter()
            .flat_map(|partial| &partial.parameters)
            .find_map(Value::reference)
            .and_then(|base| unit_scale_mm_inner(base, exchange, active, depth + 1))?;
        Some(value * base)
    } else {
        None
    };
    active.remove(&id);
    result.filter(|scale| scale.is_finite() && *scale > 0.0)
}

fn si_prefix(prefix: &str) -> Option<f64> {
    Some(match prefix {
        "EXA" => 1e18,
        "PETA" => 1e15,
        "TERA" => 1e12,
        "GIGA" => 1e9,
        "MEGA" => 1e6,
        "KILO" => 1e3,
        "HECTO" => 1e2,
        "DECA" => 1e1,
        "DECI" => 1e-1,
        "CENTI" => 1e-2,
        "MILLI" => 1e-3,
        "MICRO" => 1e-6,
        "NANO" => 1e-9,
        "PICO" => 1e-12,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => return None,
    })
}

fn linear_uncertainty(exchange: &Exchange) -> Option<f64> {
    let uncertainty = exchange.records.values().find_map(|record| {
        record
            .partial("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")?
            .parameters
            .first()?
            .list()?
            .iter()
            .find_map(Value::reference)
    })?;
    let measure = exchange.records.get(&uncertainty)?;
    let value = record_values(measure).find_map(measure_number)?;
    let unit = record_values(measure).find_map(Value::reference)?;
    let scale = unit_scale_mm(unit, exchange, &mut BTreeSet::new())?;
    let result = value * scale;
    (result.is_finite() && result > 0.0).then_some(result)
}

fn measure_number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Real(value) => Some(*value),
        Value::Typed(_, value) => measure_number(value),
        _ => None,
    }
}

fn trim_parameter(value: &Value, context: &mut TrimParameterContext<'_>) -> Option<f64> {
    let (parameter, cartesian) = match value {
        Value::List(values) => (
            values.iter().find(|value| is_parameter_trim_value(value)),
            values
                .iter()
                .find(|value| matches!(value, Value::Reference(_))),
        ),
        value if is_parameter_trim_value(value) => (Some(value), None),
        Value::Reference(_) => (None, Some(value)),
        _ => (None, None),
    };
    select_trim_parameter(parameter, cartesian, context)
}

fn is_parameter_trim_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Real(_) => true,
        Value::Typed(name, _) => name == "PARAMETER_VALUE",
        _ => false,
    }
}

fn trim_parameter_value(value: &Value, context: &TrimParameterContext<'_>) -> Option<f64> {
    match value {
        Value::Integer(value) => Some(
            parameter_scale(
                context.geometry,
                context.angle_scale,
                context.linear_parameter_scale,
            ) * *value as f64,
        ),
        Value::Real(value) => Some(
            parameter_scale(
                context.geometry,
                context.angle_scale,
                context.linear_parameter_scale,
            ) * *value,
        ),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => {
            trim_parameter_value(value, context)
        }
        _ => None,
    }
}

fn trim_cartesian_parameter(value: &Value, context: &TrimParameterContext<'_>) -> Option<f64> {
    let Value::Reference(id) = value else {
        return None;
    };
    context
        .points
        .get(id)
        .and_then(|point| curve_parameter_at_point(context.geometry, *point, context.tolerance))
}

fn select_trim_parameter(
    parameter: Option<&Value>,
    cartesian: Option<&Value>,
    context: &mut TrimParameterContext<'_>,
) -> Option<f64> {
    match context.master_representation {
        TrimMasterRepresentation::Parameter => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                if cartesian.is_some() {
                    context.warnings.push(format!(
                        "TRIMMED_CURVE #{} fell back to a Cartesian trim selector because master_representation is .PARAMETER.",
                        context.record_id
                    ));
                }
                cartesian.and_then(|value| trim_cartesian_parameter(value, context))
            }
        }
        TrimMasterRepresentation::Cartesian => {
            if let Some(value) = cartesian {
                trim_cartesian_parameter(value, context)
            } else {
                if parameter.is_some() {
                    context.warnings.push(format!(
                        "TRIMMED_CURVE #{} fell back to a parameter trim selector because master_representation is .CARTESIAN.",
                        context.record_id
                    ));
                }
                parameter.and_then(|value| trim_parameter_value(value, context))
            }
        }
        TrimMasterRepresentation::Unspecified => {
            if let Some(value) = parameter {
                trim_parameter_value(value, context)
            } else {
                cartesian.and_then(|value| trim_cartesian_parameter(value, context))
            }
        }
    }
}

fn parameter_scale(geometry: &CurveGeometry, angle_scale: f64, linear_parameter_scale: f64) -> f64 {
    if matches!(
        geometry,
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
    ) {
        angle_scale
    } else if matches!(geometry, CurveGeometry::Line { .. }) {
        linear_parameter_scale
    } else {
        1.0
    }
}

fn line_parameter_scale(
    exchange: &Exchange,
    curve: u64,
    length_scale: f64,
    losses: &mut Vec<LossNote>,
) -> f64 {
    exchange
        .records
        .get(&curve)
        .filter(|record| record.partial("LINE").is_some())
        .and_then(|record| named_parameter(record, "LINE", 2))
        .and_then(ValueExt::reference)
        .and_then(|vector| exchange.records.get(&vector))
        .filter(|record| record.partial("VECTOR").is_some())
        .and_then(|record| named_parameter(record, "VECTOR", 2))
        .and_then(ValueExt::number)
        .map(|magnitude| magnitude * length_scale)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or_else(|| {
            losses.push(unresolved_unit_loss(format!(
                "LINE #{curve} parameter scale did not resolve; the document length scale was used"
            )));
            length_scale
        })
}

fn unresolved_unit_loss(message: impl Into<String>) -> LossNote {
    LossNote {
        code: LossKind::GeometryNotTransferred,
        severity: Severity::Error,
        message: message.into(),
        provenance: None,
    }
}

fn orthogonal_reference(axis: Vector3, reference: Vector3) -> Option<Vector3> {
    let projection = dot(axis, reference);
    normalize(Vector3::new(
        reference.x - projection * axis.x,
        reference.y - projection * axis.y,
        reference.z - projection * axis.z,
    ))
}

fn curve_parameter_at_point(
    geometry: &CurveGeometry,
    point: Point3,
    tolerance: f64,
) -> Option<f64> {
    let offset =
        |origin: Point3| Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(dot(offset(*origin), *direction)),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => {
            let radial = offset(*center);
            let y_axis = cross(*axis, *ref_direction);
            Some(dot(radial, y_axis).atan2(dot(radial, *ref_direction)))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let radial = offset(*center);
            let minor_direction = cross(*axis, *major_direction);
            Some(
                (dot(radial, minor_direction) / minor_radius)
                    .atan2(dot(radial, *major_direction) / major_radius),
            )
        }
        CurveGeometry::Nurbs(curve) => {
            let domain = nurbs_curve_parameter_domain(curve)?;
            nurbs_curve_parameter_near_point(curve, point, tolerance, (domain[0] + domain[1]) * 0.5)
        }
        _ => None,
    }
}

fn dot(a: Vector3, b: Vector3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

type CompositeCurveData = (Vec<(u64, CompositeCurveSegment)>, Option<bool>);

fn wake_deferred_dependents(
    id: u64,
    waiting_on: &mut HashMap<u64, Vec<u64>>,
    queue: &mut VecDeque<u64>,
) {
    if let Some(dependents) = waiting_on.remove(&id) {
        queue.extend(dependents);
    }
}

fn composite_curve_partial(record: &RawRecord) -> Option<&crate::parse::PartialRecord> {
    record
        .partial("COMPOSITE_CURVE")
        .or_else(|| record.partial("BOUNDARY_CURVE"))
        .or_else(|| record.partial("OUTER_BOUNDARY_CURVE"))
}

fn composite_curve_segment_ids(record: &RawRecord, exchange: &Exchange) -> Option<Vec<u64>> {
    let complex = record.partials.len() > 1;
    let offset = usize::from(!complex);
    let segments = composite_curve_partial(record)?
        .parameters
        .get(offset)?
        .list()?
        .iter()
        .map(Value::reference)
        .collect::<Option<Vec<_>>>()?;
    (!segments.is_empty()
        && segments.iter().all(|segment| {
            exchange
                .records
                .get(segment)
                .and_then(composite_curve_segment_parameters)
                .is_some()
        }))
    .then_some(segments)
}

fn composite_curve_segment_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("COMPOSITE_CURVE_SEGMENT")
        .map(|partial| partial.parameters.as_slice())
}

fn composite_curve_dependencies(record: &RawRecord, exchange: &Exchange) -> Vec<u64> {
    composite_curve_segment_ids(record, exchange)
        .into_iter()
        .flatten()
        .filter_map(|segment| exchange.records.get(&segment))
        .filter_map(composite_curve_segment_parameters)
        .filter_map(|parameters| parameters.get(2).and_then(Value::reference))
        .filter_map(|curve| curve_carrier_record(curve, exchange))
        .collect()
}

fn composite_curve(
    record: &RawRecord,
    exchange: &Exchange,
    decoded: &CarrierIndex,
) -> Option<CompositeCurveData> {
    let complex = record.partials.len() > 1;
    let composite = composite_curve_partial(record)?;
    let offset = usize::from(!complex);
    let segments = composite
        .parameters
        .get(offset)?
        .list()?
        .iter()
        .map(|value| {
            let id = value.reference()?;
            let record = exchange.records.get(&id)?;
            let parameters = composite_curve_segment_parameters(record)?;
            let transition = match parameters.first()?.enumeration()? {
                "DISCONTINUOUS" => CompositeCurveTransition::Discontinuous,
                "CONTINUOUS" => CompositeCurveTransition::Continuous,
                "CONTSAMEGRADIENT" => CompositeCurveTransition::ContSameGradient,
                "CONTSAMEGRADIENTSAMECURVATURE" => {
                    CompositeCurveTransition::ContSameGradientSameCurvature
                }
                _ => return None,
            };
            let curve_step = parameters.get(2)?.reference()?;
            let carrier_step = curve_carrier_record(curve_step, exchange)?;
            decoded.curves.contains_key(&carrier_step).then_some((
                id,
                CompositeCurveSegment {
                    curve: CurveId(format!("step:data:curve#{carrier_step}")),
                    same_sense: parameters.get(1)?.logical()?,
                    transition,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    (!segments.is_empty()).then_some((
        segments,
        composite
            .parameters
            .get(offset + 1)
            .and_then(logical_value)?
            .into_option(),
    ))
}

fn boundary_pcurve_steps(boundary: u64, support: u64, exchange: &Exchange) -> Vec<u64> {
    let Some(record) = exchange.records.get(&boundary) else {
        return Vec::new();
    };
    let Some(segments) = composite_curve_segment_ids(record, exchange) else {
        return Vec::new();
    };
    segments
        .into_iter()
        .filter_map(|segment| exchange.records.get(&segment))
        .filter_map(composite_curve_segment_parameters)
        .filter_map(|parameters| parameters.get(2).and_then(Value::reference))
        .filter_map(|curve| exchange.records.get(&curve))
        .filter(|curve| {
            curve.partials.iter().any(|partial| {
                matches!(
                    partial.name.as_str(),
                    "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                )
            })
        })
        .flat_map(|curve| surface_curve_pcurves(curve).unwrap_or_default())
        .filter(|pcurve| {
            exchange
                .records
                .get(pcurve)
                .and_then(|record| named_parameter(record, "PCURVE", 1))
                .and_then(Value::reference)
                == Some(support)
        })
        .collect()
}

#[derive(Clone, Copy)]
enum StepLogical {
    Known(bool),
    Unknown,
}

impl StepLogical {
    fn into_option(self) -> Option<bool> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }
}

fn logical_value(value: &Value) -> Option<StepLogical> {
    match value {
        Value::Enumeration(value) if value == "T" => Some(StepLogical::Known(true)),
        Value::Enumeration(value) if value == "F" => Some(StepLogical::Known(false)),
        Value::Enumeration(value) if value == "U" => Some(StepLogical::Unknown),
        _ => None,
    }
}

fn periodic_value(
    value: Option<&Value>,
    field: &str,
    record_id: u64,
    warnings: &mut Vec<String>,
) -> Option<bool> {
    match logical_value(value?)? {
        StepLogical::Known(value) => Some(value),
        StepLogical::Unknown => {
            warnings.push(format!(
                "{field} #{record_id} has UNKNOWN periodicity; decoded as non-periodic"
            ));
            Some(false)
        }
    }
}

fn record_values(record: &RawRecord) -> impl Iterator<Item = &Value> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
}

fn coordinates(record: &RawRecord, index: usize, scale: f64) -> Option<Point3> {
    let values = record.parameter(index)?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn coordinates2(record: &RawRecord, index: usize) -> Option<Point2> {
    let values = record.parameter(index)?.list()?;
    if values.len() != 2 {
        return None;
    }
    Some(Point2::new(values[0].number()?, values[1].number()?))
}

fn vector2(value: Option<&Value>) -> Option<Point2> {
    let values = value?.list()?;
    if values.len() != 2 {
        return None;
    }
    Some(Point2::new(values[0].number()?, values[1].number()?))
}

fn normalize2(vector: Point2) -> Option<Point2> {
    let length = vector.u.hypot(vector.v);
    (length.is_finite() && length > 0.0).then(|| Point2::new(vector.u / length, vector.v / length))
}

fn vector3(value: Option<&Value>, scale: f64) -> Option<Vector3> {
    let values = value?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Vector3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn positive(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::number)
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn nonnegative(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::number)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn nurbs_curve(
    record: &RawRecord,
    points: &BTreeMap<u64, Point3>,
    warnings: &mut Vec<String>,
) -> Option<NurbsCurve> {
    let complex = record.partials.len() > 1;
    let base = if complex {
        record.partial("B_SPLINE_CURVE")?
    } else {
        record.partial("B_SPLINE_CURVE_WITH_KNOTS")?
    };
    let offset = usize::from(!complex);
    let degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let control_points = references(base.parameters.get(offset + 1)?)?
        .into_iter()
        .map(|id| points.get(&id).copied())
        .collect::<Option<Vec<_>>>()?;
    if usize::try_from(degree).ok()? >= control_points.len() {
        return None;
    }
    let periodic = periodic_value(
        base.parameters.get(offset + 3),
        "B_SPLINE_CURVE_WITH_KNOTS",
        record.id,
        warnings,
    )?;
    let knot_leaf = record.partial("B_SPLINE_CURVE_WITH_KNOTS")?;
    let tail = knot_leaf.parameters.len().checked_sub(3)?;
    let expected_knots = control_points.len().checked_add(degree as usize + 1)?;
    let knots = expand_knots(
        knot_leaf.parameters.get(tail)?,
        knot_leaf.parameters.get(tail + 1)?,
        expected_knots,
    )?;
    if knots.len() != expected_knots {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_CURVE") {
        let values = numbers(leaf.parameters.first()?)?;
        (values.len() == control_points.len())
            .then_some(values)
            .map(Some)?
    } else {
        None
    };
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    })
}

fn nurbs_pcurve(
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    warnings: &mut Vec<String>,
) -> Option<PcurveGeometry> {
    let complex = record.partials.len() > 1;
    let base = if complex {
        record.partial("B_SPLINE_CURVE")?
    } else {
        record.partial("B_SPLINE_CURVE_WITH_KNOTS")?
    };
    let offset = usize::from(!complex);
    let degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let control_points = references(base.parameters.get(offset + 1)?)?
        .into_iter()
        .map(|id| points.get(&id).copied())
        .collect::<Option<Vec<_>>>()?;
    if usize::try_from(degree).ok()? >= control_points.len() {
        return None;
    }
    let periodic = periodic_value(
        base.parameters.get(offset + 3),
        "B_SPLINE_CURVE_WITH_KNOTS pcurve",
        record.id,
        warnings,
    )?;
    let knot_leaf = record.partial("B_SPLINE_CURVE_WITH_KNOTS")?;
    let tail = knot_leaf.parameters.len().checked_sub(3)?;
    let expected_knots = control_points.len().checked_add(degree as usize + 1)?;
    let knots = expand_knots(
        knot_leaf.parameters.get(tail)?,
        knot_leaf.parameters.get(tail + 1)?,
        expected_knots,
    )?;
    if knots.len() != expected_knots {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_CURVE") {
        let values = numbers(leaf.parameters.first()?)?;
        (values.len() == control_points.len())
            .then_some(values)
            .map(Some)?
    } else {
        None
    };
    Some(PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    })
}

#[allow(
    clippy::too_many_arguments,
    reason = "The recursive STEP curve resolver needs each dimension-specific entity table and its cycle guard."
)]
fn decode_pcurve_geometry(
    id: u64,
    exchange: &Exchange,
    points: &BTreeMap<u64, Point2>,
    vectors: &BTreeMap<u64, Point2>,
    placements: &BTreeMap<u64, (Point2, Point2, Point2)>,
    angle_scale: f64,
    warnings: &mut Vec<String>,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<(PcurveGeometry, BTreeSet<u64>)> {
    if depth >= 256 || !active.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    let mut records = BTreeSet::from([id]);
    let geometry = if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_some() {
        nurbs_pcurve(record, points, warnings)?
    } else {
        let curve_type = entity_type(
            record,
            &[
                "LINE",
                "CIRCLE",
                "ELLIPSE",
                "PARABOLA",
                "HYPERBOLA",
                "POLYLINE",
                "TRIMMED_CURVE",
                "OFFSET_CURVE_2D",
            ],
        )?;
        match curve_type {
            "LINE" => {
                let origin = named_parameter(record, "LINE", 1)?
                    .reference()
                    .and_then(|point| points.get(&point).copied())?;
                let direction = named_parameter(record, "LINE", 2)?
                    .reference()
                    .and_then(|vector| vectors.get(&vector).copied())?;
                PcurveGeometry::Line { origin, direction }
            }
            "CIRCLE" => {
                let placement = named_parameter(record, "CIRCLE", 1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let radius = positive(named_parameter(record, "CIRCLE", 2))?;
                records.insert(placement);
                PcurveGeometry::Circle {
                    center,
                    x_axis,
                    y_axis,
                    radius,
                }
            }
            "ELLIPSE" => {
                let placement = named_parameter(record, "ELLIPSE", 1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let major_radius = positive(named_parameter(record, "ELLIPSE", 2))?;
                let minor_radius = positive(named_parameter(record, "ELLIPSE", 3))?;
                records.insert(placement);
                PcurveGeometry::Ellipse {
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                }
            }
            "PARABOLA" => {
                let placement = named_parameter(record, "PARABOLA", 1)?.reference()?;
                let (vertex, x_axis, y_axis) = placements.get(&placement).copied()?;
                let focal_distance = positive(named_parameter(record, "PARABOLA", 2))?;
                records.insert(placement);
                PcurveGeometry::Parabola {
                    vertex,
                    x_axis,
                    y_axis,
                    focal_distance,
                }
            }
            "HYPERBOLA" => {
                let placement = named_parameter(record, "HYPERBOLA", 1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let major_radius = positive(named_parameter(record, "HYPERBOLA", 2))?;
                let minor_radius = positive(named_parameter(record, "HYPERBOLA", 3))?;
                records.insert(placement);
                PcurveGeometry::Hyperbola {
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                }
            }
            "POLYLINE" => polyline_pcurve(record, points)?,
            "TRIMMED_CURVE" => {
                let basis_id = named_parameter(record, "TRIMMED_CURVE", 1)?.reference()?;
                let sense = named_parameter(record, "TRIMMED_CURVE", 4)?.logical()?;
                let (basis, basis_records) = decode_pcurve_geometry(
                    basis_id,
                    exchange,
                    points,
                    vectors,
                    placements,
                    angle_scale,
                    warnings,
                    active,
                    depth + 1,
                )?;
                let scale = if matches!(
                    basis,
                    PcurveGeometry::Circle { .. } | PcurveGeometry::Ellipse { .. }
                ) {
                    angle_scale
                } else {
                    1.0
                };
                let start =
                    pcurve_trim_parameter(named_parameter(record, "TRIMMED_CURVE", 2)?)? * scale;
                let end =
                    pcurve_trim_parameter(named_parameter(record, "TRIMMED_CURVE", 3)?)? * scale;
                records.extend(basis_records);
                PcurveGeometry::Trimmed {
                    parameter_range: if sense { [start, end] } else { [end, start] },
                    basis: Box::new(basis),
                }
            }
            "OFFSET_CURVE_2D" => {
                let basis_id = named_parameter(record, "OFFSET_CURVE_2D", 1)?.reference()?;
                let distance = named_parameter(record, "OFFSET_CURVE_2D", 2)?.number()?;
                if !distance.is_finite()
                    || named_parameter(record, "OFFSET_CURVE_2D", 3)?
                        .logical()
                        .is_none()
                {
                    active.remove(&id);
                    return None;
                }
                let (basis, basis_records) = decode_pcurve_geometry(
                    basis_id,
                    exchange,
                    points,
                    vectors,
                    placements,
                    angle_scale,
                    warnings,
                    active,
                    depth + 1,
                )?;
                records.extend(basis_records);
                PcurveGeometry::Offset {
                    distance,
                    basis: Box::new(basis),
                }
            }
            _ => {
                active.remove(&id);
                return None;
            }
        }
    };
    active.remove(&id);
    Some((geometry, records))
}

fn pcurve_trim_parameter(value: &Value) -> Option<f64> {
    fn bare_number(value: &Value) -> Option<f64> {
        match value {
            Value::Integer(value) => Some(*value as f64),
            Value::Real(value) => Some(*value),
            _ => None,
        }
    }

    match value {
        Value::Integer(_) | Value::Real(_) => bare_number(value),
        Value::Typed(name, value) if name == "PARAMETER_VALUE" => bare_number(value),
        Value::List(values) => values
            .iter()
            .find_map(|value| match value {
                Value::Typed(name, value) if name == "PARAMETER_VALUE" => bare_number(value),
                _ => None,
            })
            .or_else(|| {
                values.iter().find_map(|value| match value {
                    Value::Integer(_) | Value::Real(_) => bare_number(value),
                    _ => None,
                })
            }),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn scale_planar_pcurve_geometry(geometry: &mut PcurveGeometry, scale: f64) {
    fn point(point: &mut Point2, scale: f64) {
        point.u *= scale;
        point.v *= scale;
    }

    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            point(origin, scale);
            point(direction, scale);
        }
        PcurveGeometry::Circle { center, radius, .. } => {
            point(center, scale);
            *radius *= scale;
        }
        PcurveGeometry::Ellipse {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            point(center, scale);
            *major_radius *= scale;
            *minor_radius *= scale;
        }
        PcurveGeometry::Parabola {
            vertex,
            focal_distance,
            ..
        } => {
            point(vertex, scale);
            *focal_distance *= scale;
        }
        PcurveGeometry::Hyperbola {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            point(center, scale);
            *major_radius *= scale;
            *minor_radius *= scale;
        }
        PcurveGeometry::Harmonic {
            center,
            cosine,
            sine,
        }
        | PcurveGeometry::Hyperbolic {
            center,
            cosine,
            sine,
        } => {
            point(center, scale);
            point(cosine, scale);
            point(sine, scale);
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            for control_point in control_points {
                point(control_point, scale);
            }
        }
        PcurveGeometry::Trimmed { basis, .. } => {
            scale_planar_pcurve_geometry(basis, scale);
        }
        PcurveGeometry::Offset { distance, basis } => {
            *distance *= scale;
            scale_planar_pcurve_geometry(basis, scale);
        }
        PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => {}
    }
}

fn polyline_pcurve(record: &RawRecord, points: &BTreeMap<u64, Point2>) -> Option<PcurveGeometry> {
    let control_points = record
        .parameter(1)?
        .list()?
        .iter()
        .map(|value| value.reference().and_then(|id| points.get(&id).copied()))
        .collect::<Option<Vec<_>>>()?;
    if control_points.len() < 2 {
        return None;
    }
    let last = (control_points.len() - 1) as f64;
    let mut knots = Vec::with_capacity(control_points.len() + 2);
    knots.push(0.0);
    knots.extend((0..control_points.len()).map(|index| index as f64));
    knots.push(last);
    Some(PcurveGeometry::Nurbs {
        degree: 1,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

fn polyline(record: &RawRecord, points: &BTreeMap<u64, Point3>) -> Option<NurbsCurve> {
    let control_points = record
        .parameter(1)?
        .list()?
        .iter()
        .map(|value| value.reference().and_then(|id| points.get(&id).copied()))
        .collect::<Option<Vec<_>>>()?;
    if control_points.len() < 2 {
        return None;
    }
    let last = (control_points.len() - 1) as f64;
    let mut knots = Vec::with_capacity(control_points.len() + 2);
    knots.push(0.0);
    knots.extend((0..control_points.len()).map(|index| index as f64));
    knots.push(last);
    Some(NurbsCurve {
        degree: 1,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

fn nurbs_surface(
    record: &RawRecord,
    points: &BTreeMap<u64, Point3>,
    warnings: &mut Vec<String>,
) -> Option<NurbsSurface> {
    let complex = record.partials.len() > 1;
    let base = if complex {
        record.partial("B_SPLINE_SURFACE")?
    } else {
        record.partial("B_SPLINE_SURFACE_WITH_KNOTS")?
    };
    let offset = usize::from(!complex);
    let u_degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let v_degree = u32::try_from(base.parameters.get(offset + 1)?.integer()?).ok()?;
    let rows = base.parameters.get(offset + 2)?.list()?;
    let u_count = u32::try_from(rows.len()).ok()?;
    let v_count = u32::try_from(rows.first()?.list()?.len()).ok()?;
    if u_count == 0
        || v_count == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || rows.iter().any(|row| {
            row.list()
                .is_none_or(|values| values.len() != v_count as usize)
        })
    {
        return None;
    }
    let control_points = rows
        .iter()
        .flat_map(|row| row.list().expect("row shape was validated"))
        .map(|value| value.reference().and_then(|id| points.get(&id).copied()))
        .collect::<Option<Vec<_>>>()?;
    let u_periodic = periodic_value(
        base.parameters.get(offset + 4),
        "B_SPLINE_SURFACE_WITH_KNOTS U direction",
        record.id,
        warnings,
    )?;
    let v_periodic = periodic_value(
        base.parameters.get(offset + 5),
        "B_SPLINE_SURFACE_WITH_KNOTS V direction",
        record.id,
        warnings,
    )?;
    let knot_leaf = record.partial("B_SPLINE_SURFACE_WITH_KNOTS")?;
    let tail = knot_leaf.parameters.len().checked_sub(5)?;
    let expected_u = usize::try_from(u_count)
        .ok()?
        .checked_add(usize::try_from(u_degree).ok()?)?
        .checked_add(1)?;
    let expected_v = usize::try_from(v_count)
        .ok()?
        .checked_add(usize::try_from(v_degree).ok()?)?
        .checked_add(1)?;
    let u_knots = expand_knots(
        &knot_leaf.parameters[tail],
        &knot_leaf.parameters[tail + 2],
        expected_u,
    )?;
    let v_knots = expand_knots(
        &knot_leaf.parameters[tail + 1],
        &knot_leaf.parameters[tail + 3],
        expected_v,
    )?;
    if u_knots.len() != expected_u || v_knots.len() != expected_v {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_SURFACE") {
        let rows = leaf.parameters.first()?.list()?;
        let mut values = Vec::new();
        for row in rows {
            values.extend(
                row.list()?
                    .iter()
                    .map(Value::number)
                    .collect::<Option<Vec<_>>>()?,
            );
        }
        (values.len() == control_points.len())
            .then_some(values)
            .map(Some)?
    } else {
        None
    };
    Some(NurbsSurface {
        u_degree,
        v_degree,
        u_knots,
        v_knots,
        u_count,
        v_count,
        control_points,
        weights,
        u_periodic,
        v_periodic,
    })
}

fn expand_knots(multiplicities: &Value, distinct: &Value, expected: usize) -> Option<Vec<f64>> {
    let multiplicities = multiplicities.list()?;
    let distinct = distinct.list()?;
    if multiplicities.len() != distinct.len() {
        return None;
    }
    let mut knots = Vec::new();
    knots.try_reserve_exact(expected).ok()?;
    for (multiplicity, knot) in multiplicities.iter().zip(distinct) {
        let count = usize::try_from(multiplicity.integer()?).ok()?;
        let knot = knot.number()?;
        if count == 0 || !knot.is_finite() {
            return None;
        }
        if knots.len().checked_add(count)? > expected {
            return None;
        }
        knots.extend(std::iter::repeat_n(knot, count));
    }
    knots
        .windows(2)
        .all(|pair| pair[0] <= pair[1])
        .then_some(knots)
}

fn references(value: &Value) -> Option<Vec<u64>> {
    value.list()?.iter().map(Value::reference).collect()
}

fn curve_carrier_record(id: u64, exchange: &Exchange) -> Option<u64> {
    let record = exchange.records.get(&id)?;
    if record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    }) {
        surface_curve_basis(record)
    } else {
        Some(id)
    }
}

fn surface_curve_pcurves(record: &RawRecord) -> Option<Vec<u64>> {
    if record.partials.len() == 1 {
        return record.parameter(2).and_then(references);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
        .or_else(|| record.partial("INTERSECTION_CURVE"))
        .and_then(|partial| partial.parameters.get(1).and_then(references))
}

fn numbers(value: &Value) -> Option<Vec<f64>> {
    value.list()?.iter().map(Value::number).collect()
}

fn normalize(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| scale_vector(vector, 1.0 / norm))
}

fn cartesian_transformation_operator(
    record: &RawRecord,
    points: &BTreeMap<u64, Point3>,
    directions: &BTreeMap<u64, Vector3>,
) -> Option<Transform> {
    let axis_x = named_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 1)?
        .reference()
        .and_then(|id| directions.get(&id).copied())?;
    let axis_y = named_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 2)?
        .reference()
        .and_then(|id| directions.get(&id).copied())?;
    let origin = named_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 3)?
        .reference()
        .and_then(|id| points.get(&id).copied())?;
    let scale = match named_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 4) {
        Some(Value::Omitted | Value::Derived) | None => 1.0,
        Some(value) => value.number()?,
    };
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let axis_z = match named_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 5) {
        Some(Value::Reference(id)) => directions.get(id).copied()?,
        Some(Value::Omitted | Value::Derived) | None => normalize(cross(axis_x, axis_y))?,
        Some(_) => return None,
    };
    Some(Transform {
        rows: [
            [
                axis_x.x * scale,
                axis_y.x * scale,
                axis_z.x * scale,
                origin.x,
            ],
            [
                axis_x.y * scale,
                axis_y.y * scale,
                axis_z.y * scale,
                origin.y,
            ],
            [
                axis_x.z * scale,
                axis_y.z * scale,
                axis_z.z * scale,
                origin.z,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ],
    })
}

fn scale_vector(vector: Vector3, scale: f64) -> Vector3 {
    Vector3::new(vector.x * scale, vector.y * scale, vector.z * scale)
}

fn optional_direction(
    value: Option<&Value>,
    directions: &BTreeMap<u64, Vector3>,
) -> Option<Vector3> {
    match value? {
        Value::Omitted => None,
        Value::Reference(id) => directions.get(id).copied(),
        _ => None,
    }
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord>;
    fn parameter(&self, index: usize) -> Option<&Value>;
}

impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn partial(&self, name: &str) -> Option<&crate::parse::PartialRecord> {
        self.partials.iter().find(|partial| partial.name == name)
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.partials.first()?.parameters.get(index)
    }
}

trait ValueExt {
    fn number(&self) -> Option<f64>;
    fn reference(&self) -> Option<u64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
    fn integer(&self) -> Option<i64>;
    fn logical(&self) -> Option<bool>;
}

impl ValueExt for Value {
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(v) => Some(*v),
            Value::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }
    fn reference(&self) -> Option<u64> {
        match self {
            Value::Reference(id) => Some(*id),
            _ => None,
        }
    }
    fn list(&self) -> Option<&[Value]> {
        match self {
            Value::List(values) => Some(values),
            _ => None,
        }
    }
    fn enumeration(&self) -> Option<&str> {
        match self {
            Value::Enumeration(value) => Some(value),
            _ => None,
        }
    }
    fn integer(&self) -> Option<i64> {
        match self {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }
    fn logical(&self) -> Option<bool> {
        match self {
            Value::Enumeration(value) if value == "T" => Some(true),
            Value::Enumeration(value) if value == "F" => Some(false),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_iso_si_prefix_resolves_to_its_exact_factor() {
        let expected = [
            ("EXA", 1e18),
            ("PETA", 1e15),
            ("TERA", 1e12),
            ("GIGA", 1e9),
            ("MEGA", 1e6),
            ("KILO", 1e3),
            ("HECTO", 1e2),
            ("DECA", 1e1),
            ("DECI", 1e-1),
            ("CENTI", 1e-2),
            ("MILLI", 1e-3),
            ("MICRO", 1e-6),
            ("NANO", 1e-9),
            ("PICO", 1e-12),
            ("FEMTO", 1e-15),
            ("ATTO", 1e-18),
        ];
        for (prefix, factor) in expected {
            assert_eq!(si_prefix(prefix), Some(factor), "prefix {prefix}");
        }
    }

    #[test]
    fn pcurve_trim_select_ignores_cartesian_point_coordinates() {
        let value = Value::List(vec![
            Value::Typed(
                "CARTESIAN_POINT".into(),
                Box::new(Value::List(vec![Value::Real(17.0), Value::Real(23.0)])),
            ),
            Value::Real(0.25),
        ]);
        assert_eq!(pcurve_trim_parameter(&value), Some(0.25));
    }

    #[test]
    fn pcurve_trim_select_prefers_parameter_value() {
        let value = Value::List(vec![
            Value::Real(17.0),
            Value::Typed("PARAMETER_VALUE".into(), Box::new(Value::Real(0.25))),
        ]);
        assert_eq!(pcurve_trim_parameter(&value), Some(0.25));
    }

    #[test]
    fn periodic_edge_range_normalizes_the_upper_domain_endpoint() {
        let geometry = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 1.0,
        };
        let tau = std::f64::consts::TAU;
        let [start, end] = edge_parameter_range(&geometry, tau, tau + 0.5).expect("edge range");
        assert!(start.abs() < 1.0e-12);
        assert!((end - 0.5).abs() < 1.0e-12);
    }
}
