// SPDX-License-Identifier: Apache-2.0
//! Analytic and free-form surface projection.

use super::curve_conversion::circular_arc_nurbs;
use super::geometry::{entity_loss, resolve_transform, source_object};
use crate::directory::DirectoryEntry;
use crate::global::Global;
use crate::parameter::ParameterRecord;
use cadmpeg_ir::geometry::{
    derive_reference_direction, Curve, CurveGeometry, NurbsCurve, NurbsSurface, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SURFACE_POLES: usize = 1_000_000;

fn similarity_orientation(transform: super::geometry::Affine) -> Option<f64> {
    let column = |index| {
        Vector3::new(
            transform.rows[0][index],
            transform.rows[1][index],
            transform.rows[2][index],
        )
    };
    let [x, y, z] = [column(0), column(1), column(2)];
    let squared_scale = dot(x, x);
    if !squared_scale.is_finite() || squared_scale <= 0.0 {
        return None;
    }
    let tolerance = squared_scale * 1.0e-10;
    if (dot(y, y) - squared_scale).abs() > tolerance
        || (dot(z, z) - squared_scale).abs() > tolerance
        || dot(x, y).abs() > tolerance
        || dot(x, z).abs() > tolerance
        || dot(y, z).abs() > tolerance
    {
        return None;
    }
    let determinant = dot(x, cross(y, z));
    let determinant_tolerance = squared_scale.sqrt() * squared_scale * 1.0e-10;
    (determinant.is_finite() && determinant.abs() > determinant_tolerance)
        .then(|| determinant.signum())
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn normalized(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

fn point_for_vertex(ir: &CadIr, id: &cadmpeg_ir::ids::VertexId) -> Option<Point3> {
    let point_id = &ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *id)?
        .point;
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(|point| point.position)
}

fn bounded_nurbs(ir: &CadIr, sequence: u32) -> Option<(NurbsCurve, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    let curve = ir.model.curves.iter().find(|curve| curve.id == curve_id)?;
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.curve.as_ref() == Some(&curve_id))?;
    let interval = edge.param_range?;
    match &curve.geometry {
        CurveGeometry::Nurbs(nurbs) => Some((nurbs.clone(), interval)),
        CurveGeometry::Line { .. } => Some((
            NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 1.0, 1.0],
                control_points: vec![
                    point_for_vertex(ir, &edge.start)?,
                    point_for_vertex(ir, &edge.end)?,
                ],
                weights: None,
                periodic: false,
            },
            [0.0, 1.0],
        )),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some((
            circular_arc_nurbs(*center, *axis, *ref_direction, *radius, interval)?,
            interval,
        )),
        _ => None,
    }
}

fn reverse_knots(knots: &[f64]) -> Option<Vec<f64>> {
    let first = *knots.first()?;
    let last = *knots.last()?;
    Some(knots.iter().rev().map(|knot| first + last - knot).collect())
}

fn dot(left: Vector3, right: Vector3) -> f64 {
    left.x * right.x + left.y * right.y + left.z * right.z
}

fn subtract(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(left.x - right.x, left.y - right.y, left.z - right.z)
}

fn scale(vector: Vector3, factor: f64) -> Vector3 {
    Vector3::new(vector.x * factor, vector.y * factor, vector.z * factor)
}

fn add_point_vector(point: Point3, vector: Vector3) -> Point3 {
    Point3::new(point.x + vector.x, point.y + vector.y, point.z + vector.z)
}

fn rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    let parallel = scale(axis, dot(axis, vector));
    let perpendicular = subtract(vector, parallel);
    let tangent = cross(axis, perpendicular);
    Vector3::new(
        parallel.x + cosine * perpendicular.x + sine * tangent.x,
        parallel.y + cosine * perpendicular.y + sine * tangent.y,
        parallel.z + cosine * perpendicular.z + sine * tangent.z,
    )
}

struct AngularBasis {
    knots: Vec<f64>,
    controls: Vec<(f64, f64)>,
}

