// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{
    nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point, pcurve_uv, surface_point,
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
use cadmpeg_ir::topology::{PcurveUse, Point};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;
use cadmpeg_ir::SourceObjectAssociation;

use crate::parse::{Exchange, RawRecord, Value};

use super::index::{step_instance_id, CarrierIndex};
use super::opaque_record_id;

pub(super) struct GeometryResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
    pub placements: BTreeMap<u64, (Point3, Vector3, Vector3)>,
    pub length_scale: f64,
    pub plane_angle_scale: f64,
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
            .and_then(representation_items)
        else {
            continue;
        };
        let decoded = items
            .iter()
            .filter_map(|curve| {
                decode_pcurve_geometry(
                    *curve,
                    exchange,
                    &points2,
                    &vectors2,
                    &placements2,
                    angle_scale,
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
                    |(((center, axis, reference_direction), first_radius), second_radius)| {
                        let first_radius = first_radius * scale;
                        let second_radius = second_radius * scale;
                        let (major_direction, major_radius, minor_radius) =
                            if first_radius >= second_radius {
                                (reference_direction, first_radius, second_radius)
                            } else {
                                // STEP ELLIPSE stores two ordered semiaxes;
                                // neither position is required to be the
                                // longer one. The IR ellipse is canonicalized
                                // around its semi-major direction.
                                (
                                    cross(axis, reference_direction),
                                    second_radius,
                                    first_radius,
                                )
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
            "B_SPLINE_CURVE_WITH_KNOTS"
            | "UNIFORM_CURVE"
            | "QUASI_UNIFORM_CURVE"
            | "BEZIER_CURVE" => {
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
        if let Some(parent_step) = record
            .partial("CURVE_REPLICA")
            .and_then(|_| named_parameter(record, "CURVE_REPLICA", 1))
            .and_then(Value::reference)
        {
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
            wake_deferred_dependents(id, &mut waiting_on, &mut deferred_queue);
            continue;
        }
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
            let parameter_range = trimmed_curve_parameter_range(&geometry, start, end, sense);
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
                    parameter_range,
                },
                cache_fit_tolerance: Some(0.0),
            });
            carrier_index.curves.insert(id, curve_index);
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

    // Surface constructors form the same kind of dependency graph as curves.
    // Resolve replicas in the same fixpoint as trims, bounded surfaces, and
    // offsets so a forward or nested replica cannot become an opaque carrier.
    carrier_index = CarrierIndex::from_ir(ir);
    let deferred_surface_count = exchange
        .entities_any(&[
            "CURVE_BOUNDED_SURFACE",
            "OFFSET_SURFACE",
            "RECTANGULAR_TRIMMED_SURFACE",
            "SURFACE_REPLICA",
        ])
        .count();
    for _ in 0..=deferred_surface_count {
        let mut progress = false;
        for (id, record) in exchange.entities("RECTANGULAR_TRIMMED_SURFACE") {
            let surface = SurfaceId(format!("step:data:surface#{id}"));
            if carrier_index.surfaces.contains_key(&id) {
                continue;
            }
            let Some(parameters) = entity_parameters(record, "RECTANGULAR_TRIMMED_SURFACE") else {
                continue;
            };
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
                continue;
            };
            let parameter_scales = surface_parameter_scales(&geometry, scale, angle_scale);
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
            ir.model.surfaces.push(Surface {
                id: surface.clone(),
                geometry,
                source_object: None,
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!(
                    "step:construction:rectangular_trimmed_surface#{id}"
                )),
                surface,
                definition: ProceduralSurfaceDefinition::Subset {
                    support: SurfaceId(format!("step:data:surface#{support_step}")),
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
            progress = true;
        }
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
            let boundaries = parameters.get(2).and_then(references).map(|boundaries| {
                boundaries
                    .into_iter()
                    .map(|boundary| CurveId(format!("step:data:curve#{boundary}")))
                    .collect::<Vec<_>>()
            });
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
    for (id, _) in exchange.entities_any(&[
        "CURVE_BOUNDED_SURFACE",
        "OFFSET_SURFACE",
        "RECTANGULAR_TRIMMED_SURFACE",
    ]) {
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
    let surface_parameter_scales = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            step_instance_id(&surface.id.0).map(|id| {
                (
                    id,
                    surface_parameter_scales(&surface.geometry, scale, angle_scale),
                )
            })
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
        let mut geometry = geometry.clone();
        if let Some(scales) =
            surface_step.and_then(|surface| surface_parameter_scales.get(&surface))
        {
            if !scale_pcurve_geometry(&mut geometry, *scales) {
                warnings.push(format!(
                    "PCURVE #{id} has a 2D carrier that cannot be scaled into the owning surface parameter units"
                ));
                continue;
            }
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

/// Repair angular pcurve coordinates when a source writes degrees while its
/// declared plane-angle unit is radians.
///
/// A pcurve has no independent unit declaration. The surface parameterization
/// and the topological edge endpoints provide the only unambiguous check. Keep
/// the declared unit when it fits; use the degree/radian alternative only when
/// it brings every observed endpoint within the normal coincidence allowance.
pub(super) fn repair_angular_pcurve_units(
    ir: &mut CadIr,
    plane_angle_scale: f64,
    warnings: &mut Vec<String>,
) {
    let observations = {
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
        let vertices = ir
            .model
            .vertices
            .iter()
            .map(|vertex| (vertex.id.0.as_str(), vertex))
            .collect::<HashMap<_, _>>();
        let points = ir
            .model
            .points
            .iter()
            .map(|point| (point.id.0.as_str(), point.position))
            .collect::<HashMap<_, _>>();
        let pcurves = ir
            .model
            .pcurves
            .iter()
            .map(|pcurve| (pcurve.id.0.as_str(), pcurve))
            .collect::<HashMap<_, _>>();
        let mut observations = HashMap::<String, Vec<AngularPcurveObservation>>::new();

        for coedge in &ir.model.coedges {
            let Some(loop_) = loops.get(coedge.owner_loop.0.as_str()) else {
                continue;
            };
            let Some(face) = faces.get(loop_.face.0.as_str()) else {
                continue;
            };
            let Some(surface) = surfaces.get(face.surface.0.as_str()) else {
                continue;
            };
            if angular_parameter_axes(surface).is_none() {
                continue;
            }
            let Some(edge) = edges.get(coedge.edge.0.as_str()) else {
                continue;
            };
            let Some(start) = vertices
                .get(edge.start.0.as_str())
                .and_then(|vertex| points.get(vertex.point.0.as_str()))
                .copied()
            else {
                continue;
            };
            let Some(end) = vertices
                .get(edge.end.0.as_str())
                .and_then(|vertex| points.get(vertex.point.0.as_str()))
                .copied()
            else {
                continue;
            };
            let first_tolerance = edge
                .tolerance
                .into_iter()
                .chain(
                    vertices
                        .get(edge.start.0.as_str())
                        .and_then(|vertex| vertex.tolerance),
                )
                .chain(face.tolerance)
                .chain(std::iter::once(ir.tolerances.linear))
                .fold(COINCIDENCE_TOLERANCE, f64::max);
            let last_tolerance = edge
                .tolerance
                .into_iter()
                .chain(
                    vertices
                        .get(edge.end.0.as_str())
                        .and_then(|vertex| vertex.tolerance),
                )
                .chain(face.tolerance)
                .chain(std::iter::once(ir.tolerances.linear))
                .fold(COINCIDENCE_TOLERANCE, f64::max);

            let Some(first_use) = coedge.pcurves.first() else {
                continue;
            };
            let Some(last_use) = coedge.pcurves.last() else {
                continue;
            };
            let Some(first_pcurve) = pcurves.get(first_use.pcurve.0.as_str()) else {
                continue;
            };
            let Some(last_pcurve) = pcurves.get(last_use.pcurve.0.as_str()) else {
                continue;
            };
            let Some(first_range) = pcurve_use_parameter_range(first_pcurve, first_use) else {
                continue;
            };
            let Some(last_range) = pcurve_use_parameter_range(last_pcurve, last_use) else {
                continue;
            };

            if coedge.pcurves.len() == 1 {
                observations
                    .entry(first_use.pcurve.0.clone())
                    .or_default()
                    .extend([
                        AngularPcurveObservation {
                            surface: (*surface).clone(),
                            parameter: first_range[0],
                            target: start,
                            tolerance: first_tolerance,
                        },
                        AngularPcurveObservation {
                            surface: (*surface).clone(),
                            parameter: first_range[1],
                            target: end,
                            tolerance: last_tolerance,
                        },
                    ]);
            } else {
                observations
                    .entry(first_use.pcurve.0.clone())
                    .or_default()
                    .push(AngularPcurveObservation {
                        surface: (*surface).clone(),
                        parameter: first_range[0],
                        target: start,
                        tolerance: first_tolerance,
                    });
                observations
                    .entry(last_use.pcurve.0.clone())
                    .or_default()
                    .push(AngularPcurveObservation {
                        surface: (*surface).clone(),
                        parameter: last_range[1],
                        target: end,
                        tolerance: last_tolerance,
                    });
            }
        }
        observations
    };

    let repairs = observations
        .iter()
        .filter_map(|(id, observations)| {
            let pcurve = ir.model.pcurves.iter().find(|pcurve| pcurve.id.0 == *id)?;
            let candidates =
                angular_parameter_candidates(&observations.first()?.surface, plane_angle_scale)?;
            let scales =
                choose_angular_parameter_repair(&pcurve.geometry, observations, &candidates)?;
            Some((id.clone(), scales))
        })
        .collect::<HashMap<_, _>>();

    let mut repaired = 0;
    for pcurve in &mut ir.model.pcurves {
        let Some(scales) = repairs.get(&pcurve.id.0) else {
            continue;
        };
        if scale_pcurve_geometry(&mut pcurve.geometry, *scales) {
            repaired += 1;
        }
    }
    if repaired > 0 {
        warnings.push(format!(
            "normalized {repaired} angular pcurve(s) to match their surface parameter units using edge endpoints"
        ));
    }
}

#[derive(Debug, Clone)]
struct AngularPcurveObservation {
    surface: SurfaceGeometry,
    parameter: f64,
    target: Point3,
    tolerance: f64,
}

fn angular_parameter_axes(geometry: &SurfaceGeometry) -> Option<[bool; 2]> {
    match geometry {
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => Some([true, false]),
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => Some([true, true]),
        SurfaceGeometry::Transformed { basis, .. } => angular_parameter_axes(basis),
        SurfaceGeometry::Plane { .. }
        | SurfaceGeometry::Nurbs { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn angular_parameter_candidates(
    geometry: &SurfaceGeometry,
    plane_angle_scale: f64,
) -> Option<Vec<[f64; 2]>> {
    let axes = angular_parameter_axes(geometry)?;
    let current = if plane_angle_scale.is_finite() && plane_angle_scale > 0.0 {
        plane_angle_scale
    } else {
        1.0
    };
    let alternate = if (current - 1.0).abs() <= 1.0e-12 {
        std::f64::consts::PI / 180.0
    } else {
        1.0
    };
    let alternate_ratio = alternate / current;
    if !alternate_ratio.is_finite() || alternate_ratio <= 0.0 {
        return None;
    }

    let u_factors = if axes[0] && (alternate_ratio - 1.0).abs() > 1.0e-12 {
        vec![1.0, alternate_ratio]
    } else {
        vec![1.0]
    };
    let v_factors = if axes[1] && (alternate_ratio - 1.0).abs() > 1.0e-12 {
        vec![1.0, alternate_ratio]
    } else {
        vec![1.0]
    };
    let mut candidates = Vec::with_capacity(u_factors.len() * v_factors.len());
    for u in u_factors {
        for v in &v_factors {
            candidates.push([u, *v]);
        }
    }
    Some(candidates)
}

fn choose_angular_parameter_repair(
    geometry: &PcurveGeometry,
    observations: &[AngularPcurveObservation],
    candidates: &[[f64; 2]],
) -> Option<[f64; 2]> {
    let mut scores = vec![0.0; candidates.len()];
    for (index, scales) in candidates.iter().enumerate() {
        let mut scaled = geometry.clone();
        if !scale_pcurve_geometry(&mut scaled, *scales) {
            scores[index] = f64::INFINITY;
            continue;
        }
        for observation in observations {
            let Some(uv) = pcurve_uv(&scaled, observation.parameter) else {
                scores[index] = f64::INFINITY;
                break;
            };
            let Some(mapped) = surface_point(&observation.surface, uv.u, uv.v) else {
                scores[index] = f64::INFINITY;
                break;
            };
            let tolerance = observation.tolerance.max(COINCIDENCE_TOLERANCE);
            let normalized = mapped.distance(observation.target) / tolerance;
            if !normalized.is_finite() {
                scores[index] = f64::INFINITY;
                break;
            }
            scores[index] = scores[index].max(normalized);
        }
    }

    let current = scores.first().copied()?;
    let (best_index, &best) = scores
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| left.total_cmp(right))?;
    if best_index == 0 || !best.is_finite() || best > 1.0 || current <= 10.0 {
        return None;
    }
    (current > best * 100.0).then(|| candidates[best_index])
}

fn pcurve_use_parameter_range(pcurve: &Pcurve, pcurve_use: &PcurveUse) -> Option<[f64; 2]> {
    pcurve_use
        .parameter_range
        .or(pcurve.parameter_range)
        .or_else(|| pcurve_parameter_domain(&pcurve.geometry))
}

fn pcurve_parameter_domain(geometry: &PcurveGeometry) -> Option<[f64; 2]> {
    match geometry {
        PcurveGeometry::Nurbs { knots, .. } | PcurveGeometry::PolarNurbs { knots, .. } => {
            let start = knots.first().copied()?;
            let end = knots.last().copied()?;
            (start.is_finite() && end.is_finite() && start != end).then_some([start, end])
        }
        PcurveGeometry::Trimmed {
            parameter_range, ..
        } => Some(*parameter_range),
        PcurveGeometry::Offset { basis, .. } => pcurve_parameter_domain(basis),
        PcurveGeometry::Line { .. }
        | PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::Harmonic { .. }
        | PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Hyperbolic { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => None,
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
        .and_then(|record| record.parameter(0))
        .and_then(|value| {
            super::decode_text(
                value,
                losses,
                target,
                "presentation carrier name",
                LossKind::MetadataNotTransferred,
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
            .any(|partial| partial.name.ends_with("REPRESENTATION"))
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
                    losses.push(unresolved_unit_loss(format!(
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
        CurveGeometry::Transformed { basis, transform } => curve_parameter_at_point(
            basis,
            transform.try_inverse_affine()?.apply_point(point),
            tolerance,
        ),
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
                    curve: CurveId(format!("step:data:curve#{curve_step}")),
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
    let geometry = if record.partials.iter().any(|partial| {
        matches!(
            partial.name.as_str(),
            "B_SPLINE_CURVE_WITH_KNOTS" | "UNIFORM_CURVE" | "QUASI_UNIFORM_CURVE" | "BEZIER_CURVE"
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
                    parameter_range: trimmed_pcurve_parameter_range(&basis, start, end, sense),
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

fn trimmed_pcurve_parameter_range(
    geometry: &PcurveGeometry,
    start: f64,
    end: f64,
    sense: bool,
) -> [f64; 2] {
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
    let range = if sense { [start, end] } else { [end, start] };
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
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

fn surface_parameter_scales(
    geometry: &SurfaceGeometry,
    length_scale: f64,
    angle_scale: f64,
) -> [f64; 2] {
    match geometry {
        SurfaceGeometry::Plane { .. } => [length_scale, length_scale],
        SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
            [angle_scale, length_scale]
        }
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            [angle_scale, angle_scale]
        }
        SurfaceGeometry::Transformed { basis, .. } => {
            surface_parameter_scales(basis, length_scale, angle_scale)
        }
        SurfaceGeometry::Nurbs { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Unknown { .. } => [1.0, 1.0],
    }
}

fn surface_parameter_periods(geometry: &SurfaceGeometry) -> [Option<f64>; 2] {
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
fn scale_pcurve_geometry(geometry: &mut PcurveGeometry, scales: [f64; 2]) -> bool {
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
    fn surface_parameter_units_follow_the_surface_chart() {
        let plane = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let sphere = SurfaceGeometry::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let transformed = SurfaceGeometry::Transformed {
            basis: Box::new(cylinder.clone()),
            transform: Transform::identity(),
        };
        assert_eq!(surface_parameter_scales(&plane, 10.0, 0.25), [10.0, 10.0]);
        assert_eq!(
            surface_parameter_scales(&cylinder, 10.0, 0.25),
            [0.25, 10.0]
        );
        assert_eq!(surface_parameter_scales(&sphere, 10.0, 0.25), [0.25, 0.25]);
        assert_eq!(
            surface_parameter_scales(&transformed, 10.0, 0.25),
            [0.25, 10.0]
        );
        assert_eq!(
            surface_parameter_scales(&SurfaceGeometry::Unknown { record: None }, 10.0, 0.25),
            [1.0, 1.0]
        );
    }

    #[test]
    fn anisotropic_circle_scaling_preserves_its_native_parameterization() {
        let original = PcurveGeometry::Circle {
            center: Point2::new(1.0, -2.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 3.0,
        };
        let mut scaled = original.clone();
        assert!(scale_pcurve_geometry(&mut scaled, [2.0, 3.0]));
        assert!(matches!(scaled, PcurveGeometry::Harmonic { .. }));
        for parameter in [0.0, 0.25, 1.0, 2.0] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&original, parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&scaled, parameter).unwrap();
            assert!((actual.u - expected.u * 2.0).abs() < 1.0e-12);
            assert!((actual.v - expected.v * 3.0).abs() < 1.0e-12);
        }
    }

    #[test]
    fn unsupported_anisotropic_pcurve_forms_are_not_reshaped_by_scalar_scaling() {
        let mut parabola = PcurveGeometry::Parabola {
            vertex: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            focal_distance: 1.0,
        };
        assert!(!scale_pcurve_geometry(&mut parabola, [2.0, 3.0]));
        assert!(matches!(parabola, PcurveGeometry::Parabola { .. }));
    }

    #[test]
    fn angular_pcurve_repair_requires_topological_endpoint_evidence() {
        let surface = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let degree_geometry = PcurveGeometry::Line {
            origin: Point2::new(180.0, 0.0),
            direction: Point2::new(0.0, 5.0),
        };
        let observations = [
            AngularPcurveObservation {
                surface: surface.clone(),
                parameter: 0.0,
                target: surface_point(&surface, std::f64::consts::PI, 0.0).unwrap(),
                tolerance: COINCIDENCE_TOLERANCE,
            },
            AngularPcurveObservation {
                surface: surface.clone(),
                parameter: 1.0,
                target: surface_point(&surface, std::f64::consts::PI, 5.0).unwrap(),
                tolerance: COINCIDENCE_TOLERANCE,
            },
        ];
        let candidates = angular_parameter_candidates(&surface, 1.0).unwrap();
        assert_eq!(
            choose_angular_parameter_repair(&degree_geometry, &observations, &candidates),
            Some([std::f64::consts::PI / 180.0, 1.0])
        );

        let radians = PcurveGeometry::Line {
            origin: Point2::new(std::f64::consts::PI, 0.0),
            direction: Point2::new(0.0, 5.0),
        };
        assert_eq!(
            choose_angular_parameter_repair(&radians, &observations, &candidates),
            None
        );
    }

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
}
