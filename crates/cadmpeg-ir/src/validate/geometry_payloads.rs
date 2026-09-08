// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.
#![allow(clippy::wildcard_imports)]

use super::*;
const EPS_SPATIAL_CURVE_DIRECTION: f64 = 1.0e-9;

const EPS_GEOMETRY_PAYLOADS_LAW_VALID_4_E10: f64 = 1.0e-10;

pub(super) fn check_tessellations(ir: &CadIr, findings: &mut Vec<Finding>) {
    for mesh in &ir.model.tessellations {
        if mesh.body.as_ref().is_some_and(|body| {
            !ir.model
                .bodies
                .iter()
                .any(|candidate| candidate.id == *body)
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation body".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .faces
            .iter()
            .any(|face| !ir.model.faces.iter().any(|candidate| candidate.id == *face))
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation face".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .chordal_deflection
            .is_some_and(|value| !value.is_finite() || value < 0.0)
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "has an invalid tessellation deflection".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .vertices()
            .iter()
            .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation vertex".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .normals()
            .iter()
            .chain(mesh.corner_normals())
            .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation normal".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh.texture_assignments().iter().any(|assignment| {
            !ir.model
                .assets
                .iter()
                .any(|asset| asset.id == assignment.texture)
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "references a missing tessellation texture asset".into(),
                entity: Some(mesh.id.clone()),
            });
        }
    }
}

