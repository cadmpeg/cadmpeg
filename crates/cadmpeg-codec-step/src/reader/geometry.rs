// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point};
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, NurbsCurve,
    NurbsSurface, Pcurve, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    CurveId, PcurveId, PointId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Point;
use cadmpeg_ir::transform::{Transform, Transform2};
use cadmpeg_ir::SourceObjectAssociation;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::index::{step_instance_id, CarrierIndex};
use super::{opaque_record_id, StageOutcome};

const RANGE_INFERENCE_WORK_UNITS: u64 = 4_096;

pub(super) struct GeometryData {
    pub placements: BTreeMap<u64, (Point3, Vector3, Vector3)>,
    pub transformation_operators: BTreeMap<u64, Transform>,
    pub length_scale: f64,
    pub plane_angle_scale: f64,
    pub length_scales: BTreeMap<u64, f64>,
    pub plane_angle_scales: BTreeMap<u64, f64>,
}

pub(super) fn placement_transform(
    (origin, z_axis, x_axis): (Point3, Vector3, Vector3),
) -> Transform {
    let y_axis = Vector3::new(
        z_axis.y * x_axis.z - z_axis.z * x_axis.y,
        z_axis.z * x_axis.x - z_axis.x * x_axis.z,
        z_axis.x * x_axis.y - z_axis.y * x_axis.x,
    );
    let placement_basis = [
        [x_axis.x, y_axis.x, z_axis.x],
        [x_axis.y, y_axis.y, z_axis.y],
        [x_axis.z, y_axis.z, z_axis.z],
    ];
    let mut rows = Transform::identity().rows;
    for row in 0..3 {
        for column in 0..3 {
            rows[row][column] = placement_basis[row][column];
        }
    }
    rows[0][3] = origin.x;
    rows[1][3] = origin.y;
    rows[2][3] = origin.z;
    Transform { rows }
}

