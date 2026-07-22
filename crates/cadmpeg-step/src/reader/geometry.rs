// SPDX-License-Identifier: Apache-2.0
//! STEP representation units, placements, and geometry carriers.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use cadmpeg_ir::document::CadIr;
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
use cadmpeg_ir::topology::Point;

use crate::parse::{Exchange, RawRecord, Value};

pub(super) struct GeometryResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub placements: BTreeMap<u64, (Point3, Vector3, Vector3)>,
    pub length_scale: f64,
    pub plane_angle_scale: f64,
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> GeometryResult {
    let scale = length_scale(exchange).unwrap_or(1.0);
    let angle_scale = plane_angle_scale(exchange).unwrap_or(1.0);
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut points = BTreeMap::new();
    let mut points2 = BTreeMap::new();
    let mut directions = BTreeMap::new();
    let mut directions2 = BTreeMap::new();
    let mut vectors = BTreeMap::new();
    let mut vectors2 = BTreeMap::new();
    let mut placements = BTreeMap::new();
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
        if record.simple_name() == Some("VERTEX_POINT") {
            if let Some(id) = record.parameter(1).and_then(Value::reference) {
                point_carriers.insert(id);
            }
        }
        if record
            .simple_name()
            .is_some_and(|name| name.ends_with("REPRESENTATION"))
        {
            if let Some(items) = record.parameter(1).and_then(Value::list) {
                point_carriers.extend(
                    items
                        .iter()
                        .filter_map(Value::reference)
                        .filter(|id| points.contains_key(id)),
                );
            }
        }
        if matches!(
            record.simple_name(),
            Some("GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET")
        ) {
            if let Some(items) = record.parameter(1).and_then(Value::list) {
                point_carriers.extend(
                    items
                        .iter()
                        .filter_map(Value::reference)
                        .filter(|id| points.contains_key(id)),
                );
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
    let mut pcurve_geometries = exchange
        .entities("LINE")
        .filter_map(|(id, record)| {
            if record.simple_name() != Some("LINE") {
                return None;
            }
            let origin = record
                .parameter(1)?
                .reference()
                .and_then(|point| points2.get(&point).copied())?;
            let direction = record
                .parameter(2)?
                .reference()
                .and_then(|vector| vectors2.get(&vector).copied())?;
            Some((id, PcurveGeometry::Line { origin, direction }))
        })
        .collect::<BTreeMap<_, _>>();
    for (id, record) in exchange.entities("B_SPLINE_CURVE_WITH_KNOTS") {
        if record.partial("B_SPLINE_CURVE_WITH_KNOTS").is_some() {
            if let Some(geometry) = nurbs_pcurve(record, &points2) {
                pcurve_geometries.insert(id, geometry);
            }
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
            Some("LINE") if pcurve_geometries.contains_key(&id) => continue,
            Some("B_SPLINE_CURVE_WITH_KNOTS") if pcurve_geometries.contains_key(&id) => continue,
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
            || pcurve_geometries.contains_key(&id)
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

    for (id, record) in exchange.entities_any(&["SURFACE_CURVE", "SEAM_CURVE"]) {
        if !matches!(record.simple_name(), Some("SURFACE_CURVE" | "SEAM_CURVE")) {
            continue;
        }
        let basis = record
            .parameter(1)
            .and_then(Value::reference)
            .map(|basis| CurveId(format!("step:data:curve#{basis}")));
        if !basis.is_some_and(|basis| ir.model.curves.iter().any(|curve| curve.id == basis)) {
            warnings.push(format!(
                "{} #{id} has no decoded 3D curve",
                record.simple_name().expect("matched surface curve")
            ));
            continue;
        }
        typed.insert(id);
    }
    let mut pending_composites = exchange
        .entities("COMPOSITE_CURVE")
        .map(|(id, _)| id)
        .collect::<BTreeSet<_>>();
    let curve_geometries = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.clone(), curve.geometry.clone()))
        .collect::<BTreeMap<_, _>>();
    for (id, record) in exchange.entities("TRIMMED_CURVE") {
        if record.simple_name() != Some("TRIMMED_CURVE") {
            continue;
        }
        let basis_step = record.parameter(1).and_then(Value::reference);
        let sense = record.parameter(4).and_then(Value::logical);
        let Some((basis_step, sense)) = basis_step.zip(sense) else {
            warnings.push(format!("TRIMMED_CURVE #{id} has invalid basis or sense"));
            continue;
        };
        let basis = CurveId(format!("step:data:curve#{basis_step}"));
        let Some(geometry) = curve_geometries.get(&basis).cloned() else {
            warnings.push(format!("TRIMMED_CURVE #{id} has no decoded basis curve"));
            continue;
        };
        let linear_parameter_scale = line_parameter_scale(exchange, basis_step, scale);
        let start = record.parameter(2).and_then(|value| {
            trim_parameter(
                value,
                &points,
                &geometry,
                angle_scale,
                linear_parameter_scale,
            )
        });
        let end = record.parameter(3).and_then(|value| {
            trim_parameter(
                value,
                &points,
                &geometry,
                angle_scale,
                linear_parameter_scale,
            )
        });
        let Some((start, end)) = start.zip(end) else {
            warnings.push(format!(
                "TRIMMED_CURVE #{id} has trim selectors incompatible with its basis curve"
            ));
            continue;
        };
        let curve = CurveId(format!("step:data:curve#{id}"));
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry,
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("step:construction:trimmed_curve#{id}")),
            curve,
            definition: ProceduralCurveDefinition::Subset {
                source: basis,
                parameter_range: if sense { [start, end] } else { [end, start] },
            },
            cache_fit_tolerance: Some(0.0),
        });
        typed.insert(id);
    }

    let mut decoded_curve_ids = ir
        .model
        .curves
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<BTreeSet<_>>();
    let mut decoded_curve_steps = decoded_curve_ids
        .iter()
        .filter_map(|id| id.0.rsplit('#').next()?.parse::<u64>().ok())
        .collect::<BTreeSet<_>>();
    let mut unresolved = HashMap::<u64, usize>::new();
    let mut dependents = HashMap::<u64, Vec<u64>>::new();
    let mut ready = VecDeque::new();
    for &id in &pending_composites {
        let Some(dependencies) = exchange
            .records
            .get(&id)
            .and_then(|record| composite_dependencies(record, exchange))
        else {
            continue;
        };
        let mut count = 0;
        for dependency in dependencies {
            if decoded_curve_steps.contains(&dependency) {
                continue;
            }
            count += 1;
            dependents.entry(dependency).or_default().push(id);
        }
        unresolved.insert(id, count);
        if count == 0 {
            ready.push_back(id);
        }
    }
    while let Some(id) = ready.pop_front() {
        let Some((segments, self_intersect)) = exchange
            .records
            .get(&id)
            .and_then(|record| composite_curve(record, exchange, &decoded_curve_ids))
        else {
            continue;
        };
        typed.extend(segments.iter().map(|(id, _)| *id));
        ir.model.curves.push(Curve {
            id: CurveId(format!("step:data:curve#{id}")),
            geometry: CurveGeometry::Composite {
                segments: segments.into_iter().map(|(_, segment)| segment).collect(),
                self_intersect,
            },
            source_object: None,
        });
        typed.insert(id);
        pending_composites.remove(&id);
        decoded_curve_ids.insert(CurveId(format!("step:data:curve#{id}")));
        decoded_curve_steps.insert(id);
        for dependent in dependents.get(&id).into_iter().flatten() {
            let Some(count) = unresolved.get_mut(dependent) else {
                continue;
            };
            *count -= 1;
            if *count == 0 {
                ready.push_back(*dependent);
            }
        }
    }
    for id in pending_composites {
        warnings.push(format!(
            "COMPOSITE_CURVE #{id} has invalid, cyclic, or unresolved segments"
        ));
    }

    let offset_sources = ir
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.clone(), curve.geometry.clone()))
        .collect::<BTreeMap<_, _>>();
    for (id, record) in exchange.entities("OFFSET_CURVE_3D") {
        if record.simple_name() != Some("OFFSET_CURVE_3D") {
            continue;
        }
        let source = record
            .parameter(1)
            .and_then(Value::reference)
            .map(|source| CurveId(format!("step:data:curve#{source}")));
        let distance = record.parameter(2).and_then(Value::number);
        let self_intersect = record
            .parameter(3)
            .and_then(logical_value)
            .map(StepLogical::into_option);
        let reference_direction = record
            .parameter(4)
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
            warnings.push(format!("OFFSET_CURVE_3D #{id} has invalid parameters"));
            continue;
        };
        let Some(geometry) = offset_sources.get(&source).cloned() else {
            warnings.push(format!("OFFSET_CURVE_3D #{id} has no decoded basis curve"));
            continue;
        };
        let curve = CurveId(format!("step:data:curve#{id}"));
        ir.model.curves.push(Curve {
            id: curve.clone(),
            geometry,
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId(format!("step:construction:offset_curve#{id}")),
            curve,
            definition: ProceduralCurveDefinition::SpatialOffset {
                source,
                distance: distance * scale,
                reference_direction,
                self_intersect,
            },
            cache_fit_tolerance: None,
        });
        typed.insert(id);
    }

    let curve_ids = ir
        .model
        .curves
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<BTreeSet<_>>();
    for (id, record) in
        exchange.entities_any(&["SURFACE_OF_LINEAR_EXTRUSION", "SURFACE_OF_REVOLUTION"])
    {
        let definition = match record.simple_name() {
            Some("SURFACE_OF_LINEAR_EXTRUSION") => record
                .parameter(1)
                .and_then(Value::reference)
                .map(|curve| CurveId(format!("step:data:curve#{curve}")))
                .filter(|curve| curve_ids.contains(curve))
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
                .map(|curve| CurveId(format!("step:data:curve#{curve}")))
                .filter(|curve| curve_ids.contains(curve))
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
                .filter(|(_, angle)| angle.is_finite() && *angle > 0.0)
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

    let base_surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| (surface.id.clone(), surface.geometry.clone()))
        .collect::<BTreeMap<_, _>>();
    let decoded_curves = ir
        .model
        .curves
        .iter()
        .map(|curve| curve.id.clone())
        .collect::<BTreeSet<_>>();
    for (id, record) in exchange.entities("CURVE_BOUNDED_SURFACE") {
        if record.simple_name() != Some("CURVE_BOUNDED_SURFACE") {
            continue;
        }
        let support = record
            .parameter(1)
            .and_then(Value::reference)
            .map(|support| SurfaceId(format!("step:data:surface#{support}")));
        let boundaries = record.parameter(2).and_then(references).map(|boundaries| {
            boundaries
                .into_iter()
                .map(|boundary| CurveId(format!("step:data:curve#{boundary}")))
                .collect::<Vec<_>>()
        });
        let implicit_outer = record.parameter(3).and_then(Value::logical);
        let Some((support, boundaries, implicit_outer, geometry)) = support
            .as_ref()
            .and_then(|support| base_surfaces.get(support).cloned())
            .zip(support)
            .zip(boundaries)
            .zip(implicit_outer)
            .map(|(((geometry, support), boundaries), implicit_outer)| {
                (support, boundaries, implicit_outer, geometry)
            })
            .filter(|(_, boundaries, _, _)| {
                !boundaries.is_empty()
                    && boundaries
                        .iter()
                        .all(|curve| decoded_curves.contains(curve))
            })
        else {
            warnings.push(format!(
                "CURVE_BOUNDED_SURFACE #{id} has unresolved support or boundaries"
            ));
            continue;
        };
        let surface = SurfaceId(format!("step:data:surface#{id}"));
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
        typed.insert(id);
    }

    let surface_ids = ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.clone())
        .collect::<BTreeSet<_>>();
    for (id, record) in exchange.entities("OFFSET_SURFACE") {
        if record.simple_name() != Some("OFFSET_SURFACE") {
            continue;
        }
        let support = record
            .parameter(1)
            .and_then(Value::reference)
            .map(|support| SurfaceId(format!("step:data:surface#{support}")))
            .filter(|support| surface_ids.contains(support));
        let distance = record.parameter(2).and_then(Value::number);
        let self_intersect = record
            .parameter(3)
            .and_then(logical_value)
            .map(StepLogical::into_option);
        let Some((support, distance, self_intersect)) = support
            .zip(distance)
            .zip(self_intersect)
            .map(|((support, distance), self_intersect)| (support, distance, self_intersect))
        else {
            warnings.push(format!("OFFSET_SURFACE #{id} has invalid parameters"));
            continue;
        };
        let surface = SurfaceId(format!("step:data:surface#{id}"));
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
        typed.insert(id);
    }

    let decoded_surfaces = ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.clone())
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
        let curve_step = representation
            .and_then(|representation| representation.parameter(1))
            .and_then(Value::list)
            .and_then(|items| items.first())
            .and_then(Value::reference);
        let surface = surface_step.map(|surface| SurfaceId(format!("step:data:surface#{surface}")));
        let Some(geometry) = surface
            .filter(|surface| decoded_surfaces.contains(surface))
            .and_then(|_| curve_step.and_then(|curve| pcurve_geometries.get(&curve).cloned()))
        else {
            warnings.push(format!("PCURVE #{id} has no decoded surface or 2D curve"));
            continue;
        };
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
        if let Some(curve) = curve_step {
            typed.insert(curve);
        }
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
        if !ir
            .model
            .surfaces
            .iter()
            .any(|candidate| candidate.id == surface)
        {
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
        placements,
        length_scale: scale,
        plane_angle_scale: angle_scale,
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
) -> Option<f64> {
    match value {
        Value::Integer(value) => {
            Some(parameter_scale(geometry, angle_scale, linear_parameter_scale) * *value as f64)
        }
        Value::Real(value) => {
            Some(parameter_scale(geometry, angle_scale, linear_parameter_scale) * *value)
        }
        Value::Typed(_, value) => {
            trim_parameter(value, points, geometry, angle_scale, linear_parameter_scale)
        }
        Value::Reference(id) => points
            .get(id)
            .and_then(|point| curve_parameter_at_point(geometry, *point)),
        Value::List(values) => values.iter().find_map(|value| {
            trim_parameter(value, points, geometry, angle_scale, linear_parameter_scale)
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

fn curve_parameter_at_point(geometry: &CurveGeometry, point: Point3) -> Option<f64> {
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

fn composite_curve(
    record: &RawRecord,
    exchange: &Exchange,
    decoded: &BTreeSet<CurveId>,
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
            let curve = CurveId(format!(
                "step:data:curve#{}",
                record.parameter(2)?.reference()?
            ));
            decoded.contains(&curve).then_some((
                id,
                CompositeCurveSegment {
                    curve,
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

fn composite_dependencies(record: &RawRecord, exchange: &Exchange) -> Option<Vec<u64>> {
    let complex = record.partials.len() > 1;
    let composite = record.partial("COMPOSITE_CURVE")?;
    let offset = usize::from(!complex);
    composite
        .parameters
        .get(offset)?
        .list()?
        .iter()
        .map(|value| {
            let segment = exchange.records.get(&value.reference()?)?;
            (segment.simple_name() == Some("COMPOSITE_CURVE_SEGMENT"))
                .then(|| segment.parameter(2)?.reference())?
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