pub(super) fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for (id, tolerance) in ir
        .model
        .vertices
        .iter()
        .map(|entity| (entity.id.as_str(), entity.tolerance))
        .chain(
            ir.model
                .edges
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
        .chain(
            ir.model
                .faces
                .iter()
                .map(|entity| (entity.id.as_str(), entity.tolerance)),
        )
    {
        if tolerance.is_some_and(nonpositive) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Error,
                message: "topology tolerance is not positive and finite".into(),
                entity: Some(id.to_owned()),
            });
        } else if tolerance.is_some_and(|value| value > 1.0e6) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Warning,
                message: "topology tolerance is outside a sane canonical range".into(),
                entity: Some(id.to_owned()),
            });
        }
    }
    for procedural in &ir.model.procedural_surfaces {
        if let ProceduralSurfaceDefinition::TSpline { construction } = procedural.definition() {
            if construction.subtransform.inline().is_none() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "T-spline surface subtransform is unresolved",
                );
            }
        }
    }
    for procedural in &ir.model.procedural_curves {
        if let ProceduralCurveDefinition::Offset {
            distance,
            side,
            range,
            ..
        } = procedural.definition()
        {
            let side_valid = match side {
                crate::geometry::OffsetSide::PlaneNormal(normal) => {
                    normal.x.is_finite()
                        && normal.y.is_finite()
                        && normal.z.is_finite()
                        && (normal.norm() - 1.0).abs() <= EPS_GEOMETRY_PAYLOADS_LAW_VALID_4_E10
                }
                crate::geometry::OffsetSide::Direction { direction, .. } => {
                    direction.x.is_finite()
                        && direction.y.is_finite()
                        && direction.z.is_finite()
                        && direction.norm() > 0.0
                }
            };
            let range_valid = range.as_ref().is_none_or(|range| {
                let parameter_range = match range {
                    crate::geometry::CurveOffsetRange::Uniform { parameter_range }
                    | crate::geometry::CurveOffsetRange::Variable {
                        parameter_range, ..
                    } => parameter_range,
                };
                parameter_range.iter().all(|value| value.is_finite())
                    && parameter_range[0] < parameter_range[1]
            });
            let law_valid = match range {
                Some(crate::geometry::CurveOffsetRange::Variable { distance_law, .. }) => {
                    match distance_law {
                        crate::geometry::CurveOffsetDistanceLaw::Linear {
                            distances,
                            control_range,
                            ..
                        } => {
                            distances.iter().all(|value| value.is_finite())
                                && control_range.iter().all(|value| value.is_finite())
                                && control_range[0] < control_range[1]
                        }
                        crate::geometry::CurveOffsetDistanceLaw::Coordinate {
                            function_parameter_offset,
                            function_parameter_scale,
                            ..
                        } => {
                            function_parameter_offset.is_finite()
                                && function_parameter_scale.is_finite()
                                && *function_parameter_scale != 0.0
                        }
                    }
                }
                _ => true,
            };
            if !distance.is_finite() || !side_valid || !range_valid || !law_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "curve offset distance, side, range, or law is invalid",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::SpatialOffset {
            distance,
            reference_direction,
            ..
        } = procedural.definition()
        {
            if !distance.is_finite()
                || ![
                    reference_direction.x,
                    reference_direction.y,
                    reference_direction.z,
                ]
                .into_iter()
                .all(f64::is_finite)
                || (reference_direction.norm() - 1.0).abs() > EPS_SPATIAL_CURVE_DIRECTION
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "invalid spatial curve offset",
                );
            }
        }
        if let ProceduralCurveDefinition::Deformable {
            source_parameter_range,
            data,
            ..
        } = procedural.definition()
        {
            let finite_vector = |vector: &crate::math::Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let payload_finite = match data {
                crate::geometry::DeformableCurveData::VectorField {
                    vectors,
                    parameter_pairs,
                } => {
                    vectors.iter().all(finite_vector)
                        && parameter_pairs
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                }
                crate::geometry::DeformableCurveData::Mode3 {
                    leading_vectors,
                    leading_parameter,
                    trailing_point,
                    trailing_vectors,
                    frame_parameter,
                    parameters,
                    trailing_parameter,
                    ..
                } => {
                    leading_vectors.iter().all(finite_vector)
                        && leading_parameter.is_finite()
                        && [trailing_point.x, trailing_point.y, trailing_point.z]
                            .into_iter()
                            .all(f64::is_finite)
                        && trailing_vectors.iter().all(finite_vector)
                        && frame_parameter.is_finite()
                        && parameters.iter().all(|value| value.is_finite())
                        && trailing_parameter.is_finite()
                }
            };
            let range_valid = source_parameter_range
                .iter()
                .flatten()
                .all(|value| value.is_finite());
            if !payload_finite || !range_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "deformable curve payload is not finite",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Spring { layout, .. } = procedural.definition() {
            let context = layout.support_context();
            let inline_ranges_finite = match layout {
                crate::geometry::SpringLayout::ContextFirst {
                    supports,
                    first_pcurve,
                    ..
                } => {
                    supports.iter().all(|support| match support {
                        crate::geometry::SpringSupport::Surface(_) => true,
                        crate::geometry::SpringSupport::Ranges(ranges) => {
                            ranges.iter().all(|range| {
                                range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                            })
                        }
                    }) && match first_pcurve {
                        crate::geometry::SpringPcurve::Pcurve(_) => true,
                        crate::geometry::SpringPcurve::Range(range) => {
                            range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                        }
                    }
                }
                crate::geometry::SpringLayout::CacheFirst { .. } => true,
            };
            if context.is_err() || !inline_ranges_finite {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "spring context or null-support ranges are invalid",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::SurfaceOffset {
            base_u_range,
            base_v_range,
            base_range,
            distance,
            shift,
            scale,
            ..
        } = procedural.definition()
        {
            let ranges = [base_u_range, base_v_range, base_range];
            if ranges
                .iter()
                .any(|range| !range.iter().all(|value| value.is_finite()) || range[0] > range[1])
                || !distance.is_finite()
                || !shift.is_finite()
                || !scale.is_finite()
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "surface-offset fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Silhouette {
            silhouette,
            light_direction,
            ..
        } = procedural.definition()
        {
            let draft_finite = match silhouette {
                crate::geometry::SilhouetteKind::Taper { draft_factor } => draft_factor.is_finite(),
                _ => true,
            };
            if !light_direction.x.is_finite()
                || !light_direction.y.is_finite()
                || !light_direction.z.is_finite()
                || light_direction.norm() <= f64::EPSILON
                || !draft_finite
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "silhouette fields are not finite or the light direction is degenerate",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } =
            procedural.definition()
        {
            if third
                .pcurve
                .as_ref()
                .is_some_and(|pcurve| pcurve.parameter_range.is_some())
                && context.parameter_range()[0] == context.parameter_range()[1]
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "three-surface intersection context is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Projection { tail, .. } = procedural.definition() {
            let tail_finite = match tail {
                crate::geometry::ProjectionTail::EarlyClose { .. } => true,
                crate::geometry::ProjectionTail::Ranged {
                    parameter_range, ..
                } => {
                    parameter_range.iter().all(|value| value.is_finite())
                        && parameter_range[0] <= parameter_range[1]
                }
            };
            if !tail_finite {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "projection fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::TwoSidedOffset { offsets, .. } = procedural.definition() {
            let finite = offsets.iter().all(|value| value.is_finite());
            if !finite {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "two-sided offset fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } = procedural.definition()
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "subset-curve range is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Replica { .. } = procedural.definition() {
            continue;
        }
        if let ProceduralCurveDefinition::VectorOffset {
            parameter_range,
            offset,
            ..
        } = procedural.definition()
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
                || !offset.x.is_finite()
                || !offset.y.is_finite()
                || !offset.z.is_finite()
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "vector-offset fields are not finite and ordered",
                );
            }
            continue;
        }
    }
}

pub(super) fn bounds_err(findings: &mut Vec<Finding>, id: &str, msg: &str) {
    findings.push(Finding {
        check: Check::Bounds,
        severity: Severity::Error,
        message: msg.to_string(),
        entity: Some(id.to_string()),
    });
}

#[cfg(test)]
mod tests;
