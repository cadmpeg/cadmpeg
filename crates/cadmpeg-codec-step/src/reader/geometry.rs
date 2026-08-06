// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{hash_map::Entry, BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{nurbs_curve_parameter_domain, nurbs_curve_parameter_near_point};
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
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Point;
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
    let scale = length_scale(exchange).unwrap_or(1.0);
    let angle_scale = plane_angle_scale(exchange).unwrap_or(1.0);
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let losses = Vec::new();
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
        if record.simple_name() == Some("VECTOR") {
            let value = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|direction| directions.get(&direction).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .map(|(direction, magnitude)| scale_vector(direction, magnitude * scale));
            let value2 = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|direction| directions2.get(&direction).copied())
                .zip(record.parameter(2).and_then(Value::number))
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
        if matches!(
            record.simple_name(),
            Some("AXIS2_PLACEMENT_3D" | "AXIS1_PLACEMENT")
        ) {
            let placement = record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .map(|origin| {
                    let axis = optional_direction(record.parameter(2), &directions)
                        .unwrap_or(Vector3::new(0.0, 0.0, 1.0));
                    let reference = optional_direction(record.parameter(3), &directions)
                        .and_then(|reference| orthogonal_reference(axis, reference))
                        .unwrap_or_else(|| derive_reference_direction(axis));
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
    for (id, record) in exchange.entities("AXIS2_PLACEMENT_2D") {
        if record.simple_name() != Some("AXIS2_PLACEMENT_2D") {
            continue;
        }
        let placement = record
            .parameter(1)
            .and_then(Value::reference)
            .and_then(|point| points2.get(&point).copied())
            .and_then(|origin| {
                let x_axis = match record.parameter(2) {
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
        if record.simple_name() != Some("PCURVE") {
            continue;
        }
        let Some(items) = record
            .parameter(2)
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
        "POLYLINE",
        "B_SPLINE_CURVE_WITH_KNOTS",
    ]) {
        let geometry = match record.simple_name() {
            Some("LINE" | "CIRCLE" | "ELLIPSE" | "POLYLINE" | "B_SPLINE_CURVE_WITH_KNOTS")
                if pcurve_geometry_records.contains(&id) =>
            {
                continue;
            }
            Some("LINE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|point| points.get(&point).copied())
                .zip(
                    record
                        .parameter(2)
                        .and_then(Value::reference)
                        .and_then(|vector| vectors.get(&vector).copied())
                        .and_then(normalize),
                )
                .map(|(origin, direction)| CurveGeometry::Line { origin, direction }),
            Some("CIRCLE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .filter(|(_, radius)| radius.is_finite() && *radius > 0.0)
                .map(
                    |((center, axis, ref_direction), radius)| CurveGeometry::Circle {
                        center,
                        axis,
                        ref_direction,
                        radius: radius * scale,
                    },
                ),
            Some("ELLIPSE") => record
                .parameter(1)
                .and_then(Value::reference)
                .and_then(|placement| placements.get(&placement).copied())
                .zip(record.parameter(2).and_then(Value::number))
                .zip(record.parameter(3).and_then(Value::number))
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
            Some("POLYLINE") => polyline(record, &points).map(CurveGeometry::Nurbs),
            Some("B_SPLINE_CURVE_WITH_KNOTS") => {
                nurbs_curve(record, &points).map(CurveGeometry::Nurbs)
            }
            _ => continue,
        };
        if let Some(geometry) = geometry {
            ir.model.curves.push(Curve {
                id: CurveId(format!("step:data:curve#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has invalid geometry",
                record.simple_name().expect("matched simple name")
            ));
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_CURVE_WITH_KNOTS") {
        if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_CURVE_WITH_KNOTS")
            || pcurve_geometry_records.contains(&id)
        {
            continue;
        }
        if let Some(nurbs) = nurbs_curve(record, &points) {
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
        .entities_any(&["TRIMMED_CURVE", "COMPOSITE_CURVE", "OFFSET_CURVE_3D"])
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
            let Some((basis_step, sense)) = trimmed_curve_attributes(parameters) else {
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
            let linear_parameter_scale = line_parameter_scale(exchange, basis_step, scale);
            let start = parameters.get(2).and_then(|value| {
                trim_parameter(
                    value,
                    &points,
                    &geometry,
                    angle_scale,
                    linear_parameter_scale,
                    ir.tolerances.linear,
                )
            });
            let end = parameters.get(3).and_then(|value| {
                trim_parameter(
                    value,
                    &points,
                    &geometry,
                    angle_scale,
                    linear_parameter_scale,
                    ir.tolerances.linear,
                )
            });
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
        if record.partial("COMPOSITE_CURVE").is_some() {
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
    for (id, _) in exchange.entities("TRIMMED_CURVE") {
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
    for (id, _) in exchange.entities("OFFSET_CURVE_3D") {
        if !carrier_index.curves.contains_key(&id) {
            warnings.push(format!(
                "OFFSET_CURVE_3D #{id} has invalid or unresolved basis parameters"
            ));
        }
    }
    for (id, _) in exchange.entities_any(&["TRIMMED_CURVE", "COMPOSITE_CURVE", "OFFSET_CURVE_3D"]) {
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
    for (id, record) in exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE"]) {
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
        "TOROIDAL_SURFACE",
        "DEGENERATE_TOROIDAL_SURFACE",
        "B_SPLINE_SURFACE_WITH_KNOTS",
    ]) {
        let placement = record
            .parameter(1)
            .and_then(Value::reference)
            .and_then(|placement| placements.get(&placement).copied());
        let geometry = match record.simple_name() {
            Some("PLANE") => placement.map(|(origin, normal, u_axis)| SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            }),
            Some("CYLINDRICAL_SURFACE") => placement.zip(positive(record.parameter(2))).map(
                |((origin, axis, ref_direction), radius)| SurfaceGeometry::Cylinder {
                    origin,
                    axis,
                    ref_direction,
                    radius: radius * scale,
                },
            ),
            Some("CONICAL_SURFACE") => placement
                .zip(nonnegative(record.parameter(2)))
                .zip(record.parameter(3).and_then(Value::number))
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
            Some("SPHERICAL_SURFACE") => placement.zip(positive(record.parameter(2))).map(
                |((center, axis, ref_direction), radius)| SurfaceGeometry::Sphere {
                    center,
                    axis,
                    ref_direction,
                    radius: radius * scale,
                },
            ),
            Some("TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE") => placement
                .zip(positive(record.parameter(2)))
                .zip(positive(record.parameter(3)))
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
            Some("B_SPLINE_SURFACE_WITH_KNOTS") => {
                nurbs_surface(record, &points).map(SurfaceGeometry::Nurbs)
            }
            _ => continue,
        };
        if let Some(geometry) = geometry {
            ir.model.surfaces.push(Surface {
                id: SurfaceId(format!("step:data:surface#{id}")),
                geometry,
                source_object: None,
            });
            typed.insert(id);
        } else {
            warnings.push(format!(
                "{} #{id} has invalid geometry",
                record.simple_name().expect("matched simple name")
            ));
        }
    }
    for (id, record) in exchange.entities("B_SPLINE_SURFACE_WITH_KNOTS") {
        if record.partial("B_SPLINE_SURFACE_WITH_KNOTS").is_none()
            || record.simple_name() == Some("B_SPLINE_SURFACE_WITH_KNOTS")
        {
            continue;
        }
        if let Some(nurbs) = nurbs_surface(record, &points) {
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
        if !progress {
            break;
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
        if record.simple_name() != Some("PCURVE") {
            continue;
        }
        let surface_step = record.parameter(1).and_then(Value::reference);
        let representation = record
            .parameter(2)
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
        if let Some(representation) = record.parameter(2).and_then(Value::reference) {
            typed.insert(representation);
        }
        typed.insert(curve_step);
        typed.extend(geometry_records.iter().copied());
    }

    for (id, record) in exchange.entities("DEGENERATE_TOROIDAL_SURFACE") {
        if record.simple_name() != Some("DEGENERATE_TOROIDAL_SURFACE") {
            continue;
        }
        let select_outer = record
            .parameter(4)
            .and_then(logical_value)
            .and_then(StepLogical::into_option);
        let surface = SurfaceId(format!("step:data:surface#{id}"));
        if !carrier_index.surfaces.contains_key(&id) {
            continue;
        }
        let Some(select_outer) = select_outer else {
            warnings.push(format!(
                "DEGENERATE_TOROIDAL_SURFACE #{id} has invalid sheet selection"
            ));
            continue;
        };
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
) {
    for set in exchange.records.values() {
        if !matches!(
            set.simple_name(),
            Some("GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET")
        ) {
            continue;
        }
        let Some(members) = set.parameter(1).and_then(Value::list) else {
            continue;
        };
        for member in members.iter().filter_map(Value::reference) {
            let name = exchange
                .records
                .get(&member)
                .and_then(|record| record.parameter(0))
                .and_then(|value| match value {
                    Value::String(bytes) => crate::strings::decode(bytes).ok(),
                    _ => None,
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
                .and_then(|value| match value {
                    Value::String(bytes) => crate::strings::decode(bytes).ok(),
                    _ => None,
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

fn trimmed_curve_attributes(parameters: &[Value]) -> Option<(u64, bool)> {
    let basis = parameters.get(1).and_then(Value::reference)?;
    let sense = parameters.get(4).and_then(Value::logical)?;
    Some((basis, sense))
}

fn surface_curve_basis(record: &RawRecord) -> Option<u64> {
    if record.partials.len() == 1 {
        return record.parameter(1).and_then(Value::reference);
    }
    record
        .partial("SURFACE_CURVE")
        .or_else(|| record.partial("SEAM_CURVE"))
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

fn trim_parameter(
    value: &Value,
    points: &BTreeMap<u64, Point3>,
    geometry: &CurveGeometry,
    angle_scale: f64,
    linear_parameter_scale: f64,
    tolerance: f64,
) -> Option<f64> {
    match value {
        Value::Integer(value) => {
            Some(parameter_scale(geometry, angle_scale, linear_parameter_scale) * *value as f64)
        }
        Value::Real(value) => {
            Some(parameter_scale(geometry, angle_scale, linear_parameter_scale) * *value)
        }
        Value::Typed(_, value) => trim_parameter(
            value,
            points,
            geometry,
            angle_scale,
            linear_parameter_scale,
            tolerance,
        ),
        Value::Reference(id) => points
            .get(id)
            .and_then(|point| curve_parameter_at_point(geometry, *point, tolerance)),
        Value::List(values) => values.iter().find_map(|value| {
            trim_parameter(
                value,
                points,
                geometry,
                angle_scale,
                linear_parameter_scale,
                tolerance,
            )
        }),
        _ => None,
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

fn line_parameter_scale(exchange: &Exchange, curve: u64, length_scale: f64) -> f64 {
    exchange
        .records
        .get(&curve)
        .filter(|record| record.simple_name() == Some("LINE"))
        .and_then(|record| record.parameter(2))
        .and_then(ValueExt::reference)
        .and_then(|vector| exchange.records.get(&vector))
        .filter(|record| record.simple_name() == Some("VECTOR"))
        .and_then(|record| record.parameter(2))
        .and_then(ValueExt::number)
        .map(|magnitude| magnitude * length_scale)
        .filter(|scale| scale.is_finite() && *scale > 0.0)
        .unwrap_or(length_scale)
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

fn composite_curve_dependencies(record: &RawRecord, exchange: &Exchange) -> Vec<u64> {
    let complex = record.partials.len() > 1;
    let offset = usize::from(!complex);
    record
        .partial("COMPOSITE_CURVE")
        .and_then(|composite| composite.parameters.get(offset))
        .and_then(Value::list)
        .into_iter()
        .flatten()
        .filter_map(Value::reference)
        .filter_map(|segment| exchange.records.get(&segment))
        .filter(|segment| segment.simple_name() == Some("COMPOSITE_CURVE_SEGMENT"))
        .filter_map(|segment| segment.parameter(2).and_then(Value::reference))
        .collect()
}

fn composite_curve(
    record: &RawRecord,
    exchange: &Exchange,
    decoded: &CarrierIndex,
) -> Option<CompositeCurveData> {
    let complex = record.partials.len() > 1;
    let composite = record.partial("COMPOSITE_CURVE")?;
    let offset = usize::from(!complex);
    let segments = composite
        .parameters
        .get(offset)?
        .list()?
        .iter()
        .map(|value| {
            let id = value.reference()?;
            let record = exchange.records.get(&id)?;
            if record.simple_name() != Some("COMPOSITE_CURVE_SEGMENT") {
                return None;
            }
            let transition = match record.parameter(0)?.enumeration()? {
                "DISCONTINUOUS" => CompositeCurveTransition::Discontinuous,
                "CONTINUOUS" => CompositeCurveTransition::Continuous,
                "CONTSAMEGRADIENT" => CompositeCurveTransition::ContSameGradient,
                "CONTSAMEGRADIENTSAMECURVATURE" => {
                    CompositeCurveTransition::ContSameGradientSameCurvature
                }
                _ => return None,
            };
            let curve_step = record.parameter(2)?.reference()?;
            decoded.curves.contains_key(&curve_step).then_some((
                id,
                CompositeCurveSegment {
                    curve: CurveId(format!("step:data:curve#{curve_step}")),
                    same_sense: record.parameter(1)?.logical()?,
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

fn nurbs_curve(record: &RawRecord, points: &BTreeMap<u64, Point3>) -> Option<NurbsCurve> {
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
    let periodic = logical_value(base.parameters.get(offset + 3)?)?
        .into_option()
        .unwrap_or(false);
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

fn nurbs_pcurve(record: &RawRecord, points: &BTreeMap<u64, Point2>) -> Option<PcurveGeometry> {
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
    let periodic = logical_value(base.parameters.get(offset + 3)?)?
        .into_option()
        .unwrap_or(false);
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
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<(PcurveGeometry, BTreeSet<u64>)> {
    if depth >= 256 || !active.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    let mut records = BTreeSet::from([id]);
    let geometry = if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_some() {
        nurbs_pcurve(record, points)?
    } else {
        match record.simple_name() {
            Some("LINE") => {
                let origin = record
                    .parameter(1)?
                    .reference()
                    .and_then(|point| points.get(&point).copied())?;
                let direction = record
                    .parameter(2)?
                    .reference()
                    .and_then(|vector| vectors.get(&vector).copied())?;
                PcurveGeometry::Line { origin, direction }
            }
            Some("CIRCLE") => {
                let placement = record.parameter(1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let radius = positive(record.parameter(2))?;
                records.insert(placement);
                PcurveGeometry::Circle {
                    center,
                    x_axis,
                    y_axis,
                    radius,
                }
            }
            Some("ELLIPSE") => {
                let placement = record.parameter(1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let major_radius = positive(record.parameter(2))?;
                let minor_radius = positive(record.parameter(3))?;
                records.insert(placement);
                PcurveGeometry::Ellipse {
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                }
            }
            Some("PARABOLA") => {
                let placement = record.parameter(1)?.reference()?;
                let (vertex, x_axis, y_axis) = placements.get(&placement).copied()?;
                let focal_distance = positive(record.parameter(2))?;
                records.insert(placement);
                PcurveGeometry::Parabola {
                    vertex,
                    x_axis,
                    y_axis,
                    focal_distance,
                }
            }
            Some("HYPERBOLA") => {
                let placement = record.parameter(1)?.reference()?;
                let (center, x_axis, y_axis) = placements.get(&placement).copied()?;
                let major_radius = positive(record.parameter(2))?;
                let minor_radius = positive(record.parameter(3))?;
                records.insert(placement);
                PcurveGeometry::Hyperbola {
                    center,
                    x_axis,
                    y_axis,
                    major_radius,
                    minor_radius,
                }
            }
            Some("POLYLINE") => polyline_pcurve(record, points)?,
            Some("TRIMMED_CURVE") => {
                let basis_id = record.parameter(1)?.reference()?;
                let sense = record.parameter(4)?.logical()?;
                let (basis, basis_records) = decode_pcurve_geometry(
                    basis_id,
                    exchange,
                    points,
                    vectors,
                    placements,
                    angle_scale,
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
                let start = pcurve_trim_parameter(record.parameter(2)?)? * scale;
                let end = pcurve_trim_parameter(record.parameter(3)?)? * scale;
                records.extend(basis_records);
                PcurveGeometry::Trimmed {
                    parameter_range: if sense { [start, end] } else { [end, start] },
                    basis: Box::new(basis),
                }
            }
            Some("OFFSET_CURVE_2D") => {
                let basis_id = record.parameter(1)?.reference()?;
                let distance = record.parameter(2)?.number()?;
                if !distance.is_finite() || record.parameter(3)?.logical().is_none() {
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

fn nurbs_surface(record: &RawRecord, points: &BTreeMap<u64, Point3>) -> Option<NurbsSurface> {
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
    let u_periodic = logical_value(base.parameters.get(offset + 4)?)?
        .into_option()
        .unwrap_or(false);
    let v_periodic = logical_value(base.parameters.get(offset + 5)?)?
        .into_option()
        .unwrap_or(false);
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
    if record
        .partials
        .iter()
        .any(|partial| matches!(partial.name.as_str(), "SURFACE_CURVE" | "SEAM_CURVE"))
    {
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