fn angular_basis(start: f64, end: f64) -> Option<AngularBasis> {
    let sweep = end - start;
    let tolerance = std::f64::consts::TAU * 1.0e-12;
    if !sweep.is_finite() || sweep <= 0.0 || sweep > std::f64::consts::TAU + tolerance {
        return None;
    }
    let sweep = sweep.min(std::f64::consts::TAU);
    let end = start + sweep;
    let segment_count = (sweep / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let segment_angle = sweep / segment_count as f64;
    let mut knots = vec![start; 3];
    let mut controls = Vec::with_capacity(segment_count * 2 + 1);
    controls.push((start, 1.0));
    for segment in 0..segment_count {
        let segment_start = start + segment as f64 * segment_angle;
        let midpoint = segment_start + segment_angle / 2.0;
        let segment_end = segment_start + segment_angle;
        controls.push((midpoint, (segment_angle / 2.0).cos()));
        controls.push((segment_end, 1.0));
        if segment + 1 < segment_count {
            knots.extend([segment_end; 2]);
        }
    }
    knots.extend([end; 3]);
    Some(AngularBasis { knots, controls })
}

fn offset_analytic(
    geometry: &SurfaceGeometry,
    indicator: Vector3,
    distance: f64,
) -> Option<(SurfaceGeometry, f64)> {
    let (normal, solved) = match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => (
            *normal,
            SurfaceGeometry::Plane {
                origin: add_point_vector(*origin, scale(*normal, distance)),
                normal: *normal,
                u_axis: *u_axis,
            },
        ),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => (
            *ref_direction,
            SurfaceGeometry::Cylinder {
                origin: *origin,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: radius + distance,
            },
        ),
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => (
            *ref_direction,
            SurfaceGeometry::Sphere {
                center: *center,
                axis: *axis,
                ref_direction: *ref_direction,
                radius: radius + distance,
            },
        ),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => (
            *ref_direction,
            SurfaceGeometry::Torus {
                center: *center,
                axis: *axis,
                ref_direction: *ref_direction,
                major_radius: *major_radius,
                minor_radius: minor_radius + distance,
            },
        ),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if *ratio == 1.0 => {
            let normal = Vector3::new(
                half_angle.cos() * ref_direction.x - half_angle.sin() * axis.x,
                half_angle.cos() * ref_direction.y - half_angle.sin() * axis.y,
                half_angle.cos() * ref_direction.z - half_angle.sin() * axis.z,
            );
            (
                normal,
                SurfaceGeometry::Cone {
                    origin: add_point_vector(*origin, scale(*axis, -distance * half_angle.sin())),
                    axis: *axis,
                    ref_direction: *ref_direction,
                    radius: radius + distance * half_angle.cos(),
                    ratio: *ratio,
                    half_angle: *half_angle,
                },
            )
        }
        SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => return None,
    };
    if dot(normal, indicator) >= 0.0 {
        Some((solved, distance))
    } else {
        offset_analytic(geometry, scale(indicator, -1.0), -distance)
    }
}