/// Infer the carrier interval trimmed by each edge's endpoint vertices.
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
    let work = u64::try_from(candidates.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(RANGE_INFERENCE_WORK_UNITS);
    if let Some(ctx) = ctx {
        ctx.charge_work(work, "step_edge_parameter_inference")?;
    }

    let model_index = cadmpeg_ir::index::ModelIndex::new(ir);
    let inferred = candidates
        .into_iter()
        .filter_map(|(edge_index, curve, start, end)| {
            let geometry = &model_index.curves(curve.0.as_str())?.geometry;
            let start_seed = curve_endpoint_seed(geometry, false, 0.0);
            let start_parameter = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                &model_index,
                &curve,
                start,
                start_seed,
            )?;
            let end_seed = curve_endpoint_seed(geometry, true, start_parameter);
            let end_parameter = cadmpeg_ir::eval::model_curve_parameter_near_point_in_index(
                &model_index,
                &curve,
                end,
                end_seed,
            )?;
            edge_parameter_range(geometry, start_parameter, end_parameter)
                .map(|range| (edge_index, range))
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

fn curve_endpoint_seed(geometry: &CurveGeometry, upper: bool, fallback: f64) -> f64 {
    match geometry {
        CurveGeometry::Nurbs(nurbs) if !nurbs.periodic => {
            nurbs_curve_parameter_domain(nurbs).map_or(fallback, |[lower, upper_bound]| {
                if upper {
                    upper_bound
                } else {
                    lower
                }
            })
        }
        CurveGeometry::Transformed { basis, .. } => curve_endpoint_seed(basis, upper, fallback),
        _ => fallback,
    }
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
        CurveGeometry::Transformed { basis, .. } => {
            return edge_parameter_range(basis, start, end);
        }
        _ => None,
    };
    let Some([lower, upper]) = periodic_domain else {
        return (end > start).then_some([start, end]);
    };
    let period = upper - lower;
    if !period.is_finite() || period <= 0.0 {
        return None;
    }
    let sweep = (end - start).rem_euclid(period);
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

struct UnitScales {
    length: BTreeMap<u64, f64>,
    angle: BTreeMap<u64, f64>,
}

impl UnitScales {
    fn length(&self, id: u64, fallback: f64) -> f64 {
        self.length.get(&id).copied().unwrap_or(fallback)
    }

    fn angle(&self, id: u64, fallback: f64) -> f64 {
        self.angle.get(&id).copied().unwrap_or(fallback)
    }
}

fn resolve_source_curve_parameter_scales(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    default_length: f64,
    default_angle: f64,
) -> BTreeMap<u64, f64> {
    exchange
        .records
        .keys()
        .filter_map(|id| {
            source_curve_parameter_scale(
                *id,
                exchange,
                unit_scales,
                default_length,
                default_angle,
                &mut BTreeSet::new(),
            )
            .map(|scale| (*id, scale))
        })
        .collect()
}

fn source_curve_parameter_scale(
    id: u64,
    exchange: &Exchange,
    unit_scales: &UnitScales,
    default_length: f64,
    default_angle: f64,
    active: &mut BTreeSet<u64>,
) -> Option<f64> {
    if !active.insert(id) {
        return None;
    }
    let scale = (|| {
        let record = exchange.records.get(&id)?;
        if record.partial("LINE").is_some() {
            let magnitude = named_parameter(record, "LINE", 2)
                .and_then(Value::reference)
                .and_then(|vector| exchange.records.get(&vector))
                .filter(|vector| vector.partial("VECTOR").is_some())
                .and_then(|vector| named_parameter(vector, "VECTOR", 2))
                .and_then(Value::number)
                .filter(|magnitude| magnitude.is_finite() && *magnitude > 0.0)?;
            let scale = magnitude * unit_scales.length(id, default_length);
            return scale.is_finite().then_some(scale);
        }
        if record.partial("CIRCLE").is_some() || record.partial("ELLIPSE").is_some() {
            return Some(unit_scales.angle(id, default_angle));
        }
        if record.partial("PARABOLA").is_some()
            || record.partial("HYPERBOLA").is_some()
            || record.partial("POLYLINE").is_some()
            || record.partials.iter().any(|partial| {
                matches!(
                    partial.name.as_str(),
                    "B_SPLINE_CURVE_WITH_KNOTS"
                        | "UNIFORM_CURVE"
                        | "QUASI_UNIFORM_CURVE"
                        | "BEZIER_CURVE"
                )
            })
        {
            return Some(1.0);
        }
        let parent = ["CURVE_REPLICA", "TRIMMED_CURVE", "OFFSET_CURVE_3D"]
            .into_iter()
            .find_map(|name| {
                named_parameter(record, name, 1)
                    .and_then(Value::reference)
                    .and_then(|parent| curve_carrier_record(parent, exchange))
            })
            .or_else(|| {
                record
                    .partials
                    .iter()
                    .any(|partial| {
                        matches!(
                            partial.name.as_str(),
                            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
                        )
                    })
                    .then(|| surface_curve_basis(record))
                    .flatten()
            })?;
        source_curve_parameter_scale(
            parent,
            exchange,
            unit_scales,
            default_length,
            default_angle,
            active,
        )
    })();
    active.remove(&id);
    scale
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> StageOutcome<GeometryData> {
    let mut losses = Vec::new();
    let scale = length_scale(exchange).unwrap_or_else(|| {
        losses.push(StepLossCode::DocumentLengthUnitUnresolved.note(
            "the document length unit did not resolve; coordinates are unscaled and reported as millimetres",
        ));
        1.0
    });
    let angle_scale = plane_angle_scale(exchange).unwrap_or_else(|| {
        losses.push(StepLossCode::DocumentAngleUnitUnresolved.note(
            "the document plane-angle unit did not resolve; angles are unscaled and reported as radians",
        ));
        1.0
    });
    let unit_scales = resolve_unit_scales(exchange, scale, angle_scale, &mut losses);
    let source_curve_parameter_scales =
        resolve_source_curve_parameter_scales(exchange, &unit_scales, scale, angle_scale);
    let mut typed = HashSet::new();
    let mut warnings = Vec::new();
    let mut points = BTreeMap::new();
    let mut points2 = BTreeMap::new();
    let mut apll_point_names = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut directions2 = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut vectors2 = BTreeMap::new();
    let mut placements = BTreeMap::new();
    let mut placements2 = BTreeMap::new();
    match linear_uncertainty(exchange) {
        LinearUncertainty::Value(uncertainty) => ir.tolerances.linear = uncertainty,
        LinearUncertainty::Empty { unresolved } => {
            if unresolved > 0 {
                losses.push(StepLossCode::UncertaintyLengthUnresolved.note(format!(
                    "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT has no resolvable length measure and {unresolved} unresolved measure(s); the linear tolerance was not transferred"
                )));
            }
        }
        LinearUncertainty::Ambiguous { values, unresolved } => {
            let default_linear = ir.tolerances.linear;
            let listed = values
                .iter()
                .map(|value| format!("{value:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(StepLossCode::UncertaintyLengthAmbiguous.note(format!(
                "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT records give {} different linear uncertainty values in millimetres ({listed}) and {unresolved} unresolved measure(s); the linear tolerance keeps the default {default_linear:?}",
                values.len()
            )));
        }
    }

    for (id, record) in exchange.entities_any(&[
        "APLL_POINT",
        "APLL_POINT_WITH_SURFACE",
        "CARTESIAN_POINT",
        "DIRECTION",
    ]) {
        match entity_type(
            record,
            &[
                "APLL_POINT",
                "APLL_POINT_WITH_SURFACE",
                "CARTESIAN_POINT",
                "DIRECTION",
            ],
        ) {
            Some(point_type @ ("APLL_POINT" | "APLL_POINT_WITH_SURFACE")) => {
                let record_scale = unit_scales.length(id, scale);
                if let Some(position) = apll_point_coordinates(record, point_type, record_scale) {
                    points.insert(id, position);
                    let source_name = representation_item_name(record)
                        .and_then(|value| {
                            super::decode_text(
                                exchange,
                                value,
                                &mut losses,
                                id,
                                "APLL point name",
                                StepLossCode::MetadataStringInvalid,
                            )
                        })
                        .filter(|name| !name.is_empty());
                    apll_point_names.insert(id, source_name);
                } else {
                    warnings.push(format!("{point_type} #{id} has invalid coordinates"));
                }
            }
            Some("CARTESIAN_POINT") => {
                let record_scale = unit_scales.length(id, scale);
                if let Some(position) =
                    named_coordinates(record, "CARTESIAN_POINT", 1, record_scale)
                {
                    points.insert(id, position);
                    typed.insert(id);
                } else if let Some(position) = named_coordinates2(record, "CARTESIAN_POINT", 1) {
                    points2.insert(id, position);
                    typed.insert(id);
                } else {
                    warnings.push(format!("CARTESIAN_POINT #{id} has invalid coordinates"));
                }
            }
            Some("DIRECTION") => {
                if let Some(direction) =
                    vector3(named_parameter(record, "DIRECTION", 1), 1.0).and_then(normalize)
                {
                    directions.insert(id, direction);
                    typed.insert(id);
                } else if let Some(direction) =
                    vector2(named_parameter(record, "DIRECTION", 1)).and_then(normalize2)
                {
                    directions2.insert(id, direction);
                    typed.insert(id);
                } else {
                    warnings.push(format!("DIRECTION #{id} is invalid or zero"));
                }
            }
            _ => {}
        }
    }
    decode_tessellated_curve_sets(
        exchange,
        &unit_scales,
        scale,
        ir,
        &mut typed,
        &mut warnings,
        &mut losses,
    );
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
            .any(|partial| super::representation::is_representation_name(&partial.name))
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
        if is_apll_leader_line(record) {
            let mut references = Vec::new();
            for parameter in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
            {
                collect_references(parameter, &mut references);
            }
            point_carriers.extend(references.into_iter().filter(|id| points.contains_key(id)));
        }
        if let Some(item) = record
            .partials
            .iter()
            .find(|partial| {
                matches!(
                    partial.name.as_str(),
                    "GEOMETRIC_ITEM_SPECIFIC_USAGE" | "ITEM_IDENTIFIED_REPRESENTATION_USAGE"
                )
            })
            .and_then(|partial| partial.parameters.get(4))
            .and_then(Value::reference)
        {
            if points.contains_key(&item) {
                point_carriers.insert(item);
            }
        }
        if let Some(id) = super::presentation::styled_item_target(record) {
            if points.contains_key(&id) {
                point_carriers.insert(id);
            }
        }
    }
    ir.model
        .points
        .extend(point_carriers.into_iter().filter_map(|id| {
            points.get(&id).copied().map(|position| Point {
                source_object: apll_point_names
                    .get(&id)
                    .map(|name| SourceObjectAssociation {
                        format: "step".into(),
                        object_id: format!("#{id}"),
                        name: name.clone(),
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                id: PointId(StepIdentity::data("point", id)),
                position,
            })
        }));
    for (id, record) in exchange.entities("VECTOR") {
        if record.partial("VECTOR").is_some() {
            let record_scale = unit_scales.length(id, scale);
            let value = named_parameter(record, "VECTOR", 1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(named_parameter(record, "VECTOR", 2).and_then(Value::number))
                .map(|(direction, magnitude)| direction.scale(magnitude * record_scale));
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
                                losses.push(StepLossCode::PlacementReferenceInferred.note(format!(
                                        "AXIS2_PLACEMENT_3D #{id} has a reference direction parallel to its axis; inferred an orthogonal reference"
                                    )));
                                first_projected_axis(axis)
                                    .unwrap_or(Vector3::new(1.0, 0.0, 0.0))
                            }
                        }
                        None => first_projected_axis(axis).unwrap_or(Vector3::new(1.0, 0.0, 0.0)),
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
    let mut transformation_operators2 = BTreeMap::new();
    for (id, record) in exchange.entities("CARTESIAN_TRANSFORMATION_OPERATOR_2D") {
        if let Some(transform) =
            cartesian_transformation_operator_2d(record, &points2, &directions2)
        {
            transformation_operators2.insert(id, transform);
            typed.insert(id);
        } else {
            warnings.push(format!(
                "CARTESIAN_TRANSFORMATION_OPERATOR_2D #{id} has invalid axes, origin, or scale"
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
        let representation_id = named_parameter(record, "PCURVE", 2).and_then(Value::reference);
        let Some(items) = representation_id
            .and_then(|representation| exchange.records.get(&representation))
            .and_then(representation_items)
        else {
            continue;
        };
        let pcurve_angle_scale = representation_id.map_or(angle_scale, |representation| {
            unit_scales.angle(representation, angle_scale)
        });
        let decoded = items
            .iter()
            .filter_map(|curve| {
                decode_pcurve_geometry(
                    *curve,
                    exchange,
                    &points2,
                    &vectors2,
                    &placements2,
                    &transformation_operators2,
                    pcurve_angle_scale,
                    &mut warnings,
                    &mut BTreeSet::new(),
                    0,
                )
                .map(|decoded| (*curve, decoded))
            })
            .collect::<Vec<_>>();
        if let [(curve, decoded)] = decoded.as_slice() {
            pcurve_geometry_records.extend(decoded.1.iter().copied());
            pcurve_geometries.insert(*curve, decoded.clone());
        }
    }
    let mut curve_parameter_offsets = BTreeMap::<u64, f64>::new();
    for (id, record) in exchange.entities_any(&[
        "LINE",
        "CIRCLE",
        "ELLIPSE",
        "PARABOLA",
        "HYPERBOLA",
        "POLYLINE",
        "B_SPLINE_CURVE_WITH_KNOTS",
        "UNIFORM_CURVE",
        "QUASI_UNIFORM_CURVE",
        "BEZIER_CURVE",
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
                "UNIFORM_CURVE",
                "QUASI_UNIFORM_CURVE",
                "BEZIER_CURVE",
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
        let record_scale = unit_scales.length(id, scale);
        let parameter_offset = if curve_type == "ELLIPSE" {
            let first_radius = named_parameter(record, "ELLIPSE", 2).and_then(Value::number);
            let second_radius = named_parameter(record, "ELLIPSE", 3).and_then(Value::number);
            first_radius
                .zip(second_radius)
                .filter(|(first, second)| first.is_finite() && second.is_finite())
                .and_then(|(first, second)| {
                    (first < second).then_some(-std::f64::consts::FRAC_PI_2)
                })
        } else {
            None
        };
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
                        radius: radius * record_scale,
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
                    |(((center, axis, reference_direction), first_radius), second_radius)| {
                        let first_radius = first_radius * record_scale;
                        let second_radius = second_radius * record_scale;
                        let (major_direction, major_radius, minor_radius) =
                            if first_radius >= second_radius {
                                (reference_direction, first_radius, second_radius)
                            } else {
                                // STEP ELLIPSE stores two ordered semiaxes;
                                // neither position is required to be the
                                // longer one. The IR ellipse is canonicalized
                                // around its semi-major direction.
                                (axis.cross(reference_direction), second_radius, first_radius)
                            };
                        CurveGeometry::Ellipse {
                            center,
                            axis,
                            major_direction,
                            major_radius,
                            minor_radius,
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
                        focal_distance: focal_distance * record_scale,
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
                            major_radius: major_radius * record_scale,
                            minor_radius: minor_radius * record_scale,
                        }
                    },
                ),
            "POLYLINE" => polyline(record, &points).map(CurveGeometry::Nurbs),
            "B_SPLINE_CURVE_WITH_KNOTS"
            | "UNIFORM_CURVE"
            | "QUASI_UNIFORM_CURVE"
            | "BEZIER_CURVE" => {
                nurbs_curve(record, &points, &mut warnings).map(CurveGeometry::Nurbs)
            }
            _ => unreachable!("curve type was selected from the dispatch list"),
        };
        if let Some(geometry) = geometry {
            if let Some(offset) = parameter_offset {
                curve_parameter_offsets.insert(id, offset);
            }
            ir.model.curves.push(Curve {
                id: CurveId(StepIdentity::data("curve", id)),
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
                id: CurveId(StepIdentity::data("curve", id)),
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
            "CURVE_REPLICA",
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
        if let Some(parent_reference_step) = record
            .partial("CURVE_REPLICA")
            .and_then(|_| named_parameter(record, "CURVE_REPLICA", 1))
            .and_then(Value::reference)
        {
            let Some(parent_step) = curve_carrier_record(parent_reference_step, exchange) else {
                continue;
            };
            let Some(operator_step) =
                named_parameter(record, "CURVE_REPLICA", 2).and_then(Value::reference)
            else {
                continue;
            };
            let Some(parent_index) = carrier_index.curves.get(&parent_step).copied() else {
                waiting_on.entry(parent_step).or_default().push(id);
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
            let curve = CurveId(StepIdentity::data("curve", id));
            ir.model.curves.push(Curve {
                id: curve.clone(),
                geometry: CurveGeometry::Transformed {
                    basis: Box::new(basis),
                    transform,
                },
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: ProceduralCurveId(StepIdentity::construction("curve_replica", id)),
                curve,
                definition: ProceduralCurveDefinition::Replica {
                    source: CurveId(StepIdentity::data("curve", parent_step)),
                    transform,
                },
                cache_fit_tolerance: None,
            });
            carrier_index.curves.insert(id, curve_index);
            if let Some(offset) = curve_parameter_offsets.get(&parent_step).copied() {
                curve_parameter_offsets.insert(id, offset);
            }
            typed.insert(id);
            typed.insert(operator_step);
            wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
            continue;
        }
        if let Some(parameters) = entity_parameters(record, "TRIMMED_CURVE") {
            let Some((basis_reference_step, sense, master_representation)) =
                trimmed_curve_attributes(parameters)
            else {
                continue;
            };
            let Some(basis_step) = curve_carrier_record(basis_reference_step, exchange) else {
                continue;
            };
            if !carrier_index.curves.contains_key(&basis_step) {
                waiting_on.entry(basis_step).or_default().push(id);
                continue;
            }
            let curve = CurveId(StepIdentity::data("curve", id));
            let basis = CurveId(StepIdentity::data("curve", basis_step));
            let Some(geometry) = carrier_index
                .curves
                .get(&basis_step)
                .and_then(|index| ir.model.curves.get(*index))
                .map(|candidate| candidate.geometry.clone())
            else {
                continue;
            };
            let record_scale = unit_scales.length(id, scale);
            let record_angle_scale = unit_scales.angle(id, angle_scale);
            let parameter_offset = curve_parameter_offsets
                .get(&basis_step)
                .copied()
                .unwrap_or(0.0);
            let linear_parameter_scale =
                line_parameter_scale(exchange, basis_reference_step, record_scale, &mut losses);
            let (start, end) = {
                let mut trim_context = TrimParameterContext {
                    points: &points,
                    geometry: &geometry,
                    angle_scale: record_angle_scale,
                    linear_parameter_scale,
                    parameter_offset,
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
            let parameter_range = trimmed_curve_parameter_range(&geometry, start, end, sense);
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: curve.clone(),
                geometry,
                source_object: None,
            });
            ir.model.procedural_curves.push(ProceduralCurve {
                id: ProceduralCurveId(StepIdentity::construction("trimmed_curve", id)),
                curve: curve.clone(),
                definition: ProceduralCurveDefinition::Subset {
                    source: basis,
                    parameter_range,
                    sense,
                },
                cache_fit_tolerance: Some(0.0),
            });
            carrier_index.curves.insert(id, curve_index);
            if parameter_offset != 0.0 {
                curve_parameter_offsets.insert(id, parameter_offset);
            }
            typed.insert(id);
            wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
            continue;
        }
        if composite_curve_parameters(record).is_some() {
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
            let curve = CurveId(StepIdentity::data("curve", id));
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
        let source_reference_step = parameters.get(1).and_then(Value::reference);
        let source_step =
            source_reference_step.and_then(|source| curve_carrier_record(source, exchange));
        let source = source_step.map(|source| CurveId(StepIdentity::data("curve", source)));
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
        let curve = CurveId(StepIdentity::data("curve", id));
        let curve_index = ir.model.curves.len();
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry,
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(StepIdentity::construction("offset_curve", id)),
            curve: curve.clone(),
            definition: ProceduralCurveDefinition::SpatialOffset {
                source,
                distance: distance * unit_scales.length(id, scale),
                reference_direction,
                self_intersect,
            },
            cache_fit_tolerance: None,
        });
        carrier_index.curves.insert(id, curve_index);
        if let Some(offset) = curve_parameter_offsets.get(&source_step).copied() {
            curve_parameter_offsets.insert(id, offset);
        }
        typed.insert(id);
        wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
    }
    for (id, _) in exchange.entities("CURVE_REPLICA") {
        if let Entry::Vacant(entry) = carrier_index.curves.entry(id) {
            warnings.push(format!(
                "CURVE_REPLICA #{id} has invalid or unresolved parent/operator"
            ));
            let curve_index = ir.model.curves.len();
            ir.model.curves.push(Curve {
                id: CurveId(StepIdentity::data("curve", id)),
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
    for (id, record) in
        exchange.entities_any(&["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"])
    {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "{} #{id} has invalid, cyclic, or unresolved segments",
                record.simple_name().unwrap_or("COMPOSITE_CURVE")
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
            let curve = CurveId(StepIdentity::data("curve", id));
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
        let definition = match entity_type(
            record,
            &["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"],
        ) {
            Some("SURFACE_OF_LINEAR_EXTRUSION") => {
                named_parameter(record, "SURFACE_OF_LINEAR_EXTRUSION", 1)
                    .and_then(Value::reference)
                    .filter(|curve| carrier_index.curves.contains_key(curve))
                    .map(|curve| CurveId(StepIdentity::data("curve", curve)))
                    .zip(
                        named_parameter(record, "SURFACE_OF_LINEAR_EXTRUSION", 2)
                            .and_then(Value::reference)
                            .and_then(|vector| vectors.get(&vector).copied()),
                    )
                    .map(
                        |(directrix, direction)| ProceduralSurfaceDefinition::LinearSweep {
                            directrix,
                            direction,
                        },
                    )
            }
            Some("SURFACE_OF_REVOLUTION") => named_parameter(record, "SURFACE_OF_REVOLUTION", 1)
                .and_then(Value::reference)
                .filter(|curve| carrier_index.curves.contains_key(curve))
                .map(|curve| CurveId(StepIdentity::data("curve", curve)))
                .zip(
                    named_parameter(record, "SURFACE_OF_REVOLUTION", 2)
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
                entity_type(
                    record,
                    &["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"],
                )
                .expect("matched swept surface")
            ));
            continue;
        };
        let surface = SurfaceId(StepIdentity::data("surface", id));
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(StepIdentity::construction("swept_surface", id)),
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
        "UNIFORM_SURFACE",
        "QUASI_UNIFORM_SURFACE",
        "BEZIER_SURFACE",
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
                "UNIFORM_SURFACE",
                "QUASI_UNIFORM_SURFACE",
                "BEZIER_SURFACE",
            ],
        ) else {
            continue;
        };
        if surface_type == "B_SPLINE_SURFACE_WITH_KNOTS" && record.simple_name().is_none() {
            continue;
        }
        let record_scale = unit_scales.length(id, scale);
        let record_angle_scale = unit_scales.angle(id, angle_scale);
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
                        radius: radius * record_scale,
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
                        radius: radius * record_scale,
                        ratio: 1.0,
                        half_angle: half_angle * record_angle_scale,
                    }
                }),
            "SPHERICAL_SURFACE" => placement
                .zip(positive(named_parameter(record, "SPHERICAL_SURFACE", 2)))
                .map(
                    |((center, axis, ref_direction), radius)| SurfaceGeometry::Sphere {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * record_scale,
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
                            major_radius: major_radius * record_scale,
                            minor_radius: minor_radius * record_scale,
                        }
                    },
                ),
            "B_SPLINE_SURFACE_WITH_KNOTS"
            | "UNIFORM_SURFACE"
            | "QUASI_UNIFORM_SURFACE"
            | "BEZIER_SURFACE" => {
                nurbs_surface(record, &points, &mut warnings).map(SurfaceGeometry::Nurbs)
            }
            _ => unreachable!("surface type was selected from the dispatch list"),
        };
        if let Some(geometry) = geometry {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(StepIdentity::data("surface", id)),
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
                id: SurfaceId(StepIdentity::data("surface", id)),
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

    // Surface constructors form the same kind of dependency graph as curves.
    // Resolve replicas in the same fixpoint as trims, bounded surfaces, and
    // offsets so a forward or nested replica cannot become an opaque carrier.
    carrier_index = CarrierIndex::from_ir(ir);
    let deferred_surface_ids = exchange
        .entities_any(&[
            "CURVE_BOUNDED_SURFACE",
            "OFFSET_SURFACE",
            "RECTANGULAR_TRIMMED_SURFACE",
            "SURFACE_REPLICA",
        ])
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    let mut deferred_surface_queue = VecDeque::from(deferred_surface_ids);
    let mut surface_waiting_on = HashMap::<u64, Vec<u64>>::new();
    while let Some(id) = deferred_surface_queue.pop_front() {
        if carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let Some(record) = exchange.records.get(&id) else {
            continue;
        };
        let resolved = if record.partial("RECTANGULAR_TRIMMED_SURFACE").is_some() {
            let Some(parameters) = entity_parameters(record, "RECTANGULAR_TRIMMED_SURFACE") else {
                continue;
            };
            let record_scale = unit_scales.length(id, scale);
            let record_angle_scale = unit_scales.angle(id, angle_scale);
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(mut parameter_ranges) = parameters
                .get(2)
                .and_then(Value::number)
                .zip(parameters.get(3).and_then(Value::number))
                .zip(
                    parameters
                        .get(4)
                        .and_then(Value::number)
                        .zip(parameters.get(5).and_then(Value::number)),
                )
                .map(|((u1, u2), (v1, v2))| [[u1, u2], [v1, v2]])
            else {
                continue;
            };
            let Some((u_sense, v_sense)) = parameters
                .get(6)
                .and_then(Value::logical)
                .zip(parameters.get(7).and_then(Value::logical))
            else {
                continue;
            };
            if !parameter_ranges
                .iter()
                .flatten()
                .all(|parameter| parameter.is_finite())
                || parameter_ranges[0][0] == parameter_ranges[0][1]
                || parameter_ranges[1][0] == parameter_ranges[1][1]
            {
                continue;
            }
            let Some(geometry) = carrier_index
                .surfaces
                .get(&support_step)
                .and_then(|index| ir.model.surfaces.get(*index))
                .map(|surface| surface.geometry.clone())
            else {
                surface_waiting_on.entry(support_step).or_default().push(id);
                continue;
            };
            let Some(parameter_scales) = surface_parameter_scales_for_step(
                ir,
                &SurfaceId(StepIdentity::data("surface", support_step)),
                &geometry,
                record_scale,
                record_angle_scale,
                &source_curve_parameter_scales,
            ) else {
                warnings.push(format!(
                    "RECTANGULAR_TRIMMED_SURFACE #{id} has no established support parameterization"
                ));
                continue;
            };
            for (range, parameter_scale) in parameter_ranges.iter_mut().zip(parameter_scales) {
                range[0] *= parameter_scale;
                range[1] *= parameter_scale;
            }
            for ((range, sense), period) in parameter_ranges
                .iter_mut()
                .zip([u_sense, v_sense])
                .zip(surface_parameter_periods(&geometry))
            {
                if let Some(period) = period {
                    if sense && range[1] < range[0] {
                        range[1] += period;
                    } else if !sense && range[0] < range[1] {
                        range[0] += period;
                    }
                }
            }
            if parameter_ranges
                .iter()
                .flatten()
                .any(|parameter| !parameter.is_finite())
            {
                continue;
            }
            let surface = SurfaceId(StepIdentity::data("surface", id));
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry,
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(StepIdentity::construction(
                    "rectangular_trimmed_surface",
                    id,
                )),
                surface,
                definition: ProceduralSurfaceDefinition::Subset {
                    support: SurfaceId(StepIdentity::data("surface", support_step)),
                    parameter_ranges,
                    u_sense: Some(u_sense),
                    v_sense: Some(v_sense),
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            carrier_index
                .surfaces
                .insert(id, ir.model.surfaces.len() - 1);
            typed.insert(id);
            true
        } else if record.partial("CURVE_BOUNDED_SURFACE").is_some() {
            let surface = SurfaceId(StepIdentity::data("surface", id));
            let Some(parameters) = entity_parameters(record, "CURVE_BOUNDED_SURFACE") else {
                continue;
            };
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            let Some(support_index) = carrier_index.surfaces.get(&support_step).copied() else {
                surface_waiting_on.entry(support_step).or_default().push(id);
                continue;
            };
            let support = SurfaceId(StepIdentity::data("surface", support_step));
            let boundary_steps = parameters.get(2).and_then(references);
            let boundaries = boundary_steps.as_ref().map(|boundaries| {
                boundaries
                    .iter()
                    .copied()
                    .map(|boundary| CurveId(StepIdentity::data("curve", boundary)))
                    .collect::<Vec<_>>()
            });
            let boundary_pcurves = boundary_steps
                .iter()
                .flatten()
                .flat_map(|boundary| boundary_pcurve_steps(*boundary, support_step, exchange))
                .map(|pcurve| PcurveId(StepIdentity::data("pcurve", pcurve)))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let implicit_outer = parameters.get(3).and_then(Value::logical);
            let Some((boundaries, implicit_outer, geometry)) = ir
                .model
                .surfaces
                .get(support_index)
                .map(|surface| surface.geometry.clone())
                .zip(boundaries)
                .zip(implicit_outer)
                .map(|((geometry, boundaries), implicit_outer)| {
                    (boundaries, implicit_outer, geometry)
                })
                .filter(|(boundaries, _, _)| {
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
                id: ProceduralSurfaceId(StepIdentity::construction("curve_bounded_surface", id)),
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
            true
        } else if record.partial("OFFSET_SURFACE").is_some() {
            let surface = SurfaceId(StepIdentity::data("surface", id));
            let Some(parameters) = entity_parameters(record, "OFFSET_SURFACE") else {
                continue;
            };
            let record_scale = unit_scales.length(id, scale);
            let Some(support_step) = parameters.get(1).and_then(Value::reference) else {
                continue;
            };
            if !carrier_index.surfaces.contains_key(&support_step) {
                surface_waiting_on.entry(support_step).or_default().push(id);
                continue;
            }
            let support = SurfaceId(StepIdentity::data("surface", support_step));
            let distance = parameters.get(2).and_then(Value::number);
            let self_intersect = parameters
                .get(3)
                .and_then(logical_value)
                .map(StepLogical::into_option);
            let Some((distance, self_intersect)) = distance.zip(self_intersect) else {
                continue;
            };
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Unknown { record: None },
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(StepIdentity::construction("offset_surface", id)),
                surface,
                definition: ProceduralSurfaceDefinition::ParallelOffset {
                    support,
                    distance: distance * record_scale,
                    self_intersect,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            carrier_index.surfaces.insert(id, surface_index);
            typed.insert(id);
            true
        } else if record.partial("SURFACE_REPLICA").is_some() {
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
                surface_waiting_on.entry(parent_step).or_default().push(id);
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
            let surface = SurfaceId(StepIdentity::data("surface", id));
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry: SurfaceGeometry::Transformed {
                    basis: Box::new(basis),
                    transform,
                },
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(StepIdentity::construction("surface_replica", id)),
                surface,
                definition: ProceduralSurfaceDefinition::Replica {
                    source: SurfaceId(StepIdentity::data("surface", parent_step)),
                    transform,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            carrier_index.surfaces.insert(id, surface_index);
            typed.insert(id);
            typed.insert(operator_step);
            true
        } else {
            false
        };
        if resolved {
            wake_deferred_dependents(id, &mut surface_waiting_on, &mut deferred_surface_queue);
        }
    }
    for (id, _) in exchange.entities("SURFACE_REPLICA") {
        if let Entry::Vacant(entry) = carrier_index.surfaces.entry(id) {
            warnings.push(format!(
                "SURFACE_REPLICA #{id} has invalid or unresolved parent/operator"
            ));
            let surface_index = ir.model.surfaces.len();
            ir.model.surfaces.push(Surface {
                id: SurfaceId(StepIdentity::data("surface", id)),
                geometry: SurfaceGeometry::Unknown {
                    record: exchange.records.get(&id).map(opaque_record_id),
                },
                source_object: None,
            });
            entry.insert(surface_index);
        }
    }
    for (id, _) in exchange.entities("RECTANGULAR_TRIMMED_SURFACE") {
        if !carrier_index.surfaces.contains_key(&id) {
            warnings.push(format!(
                "RECTANGULAR_TRIMMED_SURFACE #{id} has invalid or unresolved basis/trim selectors"
            ));
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
                id: CurveId(StepIdentity::data("curve", curve_step)),
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
    for (id, _) in exchange.entities_any(&[
        "CURVE_BOUNDED_SURFACE",
        "OFFSET_SURFACE",
        "RECTANGULAR_TRIMMED_SURFACE",
    ]) {
        let surface = SurfaceId(StepIdentity::data("surface", id));
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
                id: SurfaceId(StepIdentity::data("surface", surface_step)),
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
    let surface_parameter_scales = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let id = step_instance_id(&surface.id.0)?;
            let scales = surface_parameter_scales_for_step(
                ir,
                &surface.id,
                &surface.geometry,
                unit_scales.length(id, scale),
                unit_scales.angle(id, angle_scale),
                &source_curve_parameter_scales,
            )?;
            Some((id, scales))
        })
        .collect::<BTreeMap<_, _>>();
    for (id, record) in exchange.entities("PCURVE") {
        if record.partial("PCURVE").is_none() {
            continue;
        }
        let surface_step = named_parameter(record, "PCURVE", 1).and_then(Value::reference);
        let representation = named_parameter(record, "PCURVE", 2)
            .and_then(Value::reference)
            .and_then(|representation| exchange.records.get(&representation));
        let curve_steps = representation
            .and_then(representation_items)
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
        let Some(scales) = surface_step.and_then(|surface| surface_parameter_scales.get(&surface))
        else {
            warnings.push(format!(
                "PCURVE #{id} has no established owning surface parameterization"
            ));
            continue;
        };
        let mut geometry = geometry.clone();
        if !scale_pcurve_geometry(&mut geometry, *scales) {
            warnings.push(format!(
                "PCURVE #{id} has a 2D carrier that cannot be scaled into the owning surface parameter units"
            ));
            continue;
        }
        ir.model.pcurves.push(Pcurve {
            id: PcurveId(StepIdentity::data("pcurve", id)),
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

    // Curve-bounded surfaces resolve before the PCURVE pass because their 3D
    // boundaries do not depend on parameter-space geometry. Remove candidate
    // references whose pcurve carrier did not decode.
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
        let surface = SurfaceId(StepIdentity::data("surface", id));
        if !carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(StepIdentity::construction("degenerate_torus", id)),
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
        }) || entity_type(record, &["SHAPE_REPRESENTATION"]).is_some()
        {
            typed.insert(id);
        }
    }
    StageOutcome {
        value: GeometryData {
            placements,
            transformation_operators,
            length_scale: scale,
            plane_angle_scale: angle_scale,
            length_scales: unit_scales.length,
            plane_angle_scales: unit_scales.angle,
        },
        claims: typed,
        warnings,
        losses,
        notes: Vec::new(),
    }
}

fn decode_tessellated_curve_sets(
    exchange: &Exchange,
    unit_scales: &UnitScales,
    fallback_scale: f64,
    ir: &mut CadIr,
    typed: &mut HashSet<u64>,
    warnings: &mut Vec<String>,
    losses: &mut Vec<LossNote>,
) {
    for (&id, record) in &exchange.records {
        if record.partial("TESSELLATED_CURVE_SET").is_none() {
            continue;
        }
        let Some(coordinates_id) =
            tessellated_curve_parameter(record, 0).and_then(ValueExt::reference)
        else {
            warnings.push(format!(
                "TESSELLATED_CURVE_SET #{id} has no COORDINATES_LIST reference"
            ));
            continue;
        };
        let Some(coordinates_record) = exchange.records.get(&coordinates_id) else {
            warnings.push(format!(
                "TESSELLATED_CURVE_SET #{id} references missing COORDINATES_LIST #{coordinates_id}"
            ));
            continue;
        };
        let scale = unit_scales.length(coordinates_id, fallback_scale);
        let Some(vertices) = coordinate_rows(coordinates_record, scale) else {
            warnings.push(format!(
                "TESSELLATED_CURVE_SET #{id} has invalid COORDINATES_LIST #{coordinates_id}"
            ));
            continue;
        };
        let Some(strips) =
            tessellated_line_strips(tessellated_curve_parameter(record, 1), vertices.len())
        else {
            warnings.push(format!(
                "TESSELLATED_CURVE_SET #{id} has invalid line strips"
            ));
            continue;
        };
        let source_name = representation_item_name(record)
            .and_then(|value| {
                super::decode_text(
                    exchange,
                    value,
                    losses,
                    id,
                    "tessellated curve name",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .filter(|name| !name.is_empty());
        for (strip_index, indices) in strips.into_iter().enumerate() {
            let curve_key = if strip_index == 0 {
                id.to_string()
            } else {
                format!("{id}-strip-{strip_index}")
            };
            let points = indices.into_iter().map(|index| vertices[index]).collect();
            ir.model.curves.push(Curve {
                id: CurveId(StepIdentity::data("curve", curve_key)),
                geometry: CurveGeometry::Polyline {
                    points,
                    parameters: None,
                    chordal_deflection: 0.0,
                },
                source_object: Some(SourceObjectAssociation {
                    format: "step".into(),
                    object_id: format!("#{id}"),
                    name: source_name.clone(),
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        typed.extend([id, coordinates_id]);
    }
}

fn tessellated_curve_parameter(record: &RawRecord, index: usize) -> Option<&Value> {
    let partial = record.partial("TESSELLATED_CURVE_SET")?;
    let offset = usize::from(record.partials.len() == 1);
    partial.parameters.get(index + offset)
}

fn tessellated_line_strips(value: Option<&Value>, point_count: usize) -> Option<Vec<Vec<usize>>> {
    let strips = value?.list()?;
    if strips.is_empty() {
        return None;
    }
    let mut decoded = Vec::with_capacity(strips.len());
    for strip in strips {
        let values = strip.list()?;
        if values.len() < 2 {
            return None;
        }
        let mut indices = Vec::with_capacity(values.len());
        for value in values {
            let index = usize::try_from(value.integer()?).ok()?.checked_sub(1)?;
            if index >= point_count {
                return None;
            }
            indices.push(index);
        }
        decoded.push(indices);
    }
    Some(decoded)
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
                .and_then(representation_item_name)
                .and_then(|value| {
                    super::decode_text(
                        exchange,
                        value,
                        losses,
                        member,
                        "geometric-set member name",
                        StepLossCode::MetadataStringInvalid,
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
            .any(|partial| super::representation::is_representation_name(&partial.name))
    }) {
        let Some(items) = representation_items(representation) else {
            continue;
        };
        for member in items {
            let source_name = exchange
                .records
                .get(&member)
                .and_then(representation_item_name)
                .and_then(|value| {
                    super::decode_text(
                        exchange,
                        value,
                        losses,
                        member,
                        "representation member name",
                        StepLossCode::MetadataStringInvalid,
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

/// Associate geometry that is owned by presentation records rather than by a
/// shape representation. A style is a source owner even when its style
/// assignment has no surface colour, and an annotation plane owns each
/// referenced surface used to construct that plane.
pub(super) fn associate_free_presentation_carriers(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    losses: &mut Vec<LossNote>,
) {
    for (style_id, target) in exchange.records.iter().filter_map(|(style_id, record)| {
        super::presentation::styled_item_target(record).map(|target| (*style_id, target))
    }) {
        associate_presentation_carrier(exchange, ir, index, owned, target, style_id, losses);
    }
    for (plane_id, plane) in exchange.entities("ANNOTATION_PLANE") {
        let mut targets = Vec::new();
        for parameter in plane
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_references(parameter, &mut targets);
        }
        for target in targets {
            if index.surfaces.contains_key(&target) {
                associate_presentation_carrier(
                    exchange, ir, index, owned, target, plane_id, losses,
                );
            }
        }
    }
}

fn associate_presentation_carrier(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
    target: u64,
    source_id: u64,
    losses: &mut Vec<LossNote>,
) {
    let name = exchange
        .records
        .get(&target)
        .and_then(representation_item_name)
        .and_then(|value| {
            super::decode_text(
                exchange,
                value,
                losses,
                target,
                "presentation carrier name",
                StepLossCode::MetadataStringInvalid,
            )
        })
        .filter(|name| !name.is_empty());
    let association = || SourceObjectAssociation {
        format: "step".into(),
        object_id: format!("#{source_id}"),
        name: name.clone(),
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    };
    if let Some(index) = index.curves.get(&target) {
        if !owned.curves.contains(index) {
            ir.model.curves[*index]
                .source_object
                .get_or_insert_with(association);
        }
    }
    if let Some(index) = index.points.get(&target) {
        if !owned.points.contains(index) {
            ir.model.points[*index]
                .source_object
                .get_or_insert_with(association);
        }
    }
    if let Some(index) = index.surfaces.get(&target) {
        if !owned.surfaces.contains(index) {
            ir.model.surfaces[*index]
                .source_object
                .get_or_insert_with(association);
        }
    }
}

fn collect_references(value: &Value, references: &mut Vec<u64>) {
    match value {
        Value::Reference(id) => references.push(*id),
        Value::List(values) => {
            for value in values {
                collect_references(value, references);
            }
        }
        Value::Typed(_, value) => collect_references(value, references),
        Value::Integer(_)
        | Value::Real(_)
        | Value::String(_)
        | Value::Enumeration(_)
        | Value::Binary(_)
        | Value::Resource(_)
        | Value::ValueReference(_)
        | Value::ConstantEntity(_)
        | Value::ConstantValue(_)
        | Value::Omitted
        | Value::Derived => {}
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

fn representation_item_name(record: &RawRecord) -> Option<&Value> {
    if record.partials.len() == 1 {
        record.parameter(0)
    } else {
        record
            .partial("REPRESENTATION_ITEM")
            .and_then(|partial| partial.parameters.first())
    }
}

fn entity_parameters<'a>(record: &'a RawRecord, name: &str) -> Option<&'a [Value]> {
    record
        .partial(name)
        .map(|partial| partial.parameters.as_slice())
}

fn named_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record.partial(name)?.parameters.get(index)
}

fn transformation_parameter<'a>(
    record: &'a RawRecord,
    name: &str,
    index: usize,
) -> Option<&'a Value> {
    let parameters = &record.partial(name)?.parameters;
    let (attribute_count, offset) = match (name, parameters.len()) {
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 6) => (5, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_3D", 8) => (5, 3),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 5) => (4, 1),
        ("CARTESIAN_TRANSFORMATION_OPERATOR_2D", 7) => (4, 3),
        _ => return None,
    };
    (index < attribute_count).then(|| &parameters[offset + index])
}

fn entity_type<'a>(record: &RawRecord, names: &[&'a str]) -> Option<&'a str> {
    names
        .iter()
        .copied()
        .find(|name| record.partial(name).is_some())
}

fn is_apll_leader_line(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "ANNOTATION_PLACEHOLDER_LEADER_LINE"
                | "ANNOTATION_TO_ANNOTATION_LEADER_LINE"
                | "ANNOTATION_TO_MODEL_LEADER_LINE"
                | "AUXILIARY_LEADER_LINE"
        )
    })
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
    parameter_offset: f64,
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
        .chain(
            ir.model
                .procedural_surfaces
                .iter()
                .filter_map(|surface| {
                    let ProceduralSurfaceDefinition::CurveBounded {
                        boundary_pcurves, ..
                    } = &surface.definition
                    else {
                        return None;
                    };
                    Some(boundary_pcurves)
                })
                .flatten()
                .map(|pcurve| pcurve.0.as_str()),
        )
        .collect::<BTreeSet<_>>();
    for (pcurve_id, record) in exchange.entities("PCURVE") {
        let pcurve_identity = StepIdentity::data("pcurve", pcurve_id);
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

/// Associate surfaces listed by retained `SURFACE_CURVE` records.
///
/// The associated-geometry list normally contains PCURVE records, but some
/// producers write the surface carriers directly. The IR stores the 3D basis
/// curve rather than the STEP wrapper, so this pass projects the wrapper's
/// surface dependencies onto the retained IR surfaces. Only wrappers that
/// participate in topology or an explicit free-geometry/presentation owner
/// are considered; an unrelated record with the same basis curve is not an
/// ownership proof.
pub(super) fn associate_surface_curve_supports(
    exchange: &Exchange,
    ir: &mut CadIr,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
) {
    let retained = retained_surface_curve_ids(exchange, index, owned);
    for (surface_curve_id, record) in
        exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"])
    {
        if !retained.contains(&surface_curve_id) {
            continue;
        }
        if let Some(curve_index) = surface_curve_basis(record)
            .and_then(|basis| index.curves.get(&basis))
            .copied()
        {
            ir.model.curves[curve_index]
                .source_object
                .get_or_insert_with(|| SourceObjectAssociation {
                    format: "step".into(),
                    object_id: format!("#{surface_curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                });
        }
        for surface_id in surface_curve_supports(record, exchange, index) {
            let Some(surface_index) = index.surfaces.get(&surface_id).copied() else {
                continue;
            };
            ir.model.surfaces[surface_index]
                .source_object
                .get_or_insert_with(|| SourceObjectAssociation {
                    format: "step".into(),
                    object_id: format!("#{surface_curve_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                });
        }
    }
}

fn retained_surface_curve_ids(
    exchange: &Exchange,
    index: &CarrierIndex,
    owned: &OwnedCarriers,
) -> BTreeSet<u64> {
    let mut retained = BTreeSet::new();

    for (_, edge) in exchange.entities("EDGE_CURVE") {
        let Some(surface_curve) = edge_curve_geometry_reference(edge) else {
            continue;
        };
        let Some(record) = exchange.records.get(&surface_curve) else {
            continue;
        };
        if !is_surface_curve_record(record) {
            continue;
        }
        let Some(basis) = surface_curve_basis(record) else {
            continue;
        };
        if index
            .curves
            .get(&basis)
            .is_some_and(|curve| owned.curves.contains(curve))
        {
            retained.insert(surface_curve);
        }
    }

    for set in exchange.records.values() {
        let Some(set_type) = entity_type(set, &["GEOMETRIC_SET", "GEOMETRIC_CURVE_SET"]) else {
            continue;
        };
        let Some(members) = named_parameter(set, set_type, 1).and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            if decoded_surface_curve(member, exchange, index) {
                retained.insert(member);
            }
        }
    }

    for representation in exchange.records.values().filter(|record| {
        record
            .partials
            .iter()
            .any(|partial| super::representation::is_representation_name(&partial.name))
    }) {
        let Some(items) = representation_items(representation) else {
            continue;
        };
        for item in items {
            if decoded_surface_curve(item, exchange, index) {
                retained.insert(item);
            }
        }
    }

    for record in exchange.records.values() {
        if let Some(target) = super::presentation::styled_item_target(record) {
            if decoded_surface_curve(target, exchange, index) {
                retained.insert(target);
            }
        }
    }
    for (_, plane) in exchange.entities("ANNOTATION_PLANE") {
        let mut references = Vec::new();
        for parameter in plane
            .partials
            .iter()
            .flat_map(|partial| partial.parameters.iter())
        {
            collect_references(parameter, &mut references);
        }
        for target in references {
            if decoded_surface_curve(target, exchange, index) {
                retained.insert(target);
            }
        }
    }

    retained
}

fn decoded_surface_curve(id: u64, exchange: &Exchange, index: &CarrierIndex) -> bool {
    let Some(record) = exchange.records.get(&id) else {
        return false;
    };
    is_surface_curve_record(record)
        && surface_curve_basis(record).is_some_and(|basis| index.curves.contains_key(&basis))
}

fn is_surface_curve_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
        )
    })
}

fn surface_curve_supports(
    record: &RawRecord,
    exchange: &Exchange,
    index: &CarrierIndex,
) -> Vec<u64> {
    let Some(associated_geometry) = surface_curve_associated_geometry(record) else {
        return Vec::new();
    };
    associated_geometry
        .into_iter()
        .filter_map(|associated| {
            let surface = exchange
                .records
                .get(&associated)
                .and_then(|record| named_parameter(record, "PCURVE", 1))
                .and_then(Value::reference)
                .or_else(|| {
                    index
                        .surfaces
                        .contains_key(&associated)
                        .then_some(associated)
                });
            surface.filter(|surface| index.surfaces.contains_key(surface))
        })
        .collect()
}

fn surface_curve_associated_geometry(record: &RawRecord) -> Option<Vec<u64>> {
    let values = if record.partials.len() == 1 {
        record.parameter(2)?.list()?
    } else {
        record
            .partial("SURFACE_CURVE")
            .or_else(|| record.partial("SEAM_CURVE"))
            .or_else(|| record.partial("INTERSECTION_CURVE"))
            .and_then(|partial| partial.parameters.iter().find_map(Value::list))?
    };
    Some(values.iter().filter_map(Value::reference).collect())
}

fn resolve_unit_scales(
    exchange: &Exchange,
    default_length: f64,
    default_angle: f64,
    losses: &mut Vec<LossNote>,
) -> UnitScales {
    let mut length_candidates = BTreeMap::<u64, Vec<f64>>::new();
    let mut angle_candidates = BTreeMap::<u64, Vec<f64>>::new();
    for (&representation_id, representation) in &exchange.records {
        if !is_representation_record(representation) {
            continue;
        }
        let Some(context_id) = representation_context(representation) else {
            continue;
        };
        let (length, angle) = context_unit_scales(context_id, exchange);
        if length.is_none() && angle.is_none() {
            continue;
        }
        if let Some(length) = length {
            length_candidates
                .entry(representation_id)
                .or_default()
                .push(length);
        }
        if let Some(angle) = angle {
            angle_candidates
                .entry(representation_id)
                .or_default()
                .push(angle);
        }
        let Some(items) = representation_items(representation) else {
            continue;
        };
        let mut members = BTreeSet::new();
        for item in items {
            collect_unit_scope_members(item, exchange, &mut members, &mut BTreeSet::new());
        }
        for member in members {
            if let Some(length) = length {
                length_candidates.entry(member).or_default().push(length);
            }
            if let Some(angle) = angle {
                angle_candidates.entry(member).or_default().push(angle);
            }
        }
    }
    let length = finalize_unit_candidates(length_candidates, "length", losses);
    let angle = finalize_unit_candidates(angle_candidates, "plane-angle", losses);
    UnitScales {
        length: length
            .into_iter()
            .filter(|(_, scale)| scale.is_finite() && *scale > 0.0 && *scale != default_length)
            .collect(),
        angle: angle
            .into_iter()
            .filter(|(_, scale)| scale.is_finite() && *scale > 0.0 && *scale != default_angle)
            .collect(),
    }
}

fn finalize_unit_candidates(
    candidates: BTreeMap<u64, Vec<f64>>,
    dimension: &str,
    losses: &mut Vec<LossNote>,
) -> BTreeMap<u64, f64> {
    let mut selected = BTreeMap::new();
    let mut ambiguous = 0;
    for (id, values) in candidates {
        match unique_scale(&values) {
            Some(scale) => {
                selected.insert(id, scale);
            }
            None => ambiguous += 1,
        }
    }
    if ambiguous > 0 {
        losses.push(StepLossCode::ConflictingRepresentationUnits.note(format!(
                "{ambiguous} geometry record(s) belong to representations with conflicting {dimension} units; source-order unit selection was not applied"
            )));
    }
    selected
}

fn unique_scale(values: &[f64]) -> Option<f64> {
    let first = *values.first()?;
    values
        .iter()
        .all(|value| same_scale(*value, first))
        .then_some(first)
}

fn same_scale(left: f64, right: f64) -> bool {
    let tolerance = 1.0e-12 * left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= tolerance
}

fn is_representation_record(record: &RawRecord) -> bool {
    record
        .partials
        .iter()
        .any(|partial| super::representation::is_representation_name(&partial.name))
}

fn representation_context(record: &RawRecord) -> Option<u64> {
    record
        .partials
        .iter()
        .filter(|partial| super::representation::is_representation_name(&partial.name))
        .flat_map(|partial| partial.parameters.iter().rev())
        .find_map(Value::reference)
}

fn context_unit_scales(id: u64, exchange: &Exchange) -> (Option<f64>, Option<f64>) {
    let Some(context) = exchange.records.get(&id) else {
        return (None, None);
    };
    let Some(units) = context
        .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return (None, None);
    };
    let length_values = units
        .iter()
        .filter_map(Value::reference)
        .filter_map(|unit| unit_scale_mm(unit, exchange, &mut BTreeSet::new()))
        .collect::<Vec<_>>();
    let angle_values = units
        .iter()
        .filter_map(Value::reference)
        .filter_map(|unit| unit_scale_radians(unit, exchange, &mut BTreeSet::new()))
        .collect::<Vec<_>>();
    let length = unique_scale(&length_values);
    let angle = unique_scale(&angle_values);
    (length, angle)
}

fn collect_unit_scope_members(
    id: u64,
    exchange: &Exchange,
    members: &mut BTreeSet<u64>,
    active: &mut BTreeSet<u64>,
) {
    if !active.insert(id) {
        return;
    }
    let Some(record) = exchange.records.get(&id) else {
        return;
    };
    if is_unit_record(record) || is_representation_context_record(record) {
        return;
    }
    members.insert(id);
    if record.partial("PCURVE").is_some() {
        return;
    }
    if record.partial("MAPPED_ITEM").is_some() {
        // The mapping source keeps the units of its mapped representation.
        // Only the mapping target is an item in this representation's context.
        if let Some(target) = record
            .partial("MAPPED_ITEM")
            .and_then(|partial| partial.parameters.last())
            .and_then(Value::reference)
        {
            collect_unit_scope_members(target, exchange, members, active);
        }
        return;
    }
    let mut references = Vec::new();
    for parameter in record
        .partials
        .iter()
        .flat_map(|partial| &partial.parameters)
    {
        collect_references(parameter, &mut references);
    }
    for reference in references {
        let Some(referenced) = exchange.records.get(&reference) else {
            continue;
        };
        if is_representation_record(referenced)
            || is_unit_record(referenced)
            || is_representation_context_record(referenced)
        {
            continue;
        }
        collect_unit_scope_members(reference, exchange, members, active);
    }
}

fn is_unit_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "LENGTH_UNIT"
                | "PLANE_ANGLE_UNIT"
                | "SOLID_ANGLE_UNIT"
                | "NAMED_UNIT"
                | "SI_UNIT"
                | "CONVERSION_BASED_UNIT"
        )
    })
}

fn is_representation_context_record(record: &RawRecord) -> bool {
    record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "REPRESENTATION_CONTEXT"
                | "GEOMETRIC_REPRESENTATION_CONTEXT"
                | "GLOBAL_UNIT_ASSIGNED_CONTEXT"
                | "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT"
        )
    })
}

fn length_scale(exchange: &Exchange) -> Option<f64> {
    document_unit_scale(exchange, "LENGTH_UNIT", unit_scale_mm)
}

fn plane_angle_scale(exchange: &Exchange) -> Option<f64> {
    document_unit_scale(exchange, "PLANE_ANGLE_UNIT", unit_scale_radians)
}

fn document_unit_scale(
    exchange: &Exchange,
    dimension_partial: &str,
    resolve: fn(u64, &Exchange, &mut BTreeSet<u64>) -> Option<f64>,
) -> Option<f64> {
    let mut context_scales = Vec::new();
    let mut has_context_unit = false;

    for record in exchange.records.values() {
        let Some(units) = record
            .partial("GLOBAL_UNIT_ASSIGNED_CONTEXT")
            .and_then(|partial| partial.parameters.first())
            .and_then(Value::list)
        else {
            continue;
        };
        let unit_ids = units
            .iter()
            .filter_map(Value::reference)
            .filter(|id| {
                exchange
                    .records
                    .get(id)
                    .is_some_and(|unit| unit.partial(dimension_partial).is_some())
            })
            .collect::<Vec<_>>();
        if unit_ids.is_empty() {
            continue;
        }
        has_context_unit = true;
        let scales = unit_ids
            .into_iter()
            .map(|id| resolve(id, exchange, &mut BTreeSet::new()))
            .collect::<Option<Vec<_>>>()?;
        context_scales.push(unique_scale(&scales)?);
    }

    if has_context_unit {
        return unique_scale(&context_scales);
    }

    // STEP assigns units to representation contexts, not to the document.
    // This branch is CADIR salvage for an unscoped dimension: accept only a
    // scale to which every unit occurrence in the exchange resolves.
    let scales = exchange
        .records
        .iter()
        .filter(|(_, record)| record.partial(dimension_partial).is_some())
        .map(|(&id, _)| resolve(id, exchange, &mut BTreeSet::new()))
        .collect::<Option<Vec<_>>>()?;
    unique_scale(&scales)
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
    let result = (|| {
        let record = exchange.records.get(&id)?;
        if let Some(unit) = record.partial("SI_UNIT") {
            if unit.parameters.get(1)?.enumeration()? == "RADIAN" {
                let prefix = match unit.parameters.first()? {
                    Value::Omitted => 1.0,
                    Value::Enumeration(prefix) => si_prefix(prefix)?,
                    _ => return None,
                };
                Some(prefix)
            } else {
                None
            }
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
        }
    })();
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
    let result = (|| {
        let record = exchange.records.get(&id)?;
        if let Some(unit) = record.partial("SI_UNIT") {
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
        }
    })();
    active.remove(&id);
    result.filter(|scale| scale.is_finite() && *scale > 0.0)
}

const SI_MICRO: f64 = 1.0e-6;
const SI_NANO: f64 = 1.0e-9;
const SI_PICO: f64 = 1.0e-12;

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
        "MICRO" => SI_MICRO,
        "NANO" => SI_NANO,
        "PICO" => SI_PICO,
        "FEMTO" => 1e-15,
        "ATTO" => 1e-18,
        _ => return None,
    })
}

/// Resolve one `GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT` to the linear uncertainty
/// candidates it contributes, in millimetres, and the number of its measures
/// that did not resolve.
///
/// STEP scopes an uncertainty to its representation context, so each context
/// contributes for itself. One `distance_accuracy_value` name makes that value
/// the only contribution of the context. Every other context contributes each
/// of its resolvable length measures. `linear_uncertainty` merges the equal
/// contributions of all contexts and decides what a disagreement means.
fn context_length_uncertainties(context: &RawRecord, exchange: &Exchange) -> (Vec<f64>, usize) {
    let Some(references) = context
        .partial("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT")
        .and_then(|partial| partial.parameters.first())
        .and_then(Value::list)
    else {
        return (Vec::new(), 0);
    };
    let mut measures = Vec::new();
    let mut unresolved = 0;
    for uncertainty_id in references.iter().filter_map(Value::reference) {
        let Some(measure) = exchange.records.get(&uncertainty_id) else {
            unresolved += 1;
            continue;
        };
        let Some(value) = record_values(measure).find_map(measure_number) else {
            unresolved += 1;
            continue;
        };
        let Some(unit) = record_values(measure).find_map(Value::reference) else {
            unresolved += 1;
            continue;
        };
        if let Some(scale) = unit_scale_mm(unit, exchange, &mut BTreeSet::new()) {
            let result = value * scale;
            if !result.is_finite() || result <= 0.0 {
                unresolved += 1;
                continue;
            }
            // The CADIR convention applies to the name attribute, not
            // the optional description attribute.
            let named_distance_accuracy = measure
                .partial("UNCERTAINTY_MEASURE_WITH_UNIT")
                .and_then(|partial| partial.parameters.get(2))
                .and_then(string_value)
                .is_some_and(|name| name.eq_ignore_ascii_case("distance_accuracy_value"));
            measures.push((named_distance_accuracy, result));
        } else if unit_scale_radians(unit, exchange, &mut BTreeSet::new()).is_none() {
            unresolved += 1;
        }
    }

    let named = measures
        .iter()
        .filter(|(named, _)| *named)
        .map(|(_, value)| *value)
        .collect::<Vec<_>>();
    if named.len() == 1 {
        return (named, unresolved);
    }
    (
        measures.into_iter().map(|(_, value)| value).collect(),
        unresolved,
    )
}

/// The document projection of the per-context linear uncertainty candidates.
enum LinearUncertainty {
    /// One distinct candidate, in millimetres.
    Value(f64),
    /// No candidate, with the number of measures that did not resolve.
    Empty { unresolved: usize },
    /// Several distinct candidates in millimetres, sorted and without
    /// duplicates, with the number of measures that did not resolve.
    Ambiguous { values: Vec<f64>, unresolved: usize },
}

fn linear_uncertainty(exchange: &Exchange) -> LinearUncertainty {
    let mut candidates: Vec<f64> = Vec::new();
    let mut unresolved = 0;
    for (_, context) in exchange.entities("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT") {
        let (context_candidates, context_unresolved) =
            context_length_uncertainties(context, exchange);
        unresolved += context_unresolved;
        for candidate in context_candidates {
            // Exact equality: the candidates come from one file, so equal
            // declarations corroborate each other and are not a conflict.
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates.sort_by(f64::total_cmp);

    if candidates.len() > 1 {
        return LinearUncertainty::Ambiguous {
            values: candidates,
            unresolved,
        };
    }
    match candidates.first() {
        Some(value) => LinearUncertainty::Value(*value),
        None => LinearUncertainty::Empty { unresolved },
    }
}

fn string_value(value: &Value) -> Option<String> {
    let Value::String(bytes) = value else {
        return None;
    };
    crate::strings::decode(bytes).ok()
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

fn trimmed_curve_parameter_range(
    geometry: &CurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> [f64; 2] {
    let mut start = start;
    let mut end = end;
    // A closed STEP curve may cross its parameter seam. Move the endpoint
    // that follows the declared traversal onto the next parameter branch
    // before projecting the directed trim onto the IR's ordered interval.
    if let Some(period) = curve_parameter_period(geometry) {
        if sense && end < start {
            end += period;
        } else if !sense && start < end {
            start += period;
        }
    }
    let range = if sense { [start, end] } else { [end, start] };
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}

fn curve_parameter_period(geometry: &CurveGeometry) -> Option<f64> {
    let period = match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => std::f64::consts::TAU,
        CurveGeometry::Nurbs(curve) if curve.periodic => {
            let [lower, upper] = nurbs_curve_parameter_domain(curve)?;
            upper - lower
        }
        _ => return None,
    };
    (period.is_finite() && period > 0.0).then_some(period)
}

fn is_parameter_trim_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Real(_) => true,
        Value::Typed(name, _) => name == "PARAMETER_VALUE",
        _ => false,
    }
}

fn trim_parameter_value(value: &Value, context: &TrimParameterContext<'_>) -> Option<f64> {
    let scale = parameter_scale(
        context.geometry,
        context.angle_scale,
        context.linear_parameter_scale,
    );
    match value {
        Value::Integer(value) => Some(scale * *value as f64 + context.parameter_offset),
        Value::Real(value) => Some(scale * *value + context.parameter_offset),
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
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => angle_scale,
        CurveGeometry::Line { .. } => linear_parameter_scale,
        // A replica and the constructions that inherit a parent curve's
        // parameterization keep the parent's parameter units even when their
        // model-space dimensions change.
        CurveGeometry::Transformed { basis, .. } => {
            parameter_scale(basis, angle_scale, linear_parameter_scale)
        }
        CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Nurbs(_)
        | CurveGeometry::Polyline { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => 1.0,
    }
}

fn line_parameter_scale(
    exchange: &Exchange,
    curve: u64,
    length_scale: f64,
    losses: &mut Vec<LossNote>,
) -> f64 {
    fn inherited_parent(record: &RawRecord) -> Option<u64> {
        if record.partial("CURVE_REPLICA").is_some() {
            return named_parameter(record, "CURVE_REPLICA", 1).and_then(ValueExt::reference);
        }
        if record.partial("TRIMMED_CURVE").is_some() {
            return named_parameter(record, "TRIMMED_CURVE", 1).and_then(ValueExt::reference);
        }
        if record.partial("OFFSET_CURVE_3D").is_some() {
            return named_parameter(record, "OFFSET_CURVE_3D", 1).and_then(ValueExt::reference);
        }
        if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE"
            )
        }) {
            return surface_curve_basis(record);
        }
        None
    }

    fn resolve(
        exchange: &Exchange,
        curve: u64,
        length_scale: f64,
        losses: &mut Vec<LossNote>,
        visiting: &mut BTreeSet<u64>,
    ) -> f64 {
        if !visiting.insert(curve) {
            return length_scale;
        }
        let Some(record) = exchange.records.get(&curve) else {
            visiting.remove(&curve);
            return length_scale;
        };
        let result = if record.partial("LINE").is_some() {
            named_parameter(record, "LINE", 2)
                .and_then(ValueExt::reference)
                .and_then(|vector| exchange.records.get(&vector))
                .filter(|record| record.partial("VECTOR").is_some())
                .and_then(|record| named_parameter(record, "VECTOR", 2))
                .and_then(ValueExt::number)
                .map(|magnitude| magnitude * length_scale)
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .unwrap_or_else(|| {
                    losses.push(StepLossCode::LineParameterScaleUnresolved.note(format!(
                        "LINE #{curve} parameter scale did not resolve; the document length scale was used"
                    )));
                    length_scale
                })
        } else if let Some(parent) = inherited_parent(record) {
            resolve(exchange, parent, length_scale, losses, visiting)
        } else {
            length_scale
        };
        visiting.remove(&curve);
        result
    }

    resolve(exchange, curve, length_scale, losses, &mut BTreeSet::new())
}

fn orthogonal_reference(axis: Vector3, reference: Vector3) -> Option<Vector3> {
    let projection = axis.dot(reference);
    normalize(Vector3::new(
        reference.x - projection * axis.x,
        reference.y - projection * axis.y,
        reference.z - projection * axis.z,
    ))
}

fn first_projected_axis(axis: Vector3) -> Option<Vector3> {
    let axis = normalize(axis)?;
    project_axis(default_reference_axis(axis), axis)
}

fn curve_parameter_at_point(
    geometry: &CurveGeometry,
    point: Point3,
    tolerance: f64,
) -> Option<f64> {
    let offset =
        |origin: Point3| Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(offset(*origin).dot(*direction)),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            ..
        } => {
            let radial = offset(*center);
            let y_axis = axis.cross(*ref_direction);
            Some(radial.dot(y_axis).atan2(radial.dot(*ref_direction)))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let radial = offset(*center);
            let minor_direction = axis.cross(*major_direction);
            Some(
                (radial.dot(minor_direction) / minor_radius)
                    .atan2(radial.dot(*major_direction) / major_radius),
            )
        }
        CurveGeometry::Nurbs(curve) => {
            let domain = nurbs_curve_parameter_domain(curve)?;
            nurbs_curve_parameter_near_point(curve, point, tolerance, (domain[0] + domain[1]) * 0.5)
        }
        CurveGeometry::Transformed { basis, transform } => curve_parameter_at_point(
            basis,
            transform.try_inverse_affine()?.apply_point(point),
            tolerance,
        ),
        _ => None,
    }
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

fn composite_curve_dependencies(record: &RawRecord, exchange: &Exchange) -> Vec<u64> {
    let Some((parameters, offset)) = composite_curve_parameters(record) else {
        return Vec::new();
    };
    parameters
        .get(offset)
        .and_then(Value::list)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
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
    let (parameters, offset) = composite_curve_parameters(record)?;
    let segments = parameters
        .get(offset)?
        .list()?
        .iter()
        .map(|value| {
            let id = value.reference()?;
            let segment = exchange.records.get(&id)?;
            let parameters = composite_curve_segment_parameters(segment)?;
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
            let curve_step = curve_carrier_record(curve_step, exchange)?;
            decoded.curves.contains_key(&curve_step).then_some((
                id,
                CompositeCurveSegment {
                    curve: CurveId(StepIdentity::data("curve", curve_step)),
                    same_sense: parameters.get(1)?.logical()?,
                    transition,
                },
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    (!segments.is_empty()).then_some((
        segments,
        parameters
            .get(offset + 1)
            .and_then(logical_value)?
            .into_option(),
    ))
}

fn composite_curve_parameters(record: &RawRecord) -> Option<(&[Value], usize)> {
    ["COMPOSITE_CURVE", "BOUNDARY_CURVE", "OUTER_BOUNDARY_CURVE"]
        .into_iter()
        .find_map(|name| {
            record.partial(name).map(|partial| {
                (
                    partial.parameters.as_slice(),
                    usize::from(record.partials.len() == 1),
                )
            })
        })
}

fn composite_curve_segment_parameters(record: &RawRecord) -> Option<&[Value]> {
    record
        .partial("COMPOSITE_CURVE_SEGMENT")
        .map(|partial| partial.parameters.as_slice())
}

fn boundary_pcurve_steps(boundary: u64, support: u64, exchange: &Exchange) -> Vec<u64> {
    let Some(record) = exchange.records.get(&boundary) else {
        return Vec::new();
    };
    let Some((parameters, offset)) = composite_curve_parameters(record) else {
        return Vec::new();
    };
    parameters
        .get(offset)
        .and_then(Value::list)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
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

pub(super) fn coordinate_rows(record: &RawRecord, scale: f64) -> Option<Vec<Point3>> {
    record
        .partials
        .iter()
        .flat_map(|partial| partial.parameters.iter())
        .filter_map(ValueExt::list)
        .find_map(|rows| {
            rows.iter()
                .map(|row| {
                    let values = row.list()?;
                    if values.len() != 3 {
                        return None;
                    }
                    let point = Point3::new(
                        values[0].number()? * scale,
                        values[1].number()? * scale,
                        values[2].number()? * scale,
                    );
                    [point.x, point.y, point.z]
                        .iter()
                        .all(|coordinate| coordinate.is_finite())
                        .then_some(point)
                })
                .collect::<Option<Vec<_>>>()
                .filter(|vertices| !vertices.is_empty())
        })
}

fn named_coordinates(record: &RawRecord, name: &str, index: usize, scale: f64) -> Option<Point3> {
    let values = named_parameter(record, name, index)?.list()?;
    if values.len() != 3 {
        return None;
    }
    Some(Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    ))
}

fn apll_point_coordinates(record: &RawRecord, point_type: &str, scale: f64) -> Option<Point3> {
    let values = if record.partials.len() == 1 {
        named_parameter(record, point_type, 1).and_then(Value::list)
    } else {
        [
            ("CARTESIAN_POINT", 0),
            ("CARTESIAN_POINT", 1),
            (point_type, 0),
            (point_type, 1),
        ]
        .into_iter()
        .find_map(|(name, index)| named_parameter(record, name, index).and_then(Value::list))
    }?;
    if values.len() != 3 {
        return None;
    }
    let point = Point3::new(
        values[0].number()? * scale,
        values[1].number()? * scale,
        values[2].number()? * scale,
    );
    [point.x, point.y, point.z]
        .iter()
        .all(|coordinate| coordinate.is_finite())
        .then_some(point)
}

fn named_coordinates2(record: &RawRecord, name: &str, index: usize) -> Option<Point2> {
    let values = named_parameter(record, name, index)?.list()?;
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

#[derive(Clone, Copy)]
enum DefaultNurbsKnotKind {
    Uniform,
    QuasiUniform,
    Bezier,
}

struct NurbsCurveDefinition {
    degree: u32,
    control_points: Vec<u64>,
    knots: Vec<f64>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

fn nurbs_curve_definition(
    record: &RawRecord,
    warnings: &mut Vec<String>,
    periodicity_field: &str,
) -> Option<NurbsCurveDefinition> {
    let (base, offset) = if record.partials.len() > 1 {
        (record.partial("B_SPLINE_CURVE")?, 0)
    } else {
        let base_name = [
            "B_SPLINE_CURVE_WITH_KNOTS",
            "UNIFORM_CURVE",
            "QUASI_UNIFORM_CURVE",
            "BEZIER_CURVE",
        ]
        .into_iter()
        .find(|name| record.partial(name).is_some())?;
        (record.partial(base_name)?, 1)
    };
    let degree = u32::try_from(base.parameters.get(offset)?.integer()?).ok()?;
    let control_points = references(base.parameters.get(offset + 1)?)?;
    if usize::try_from(degree).ok()? >= control_points.len() {
        return None;
    }
    let periodic = periodic_value(
        base.parameters.get(offset + 3),
        periodicity_field,
        record.id,
        warnings,
    )?;
    let expected_knots = control_points.len().checked_add(degree as usize + 1)?;
    let knots = if let Some(knot_leaf) = record.partial("B_SPLINE_CURVE_WITH_KNOTS") {
        let tail = knot_leaf.parameters.len().checked_sub(3)?;
        expand_knots(
            knot_leaf.parameters.get(tail)?,
            knot_leaf.parameters.get(tail + 1)?,
            expected_knots,
        )?
    } else {
        let kind = if record.partial("UNIFORM_CURVE").is_some() {
            DefaultNurbsKnotKind::Uniform
        } else if record.partial("QUASI_UNIFORM_CURVE").is_some() {
            DefaultNurbsKnotKind::QuasiUniform
        } else if record.partial("BEZIER_CURVE").is_some() {
            DefaultNurbsKnotKind::Bezier
        } else {
            return None;
        };
        default_nurbs_knots(control_points.len(), degree, kind)?
    };
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
    Some(NurbsCurveDefinition {
        degree,
        control_points,
        knots,
        weights,
        periodic,
    })
}

fn default_nurbs_knots(
    control_point_count: usize,
    degree: u32,
    kind: DefaultNurbsKnotKind,
) -> Option<Vec<f64>> {
    let degree = usize::try_from(degree).ok()?;
    let expected = control_point_count.checked_add(degree)?.checked_add(1)?;
    let mut knots = Vec::new();
    knots.try_reserve_exact(expected).ok()?;
    match kind {
        DefaultNurbsKnotKind::Uniform => {
            knots.extend((0..expected).map(|index| index as f64 - degree as f64));
        }
        DefaultNurbsKnotKind::QuasiUniform => {
            let distinct_count = control_point_count.checked_sub(degree)?.checked_add(1)?;
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    degree.checked_add(1)?
                } else {
                    1
                };
                knots.extend(std::iter::repeat_n(index as f64, multiplicity));
            }
        }
        DefaultNurbsKnotKind::Bezier => {
            if degree == 0 {
                return None;
            }
            let segment_count = control_point_count.checked_sub(1)?;
            if segment_count % degree != 0 {
                return None;
            }
            let segment_count = segment_count / degree;
            let distinct_count = segment_count.checked_add(1)?;
            for index in 0..distinct_count {
                let multiplicity = if index == 0 || index + 1 == distinct_count {
                    degree.checked_add(1)?
                } else {
                    degree
                };
                knots.extend(std::iter::repeat_n(index as f64, multiplicity));
            }
        }
    }
    (knots.len() == expected).then_some(knots)
}

fn nurbs_curve(
    record: &RawRecord,
    points: &BTreeMap<u64, Point3>,
    warnings: &mut Vec<String>,
) -> Option<NurbsCurve> {
    let definition = nurbs_curve_definition(record, warnings, "B_SPLINE_CURVE")?;
    let control_points = definition
        .control_points
        .into_iter()
        .map(|id| points.get(&id).copied())
        .collect::<Option<Vec<_>>>()?;
    Some(NurbsCurve {
        degree: definition.degree,
        knots: definition.knots,
        control_points,
        weights: definition.weights,
        periodic: definition.periodic,
    })
}

fn nurbs_pcurve(
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    warnings: &mut Vec<String>,
) -> Option<PcurveGeometry> {
    let definition = nurbs_curve_definition(record, warnings, "B_SPLINE_CURVE pcurve")?;
    let control_points = definition
        .control_points
        .into_iter()
        .map(|id| points.get(&id).copied())
        .collect::<Option<Vec<_>>>()?;
    Some(PcurveGeometry::Nurbs {
        degree: definition.degree,
        knots: definition.knots,
        control_points,
        weights: definition.weights,
        periodic: definition.periodic,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_pcurve_geometry(
    id: u64,
    exchange: &Exchange,
    points: &BTreeMap<u64, Point2>,
    vectors: &BTreeMap<u64, Point2>,
    placements: &BTreeMap<u64, (Point2, Point2, Point2)>,
    transformations: &BTreeMap<u64, Transform2>,
    angle_scale: f64,
    warnings: &mut Vec<String>,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<(PcurveGeometry, BTreeSet<u64>)> {
    if depth >= 256 || !active.insert(id) {
        return None;
    }
    let result = (|| {
        let record = exchange.records.get(&id)?;
        let mut records = BTreeSet::from([id]);
        let geometry = if record.partials.iter().any(|partial| {
            matches!(
                partial.name.as_str(),
                "B_SPLINE_CURVE_WITH_KNOTS"
                    | "UNIFORM_CURVE"
                    | "QUASI_UNIFORM_CURVE"
                    | "BEZIER_CURVE"
            )
        }) {
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
                    "CURVE_REPLICA",
                    "TRIMMED_CURVE",
                    "OFFSET_CURVE_2D",
                    "UNIFORM_CURVE",
                    "QUASI_UNIFORM_CURVE",
                    "BEZIER_CURVE",
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
                "CURVE_REPLICA" => {
                    let basis_id = named_parameter(record, "CURVE_REPLICA", 1)?.reference()?;
                    let operator_id = named_parameter(record, "CURVE_REPLICA", 2)?.reference()?;
                    let (basis, basis_records) = decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        points,
                        vectors,
                        placements,
                        transformations,
                        angle_scale,
                        warnings,
                        active,
                        depth + 1,
                    )?;
                    let transform = transformations.get(&operator_id).copied()?;
                    records.extend(basis_records);
                    records.insert(operator_id);
                    PcurveGeometry::Transformed {
                        basis: Box::new(basis),
                        transform,
                    }
                }
                "TRIMMED_CURVE" => {
                    let basis_id = named_parameter(record, "TRIMMED_CURVE", 1)?.reference()?;
                    let sense = named_parameter(record, "TRIMMED_CURVE", 4)?.logical()?;
                    let (basis, basis_records) = decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        points,
                        vectors,
                        placements,
                        transformations,
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
                        pcurve_trim_parameter(named_parameter(record, "TRIMMED_CURVE", 2)?)?
                            * scale;
                    let end = pcurve_trim_parameter(named_parameter(record, "TRIMMED_CURVE", 3)?)?
                        * scale;
                    records.extend(basis_records);
                    let (parameter_range, same_sense) =
                        trimmed_pcurve_parameterization(&basis, start, end, sense);
                    PcurveGeometry::Trimmed {
                        parameter_range,
                        same_sense,
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
                        return None;
                    }
                    let (basis, basis_records) = decode_pcurve_geometry(
                        basis_id,
                        exchange,
                        points,
                        vectors,
                        placements,
                        transformations,
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
                    return None;
                }
            }
        };
        Some((geometry, records))
    })();
    active.remove(&id);
    result
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

fn trimmed_pcurve_parameterization(
    geometry: &PcurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> ([f64; 2], bool) {
    let mut start = start;
    let mut end = end;
    // Closed STEP pcurves use cyclic parameter branches. Move the endpoint
    // that follows the declared traversal before projecting to an ordered
    // basis interval; non-closed malformed input still gets a valid interval.
    if let Some(period) = pcurve_parameter_period(geometry) {
        if sense && end < start {
            end += period;
        } else if !sense && start < end {
            start += period;
        }
    }
    let [from, to] = if sense { [start, end] } else { [end, start] };
    if from <= to {
        ([from, to], true)
    } else {
        ([to, from], false)
    }
}

fn pcurve_parameter_period(geometry: &PcurveGeometry) -> Option<f64> {
    let period = match geometry {
        PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. } => std::f64::consts::TAU,
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            periodic: true,
            ..
        } => pcurve_nurbs_parameter_period(*degree, knots, control_points.len())?,
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            periodic: true,
            ..
        } => pcurve_nurbs_parameter_period(*degree, knots, radial_control_points.len())?,
        PcurveGeometry::Offset { basis, .. } => pcurve_parameter_period(basis)?,
        PcurveGeometry::Transformed { basis, .. } => pcurve_parameter_period(basis)?,
        _ => return None,
    };
    (period.is_finite() && period > 0.0).then_some(period)
}

fn pcurve_nurbs_parameter_period(degree: u32, knots: &[f64], count: usize) -> Option<f64> {
    let degree = usize::try_from(degree).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    (lower.is_finite() && upper.is_finite() && upper > lower).then_some(upper - lower)
}

fn surface_parameter_scales_for_step(
    ir: &CadIr,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, f64>,
) -> Option<[f64; 2]> {
    procedural_surface_parameter_scales(
        ir,
        surface_id,
        geometry,
        length_scale,
        angle_scale,
        source_curve_parameter_scales,
        &mut BTreeSet::new(),
    )
}

fn procedural_surface_parameter_scales(
    ir: &CadIr,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, f64>,
    active: &mut BTreeSet<SurfaceId>,
) -> Option<[f64; 2]> {
    if !active.insert(surface_id.clone()) {
        return None;
    }
    let scales = surface_geometry_parameter_scales(
        ir,
        surface_id,
        geometry,
        length_scale,
        angle_scale,
        source_curve_parameter_scales,
        active,
    );
    active.remove(surface_id);
    scales
}

fn surface_geometry_parameter_scales(
    ir: &CadIr,
    surface_id: &SurfaceId,
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, f64>,
    active: &mut BTreeSet<SurfaceId>,
) -> Option<[f64; 2]> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some([length_scale, length_scale]),
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            Some([angle_scale, length_scale])
        }
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            Some([angle_scale, angle_scale])
        }
        SurfaceGeometry::Nurbs(_) => Some([1.0, 1.0]),
        SurfaceGeometry::Transformed { basis, .. } => surface_geometry_parameter_scales(
            ir,
            surface_id,
            basis,
            length_scale,
            angle_scale,
            source_curve_parameter_scales,
            active,
        ),
        SurfaceGeometry::Procedural { construction } => ir
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| procedural.id == *construction)
            .and_then(|procedural| {
                procedural_definition_parameter_scales(
                    ir,
                    &procedural.definition,
                    length_scale,
                    angle_scale,
                    source_curve_parameter_scales,
                    active,
                )
            }),
        SurfaceGeometry::Unknown { .. } => {
            let mut candidates = ir
                .model
                .procedural_surfaces
                .iter()
                .filter(|procedural| procedural.surface == *surface_id);
            let procedural = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            procedural_definition_parameter_scales(
                ir,
                &procedural.definition,
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
                active,
            )
        }
        SurfaceGeometry::Polygonal { .. } => None,
    }
}

fn procedural_definition_parameter_scales(
    ir: &CadIr,
    definition: &ProceduralSurfaceDefinition,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, f64>,
    active: &mut BTreeSet<SurfaceId>,
) -> Option<[f64; 2]> {
    let support_scales = |support: &SurfaceId, active: &mut BTreeSet<SurfaceId>| {
        let carrier = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *support)?;
        procedural_surface_parameter_scales(
            ir,
            support,
            &carrier.geometry,
            length_scale,
            angle_scale,
            source_curve_parameter_scales,
            active,
        )
    };
    match definition {
        ProceduralSurfaceDefinition::Extrusion { directrix, .. }
        | ProceduralSurfaceDefinition::LinearSweep { directrix, .. } => Some([
            directrix_parameter_scale(
                ir,
                directrix,
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
            )?,
            1.0,
        ]),
        ProceduralSurfaceDefinition::AxisRevolution { directrix, .. } => Some([
            angle_scale,
            directrix_parameter_scale(
                ir,
                directrix,
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
            )?,
        ]),
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            transposed,
            ..
        } => {
            let directrix = directrix_parameter_scale(
                ir,
                directrix,
                length_scale,
                angle_scale,
                source_curve_parameter_scales,
            )?;
            Some(if *transposed {
                [angle_scale, directrix]
            } else {
                [directrix, angle_scale]
            })
        }
        ProceduralSurfaceDefinition::Offset { support, .. }
        | ProceduralSurfaceDefinition::ParallelOffset { support, .. }
        | ProceduralSurfaceDefinition::Subset { support, .. }
        | ProceduralSurfaceDefinition::SubSurface { support, .. }
        | ProceduralSurfaceDefinition::CurveBounded { support, .. }
        | ProceduralSurfaceDefinition::Replica {
            source: support, ..
        } => support_scales(support, active),
        ProceduralSurfaceDefinition::DegenerateTorus { .. } => Some([angle_scale, angle_scale]),
        _ => None,
    }
}

fn directrix_parameter_scale(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    source_curve_parameter_scales: &BTreeMap<u64, f64>,
) -> Option<f64> {
    if let Some(source_scale) =
        step_instance_id(&curve_id.0).and_then(|id| source_curve_parameter_scales.get(&id))
    {
        return Some(*source_scale);
    }
    directrix_parameter_scale_inner(
        ir,
        curve_id,
        length_scale,
        angle_scale,
        &mut BTreeSet::new(),
    )
}

fn directrix_parameter_scale_inner(
    ir: &CadIr,
    curve_id: &CurveId,
    length_scale: f64,
    angle_scale: f64,
    active: &mut BTreeSet<CurveId>,
) -> Option<f64> {
    if !active.insert(curve_id.clone()) {
        return None;
    }
    let scale = ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .and_then(|curve| {
            directrix_geometry_parameter_scale(
                ir,
                &curve.geometry,
                length_scale,
                angle_scale,
                active,
            )
        });
    active.remove(curve_id);
    scale
}

fn directrix_geometry_parameter_scale(
    ir: &CadIr,
    geometry: &CurveGeometry,
    length_scale: f64,
    angle_scale: f64,
    active: &mut BTreeSet<CurveId>,
) -> Option<f64> {
    match geometry {
        CurveGeometry::Line { .. } => Some(length_scale),
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => Some(angle_scale),
        CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Nurbs(_)
        | CurveGeometry::Polyline { .. } => Some(1.0),
        CurveGeometry::Transformed { basis, .. } => {
            directrix_geometry_parameter_scale(ir, basis, length_scale, angle_scale, active)
        }
        CurveGeometry::Procedural { construction } => ir
            .model
            .procedural_curves
            .iter()
            .find(|procedural| procedural.id == *construction)
            .and_then(|procedural| match &procedural.definition {
                ProceduralCurveDefinition::Offset { source, .. }
                | ProceduralCurveDefinition::SpatialOffset { source, .. }
                | ProceduralCurveDefinition::Subset { source, .. }
                | ProceduralCurveDefinition::VectorOffset { source, .. }
                | ProceduralCurveDefinition::Projection { source, .. }
                | ProceduralCurveDefinition::Replica { source, .. } => {
                    directrix_parameter_scale_inner(ir, source, length_scale, angle_scale, active)
                }
                ProceduralCurveDefinition::Deformable {
                    source: cadmpeg_ir::geometry::DeformableCurveSource::Curve { curve },
                    ..
                } => directrix_parameter_scale_inner(ir, curve, length_scale, angle_scale, active),
                _ => None,
            }),
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

pub(super) fn surface_parameter_periods(geometry: &SurfaceGeometry) -> [Option<f64>; 2] {
    match geometry {
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            [Some(std::f64::consts::TAU), None]
        }
        SurfaceGeometry::Sphere { .. } => [Some(std::f64::consts::TAU), None],
        SurfaceGeometry::Torus { .. } => [Some(std::f64::consts::TAU), Some(std::f64::consts::TAU)],
        SurfaceGeometry::Nurbs(surface) => [
            surface
                .u_periodic
                .then(|| {
                    nurbs_surface_parameter_period(
                        surface.u_degree,
                        &surface.u_knots,
                        surface.u_count,
                    )
                })
                .flatten(),
            surface
                .v_periodic
                .then(|| {
                    nurbs_surface_parameter_period(
                        surface.v_degree,
                        &surface.v_knots,
                        surface.v_count,
                    )
                })
                .flatten(),
        ],
        SurfaceGeometry::Transformed { basis, .. } => surface_parameter_periods(basis),
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => [None, None],
    }
}

fn nurbs_surface_parameter_period(degree: u32, knots: &[f64], count: u32) -> Option<f64> {
    let degree = usize::try_from(degree).ok()?;
    let count = usize::try_from(count).ok()?;
    let lower = *knots.get(degree)?;
    let upper = *knots.get(count)?;
    let period = upper - lower;
    (period.is_finite() && period > 0.0).then_some(period)
}

/// Scale a pcurve's coordinates into the units of its owning surface.
///
/// The pcurve parameter itself is unchanged. Circle, ellipse, and hyperbola
/// carriers keep their native trigonometric parameterization by using the
/// general harmonic forms when the two coordinate scales differ. The remaining
/// analytic forms require an affine 2D carrier to preserve that parameterization;
/// report them as unsupported instead of applying a scalar approximation.
pub(super) fn scale_pcurve_geometry(geometry: &mut PcurveGeometry, scales: [f64; 2]) -> bool {
    let [u_scale, v_scale] = scales;
    let scale_point = |point: Point2| Point2::new(point.u * u_scale, point.v * v_scale);
    let isotropic = u_scale == v_scale;

    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            *origin = scale_point(*origin);
            *direction = scale_point(*direction);
        }
        PcurveGeometry::Circle {
            center: center_slot,
            x_axis,
            y_axis,
            radius,
        } => {
            let center = scale_point(*center_slot);
            if isotropic {
                *center_slot = center;
                *radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Harmonic {
                    center,
                    cosine: scale_point(Point2::new(*radius * x_axis.u, *radius * x_axis.v)),
                    sine: scale_point(Point2::new(*radius * y_axis.u, *radius * y_axis.v)),
                };
            }
        }
        PcurveGeometry::Ellipse {
            center: center_slot,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let center = scale_point(*center_slot);
            if isotropic {
                *center_slot = center;
                *major_radius *= u_scale;
                *minor_radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Harmonic {
                    center,
                    cosine: scale_point(Point2::new(
                        *major_radius * x_axis.u,
                        *major_radius * x_axis.v,
                    )),
                    sine: scale_point(Point2::new(
                        *minor_radius * y_axis.u,
                        *minor_radius * y_axis.v,
                    )),
                };
            }
        }
        PcurveGeometry::Parabola {
            vertex,
            focal_distance,
            ..
        } => {
            if !isotropic {
                return false;
            }
            *vertex = scale_point(*vertex);
            *focal_distance *= u_scale;
        }
        PcurveGeometry::Hyperbola {
            center: center_slot,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let center = scale_point(*center_slot);
            if isotropic {
                *center_slot = center;
                *major_radius *= u_scale;
                *minor_radius *= u_scale;
            } else {
                *geometry = PcurveGeometry::Hyperbolic {
                    center,
                    cosine: scale_point(Point2::new(
                        *major_radius * x_axis.u,
                        *major_radius * x_axis.v,
                    )),
                    sine: scale_point(Point2::new(
                        *minor_radius * y_axis.u,
                        *minor_radius * y_axis.v,
                    )),
                };
            }
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
            *center = scale_point(*center);
            *cosine = scale_point(*cosine);
            *sine = scale_point(*sine);
        }
        PcurveGeometry::Nurbs { control_points, .. } => {
            for control_point in control_points {
                *control_point = scale_point(*control_point);
            }
        }
        PcurveGeometry::Trimmed { basis, .. } => {
            if !scale_pcurve_geometry(basis, scales) {
                return false;
            }
        }
        PcurveGeometry::Offset { distance, basis } => {
            if !isotropic || !scale_pcurve_geometry(basis, scales) {
                return false;
            }
            *distance *= u_scale;
        }
        PcurveGeometry::Transformed { basis, transform } => {
            if !u_scale.is_finite()
                || !v_scale.is_finite()
                || u_scale == 0.0
                || v_scale == 0.0
                || !transform.is_affine()
            {
                return false;
            }
            // The basis is converted below. Conjugate the replica map so
            // `S * T * x` remains `S * T * S^-1 * (S * x)`.
            let mut scaled_transform = *transform;
            scaled_transform.rows[0][1] *= u_scale / v_scale;
            scaled_transform.rows[0][2] *= u_scale;
            scaled_transform.rows[1][0] *= v_scale / u_scale;
            scaled_transform.rows[1][2] *= v_scale;
            if !scaled_transform.is_affine() || !scale_pcurve_geometry(basis, scales) {
                return false;
            }
            *transform = scaled_transform;
        }
        PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => return isotropic && u_scale == 1.0,
    }
    true
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
    let (base, offset) = if record.partials.len() > 1 {
        (record.partial("B_SPLINE_SURFACE")?, 0)
    } else {
        let base_name = [
            "B_SPLINE_SURFACE_WITH_KNOTS",
            "UNIFORM_SURFACE",
            "QUASI_UNIFORM_SURFACE",
            "BEZIER_SURFACE",
        ]
        .into_iter()
        .find(|name| record.partial(name).is_some())?;
        (record.partial(base_name)?, 1)
    };
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
    let surface_name = [
        "B_SPLINE_SURFACE_WITH_KNOTS",
        "UNIFORM_SURFACE",
        "QUASI_UNIFORM_SURFACE",
        "BEZIER_SURFACE",
    ]
    .into_iter()
    .find(|name| record.partial(name).is_some())
    .unwrap_or("B_SPLINE_SURFACE");
    let u_periodic = periodic_value(
        base.parameters.get(offset + 4),
        &format!("{surface_name} U direction"),
        record.id,
        warnings,
    )?;
    let v_periodic = periodic_value(
        base.parameters.get(offset + 5),
        &format!("{surface_name} V direction"),
        record.id,
        warnings,
    )?;
    let expected_u = usize::try_from(u_count)
        .ok()?
        .checked_add(usize::try_from(u_degree).ok()?)?
        .checked_add(1)?;
    let expected_v = usize::try_from(v_count)
        .ok()?
        .checked_add(usize::try_from(v_degree).ok()?)?
        .checked_add(1)?;
    let (u_knots, v_knots) = if let Some(knot_leaf) = record.partial("B_SPLINE_SURFACE_WITH_KNOTS")
    {
        let tail = knot_leaf.parameters.len().checked_sub(5)?;
        (
            expand_knots(
                knot_leaf.parameters.get(tail)?,
                knot_leaf.parameters.get(tail + 2)?,
                expected_u,
            )?,
            expand_knots(
                knot_leaf.parameters.get(tail + 1)?,
                knot_leaf.parameters.get(tail + 3)?,
                expected_v,
            )?,
        )
    } else {
        let kind = if record.partial("UNIFORM_SURFACE").is_some() {
            DefaultNurbsKnotKind::Uniform
        } else if record.partial("QUASI_UNIFORM_SURFACE").is_some() {
            DefaultNurbsKnotKind::QuasiUniform
        } else if record.partial("BEZIER_SURFACE").is_some() {
            DefaultNurbsKnotKind::Bezier
        } else {
            return None;
        };
        (
            default_nurbs_knots(usize::try_from(u_count).ok()?, u_degree, kind)?,
            default_nurbs_knots(usize::try_from(v_count).ok()?, v_degree, kind)?,
        )
    };
    if u_knots.len() != expected_u || v_knots.len() != expected_v {
        return None;
    }
    let weights = if let Some(leaf) = record.partial("RATIONAL_B_SPLINE_SURFACE") {
        let rows = leaf.parameters.first()?.list()?;
        if rows.len() != usize::try_from(u_count).ok()? {
            return None;
        }
        let mut values = Vec::new();
        for row in rows {
            let row = row.list()?;
            if row.len() != usize::try_from(v_count).ok()? {
                return None;
            }
            values.extend(row.iter().map(Value::number).collect::<Option<Vec<_>>>()?);
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
        normal_reversed: false,
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

fn numbers(value: &Value) -> Option<Vec<f64>> {
    value.list()?.iter().map(Value::number).collect()
}

fn normalize(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| vector.scale(1.0 / norm))
}

fn project_axis(vector: Vector3, normal: Vector3) -> Option<Vector3> {
    let vector = normalize(vector)?;
    let normal = normalize(normal)?;
    normalize(vector - normal.scale(vector.dot(normal)))
}

fn second_project_axis(z_axis: Vector3, x_axis: Vector3, vector: Vector3) -> Option<Vector3> {
    let vector = normalize(vector)?;
    let z_axis = normalize(z_axis)?;
    let x_axis = normalize(x_axis)?;
    let projected = (vector - z_axis.scale(vector.dot(z_axis))) - x_axis.scale(vector.dot(x_axis));
    normalize(projected)
}

fn base_axis_3d(
    axis1: Option<Vector3>,
    axis2: Option<Vector3>,
    axis3: Option<Vector3>,
) -> Option<[Vector3; 3]> {
    let z_axis = normalize(axis3.unwrap_or(Vector3::new(0.0, 0.0, 1.0)))?;
    let default_x = default_reference_axis(z_axis);
    let x_axis = project_axis(axis1.unwrap_or(default_x), z_axis)?;
    let y_axis = second_project_axis(z_axis, x_axis, axis2.unwrap_or(Vector3::new(0.0, 1.0, 0.0)))?;
    Some([x_axis, y_axis, z_axis])
}

const AXIS_PARALLEL_TOLERANCE: f64 = 1.0e-12;

fn default_reference_axis(axis: Vector3) -> Vector3 {
    if axis.x.abs() >= 1.0 - AXIS_PARALLEL_TOLERANCE {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    }
}

#[derive(Debug, Clone, Copy)]
enum TransformationParameterError {
    Invalid,
}

fn transformation_direction<T: Copy>(
    record: &RawRecord,
    name: &str,
    index: usize,
    directions: &BTreeMap<u64, T>,
) -> Result<Option<T>, TransformationParameterError> {
    match transformation_parameter(record, name, index)
        .ok_or(TransformationParameterError::Invalid)?
    {
        Value::Omitted | Value::Derived => Ok(None),
        Value::Reference(id) => directions
            .get(id)
            .copied()
            .map(Some)
            .ok_or(TransformationParameterError::Invalid),
        _ => Err(TransformationParameterError::Invalid),
    }
}

fn cartesian_transformation_operator(
    record: &RawRecord,
    points: &BTreeMap<u64, Point3>,
    directions: &BTreeMap<u64, Vector3>,
) -> Option<Transform> {
    let axis1 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        0,
        directions,
    )
    .ok()?;
    let axis2 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        1,
        directions,
    )
    .ok()?;
    let origin = transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 2)?
        .reference()
        .and_then(|id| points.get(&id).copied())?;
    let scale = match transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_3D", 3) {
        Some(Value::Omitted | Value::Derived) | None => 1.0,
        Some(value) => value.number()?,
    };
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let axis3 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D",
        4,
        directions,
    )
    .ok()?;
    let [axis_x, axis_y, axis_z] = base_axis_3d(axis1, axis2, axis3)?;
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

fn cartesian_transformation_operator_2d(
    record: &RawRecord,
    points: &BTreeMap<u64, Point2>,
    directions: &BTreeMap<u64, Point2>,
) -> Option<Transform2> {
    let axis1 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        0,
        directions,
    )
    .ok()?;
    let axis2 = transformation_direction(
        record,
        "CARTESIAN_TRANSFORMATION_OPERATOR_2D",
        1,
        directions,
    )
    .ok()?;
    let (axis1, axis2) = base_axis_2d(axis1, axis2)?;
    let origin = transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 2)?
        .reference()
        .and_then(|id| points.get(&id).copied())?;
    let scale = match transformation_parameter(record, "CARTESIAN_TRANSFORMATION_OPERATOR_2D", 3) {
        Some(Value::Omitted | Value::Derived) | None => 1.0,
        Some(value) => value.number()?,
    };
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    Some(Transform2 {
        rows: [
            [axis1.u * scale, axis2.u * scale, origin.u],
            [axis1.v * scale, axis2.v * scale, origin.v],
            [0.0, 0.0, 1.0],
        ],
    })
}

fn base_axis_2d(axis1: Option<Point2>, axis2: Option<Point2>) -> Option<(Point2, Point2)> {
    match (axis1, axis2) {
        (Some(axis1), axis2) => {
            let axis1 = normalize2(axis1)?;
            let mut perpendicular = Point2::new(-axis1.v, axis1.u);
            if let Some(axis2) = axis2 {
                let axis2 = normalize2(axis2)?;
                if axis2.u * perpendicular.u + axis2.v * perpendicular.v < 0.0 {
                    perpendicular = Point2::new(-perpendicular.u, -perpendicular.v);
                }
            }
            Some((axis1, perpendicular))
        }
        (None, Some(axis2)) => {
            let axis2 = normalize2(axis2)?;
            Some((Point2::new(axis2.v, -axis2.u), axis2))
        }
        (None, None) => Some((Point2::new(1.0, 0.0), Point2::new(0.0, 1.0))),
    }
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
pub(crate) mod tests;
