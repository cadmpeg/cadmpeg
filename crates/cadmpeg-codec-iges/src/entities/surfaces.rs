// SPDX-License-Identifier: Apache-2.0
//! Analytic and free-form surface projection.

use super::geometry::{
    declared_unit_vector, entity_loss, resolve_transform, source_object, ProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::{refuse_local_limit, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    derive_reference_direction, knots_nondecreasing, Curve, CurveGeometry, NurbsCurve,
    NurbsSurface, ProceduralSurface, ProceduralSurfaceDefinition, SplineSurfaceParameters, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

const MAX_SURFACE_POLES: usize = 1_000_000;

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let length = vector.norm();
    (length.is_finite() && length > 0.0).then(|| vector.scale(1.0 / length))
}

fn similarity_orientation(transform: super::geometry::Affine) -> Option<f64> {
    let column = |index| {
        Vector3::new(
            transform.rows[0][index],
            transform.rows[1][index],
            transform.rows[2][index],
        )
    };
    let [x, y, z] = [column(0), column(1), column(2)];
    let squared_scale = x.dot(x);
    if !squared_scale.is_finite() || squared_scale <= 0.0 {
        return None;
    }
    let tolerance = squared_scale * 1.0e-10;
    if (y.dot(y) - squared_scale).abs() > tolerance
        || (z.dot(z) - squared_scale).abs() > tolerance
        || x.dot(y).abs() > tolerance
        || x.dot(z).abs() > tolerance
        || y.dot(z).abs() > tolerance
    {
        return None;
    }
    let determinant = x.dot(y.cross(z));
    let determinant_tolerance = squared_scale.sqrt() * squared_scale * 1.0e-10;
    (determinant.is_finite() && determinant.abs() > determinant_tolerance)
        .then(|| determinant.signum())
}

fn bounded_nurbs(
    ir: &CadIr,
    sequence: u32,
    ctx: Option<&DecodeContext<'_>>,
) -> Option<(NurbsCurve, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    super::composite::bounded_nurbs_for_curve(ir, &curve_id, ctx, None)
}

fn reverse_knots(knots: &[f64]) -> Option<Vec<f64>> {
    let first = *knots.first()?;
    let last = *knots.last()?;
    Some(knots.iter().rev().map(|knot| first + last - knot).collect())
}

fn rotate(vector: Vector3, axis: Vector3, angle: f64) -> Vector3 {
    let cosine = angle.cos();
    let sine = angle.sin();
    let parallel = axis.scale(axis.dot(vector));
    let perpendicular = vector - parallel;
    let tangent = axis.cross(perpendicular);
    parallel + perpendicular.scale(cosine) + tangent.scale(sine)
}

struct AngularBasis {
    knots: Vec<f64>,
    controls: Vec<(f64, f64)>,
}

fn angular_basis(start: f64, end: f64) -> Option<AngularBasis> {
    let sweep = end - start;
    if !sweep.is_finite()
        || sweep <= 0.0
        || sweep > std::f64::consts::TAU + super::curve_conversion::ANGULAR_TOLERANCE
    {
        return None;
    }
    let sweep = sweep.min(std::f64::consts::TAU);
    let end = start + sweep;
    let segment_count = super::curve_conversion::quarter_turn_spans(sweep);
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

fn offset_analytic(geometry: &SurfaceGeometry, distance: f64) -> Option<SurfaceGeometry> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => Some(SurfaceGeometry::Plane {
            origin: origin.translated(*normal, distance),
            normal: *normal,
            u_axis: *u_axis,
        }),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => Some(SurfaceGeometry::Cylinder {
            origin: *origin,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance,
        }),
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(SurfaceGeometry::Sphere {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance,
        }),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => Some(SurfaceGeometry::Torus {
            center: *center,
            axis: *axis,
            ref_direction: *ref_direction,
            major_radius: *major_radius,
            minor_radius: minor_radius + distance,
        }),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } if *ratio == 1.0 => Some(SurfaceGeometry::Cone {
            origin: origin.translated(*axis, -distance * half_angle.sin()),
            axis: *axis,
            ref_direction: *ref_direction,
            radius: radius + distance * half_angle.cos(),
            ratio: *ratio,
            half_angle: *half_angle,
        }),
        SurfaceGeometry::Cone { .. }
        | SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

fn offset_indicator_parameters(bounds: Option<[Option<f64>; 4]>) -> [f64; 2] {
    bounds
        .and_then(|bounds| match bounds {
            [Some(u0), Some(u1), Some(v0), Some(v1)] => Some([u0.midpoint(u1), v0.midpoint(v1)]),
            _ => None,
        })
        .unwrap_or([0.0, 0.0])
}

fn indicator_normal(ir: &CadIr, surface: &SurfaceId) -> Option<Vector3> {
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == *surface);
    let parameters =
        procedural.map(|procedural| offset_indicator_parameters(procedural.record_bounds));
    let parameters = parameters.unwrap_or([0.0, 0.0]);
    let partials = match procedural {
        Some(_) => {
            let index = cadmpeg_ir::index::ModelIndex::new(ir);
            cadmpeg_ir::eval::model_surface_partials_by_id(
                &index,
                surface,
                parameters[0],
                parameters[1],
            )?
        }
        None => {
            // A support with no procedural entry takes `model_surface_mapping`'s
            // direct arm: `surface_partials` on the carrier geometry with zero
            // offset and unit scales. Building the whole `ModelIndex` to serve
            // that one arena lookup is the bulk of this function's cost, so the
            // carrier is resolved here instead. The reverse scan is deliberate:
            // the index maps an arena through a `HashMap` where a repeated
            // identity is won by the last entry, and directory sequence numbers
            // come straight from the card, so duplicate ids are not excluded.
            let carrier = ir
                .model
                .surfaces
                .iter()
                .rev()
                .find(|carrier| carrier.id == *surface)?;
            cadmpeg_ir::eval::surface_partials(&carrier.geometry, parameters[0], parameters[1])?
        }
    };
    unit_vector(partials.du.cross(partials.dv))
}