pub(super) struct SurfaceProjection {
    pub(super) handled: BTreeSet<u32>,
    pub(super) decoded: BTreeSet<u32>,
    pub(super) losses: Vec<LossNote>,
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &Global,
) -> SurfaceProjection {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut handled = BTreeSet::new();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 108 && matches!(entry.form, -1..=1))
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let coefficients = [
            record.number(1),
            record.number(2),
            record.number(3),
            record.number(4),
        ];
        let [Some(a), Some(b), Some(c), Some(d)] = coefficients else {
            losses.push(entity_loss(entry, "plane coefficients are not numeric"));
            continue;
        };
        if coefficients
            .into_iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            losses.push(entity_loss(entry, "plane coefficients are not finite"));
            continue;
        }
        let Some(boundary) = record.integer(5) else {
            losses.push(entity_loss(
                entry,
                "plane boundary pointer is not an integer",
            ));
            continue;
        };
        if (entry.form == 0 && boundary != 0)
            || (entry.form != 0 && (boundary <= 0 || boundary % 2 == 0))
        {
            losses.push(entity_loss(
                entry,
                "plane form and boundary pointer are inconsistent",
            ));
            continue;
        }
        let local_normal = Vector3::new(a, b, c);
        let normal_squared = a * a + b * b + c * c;
        if !normal_squared.is_finite() || normal_squared <= 0.0 {
            losses.push(entity_loss(entry, "plane normal is degenerate"));
            continue;
        }
        let Some(local_normal_unit) = normalized(local_normal) else {
            losses.push(entity_loss(entry, "plane normal cannot be normalized"));
            continue;
        };
        let local_u = derive_reference_direction(local_normal_unit);
        let local_v = cross(local_normal_unit, local_u);
        let local_origin = Point3::new(
            a * d / normal_squared * factor,
            b * d / normal_squared * factor,
            c * d / normal_squared * factor,
        );
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let Some(u_axis) = normalized(transform.vector(local_u)) else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its u direction",
            ));
            continue;
        };
        let Some(v_axis) = normalized(transform.vector(local_v)) else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its v direction",
            ));
            continue;
        };
        let Some(normal) = normalized(cross(u_axis, v_axis)) else {
            losses.push(entity_loss(entry, "plane placement collapses its normal"));
            continue;
        };
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: SurfaceGeometry::Plane {
                origin: transform.point(local_origin),
                normal,
                u_axis,
            },
            source_object: Some(source_object(entry)),
        });
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 118 && matches!(entry.form, 0 | 1))
    {
        handled.insert(entry.sequence);
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(first_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "first rail pointer is invalid"));
            continue;
        };
        let Some(second_sequence) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "second rail pointer is invalid"));
            continue;
        };
        let (Some(direction_flag), Some(developable_flag)) = (record.integer(3), record.integer(4))
        else {
            losses.push(entity_loss(entry, "ruled-surface flags are not integers"));
            continue;
        };
        if !matches!(direction_flag, 0 | 1) || !matches!(developable_flag, 0 | 1) {
            losses.push(entity_loss(entry, "ruled-surface flags are not 0 or 1"));
            continue;
        }
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed ruled surfaces require transformed child-carrier projection",
            ));
            continue;
        }
        let (Some((first, first_interval)), Some((mut second, second_interval))) = (
            bounded_nurbs(ir, first_sequence),
            bounded_nurbs(ir, second_sequence),
        ) else {
            losses.push(entity_loss(
                entry,
                "rail curves do not have bounded polynomial or NURBS carriers",
            ));
            continue;
        };
        if first.weights.is_some() || second.weights.is_some() {
            losses.push(entity_loss(
                entry,
                "rational ruled rails require homogeneous denominator reconciliation",
            ));
            continue;
        }
        if entry.form == 0
            && (first.degree != 1
                || second.degree != 1
                || first.control_points.len() != 2
                || second.control_points.len() != 2)
        {
            losses.push(entity_loss(
                entry,
                "equal-arc-length ruled projection is implemented only for linear rails",
            ));
            continue;
        }
        if direction_flag == 1 {
            second.control_points.reverse();
            let Some(knots) = reverse_knots(&second.knots) else {
                losses.push(entity_loss(entry, "second rail knot vector is empty"));
                continue;
            };
            second.knots = knots;
        }
        if first.degree != second.degree
            || first.knots != second.knots
            || first.control_points.len() != second.control_points.len()
        {
            losses.push(entity_loss(
                entry,
                "ruled rails do not share one exact polynomial basis",
            ));
            continue;
        }
        let Ok(u_count) = u32::try_from(first.control_points.len()) else {
            losses.push(entity_loss(entry, "ruled rail pole count exceeds u32"));
            continue;
        };
        let control_points = first
            .control_points
            .iter()
            .copied()
            .zip(second.control_points.iter().copied())
            .flat_map(|(first, second)| [first, second])
            .collect::<Vec<_>>();
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: first.degree,
                v_degree: 1,
                u_knots: first.knots,
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count,
                v_count: 2,
                control_points,
                weights: None,
                u_periodic: first.periodic && second.periodic,
                v_periodic: false,
            }),
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Ruled {
                first: CurveId(format!("iges:model:curve#D{first_sequence}")),
                second: CurveId(format!("iges:model:curve#D{second_sequence}")),
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        let _ = (first_interval, second_interval, developable_flag);
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 122 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(directrix_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "directrix pointer is invalid"));
            continue;
        };
        let coordinates = [record.number(2), record.number(3), record.number(4)];
        let [Some(x), Some(y), Some(z)] = coordinates else {
            losses.push(entity_loss(entry, "generatrix endpoint is not numeric"));
            continue;
        };
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed tabulated cylinders require transformed directrix projection",
            ));
            continue;
        }
        let Some((directrix, interval)) = bounded_nurbs(ir, directrix_sequence) else {
            losses.push(entity_loss(
                entry,
                "directrix has no bounded polynomial or NURBS carrier",
            ));
            continue;
        };
        let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
            directrix.degree,
            &directrix.knots,
            &directrix.control_points,
            directrix.weights.as_deref(),
            interval[0],
        ) else {
            losses.push(entity_loss(entry, "directrix start cannot be evaluated"));
            continue;
        };
        let target = Point3::new(x * factor, y * factor, z * factor);
        let direction = Vector3::new(target.x - start.x, target.y - start.y, target.z - start.z);
        if !direction.norm().is_finite() || direction.norm() <= 0.0 {
            losses.push(entity_loss(entry, "generatrix is zero or non-finite"));
            continue;
        }
        let control_points = directrix
            .control_points
            .iter()
            .flat_map(|point| {
                [
                    *point,
                    Point3::new(
                        point.x + direction.x,
                        point.y + direction.y,
                        point.z + direction.z,
                    ),
                ]
            })
            .collect::<Vec<_>>();
        let Ok(u_count) = u32::try_from(directrix.control_points.len()) else {
            losses.push(entity_loss(entry, "directrix pole count exceeds u32"));
            continue;
        };
        let weights = directrix.weights.as_ref().map(|weights| {
            weights
                .iter()
                .flat_map(|weight| [*weight, *weight])
                .collect()
        });
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: directrix.degree,
                v_degree: 1,
                u_knots: directrix.knots,
                v_knots: vec![0.0, 0.0, 1.0, 1.0],
                u_count,
                v_count: 2,
                control_points,
                weights,
                u_periodic: directrix.periodic,
                v_periodic: false,
            }),
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Extrusion {
                directrix: CurveId(format!("iges:model:curve#D{directrix_sequence}")),
                parameter_interval: Some(interval),
                direction,
                native_position: Some(target),
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 120 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(axis_sequence) = record
            .integer(1)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "revolution axis pointer is invalid"));
            continue;
        };
        let Some(generatrix_sequence) = record
            .integer(2)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(
                entry,
                "revolution generatrix pointer is invalid",
            ));
            continue;
        };
        let (Some(start_angle), Some(end_angle)) = (record.number(3), record.number(4)) else {
            losses.push(entity_loss(entry, "revolution angles are not numeric"));
            continue;
        };
        let Some(AngularBasis {
            knots: v_knots,
            controls: angular_controls,
        }) = angular_basis(start_angle, end_angle)
        else {
            losses.push(entity_loss(
                entry,
                "revolution angular interval is not in (0, 2*pi]",
            ));
            continue;
        };
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let axis_id = CurveId(format!("iges:model:curve#D{axis_sequence}"));
        let Some(axis_curve) = ir.model.curves.iter().find(|curve| curve.id == axis_id) else {
            losses.push(entity_loss(entry, "revolution axis carrier is missing"));
            continue;
        };
        let CurveGeometry::Line {
            origin: axis_origin,
            direction: axis_direction,
        } = axis_curve.geometry
        else {
            losses.push(entity_loss(
                entry,
                "revolution axis is not a Line Entity carrier",
            ));
            continue;
        };
        let Some((generatrix, parameter_interval)) = bounded_nurbs(ir, generatrix_sequence) else {
            losses.push(entity_loss(
                entry,
                "generatrix has no bounded polynomial or NURBS carrier",
            ));
            continue;
        };
        let Ok(u_count) = u32::try_from(generatrix.control_points.len()) else {
            losses.push(entity_loss(entry, "generatrix pole count exceeds u32"));
            continue;
        };
        let Ok(v_count) = u32::try_from(angular_controls.len()) else {
            losses.push(entity_loss(entry, "angular pole count exceeds u32"));
            continue;
        };
        let Some(surface_pole_count) = generatrix
            .control_points
            .len()
            .checked_mul(angular_controls.len())
        else {
            losses.push(entity_loss(entry, "revolution pole count overflows"));
            continue;
        };
        if surface_pole_count > MAX_SURFACE_POLES {
            losses.push(entity_loss(
                entry,
                format!("revolution exceeds the {MAX_SURFACE_POLES}-pole limit"),
            ));
            continue;
        }
        let mut control_points = Vec::with_capacity(surface_pole_count);
        let mut weights = Vec::with_capacity(control_points.capacity());
        for (u_index, point) in generatrix.control_points.iter().enumerate() {
            let delta = Vector3::new(
                point.x - axis_origin.x,
                point.y - axis_origin.y,
                point.z - axis_origin.z,
            );
            let axis_point = add_point_vector(
                axis_origin,
                scale(axis_direction, dot(delta, axis_direction)),
            );
            let radial = Vector3::new(
                point.x - axis_point.x,
                point.y - axis_point.y,
                point.z - axis_point.z,
            );
            let u_weight = generatrix
                .weights
                .as_ref()
                .and_then(|values| values.get(u_index))
                .copied()
                .unwrap_or(1.0);
            for (angle, angular_weight) in &angular_controls {
                let rotated = rotate(radial, axis_direction, *angle);
                let radial_control = scale(rotated, 1.0 / angular_weight);
                control_points.push(transform.point(add_point_vector(axis_point, radial_control)));
                weights.push(u_weight * angular_weight);
            }
        }
        let placed_generatrix = (entry.transform != 0).then(|| generatrix.clone());
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: generatrix.degree,
                v_degree: 2,
                u_knots: generatrix.knots,
                v_knots,
                u_count,
                v_count,
                control_points,
                weights: Some(weights),
                u_periodic: generatrix.periodic,
                v_periodic: (end_angle - start_angle - std::f64::consts::TAU).abs() <= 1.0e-12,
            }),
            source_object: Some(source_object(entry)),
        });
        let mut procedural_directrix = CurveId(format!("iges:model:curve#D{generatrix_sequence}"));
        let mut procedural_axis_origin = axis_origin;
        let mut procedural_axis_direction = axis_direction;
        let procedural_is_exact = if entry.transform == 0 {
            true
        } else if let Some(orientation) = similarity_orientation(transform) {
            let mut placed_generatrix = placed_generatrix
                .expect("a transformed revolution retains its generatrix until placement");
            for point in &mut placed_generatrix.control_points {
                *point = transform.point(*point);
            }
            procedural_directrix = CurveId(format!(
                "iges:model:curve#D{}-placed-generatrix",
                entry.sequence
            ));
            ir.model.curves.push(Curve {
                id: procedural_directrix.clone(),
                geometry: CurveGeometry::Nurbs(placed_generatrix),
                source_object: Some(source_object(entry)),
            });
            procedural_axis_origin = transform.point(axis_origin);
            let Some(direction) = normalized(transform.vector(axis_direction)) else {
                losses.push(entity_loss(
                    entry,
                    "placement collapses the revolution axis",
                ));
                continue;
            };
            procedural_axis_direction = scale(direction, orientation);
            true
        } else {
            false
        };
        if procedural_is_exact {
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: ProceduralSurfaceId(format!(
                    "iges:model:procedural-surface#D{}",
                    entry.sequence
                )),
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Revolution {
                    directrix: procedural_directrix,
                    axis_origin: procedural_axis_origin,
                    axis_direction: procedural_axis_direction,
                    angular_interval: [start_angle, end_angle],
                    parameter_interval: Some(parameter_interval),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
        }
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 128 && (0..=9).contains(&entry.form))
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let indices = [record.integer(1), record.integer(2)];
        let degrees = [record.integer(3), record.integer(4)];
        let [Some(k1), Some(k2)] = indices.map(|value| value.and_then(|v| usize::try_from(v).ok()))
        else {
            losses.push(entity_loss(
                entry,
                "surface upper indices K1 or K2 are invalid",
            ));
            continue;
        };
        let [Some(u_degree), Some(v_degree)] =
            degrees.map(|value| value.and_then(|v| u32::try_from(v).ok()))
        else {
            losses.push(entity_loss(entry, "surface degrees M1 or M2 are invalid"));
            continue;
        };
        let [u_degree_usize, v_degree_usize] = [u_degree, v_degree].map(|degree| degree as usize);
        if u_degree == 0 || v_degree == 0 || k1 < u_degree_usize || k2 < v_degree_usize {
            losses.push(entity_loss(
                entry,
                "surface pole counts are smaller than their degrees plus one",
            ));
            continue;
        }
        let flags = (5..=9)
            .map(|index| record.integer(index))
            .collect::<Vec<_>>();
        if flags.iter().any(|flag| !matches!(flag, Some(0 | 1))) {
            losses.push(entity_loss(
                entry,
                "one or more surface flags are not 0 or 1",
            ));
            continue;
        }
        let (Some(u_count), Some(v_count)) = (k1.checked_add(1), k2.checked_add(1)) else {
            losses.push(entity_loss(entry, "surface pole count overflows"));
            continue;
        };
        let (Ok(u_count_u32), Ok(v_count_u32)) = (u32::try_from(u_count), u32::try_from(v_count))
        else {
            losses.push(entity_loss(entry, "surface pole dimensions exceed u32"));
            continue;
        };
        let Some(pole_count) = u_count.checked_mul(v_count) else {
            losses.push(entity_loss(entry, "surface pole grid size overflows"));
            continue;
        };
        if pole_count > MAX_SURFACE_POLES {
            losses.push(entity_loss(
                entry,
                format!("surface exceeds the {MAX_SURFACE_POLES}-pole limit"),
            ));
            continue;
        }
        let Some(u_knot_count) = u_count
            .checked_add(u_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "u-knot count overflows"));
            continue;
        };
        let Some(v_knot_count) = v_count
            .checked_add(v_degree_usize)
            .and_then(|value| value.checked_add(1))
        else {
            losses.push(entity_loss(entry, "v-knot count overflows"));
            continue;
        };
        let u_knot_start = 10_usize;
        let Some(v_knot_start) = u_knot_start.checked_add(u_knot_count) else {
            losses.push(entity_loss(entry, "v-knot offset overflows"));
            continue;
        };
        let Some(weight_start) = v_knot_start.checked_add(v_knot_count) else {
            losses.push(entity_loss(entry, "surface weight offset overflows"));
            continue;
        };
        let Some(pole_start) = weight_start.checked_add(pole_count) else {
            losses.push(entity_loss(entry, "surface pole offset overflows"));
            continue;
        };
        let Some(pole_value_count) = pole_count.checked_mul(3) else {
            losses.push(entity_loss(entry, "surface pole value count overflows"));
            continue;
        };
        let Some(range_start) = pole_start.checked_add(pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface parameter-range offset overflows",
            ));
            continue;
        };
        let collect_numbers = |start: usize, count: usize| -> Option<Vec<f64>> {
            (start..start.checked_add(count)?)
                .map(|index| record.number(index).filter(|value| value.is_finite()))
                .collect()
        };
        let Some(u_knots) = collect_numbers(u_knot_start, u_knot_count) else {
            losses.push(entity_loss(
                entry,
                "u-knot vector is truncated or non-finite",
            ));
            continue;
        };
        let Some(v_knots) = collect_numbers(v_knot_start, v_knot_count) else {
            losses.push(entity_loss(
                entry,
                "v-knot vector is truncated or non-finite",
            ));
            continue;
        };
        if u_knots.windows(2).any(|pair| pair[0] > pair[1])
            || v_knots.windows(2).any(|pair| pair[0] > pair[1])
        {
            losses.push(entity_loss(entry, "surface knot vector is decreasing"));
            continue;
        }
        let Some(native_weights) = collect_numbers(weight_start, pole_count) else {
            losses.push(entity_loss(
                entry,
                "surface weight vector is truncated or non-finite",
            ));
            continue;
        };
        if native_weights.iter().any(|weight| *weight <= 0.0) {
            losses.push(entity_loss(
                entry,
                "surface weights are not strictly positive",
            ));
            continue;
        }
        let equal_weights = native_weights
            .first()
            .is_some_and(|first| native_weights.iter().all(|weight| weight == first));
        let polynomial = flags[2] == Some(1);
        if polynomial && !equal_weights {
            losses.push(entity_loss(entry, "polynomial surface has unequal weights"));
            continue;
        }
        let Some(native_poles) = collect_numbers(pole_start, pole_value_count) else {
            losses.push(entity_loss(
                entry,
                "surface poles are truncated or non-finite",
            ));
            continue;
        };
        let Some(ranges) = collect_numbers(range_start, 4) else {
            losses.push(entity_loss(entry, "surface parameter ranges are missing"));
            continue;
        };
        if ranges[0] > ranges[1]
            || ranges[2] > ranges[3]
            || ranges[0] < u_knots[u_degree_usize]
            || ranges[1] > u_knots[u_count]
            || ranges[2] < v_knots[v_degree_usize]
            || ranges[3] > v_knots[v_count]
        {
            losses.push(entity_loss(
                entry,
                "surface parameter ranges lie outside their knot domains",
            ));
            continue;
        }
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            &mut BTreeSet::new(),
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let native_points = native_poles
            .chunks_exact(3)
            .map(|point| Point3::new(point[0] * factor, point[1] * factor, point[2] * factor))
            .collect::<Vec<_>>();
        let mut control_points = Vec::with_capacity(pole_count);
        let mut weights = (!polynomial).then(|| Vec::with_capacity(pole_count));
        for u in 0..u_count {
            for v in 0..v_count {
                let native_index = v * u_count + u;
                control_points.push(transform.point(native_points[native_index]));
                if let Some(weights) = &mut weights {
                    weights.push(native_weights[native_index]);
                }
            }
        }
        ir.model.surfaces.push(Surface {
            id: SurfaceId(format!("iges:model:surface#D{}", entry.sequence)),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree,
                v_degree,
                u_knots,
                v_knots,
                u_count: u_count_u32,
                v_count: v_count_u32,
                control_points,
                weights,
                u_periodic: flags[3] == Some(1),
                v_periodic: flags[4] == Some(1),
            }),
            source_object: Some(source_object(entry)),
        });
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 140 && entry.form == 0)
    {
        handled.insert(entry.sequence);
        let Some(factor) = global.length_factor_mm() else {
            losses.push(entity_loss(entry, "units or model scale are unsupported"));
            continue;
        };
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let components = [record.number(1), record.number(2), record.number(3)];
        let [Some(x), Some(y), Some(z)] = components else {
            losses.push(entity_loss(entry, "offset indicator is not numeric"));
            continue;
        };
        let indicator = Vector3::new(x, y, z);
        let indicator_norm = indicator.norm();
        if !indicator_norm.is_finite() || (indicator_norm - 1.0).abs() > 1.0e-10 {
            losses.push(entity_loss(entry, "offset indicator is not a unit vector"));
            continue;
        }
        let Some(distance) = record
            .number(4)
            .filter(|value| value.is_finite() && *value != 0.0)
        else {
            losses.push(entity_loss(entry, "offset distance is zero or non-finite"));
            continue;
        };
        let Some(support_sequence) = record
            .integer(5)
            .and_then(|value| u32::try_from(value).ok())
        else {
            losses.push(entity_loss(entry, "offset support pointer is invalid"));
            continue;
        };
        if entry.transform != 0 {
            losses.push(entity_loss(
                entry,
                "placed offset surfaces require transformed support projection",
            ));
            continue;
        }
        let support_id = SurfaceId(format!("iges:model:surface#D{support_sequence}"));
        let Some(support) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support_id)
        else {
            losses.push(entity_loss(entry, "offset support surface is missing"));
            continue;
        };
        let distance = distance * factor;
        let Some((geometry, signed_distance)) =
            offset_analytic(&support.geometry, indicator, distance)
        else {
            losses.push(entity_loss(
                entry,
                "support surface has no exact analytic offset carrier",
            ));
            continue;
        };
        let regular = match &geometry {
            SurfaceGeometry::Cylinder { radius, .. } | SurfaceGeometry::Sphere { radius, .. } => {
                *radius > 0.0
            }
            SurfaceGeometry::Torus {
                major_radius,
                minor_radius,
                ..
            } => *major_radius > 0.0 && *minor_radius > 0.0,
            SurfaceGeometry::Cone { radius, .. } => *radius > 0.0,
            SurfaceGeometry::Plane { .. } => true,
            SurfaceGeometry::Nurbs(_)
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Transformed { .. }
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Unknown { .. } => false,
        };
        if !regular {
            losses.push(entity_loss(
                entry,
                "offset collapses or reverses the analytic carrier",
            ));
            continue;
        }
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry,
            source_object: Some(source_object(entry)),
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Offset {
                support: support_id,
                distance: signed_distance,
                u_sense: Some(0),
                v_sense: Some(0),
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        decoded.insert(entry.sequence);
    }

    SurfaceProjection {
        handled,
        decoded,
        losses,
    }
}

#[cfg(test)]
mod tests {
    use super::angular_basis;

    #[test]
    fn angular_basis_canonicalizes_a_full_sweep_with_decimal_roundoff() {
        let basis = angular_basis(0.0, std::f64::consts::TAU + std::f64::consts::TAU * 5.0e-13)
            .expect("a near-full finite sweep has an exact rational basis");

        assert_eq!(basis.controls.len(), 9);
        assert_eq!(basis.knots.last(), Some(&std::f64::consts::TAU));
    }
}