fn indicator_orientation(
    record: &ParameterRecord,
    indicator: Vector3,
    normal: Vector3,
    global: &ProjectedGlobal,
) -> Option<f64> {
    let precision = global.real_precision();
    let values = [indicator.x, indicator.y, indicator.z];
    let contains = |candidate: Vector3| {
        [candidate.x, candidate.y, candidate.z]
            .into_iter()
            .enumerate()
            .all(|(offset, component)| {
                super::geometry::DeclaredInterval::around(
                    values[offset],
                    record.number_uncertainty(offset + 1, values[offset], precision),
                )
                .contains(component)
            })
    };
    if contains(normal) {
        Some(1.0)
    } else if contains(normal.scale(-1.0)) {
        Some(-1.0)
    } else {
        None
    }
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<ProjectionOutcome, CodecError> {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 108 && matches!(entry.form, -1..=1))
    {
        let factor = global.length_factor_mm();
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
        let Some(local_normal_unit) = unit_vector(local_normal) else {
            losses.push(entity_loss(entry, "plane normal cannot be normalized"));
            continue;
        };
        let local_u = derive_reference_direction(local_normal_unit);
        let local_v = local_normal_unit.cross(local_u);
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
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            Ok(transform) => transform,
            Err(message) => {
                losses.push(entity_loss(entry, message));
                continue;
            }
        };
        let Some(u_axis) = unit_vector(transform.vector(local_u)) else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its u direction",
            ));
            continue;
        };
        let Some(v_axis) = unit_vector(transform.vector(local_v)) else {
            losses.push(entity_loss(
                entry,
                "plane placement collapses its v direction",
            ));
            continue;
        };
        let Some(normal) = unit_vector(u_axis.cross(v_axis)) else {
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
            bounded_nurbs(ir, first_sequence, ctx),
            bounded_nurbs(ir, second_sequence, ctx),
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
            record_bounds: Some([
                Some(first_interval[0]),
                Some(first_interval[1]),
                Some(second_interval[0]),
                Some(second_interval[1]),
            ]),
        });
        losses.push(
            IgesLossCode::RuledDevelopabilityNotTransferred
                .note("Type 118 developability is retained only in the native entity record")
                .with_provenance(entry.loss_provenance()),
        );
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 122 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
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
        let Some((directrix, interval)) = bounded_nurbs(ir, directrix_sequence, ctx) else {
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
        let direction = target.vector_from(start);
        if !direction.norm().is_finite() || direction.norm() <= 0.0 {
            losses.push(entity_loss(entry, "generatrix is zero or non-finite"));
            continue;
        }
        let control_points = directrix
            .control_points
            .iter()
            .flat_map(|point| [*point, point.translated(direction, 1.0)])
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
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some([Some(interval[0]), Some(interval[1]), None, None]),
        });
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 120 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
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
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
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
        let Some((generatrix, parameter_interval)) = bounded_nurbs(ir, generatrix_sequence, ctx)
        else {
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
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                u64::MAX,
                None,
            ));
        };
        if surface_pole_count > MAX_SURFACE_POLES {
            return Err(refuse_local_limit(
                "iges_revolution_poles",
                MAX_SURFACE_POLES as u64,
                surface_pole_count as u64,
                None,
            ));
        }
        let mut control_points = Vec::with_capacity(surface_pole_count);
        let mut weights = Vec::with_capacity(control_points.capacity());
        for (u_index, point) in generatrix.control_points.iter().enumerate() {
            let delta = point.vector_from(axis_origin);
            let axis_point = axis_origin.translated(axis_direction, delta.dot(axis_direction));
            let radial = point.vector_from(axis_point);
            let u_weight = generatrix
                .weights
                .as_ref()
                .and_then(|values| values.get(u_index))
                .copied()
                .unwrap_or(1.0);
            for (angle, angular_weight) in &angular_controls {
                let rotated = rotate(radial, axis_direction, *angle);
                let radial_control = rotated.scale(1.0 / angular_weight);
                control_points.push(transform.point(axis_point.translated(radial_control, 1.0)));
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
                v_periodic: super::curve_conversion::angularly_equal(
                    end_angle - start_angle,
                    std::f64::consts::TAU,
                ),
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
            let Some(direction) = unit_vector(transform.vector(axis_direction)) else {
                losses.push(entity_loss(
                    entry,
                    "placement collapses the revolution axis",
                ));
                continue;
            };
            procedural_axis_direction = direction.scale(orientation);
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
                    angular_parameter_interval: None,
                    parameter_interval: Some(parameter_interval),
                    transposed: false,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: Some([
                    Some(parameter_interval[0]),
                    Some(parameter_interval[1]),
                    None,
                    None,
                ]),
            });
        }
        decoded.insert(entry.sequence);
    }

    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 128 && (0..=9).contains(&entry.form))
    {
        let factor = global.length_factor_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let indices = [record.integer(1), record.integer(2)];
        let degrees = [record.integer(3), record.integer(4)];
        let [Some(raw_k1), Some(raw_k2)] = indices else {
            losses.push(entity_loss(
                entry,
                "surface upper indices K1 or K2 are invalid",
            ));
            continue;
        };
        let [Some(k1), Some(k2)] = [raw_k1, raw_k2].map(|value| usize::try_from(value).ok()) else {
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
        let requested = u64::try_from(raw_k1)
            .ok()
            .and_then(|value| value.checked_add(1))
            .and_then(|u_count| {
                u64::try_from(raw_k2)
                    .ok()
                    .and_then(|value| value.checked_add(1))
                    .and_then(|v_count| u_count.checked_mul(v_count))
            });
        match requested {
            None => {
                return Err(refuse_local_limit(
                    "iges_surface_poles",
                    MAX_SURFACE_POLES as u64,
                    u64::MAX,
                    None,
                ));
            }
            Some(requested) if requested > MAX_SURFACE_POLES as u64 => {
                return Err(refuse_local_limit(
                    "iges_surface_poles",
                    MAX_SURFACE_POLES as u64,
                    requested,
                    None,
                ));
            }
            Some(_) => {}
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
            return Err(refuse_local_limit(
                "iges_surface_poles",
                MAX_SURFACE_POLES as u64,
                pole_count as u64,
                None,
            ));
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
        if !knots_nondecreasing(&u_knots) || !knots_nondecreasing(&v_knots) {
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
        let precision = global.real_precision();
        let clamp_range =
            |start_index: usize, values: [f64; 2], domain: [f64; 2]| -> Option<[f64; 2]> {
                let mut clamped = values;
                for (offset, bound) in clamped.iter_mut().enumerate() {
                    let uncertainty =
                        record.number_uncertainty(start_index + offset, *bound, precision);
                    if *bound < domain[0]
                        && super::geometry::DeclaredInterval::around(*bound, uncertainty)
                            .contains(domain[0])
                    {
                        *bound = domain[0];
                    } else if *bound > domain[1]
                        && super::geometry::DeclaredInterval::around(*bound, uncertainty)
                            .contains(domain[1])
                    {
                        *bound = domain[1];
                    }
                }
                (clamped[0] < clamped[1] && clamped[0] >= domain[0] && clamped[1] <= domain[1])
                    .then_some(clamped)
            };
        let Some(u_range) = clamp_range(
            range_start,
            [ranges[0], ranges[1]],
            [u_knots[u_degree_usize], u_knots[u_count]],
        ) else {
            losses.push(entity_loss(
                entry,
                "u parameter range is empty or lies outside its knot domain",
            ));
            continue;
        };
        let Some(v_range) = clamp_range(
            range_start + 2,
            [ranges[2], ranges[3]],
            [v_knots[v_degree_usize], v_knots[v_count]],
        ) else {
            losses.push(entity_loss(
                entry,
                "v parameter range is empty or lies outside its knot domain",
            ));
            continue;
        };
        let transform = match resolve_transform(
            entry.transform,
            &entries,
            &records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
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
        let surface_id = SurfaceId(format!("iges:model:surface#D{}", entry.sequence));
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
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
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId(format!("iges:model:procedural-surface#D{}", entry.sequence)),
            surface: surface_id,
            definition: ProceduralSurfaceDefinition::Exact {
                parameters: SplineSurfaceParameters::OrderedRanges {
                    ranges: [u_range, v_range],
                },
                extension: 0,
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: Some([
                Some(u_range[0]),
                Some(u_range[1]),
                Some(v_range[0]),
                Some(v_range[1]),
            ]),
        });
        decoded.insert(entry.sequence);
    }

    // No `ModelIndex` can be hoisted out of this loop: every accepted offset
    // surface appends to `ir.model`, and an offset may serve as the support
    // of a later one in the same pass, so an index built up front would miss
    // surfaces that must be resolvable by the time they are referenced.
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 140 && entry.form == 0)
    {
        let factor = global.length_factor_mm();
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
        if !declared_unit_vector(record, 1, indicator, global.real_precision()) {
            losses.push(entity_loss(entry, "offset indicator is not a unit vector"));
            continue;
        }
        let indicator = unit_vector(indicator).expect("validated nonzero finite offset indicator");
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
        let Some(normal) = indicator_normal(ir, &support_id) else {
            losses.push(entity_loss(
                entry,
                "support normal cannot be evaluated at the offset-indicator parameters",
            ));
            continue;
        };
        let Some(orientation) = indicator_orientation(record, indicator, normal, global) else {
            losses.push(entity_loss(
                entry,
                "offset indicator is not the support normal at the designated parameters",
            ));
            continue;
        };
        let signed_distance = distance * orientation;
        let Some(geometry) = offset_analytic(&support.geometry, signed_distance) else {
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
            | SurfaceGeometry::Procedural { .. }
            | SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Transformed { .. }
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

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
