// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;

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
            .vertices
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
            .triangles
            .iter()
            .flatten()
            .any(|index| *index as usize >= mesh.vertices.len())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains an out-of-range tessellation index".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if mesh
            .normals
            .iter()
            .any(|normal| !normal.x.is_finite() || !normal.y.is_finite() || !normal.z.is_finite())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a non-finite tessellation normal".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if !mesh.normals.is_empty() && mesh.normals.len() != mesh.vertices.len() {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "tessellation normals do not match vertex count".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if !mesh.strip_lengths.is_empty()
            && mesh.strip_lengths.iter().try_fold(0usize, |total, length| {
                usize::try_from(*length)
                    .ok()
                    .and_then(|length| total.checked_add(length))
            }) != Some(mesh.vertices.len())
        {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "tessellation strips do not match vertex count".into(),
                entity: Some(mesh.id.clone()),
            });
        }
        if !mesh.strip_lengths.is_empty() {
            let mut expected = Vec::new();
            let mut base = 0u32;
            let mut valid = true;
            for length in &mesh.strip_lengths {
                for index in 0..length.saturating_sub(2) {
                    let Some(a) = base.checked_add(index) else {
                        valid = false;
                        break;
                    };
                    let Some(b) = a.checked_add(1) else {
                        valid = false;
                        break;
                    };
                    let Some(c) = a.checked_add(2) else {
                        valid = false;
                        break;
                    };
                    let triangle = if index % 2 == 0 { [a, b, c] } else { [a, c, b] };
                    expected.push(triangle);
                }
                let Some(next) = base.checked_add(*length) else {
                    valid = false;
                    break;
                };
                base = next;
            }
            if !valid || expected != mesh.triangles {
                findings.push(Finding {
                    check: Check::Tessellation,
                    severity: Severity::Error,
                    message: "tessellation triangles do not match strips".into(),
                    entity: Some(mesh.id.clone()),
                });
            }
        }
        if mesh.channels.iter().any(|channel| {
            channel.data.len() != channel.item_size as usize * channel.count as usize
        }) {
            findings.push(Finding {
                check: Check::Tessellation,
                severity: Severity::Error,
                message: "contains a malformed tessellation channel".into(),
                entity: Some(mesh.id.clone()),
            });
        }
    }
}

pub(super) fn degenerate(v: &Vector3) -> bool {
    v.norm() <= f64::EPSILON
}

fn unit_vector(v: &Vector3) -> bool {
    (v.norm() - 1.0).abs() <= 1.0e-9
}

fn orthonormal(left: &Vector3, right: &Vector3) -> bool {
    unit_vector(left)
        && unit_vector(right)
        && (left.x * right.x + left.y * right.y + left.z * right.z).abs() <= 1.0e-9
}

fn point3_finite(point: &crate::math::Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn nurbs_weights_valid(weights: Option<&[f64]>, pole_count: usize) -> bool {
    weights.is_none_or(|weights| {
        weights.len() == pole_count
            && weights
                .iter()
                .all(|weight| weight.is_finite() && weight.abs() > f64::EPSILON)
    })
}

fn variable_blend_value_valid(value: &crate::geometry::VariableBlendValue) -> bool {
    use crate::geometry::VariableBlendValuePayload;
    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    match &value.payload {
        VariableBlendValuePayload::TwoEnds { parameters, radii } => {
            finite(parameters) && finite(radii)
        }
        VariableBlendValuePayload::FixedWidth { parameters, width } => {
            finite(parameters) && width.is_finite()
        }
        VariableBlendValuePayload::EdgeOffset { scalars, lengths } => {
            finite(scalars) && finite(lengths)
        }
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            terminal,
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && !matches!(terminal, crate::geometry::LoftBridgeToken::Double(v) if !v.is_finite())
        }
        VariableBlendValuePayload::Constant {
            parameters,
            radius,
            nested,
            ..
        } => finite(parameters) && radius.is_finite() && variable_blend_value_valid(nested),
        VariableBlendValuePayload::Interpolated {
            parameter,
            radius,
            points,
            tail,
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && tail.as_ref().is_none_or(|values| finite(values))
                && points.iter().all(|point| {
                    point.parameter.is_finite()
                        && point.radius.is_finite()
                        && finite(&point.tangents)
                        && point.location.x.is_finite()
                        && point.location.y.is_finite()
                        && point.location.z.is_finite()
                        && point.normal.x.is_finite()
                        && point.normal.y.is_finite()
                        && point.normal.z.is_finite()
                })
        }
    }
}

pub(super) fn check_bounds(ir: &CadIr, findings: &mut Vec<Finding>) {
    for (id, tolerance) in ir
        .model
        .vertices
        .iter()
        .map(|entity| (&entity.id.0, entity.tolerance))
        .chain(
            ir.model
                .edges
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
        .chain(
            ir.model
                .faces
                .iter()
                .map(|entity| (&entity.id.0, entity.tolerance)),
        )
    {
        if tolerance.is_some_and(nonpositive) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Error,
                message: "topology tolerance is not positive and finite".into(),
                entity: Some(id.clone()),
            });
        } else if tolerance.is_some_and(|value| value > 1.0e6) {
            findings.push(Finding {
                check: Check::Tolerances,
                severity: Severity::Warning,
                message: "topology tolerance is outside a sane canonical range".into(),
                entity: Some(id.clone()),
            });
        }
    }
    for s in &ir.model.surfaces {
        match &s.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                if !point3_finite(origin) {
                    bounds_err(findings, &s.id.0, "plane origin is not finite");
                }
                if !orthonormal(normal, u_axis) {
                    bounds_err(findings, &s.id.0, "plane frame is not orthonormal");
                }
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } => {
                if !point3_finite(origin) {
                    bounds_err(findings, &s.id.0, "cylinder origin is not finite");
                }
                if !orthonormal(axis, ref_direction) {
                    bounds_err(findings, &s.id.0, "cylinder frame is not orthonormal");
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &s.id.0, "cylinder radius is not positive");
                }
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } => {
                if !point3_finite(origin) {
                    bounds_err(findings, &s.id.0, "cone origin is not finite");
                }
                if !orthonormal(axis, ref_direction) {
                    bounds_err(findings, &s.id.0, "cone frame is not orthonormal");
                }
                if !radius.is_finite() || *radius < 0.0 {
                    bounds_err(findings, &s.id.0, "cone radius is negative or not finite");
                }
                if !ratio.is_finite() || *ratio <= 0.0 {
                    bounds_err(findings, &s.id.0, "cone ratio is not positive and finite");
                }
                if !half_angle.is_finite() {
                    bounds_err(findings, &s.id.0, "cone half-angle is not finite");
                }
            }
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                if !point3_finite(center) {
                    bounds_err(findings, &s.id.0, "sphere center is not finite");
                }
                if !orthonormal(axis, ref_direction) {
                    bounds_err(findings, &s.id.0, "sphere frame is not orthonormal");
                }
                if !radius.is_finite() || radius.abs() <= f64::EPSILON {
                    bounds_err(findings, &s.id.0, "sphere radius is zero or not finite");
                }
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                if !point3_finite(center) {
                    bounds_err(findings, &s.id.0, "torus center is not finite");
                }
                if !orthonormal(axis, ref_direction) {
                    bounds_err(findings, &s.id.0, "torus frame is not orthonormal");
                }
                if nonpositive(*major_radius)
                    || !minor_radius.is_finite()
                    || minor_radius.abs() <= f64::EPSILON
                {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "torus major radius is not positive or minor radius is zero",
                    );
                }
            }
            SurfaceGeometry::Nurbs(n) => {
                let shape = usize::try_from(n.u_count)
                    .ok()
                    .zip(usize::try_from(n.v_count).ok())
                    .zip(usize::try_from(n.u_degree).ok())
                    .zip(usize::try_from(n.v_degree).ok())
                    .and_then(|(((u_count, v_count), u_degree), v_degree)| {
                        u_count
                            .checked_mul(v_count)
                            .map(|pole_count| (u_count, v_count, u_degree, v_degree, pole_count))
                    });
                let valid =
                    shape.is_some_and(|(u_count, v_count, u_degree, v_degree, pole_count)| {
                        u_count > u_degree
                            && v_count > v_degree
                            && n.control_points.len() == pole_count
                            && n.control_points.iter().all(point3_finite)
                            && nurbs_weights_valid(n.weights.as_deref(), pole_count)
                            && u_count
                                .checked_add(u_degree)
                                .and_then(|count| count.checked_add(1))
                                .is_some_and(|count| n.u_knots.len() == count)
                            && v_count
                                .checked_add(v_degree)
                                .and_then(|count| count.checked_add(1))
                                .is_some_and(|count| n.v_knots.len() == count)
                    });
                if !valid {
                    bounds_err(
                        findings,
                        &s.id.0,
                        "NURBS surface degree, poles, weights, or knot cardinality is invalid",
                    );
                }
                check_knots(findings, &s.id.0, &n.u_knots, "u");
                check_knots(findings, &s.id.0, &n.v_knots, "v");
            }
            SurfaceGeometry::Procedural { .. } => {}
            SurfaceGeometry::Polygonal {
                vertices,
                triangles,
                chordal_deflection,
            } => {
                if !valid_polygonal_surface(vertices, triangles, *chordal_deflection) {
                    bounds_err(findings, &s.id.0, "polygonal surface payload is invalid");
                }
            }
            SurfaceGeometry::Transformed { basis, transform } => {
                if !valid_affine_transform(*transform) {
                    bounds_err(findings, &s.id.0, "surface transform is not finite affine");
                }
                if !valid_surface_basis(basis) {
                    bounds_err(findings, &s.id.0, "transformed surface basis is invalid");
                }
            }
            // An unknown surface carries no numeric geometry to bounds-check; its
            // record link is checked in `check_references`. A face resting on it
            // is legal (topology known, shape opaque).
            SurfaceGeometry::Unknown { .. } => {}
        }
    }
    for procedural in &ir.model.procedural_surfaces {
        if let ProceduralSurfaceDefinition::Extrusion {
            parameter_interval,
            direction,
            native_position,
            ..
        } = &procedural.definition
        {
            if parameter_interval.is_some_and(|range| !range.iter().all(|value| value.is_finite()))
                || ![direction.x, direction.y, direction.z]
                    .into_iter()
                    .all(f64::is_finite)
                || native_position.is_some_and(|point| {
                    ![point.x, point.y, point.z].into_iter().all(f64::is_finite)
                })
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "extrusion interval, direction, or native position is non-finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::LinearSweep { direction, .. } = &procedural.definition {
            if ![direction.x, direction.y, direction.z]
                .into_iter()
                .all(f64::is_finite)
                || degenerate(direction)
            {
                bounds_err(findings, &procedural.id.0, "invalid linear-sweep direction");
            }
        }
        if let ProceduralSurfaceDefinition::ParallelOffset { distance, .. } = &procedural.definition
        {
            if !distance.is_finite() {
                bounds_err(findings, &procedural.id.0, "non-finite parallel offset");
            }
        }
        if let ProceduralSurfaceDefinition::Exact { parameters, .. } = &procedural.definition {
            let valid = match parameters {
                crate::geometry::SplineSurfaceParameters::OrderedRanges { ranges } => {
                    ranges.iter().all(|range| {
                        range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                    })
                }
                crate::geometry::SplineSurfaceParameters::RevisionValues { values } => {
                    values.iter().flatten().all(|value| value.is_finite())
                }
            };
            if !valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "exact spline surface parameter fields are invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        } = &procedural.definition
        {
            if parameters.len() != components.len()
                || parameters.iter().any(|parameter| !parameter.is_finite())
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound surface parameters and components are inconsistent",
                );
            }
        }
        if let ProceduralSurfaceDefinition::SubSurface {
            parameter_ranges, ..
        } = &procedural.definition
        {
            if !parameter_ranges
                .iter()
                .flatten()
                .all(|value| value.is_finite())
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "sub-surface parameter interval is not finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Taper {
            parameter, taper, ..
        } = &procedural.definition
        {
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let tail_finite = match taper {
                crate::geometry::TaperSurfaceKind::Standard
                | crate::geometry::TaperSurfaceKind::Orthogonal { .. } => true,
                crate::geometry::TaperSurfaceKind::Edge { draft } => vector_finite(draft),
                crate::geometry::TaperSurfaceKind::Shadow {
                    draft,
                    sine,
                    cosine,
                }
                | crate::geometry::TaperSurfaceKind::Swept {
                    draft,
                    sine,
                    cosine,
                } => vector_finite(draft) && sine.is_finite() && cosine.is_finite(),
                crate::geometry::TaperSurfaceKind::Ruled {
                    draft,
                    sine,
                    cosine,
                    factor,
                } => {
                    vector_finite(draft)
                        && sine.is_finite()
                        && cosine.is_finite()
                        && factor.is_finite()
                }
            };
            if !parameter.is_finite() || !tail_finite {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "taper surface parameter or subtype tail is not finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Loft {
            sections,
            parameters,
            bridge,
            ..
        } = &procedural.definition
        {
            let parameters_valid = match parameters {
                crate::geometry::SplineSurfaceParameters::OrderedRanges { ranges } => {
                    ranges.iter().all(|range| {
                        range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]
                    })
                }
                crate::geometry::SplineSurfaceParameters::RevisionValues { values } => {
                    values.iter().flatten().all(|value| value.is_finite())
                }
            };
            let sections_valid =
                sections
                    .iter()
                    .flat_map(|section| &section.entries)
                    .all(|entry| {
                        entry.parameter.is_finite()
                            && entry.profile.iter().all(|member| {
                                let table = &member.data.subdata;
                                let expected_rows = if table.type_code == 211 {
                                    1
                                } else {
                                    usize::try_from(table.row_count).unwrap_or(usize::MAX)
                                };
                                table.rows.len() == expected_rows
                                    && table.rows.iter().all(|row| {
                                        row.parameters.iter().all(|value| value.is_finite())
                                            && row
                                                .columns
                                                .iter()
                                                .flatten()
                                                .all(|value| value.is_finite())
                                    })
                            })
                    });
            let bridge_valid = bridge.iter().all(|token| match token {
                crate::geometry::LoftBridgeToken::Double(value) => value.is_finite(),
                _ => true,
            });
            if !parameters_valid || !sections_valid || !bridge_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::CompoundLoft { construction } = &procedural.definition {
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let first_absent = construction.scales.iter().position(Option::is_none);
            let leading_scale_shape_valid = first_absent.is_none_or(|index| {
                construction.scales[index + 1..].iter().all(Option::is_none)
                    && construction.fifth_scale.is_none()
            });
            let mut scales = construction.scales.iter().flatten().collect::<Vec<_>>();
            scales.extend(construction.fifth_scale.iter().map(Box::as_ref));
            let tail_valid = match &construction.tail {
                crate::geometry::CompoundLoftTail::Six {
                    scale,
                    direction,
                    parameter_range,
                    ..
                } => {
                    scales.push(scale.as_ref());
                    vector_finite(direction)
                        && parameter_range.iter().all(|value| value.is_finite())
                        && parameter_range[0] <= parameter_range[1]
                }
                crate::geometry::CompoundLoftTail::Seven {
                    first_scale,
                    second_scale,
                    direction,
                    ..
                } => {
                    scales.extend(first_scale.iter().map(Box::as_ref));
                    scales.push(second_scale.as_ref());
                    vector_finite(direction)
                }
                crate::geometry::CompoundLoftTail::Zero { direction, .. } => match direction {
                    crate::geometry::CompoundLoftDirection::Vector { value } => {
                        vector_finite(value)
                    }
                    crate::geometry::CompoundLoftDirection::Curve { .. } => true,
                },
            };
            let scales_valid = scales.iter().all(|scale| {
                scale.members.iter().all(|member| {
                    let data = &member.data;
                    let table = &data.subdata;
                    let expected_rows = if table.type_code == 211 {
                        1
                    } else {
                        usize::try_from(table.row_count).unwrap_or(usize::MAX)
                    };
                    table.rows.len() == expected_rows
                        && table.rows.iter().all(|row| {
                            row.parameters.iter().all(|value| value.is_finite())
                                && row.columns.iter().flatten().all(|value| value.is_finite())
                        })
                        && data.direction.as_ref().is_none_or(&vector_finite)
                })
            });
            if !leading_scale_shape_valid || !tail_valid || !scales_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            &procedural.definition
        {
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let first_absent = construction.scales.iter().position(Option::is_none);
            let leading_scale_shape_valid = first_absent
                .is_none_or(|index| construction.scales[index + 1..].iter().all(Option::is_none));
            let shape_valid = match &construction.shape {
                crate::geometry::ScaledCompoundLoftShape::Full => true,
                crate::geometry::ScaledCompoundLoftShape::None {
                    parameter_ranges,
                    parameters,
                } => {
                    parameter_ranges
                        .iter()
                        .flatten()
                        .chain(parameters.iter().flatten())
                        .all(|value| value.is_finite())
                        && parameter_ranges.iter().all(|range| range[0] <= range[1])
                }
            };
            let mut scales = construction.scales.iter().flatten().collect::<Vec<_>>();
            let branch_valid = match &construction.branch {
                crate::geometry::ScaledCompoundLoftBranch::ExtendedVector {
                    first_scale,
                    second_scale,
                    direction,
                    ..
                } => {
                    scales.extend(first_scale.iter().map(Box::as_ref));
                    scales.push(second_scale.as_ref());
                    vector_finite(direction)
                }
                crate::geometry::ScaledCompoundLoftBranch::ExtendedCurve { scale, .. } => {
                    scales.extend(scale.iter().map(Box::as_ref));
                    true
                }
                crate::geometry::ScaledCompoundLoftBranch::Direct {
                    selector,
                    direction,
                    ..
                } => match direction {
                    crate::geometry::CompoundLoftDirection::Vector { value } => {
                        *selector == 0 && vector_finite(value)
                    }
                    crate::geometry::CompoundLoftDirection::Curve { .. } => *selector != 0,
                },
            };
            let scales_valid = scales.iter().all(|scale| {
                scale.members.iter().all(|member| {
                    let data = &member.data;
                    let table = &data.subdata;
                    let expected_rows = if table.type_code == 211 {
                        1
                    } else {
                        usize::try_from(table.row_count).unwrap_or(usize::MAX)
                    };
                    table.rows.len() == expected_rows
                        && table.rows.iter().all(|row| {
                            row.parameters.iter().all(|value| value.is_finite())
                                && row.columns.iter().flatten().all(|value| value.is_finite())
                        })
                        && data.direction.as_ref().is_none_or(&vector_finite)
                })
            });
            let scalars_valid = construction
                .discontinuities
                .iter()
                .flatten()
                .all(|value| value.is_finite())
                && construction.tail_directions.iter().all(vector_finite);
            if !leading_scale_shape_valid
                || !shape_valid
                || !branch_valid
                || !scales_valid
                || !scalars_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "scaled compound loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Law { construction } = &procedural.definition {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Double { value } => value.is_finite(),
                    crate::geometry::LawExpression::Point { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Vector { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Transform { scalars, .. } => {
                        scalars.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Edge { parameters, .. } => {
                        parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Spline {
                        knots,
                        controls,
                        point,
                        ..
                    } => {
                        knots.iter().chain(controls).all(|value| value.is_finite())
                            && point.x.is_finite()
                            && point.y.is_finite()
                            && point.z.is_finite()
                    }
                    crate::geometry::LawExpression::Algebraic { operands, .. } => {
                        operands.iter().all(|operand| law_valid(operand, depth + 1))
                    }
                }
            }
            let formula_valid = |formula: &crate::geometry::LawFormula| {
                if formula.name == "null_law" {
                    formula.variables.is_empty()
                } else {
                    formula.variables.iter().all(|value| law_valid(value, 0))
                }
            };
            let tail_valid = match &construction.tail {
                crate::geometry::LawSurfaceTail::Full => procedural
                    .cache_fit_tolerance
                    .is_some_and(|value| value.is_finite() && value >= 0.0),
                crate::geometry::LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    ..
                } => {
                    procedural.cache_fit_tolerance.is_none()
                        && fit_tolerance.is_finite()
                        && *fit_tolerance >= 0.0
                        && parameters.iter().flatten().all(|value| value.is_finite())
                }
                crate::geometry::LawSurfaceTail::None {
                    parameter_ranges, ..
                } => {
                    procedural.cache_fit_tolerance.is_none()
                        && parameter_ranges
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                }
                crate::geometry::LawSurfaceTail::Historical
                | crate::geometry::LawSurfaceTail::Optimal => {
                    procedural.cache_fit_tolerance.is_none()
                }
            };
            let valid = construction
                .parameter_ranges
                .iter()
                .flatten()
                .flatten()
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite())
                && tail_valid
                && formula_valid(&construction.primary)
                && construction.additional.iter().all(formula_valid);
            if !valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "law surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Skin { construction } = &procedural.definition {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null => true,
                    crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Double { value } => value.is_finite(),
                    crate::geometry::LawExpression::Point { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Vector { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Transform { scalars, .. } => {
                        scalars.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Edge { parameters, .. } => {
                        parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Spline {
                        knots,
                        controls,
                        point,
                        ..
                    } => {
                        knots.iter().chain(controls).all(|value| value.is_finite())
                            && point.x.is_finite()
                            && point.y.is_finite()
                            && point.z.is_finite()
                    }
                    crate::geometry::LawExpression::Algebraic { operands, .. } => {
                        operands.iter().all(|operand| law_valid(operand, depth + 1))
                    }
                }
            }
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let layout_valid = match &construction.layout {
                crate::geometry::SkinSurfaceLayout::Profiles { profiles, .. } => {
                    usize::try_from(construction.inner_count).ok() == Some(profiles.len())
                        && profiles.iter().all(|profile| {
                            let table = &profile.data.subdata;
                            let expected_rows = if table.type_code == 211 {
                                1
                            } else {
                                usize::try_from(table.row_count).unwrap_or(usize::MAX)
                            };
                            table.rows.len() == expected_rows
                                && table.rows.iter().all(|row| {
                                    row.parameters.iter().all(|value| value.is_finite())
                                        && row
                                            .columns
                                            .iter()
                                            .flatten()
                                            .all(|value| value.is_finite())
                                })
                                && profile.data.direction.as_ref().is_none_or(&vector_finite)
                        })
                }
                crate::geometry::SkinSurfaceLayout::Compact { subdata, .. } => {
                    let expected_rows = if subdata.type_code == 211 {
                        1
                    } else {
                        usize::try_from(subdata.row_count).unwrap_or(usize::MAX)
                    };
                    subdata.rows.len() == expected_rows
                        && subdata.rows.iter().all(|row| {
                            row.parameters.iter().all(|value| value.is_finite())
                                && row.columns.iter().flatten().all(|value| value.is_finite())
                        })
                }
            };
            let formula_valid = if construction.formula.name == "null_law" {
                construction.formula.variables.is_empty()
            } else {
                construction
                    .formula
                    .variables
                    .iter()
                    .all(|variable| law_valid(variable, 0))
            };
            let scalars_valid = construction.parameter.is_finite()
                && construction.trailing_parameter.is_finite()
                && vector_finite(&construction.direction)
                && construction
                    .discontinuities
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite());
            if !layout_valid || !formula_valid || !scalars_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "skin surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Net { construction } = &procedural.definition {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Double { value } => value.is_finite(),
                    crate::geometry::LawExpression::Point { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Vector { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Transform { scalars, .. } => {
                        scalars.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Edge { parameters, .. } => {
                        parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Spline {
                        knots,
                        controls,
                        point,
                        ..
                    } => {
                        knots.iter().chain(controls).all(|value| value.is_finite())
                            && point.x.is_finite()
                            && point.y.is_finite()
                            && point.z.is_finite()
                    }
                    crate::geometry::LawExpression::Algebraic { operands, .. } => {
                        operands.iter().all(|operand| law_valid(operand, depth + 1))
                    }
                }
            }
            let sections_valid = construction.sections.iter().all(|section| {
                section.entries.iter().all(|entry| {
                    entry.parameter.is_finite()
                        && entry.profile.iter().all(|member| {
                            let table = &member.data.subdata;
                            let expected_rows = if table.type_code == 211 {
                                1
                            } else {
                                usize::try_from(table.row_count).unwrap_or(usize::MAX)
                            };
                            table.rows.len() == expected_rows
                                && table.rows.iter().all(|row| {
                                    row.parameters.iter().all(|value| value.is_finite())
                                        && row
                                            .columns
                                            .iter()
                                            .flatten()
                                            .all(|value| value.is_finite())
                                })
                        })
                })
            });
            let formulas_valid = construction.formulas.iter().all(|formula| {
                if formula.name == "null_law" {
                    formula.variables.is_empty()
                } else {
                    formula
                        .variables
                        .iter()
                        .all(|variable| law_valid(variable, 0))
                }
            });
            let scalars_valid = construction
                .frame_parameters
                .iter()
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite())
                && construction.directions.iter().all(|direction| {
                    direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite()
                });
            if !sections_valid || !formulas_valid || !scalars_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "net surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Sweep {
            native: Some(construction),
            ..
        } = &procedural.definition
        {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Double { value } => value.is_finite(),
                    crate::geometry::LawExpression::Point { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Vector { value } => {
                        value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                    }
                    crate::geometry::LawExpression::Transform { scalars, .. } => {
                        scalars.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Edge { parameters, .. } => {
                        parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::LawExpression::Spline {
                        knots,
                        controls,
                        point,
                        ..
                    } => {
                        knots.iter().chain(controls).all(|value| value.is_finite())
                            && point.x.is_finite()
                            && point.y.is_finite()
                            && point.z.is_finite()
                    }
                    crate::geometry::LawExpression::Algebraic { operands, .. } => {
                        operands.iter().all(|operand| law_valid(operand, depth + 1))
                    }
                }
            }
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let formula_valid = |formula: &crate::geometry::LawFormula| {
                if formula.name == "null_law" {
                    formula.variables.is_empty()
                } else {
                    formula
                        .variables
                        .iter()
                        .all(|variable| law_valid(variable, 0))
                }
            };
            let layout_valid = match &construction.layout {
                crate::geometry::SweepSurfaceLayout::ProfileFirst {
                    directions,
                    origin,
                    parameters,
                    formulas,
                    ..
                } => {
                    directions.iter().all(vector_finite)
                        && point_finite(origin)
                        && parameters.iter().all(|value| value.is_finite())
                        && formulas.iter().all(formula_valid)
                }
                crate::geometry::SweepSurfaceLayout::ExplicitFormula {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    formula,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                        && formula_valid(formula)
                }
                crate::geometry::SweepSurfaceLayout::ExplicitGuide {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    guide_range,
                    guide_parameters,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .chain(guide_range)
                        .chain(guide_parameters)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                }
                crate::geometry::SweepSurfaceLayout::ExplicitSurface {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    path_range,
                    path_parameter,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && path_parameter.is_finite()
                }
                crate::geometry::SweepSurfaceLayout::LawDriven {
                    profile_range,
                    profile_frame,
                    origin,
                    directions,
                    first_law,
                    first_range,
                    law_direction,
                    path_range,
                    path_parameter,
                    second_law,
                    formula,
                    ..
                } => {
                    profile_range
                        .iter()
                        .chain(first_range)
                        .chain(path_range)
                        .all(|value| value.is_finite())
                        && profile_frame.as_ref().is_none_or(|(point, vector)| {
                            point_finite(point) && vector_finite(vector)
                        })
                        && point_finite(origin)
                        && directions.iter().all(vector_finite)
                        && vector_finite(law_direction)
                        && path_parameter.is_finite()
                        && law_valid(first_law, 0)
                        && law_valid(second_law, 0)
                        && formula_valid(formula)
                }
            };
            let scalars_valid = layout_valid
                && construction
                    .discontinuities
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite());
            if !scalars_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "sweep surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::TSpline { construction } = &procedural.definition {
            let ranges_valid = construction
                .parameter_ranges
                .iter()
                .flatten()
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite());
            let source_valid = match &construction.subtransform {
                crate::geometry::TSplineSubtransform::Inline {
                    program, values, ..
                } => {
                    !program.is_empty()
                        && !values.is_empty()
                        && construction.program_graph.as_ref()
                            == Some(&crate::geometry::TSplineProgram::parse(program))
                        && construction.values_graph.as_ref()
                            == Some(&crate::geometry::TSplineProgram::parse(values))
                }
                crate::geometry::TSplineSubtransform::Reference { index, resolved } => {
                    let resolved_program =
                        resolved.as_deref().and_then(|resolved| match resolved {
                            crate::geometry::TSplineSubtransform::Inline { program, .. } => {
                                Some(program)
                            }
                            crate::geometry::TSplineSubtransform::Reference { .. } => None,
                        });
                    *index >= 0
                        && resolved_program.is_some_and(|program| {
                            construction.program_graph.as_ref()
                                == Some(&crate::geometry::TSplineProgram::parse(program))
                        })
                        && resolved.as_deref().is_some_and(|resolved| match resolved {
                            crate::geometry::TSplineSubtransform::Inline { values, .. } => {
                                construction.values_graph.as_ref()
                                    == Some(&crate::geometry::TSplineProgram::parse(values))
                            }
                            crate::geometry::TSplineSubtransform::Reference { .. } => false,
                        })
                }
            };
            if !ranges_valid || !source_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "T-spline surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Helix { construction } = &procedural.definition {
            let path = &construction.path;
            let finite = construction
                .angle_range
                .iter()
                .chain(construction.dimension_range.iter())
                .chain(path.angle_range.iter())
                .all(|value| value.is_finite())
                && [path.center.x, path.center.y, path.center.z]
                    .into_iter()
                    .chain([path.major.x, path.major.y, path.major.z])
                    .chain([path.minor.x, path.minor.y, path.minor.z])
                    .chain([path.pitch.x, path.pitch.y, path.pitch.z])
                    .chain([path.axis.x, path.axis.y, path.axis.z])
                    .chain(std::iter::once(path.apex_factor))
                    .all(f64::is_finite);
            let major_length =
                (path.major.x.powi(2) + path.major.y.powi(2) + path.major.z.powi(2)).sqrt();
            let minor_length =
                (path.minor.x.powi(2) + path.minor.y.powi(2) + path.minor.z.powi(2)).sqrt();
            let circular_path = major_length > 0.0
                && (major_length - minor_length).abs() <= 1.0e-9 * major_length.max(1.0);
            let profile_valid = match construction.profile {
                crate::geometry::HelixSurfaceProfile::Circle { length, radius } => {
                    length.is_finite() && radius.is_finite() && radius != 0.0
                }
                crate::geometry::HelixSurfaceProfile::Line { direction } => {
                    direction.x.is_finite()
                        && direction.y.is_finite()
                        && direction.z.is_finite()
                        && direction.x * direction.x
                            + direction.y * direction.y
                            + direction.z * direction.z
                            > 0.0
                }
            };
            if !finite || !circular_path || !profile_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "helix surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Deformable { construction } = &procedural.definition {
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let frame_valid = |frame: &crate::geometry::DeformableSurfaceFrame| {
                frame.leading_vectors.iter().all(vector_finite)
                    && frame.secondary_vectors.iter().all(vector_finite)
                    && frame.leading_parameter.is_finite()
                    && frame.secondary_parameter.is_finite()
                    && frame.point.x.is_finite()
                    && frame.point.y.is_finite()
                    && frame.point.z.is_finite()
            };
            let data_valid = match &construction.data {
                crate::geometry::DeformableSurfaceData::Full {
                    leading_vectors,
                    leading_parameter,
                    first_parameter,
                    second_parameter,
                    frames,
                    ..
                } => {
                    leading_vectors.iter().all(vector_finite)
                        && leading_parameter.is_finite()
                        && first_parameter.is_finite()
                        && second_parameter.is_finite()
                        && frames.iter().all(|frame| {
                            frame.vectors.iter().all(vector_finite) && frame.parameter.is_finite()
                        })
                }
                crate::geometry::DeformableSurfaceData::SurfaceCurve {
                    first_parameter,
                    second_parameter,
                    vectors,
                    frame_parameter,
                    parameter_triples,
                    ..
                } => {
                    first_parameter.is_finite()
                        && second_parameter.is_finite()
                        && vectors.iter().all(vector_finite)
                        && frame_parameter.is_finite()
                        && parameter_triples
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                }
                crate::geometry::DeformableSurfaceData::Plain {
                    frame,
                    parameter_triples,
                } => {
                    frame_valid(frame)
                        && parameter_triples
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
                }
                crate::geometry::DeformableSurfaceData::Guided {
                    frame,
                    guide_parameter,
                    ..
                } => frame_valid(frame) && guide_parameter.is_finite(),
                crate::geometry::DeformableSurfaceData::Minimal { vectors, .. } => {
                    vectors.iter().all(vector_finite)
                }
            };
            if !data_valid
                || !construction
                    .discontinuities
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "deformable surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::G2Blend { construction } = &procedural.definition {
            let direction_finite = |direction: &Vector3| {
                direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite()
            };
            let first_shape_valid = match &construction.first_shape {
                crate::geometry::G2BlendFirstShape::Full { surface, tolerance } => {
                    surface.is_some() == tolerance.is_some()
                        && tolerance.is_none_or(|value| value.is_finite() && value >= 0.0)
                }
                crate::geometry::G2BlendFirstShape::None {
                    coefficients,
                    tolerance,
                    extension,
                    ..
                } => {
                    coefficients.iter().all(|value| value.is_finite())
                        && tolerance.is_finite()
                        && *tolerance >= 0.0
                        && extension.as_ref().is_none_or(|token| match token {
                            crate::geometry::LoftBridgeToken::Double(value) => value.is_finite(),
                            _ => true,
                        })
                }
            };
            let ranges_valid = construction
                .parameter_ranges
                .iter()
                .all(|range| range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]);
            let scalars_valid = construction
                .center_parameters
                .iter()
                .chain(construction.trailing_parameters.iter())
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite());
            if !direction_finite(&construction.first.direction)
                || !direction_finite(&construction.second.direction)
                || !first_shape_valid
                || !ranges_valid
                || !scalars_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "G2 blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::VariableBlend { construction } = &procedural.definition
        {
            use crate::geometry::VariableBlendRadiusKind;

            let ranges_valid = [
                construction.u_range,
                construction.v_range,
                construction.post_range,
                construction.slice_range,
                construction.secondary_range,
            ]
            .iter()
            .all(|range| {
                range.iter().flatten().all(|value| value.is_finite())
                    && match (range[0], range[1]) {
                        (Some(lower), Some(upper)) => lower <= upper,
                        _ => true,
                    }
            });
            let sides_valid = construction.sides.iter().all(|side| {
                side.location.x.is_finite()
                    && side.location.y.is_finite()
                    && side.location.z.is_finite()
            });
            let values_valid = variable_blend_value_valid(&construction.first_value)
                && construction
                    .second_value
                    .as_ref()
                    .is_none_or(variable_blend_value_valid)
                && construction
                    .chamfer
                    .as_ref()
                    .is_none_or(|chamfer| variable_blend_value_valid(&chamfer.value));
            let scalar_tail_valid = construction.offsets.iter().all(|value| value.is_finite())
                && construction.shape_parameter.is_finite()
                && construction.shape_length.is_finite()
                && construction
                    .single_radius_tail
                    .as_ref()
                    .is_none_or(|tail| {
                        tail.parameters.iter().all(|value| value.is_finite())
                            && !matches!(tail.selector, crate::geometry::LoftBridgeToken::Double(value) if !value.is_finite())
                    });
            let radius_branch_valid = match construction.radius_kind {
                VariableBlendRadiusKind::SingleRadius => {
                    construction.second_value.is_none() && construction.chamfer.is_none()
                }
                VariableBlendRadiusKind::TwoRadii => {
                    construction.second_value.is_some() && construction.single_radius_tail.is_none()
                }
            };
            if !ranges_valid
                || !sides_valid
                || !values_valid
                || !scalar_tail_valid
                || !radius_branch_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "variable blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::VertexBlend { construction } = &procedural.definition {
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let boundaries_valid = construction.boundaries.iter().all(|boundary| {
                point_finite(&boundary.magic)
                    && boundary.fullness.is_finite()
                    && match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle {
                            form,
                            twists,
                            parameters,
                            ..
                        } => {
                            matches!((*form, twists.len()), (0, 0) | (1, 1) | (3, 2))
                                && twists.iter().all(&point_finite)
                                && parameters.iter().all(|value| value.is_finite())
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Degenerate {
                            location,
                            normals,
                        } => {
                            point_finite(location)
                                && normals
                                    .iter()
                                    .all(|normal| vector_finite(normal) && !degenerate(normal))
                        }
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve {
                            fit_tolerance,
                            ..
                        } => fit_tolerance.is_finite() && *fit_tolerance >= 0.0,
                        crate::geometry::VertexBlendBoundaryGeometry::Plane {
                            normal,
                            parameters,
                            ..
                        } => {
                            vector_finite(normal)
                                && !degenerate(normal)
                                && parameters.iter().all(|value| value.is_finite())
                        }
                    }
            });
            if !construction.fit_tolerance.is_finite()
                || construction.fit_tolerance < 0.0
                || !boundaries_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "vertex blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Blend {
            native: Some(construction),
            ..
        } = &procedural.definition
        {
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let ranges_valid = [&construction.u_range, &construction.v_range]
                .iter()
                .all(|range| {
                    range.iter().flatten().all(|value| value.is_finite())
                        && match range {
                            [Some(lower), Some(upper)] => lower <= upper,
                            _ => true,
                        }
                });
            let selector_valid = match construction.radius_selector {
                crate::geometry::RollingBallRadiusSelector::None => true,
                crate::geometry::RollingBallRadiusSelector::Value { value } => value.is_finite(),
            };
            let scalars_valid = construction
                .offsets
                .iter()
                .chain(construction.parameters.iter())
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite());
            let sides_valid = construction
                .sides
                .iter()
                .all(|side| point_finite(&side.location));
            let third_valid = construction
                .third
                .as_ref()
                .is_none_or(|side| vector_finite(&side.direction));
            if !ranges_valid || !selector_valid || !scalars_valid || !sides_valid || !third_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "rolling-ball blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::RollingBallJet {
            degree,
            knots,
            multiplicities,
            sites,
        } = &procedural.definition
        {
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let derivative_finite = |derivative: &crate::geometry::RollingBallJetDerivative| {
                [
                    &derivative.first_limit,
                    &derivative.second_limit,
                    &derivative.center,
                ]
                .iter()
                .all(|vector| vector_finite(vector))
                    && derivative.angle.is_finite()
            };
            let sites_valid = sites.iter().all(|site| {
                let radius = |point: &crate::math::Point3| {
                    ((point.x - site.center.x).powi(2)
                        + (point.y - site.center.y).powi(2)
                        + (point.z - site.center.z).powi(2))
                    .sqrt()
                };
                let first_radius = radius(&site.first_limit);
                let second_radius = radius(&site.second_limit);
                point_finite(&site.first_limit)
                    && point_finite(&site.second_limit)
                    && point_finite(&site.center)
                    && site.angle.is_finite()
                    && derivative_finite(&site.first_derivative)
                    && derivative_finite(&site.second_derivative)
                    && first_radius.is_finite()
                    && first_radius > 0.0
                    && second_radius.is_finite()
                    && (first_radius - second_radius).abs()
                        <= 1e-9 * first_radius.max(second_radius).max(1.0)
            });
            if *degree == 0
                || knots.len() != sites.len()
                || multiplicities.len() != knots.len()
                || multiplicities.first() != Some(&(degree + 1))
                || multiplicities.last() != Some(&(degree + 1))
                || multiplicities
                    .iter()
                    .any(|multiplicity| *multiplicity == 0 || *multiplicity > degree + 1)
                || knots.len() < 2
                || knots.iter().any(|knot| !knot.is_finite())
                || knots.windows(2).any(|pair| pair[0] >= pair[1])
                || !sites_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "rolling-ball jet payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Offset {
            distance,
            extension_flags,
            ..
        } = &procedural.definition
        {
            if !distance.is_finite()
                || !matches!(
                    extension_flags.as_slice(),
                    [] | [false] | [true, _] | [true, _, _]
                )
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "offset spline surface distance or extension flags are invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Subset {
            parameter_ranges, ..
        } = &procedural.definition
        {
            if !parameter_ranges
                .iter()
                .all(|range| range[0].is_finite() && range[1].is_finite() && range[0] <= range[1])
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "surface subset ranges are not finite and ordered",
                );
            }
        }
    }
    for c in &ir.model.curves {
        match &c.geometry {
            CurveGeometry::Line { origin, direction } => {
                if !point3_finite(origin) {
                    bounds_err(findings, &c.id.0, "line origin is not finite");
                }
                if !unit_vector(direction) {
                    bounds_err(findings, &c.id.0, "line direction is not unit length");
                }
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                if !point3_finite(center) {
                    bounds_err(findings, &c.id.0, "circle center is not finite");
                }
                if !orthonormal(axis, ref_direction) {
                    bounds_err(findings, &c.id.0, "circle frame is not orthonormal");
                }
                if nonpositive(*radius) {
                    bounds_err(findings, &c.id.0, "circle radius is not positive");
                }
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if !point3_finite(center) {
                    bounds_err(findings, &c.id.0, "ellipse center is not finite");
                }
                if !orthonormal(axis, major_direction) {
                    bounds_err(findings, &c.id.0, "ellipse frame is not orthonormal");
                }
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "ellipse radius is not positive");
                } else if major_radius < minor_radius {
                    bounds_err(
                        findings,
                        &c.id.0,
                        "ellipse major radius is smaller than its minor radius",
                    );
                }
            }
            CurveGeometry::Parabola {
                vertex,
                axis,
                major_direction,
                focal_distance,
            } => {
                if !point3_finite(vertex) {
                    bounds_err(findings, &c.id.0, "parabola vertex is not finite");
                }
                if !orthonormal(axis, major_direction) {
                    bounds_err(findings, &c.id.0, "parabola frame is not orthonormal");
                }
                if nonpositive(*focal_distance) {
                    bounds_err(findings, &c.id.0, "parabola focal distance is not positive");
                }
            }
            CurveGeometry::Hyperbola {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if !point3_finite(center) {
                    bounds_err(findings, &c.id.0, "hyperbola center is not finite");
                }
                if !orthonormal(axis, major_direction) {
                    bounds_err(findings, &c.id.0, "hyperbola frame is not orthonormal");
                }
                if nonpositive(*major_radius) || nonpositive(*minor_radius) {
                    bounds_err(findings, &c.id.0, "hyperbola radius is not positive");
                }
            }
            CurveGeometry::Degenerate { point } => {
                if !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite() {
                    bounds_err(findings, &c.id.0, "degenerate curve point is not finite");
                }
            }
            CurveGeometry::Composite { segments, .. } => {
                if segments.is_empty() {
                    bounds_err(findings, &c.id.0, "composite curve has no segments");
                }
            }
            CurveGeometry::Nurbs(n) => {
                let valid = usize::try_from(n.degree).ok().is_some_and(|degree| {
                    n.control_points.len() > degree
                        && n.control_points.iter().all(point3_finite)
                        && nurbs_weights_valid(n.weights.as_deref(), n.control_points.len())
                        && n.control_points
                            .len()
                            .checked_add(degree)
                            .and_then(|count| count.checked_add(1))
                            .is_some_and(|count| n.knots.len() == count)
                });
                if !valid {
                    bounds_err(
                        findings,
                        &c.id.0,
                        "NURBS curve degree, poles, weights, or knot cardinality is invalid",
                    );
                }
                check_knots(findings, &c.id.0, &n.knots, "");
            }
            CurveGeometry::Procedural { .. } => {}
            CurveGeometry::Polyline {
                points,
                parameters,
                chordal_deflection,
            } => {
                if !valid_polyline(points, parameters.as_deref(), *chordal_deflection) {
                    bounds_err(findings, &c.id.0, "polyline payload is invalid");
                }
            }
            CurveGeometry::Transformed { basis, transform } => {
                if !valid_affine_transform(*transform) {
                    bounds_err(findings, &c.id.0, "curve transform is not finite affine");
                }
                if !valid_curve_basis(basis) {
                    bounds_err(findings, &c.id.0, "transformed curve basis is invalid");
                }
            }
            CurveGeometry::Unknown { .. } => {}
        }
    }
    for pcurve in &ir.model.pcurves {
        let point_finite = |point: &crate::math::Point2| point.u.is_finite() && point.v.is_finite();
        let direction_valid = |direction: &crate::math::Point2| {
            point_finite(direction) && direction.u.hypot(direction.v) > f64::EPSILON
        };
        let valid = match &pcurve.geometry {
            crate::geometry::PcurveGeometry::Line { origin, direction } => {
                point_finite(origin) && direction_valid(direction)
            }
            crate::geometry::PcurveGeometry::Circle {
                center,
                x_axis,
                y_axis,
                radius,
            } => {
                point_finite(center)
                    && direction_valid(x_axis)
                    && direction_valid(y_axis)
                    && !nonpositive(*radius)
            }
            crate::geometry::PcurveGeometry::Ellipse {
                center,
                x_axis,
                y_axis,
                major_radius,
                minor_radius,
            } => {
                point_finite(center)
                    && direction_valid(x_axis)
                    && direction_valid(y_axis)
                    && !nonpositive(*major_radius)
                    && !nonpositive(*minor_radius)
            }
            crate::geometry::PcurveGeometry::Parabola {
                vertex,
                x_axis,
                y_axis,
                focal_distance,
            } => {
                point_finite(vertex)
                    && direction_valid(x_axis)
                    && direction_valid(y_axis)
                    && focal_distance.is_finite()
                    && *focal_distance > 0.0
            }
            crate::geometry::PcurveGeometry::Hyperbola {
                center,
                x_axis,
                y_axis,
                major_radius,
                minor_radius,
            } => {
                point_finite(center)
                    && direction_valid(x_axis)
                    && direction_valid(y_axis)
                    && !nonpositive(*major_radius)
                    && !nonpositive(*minor_radius)
            }
            crate::geometry::PcurveGeometry::Trimmed {
                basis,
                parameter_range,
            } => {
                parameter_range.iter().all(|value| value.is_finite())
                    && parameter_range[0] <= parameter_range[1]
                    && pcurve_basis_is_valid(basis)
            }
            crate::geometry::PcurveGeometry::Offset { basis, distance } => {
                distance.is_finite() && pcurve_basis_is_valid(basis)
            }
            crate::geometry::PcurveGeometry::PolarHarmonic {
                radial_center,
                radial_cos,
                radial_sin,
                axial_origin,
                axial_cos,
                axial_sin,
            } => {
                point_finite(radial_center)
                    && point_finite(radial_cos)
                    && point_finite(radial_sin)
                    && (direction_valid(radial_cos) || direction_valid(radial_sin))
                    && axial_origin.is_finite()
                    && axial_cos.is_finite()
                    && axial_sin.is_finite()
            }
            crate::geometry::PcurveGeometry::PolarNurbs {
                degree,
                knots,
                radial_control_points,
                axial_control_points,
                weights,
                ..
            } => {
                *degree != 0
                    && radial_control_points.len() > *degree as usize
                    && axial_control_points.len() == radial_control_points.len()
                    && knots.len() == radial_control_points.len() + *degree as usize + 1
                    && radial_control_points.iter().all(point_finite)
                    && axial_control_points.iter().all(|value| value.is_finite())
                    && weights.as_ref().is_none_or(|weights| {
                        weights.len() == radial_control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
            crate::geometry::PcurveGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                ..
            } => {
                *degree != 0
                    && control_points.len() > *degree as usize
                    && knots.len() == control_points.len() + *degree as usize + 1
                    && control_points.iter().all(point_finite)
                    && weights.as_ref().is_none_or(|weights| {
                        weights.len() == control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
        };
        if !valid {
            bounds_err(findings, &pcurve.id.0, "pcurve geometry is invalid");
        }
        if let crate::geometry::PcurveGeometry::Nurbs { knots, .. }
        | crate::geometry::PcurveGeometry::PolarNurbs { knots, .. } = &pcurve.geometry
        {
            if knots.iter().any(|knot| !knot.is_finite()) {
                bounds_err(findings, &pcurve.id.0, "pcurve knots must be finite");
            }
            check_knots(findings, &pcurve.id.0, knots, "");
        }
        if pcurve
            .parameter_range
            .is_some_and(|[start, end]| !start.is_finite() || !end.is_finite() || start > end)
        {
            bounds_err(findings, &pcurve.id.0, "pcurve parameter range is invalid");
        }
    }
    for procedural in &ir.model.procedural_curves {
        if let ProceduralCurveDefinition::Offset {
            distance,
            normal,
            parameter_range,
            distance_law,
            ..
        } = &procedural.definition
        {
            let normal_valid = normal.is_none_or(|normal| {
                normal.x.is_finite()
                    && normal.y.is_finite()
                    && normal.z.is_finite()
                    && (normal.norm() - 1.0).abs() <= 1.0e-10
            });
            let range_valid = parameter_range.is_none_or(|range| {
                range.iter().all(|value| value.is_finite()) && range[0] < range[1]
            });
            let law_valid = distance_law.as_ref().is_none_or(|law| match law {
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
                    coordinate,
                    function_parameter_offset,
                    function_parameter_scale,
                    ..
                } => {
                    matches!(coordinate, 1..=3)
                        && function_parameter_offset.is_finite()
                        && function_parameter_scale.is_finite()
                        && *function_parameter_scale != 0.0
                }
            });
            if !distance.is_finite() || !normal_valid || !range_valid || !law_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "curve offset distance, normal, range, or law is invalid",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::SpatialOffset {
            distance,
            reference_direction,
            ..
        } = &procedural.definition
        {
            if !distance.is_finite()
                || ![
                    reference_direction.x,
                    reference_direction.y,
                    reference_direction.z,
                ]
                .into_iter()
                .all(f64::is_finite)
                || (reference_direction.norm() - 1.0).abs() > 1e-9
            {
                bounds_err(findings, &procedural.id.0, "invalid spatial curve offset");
            }
        }
        if let ProceduralCurveDefinition::Deformable { data, .. } = &procedural.definition {
            if let crate::geometry::DeformableCurveData::VectorField {
                vectors,
                parameter_pairs,
            } = data
            {
                let vectors_finite = vectors.iter().all(|vector| {
                    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
                });
                let pairs_finite = parameter_pairs
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite());
                if !vectors_finite || !pairs_finite {
                    bounds_err(
                        findings,
                        &procedural.id.0,
                        "deformable vector-field payload is not finite",
                    );
                }
            }
            continue;
        }
        if let ProceduralCurveDefinition::Spring {
            context,
            surface_parameter_ranges,
            first_pcurve_parameter_range,
            ..
        } = &procedural.definition
        {
            let surface_ranges_valid =
                surface_parameter_ranges
                    .iter()
                    .enumerate()
                    .all(|(side, ranges)| {
                        ranges.is_some() == context.sides[side].surface.is_none()
                            && ranges.is_none_or(|ranges| {
                                ranges.into_iter().all(|range| {
                                    range.iter().all(|value| value.is_finite())
                                        && range[0] <= range[1]
                                })
                            })
                    });
            let first_pcurve_range_valid = first_pcurve_parameter_range.is_some()
                == context.sides[0].pcurve.is_none()
                && first_pcurve_parameter_range.is_none_or(|range| {
                    range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                });
            if !support_context_is_finite(context)
                || !surface_ranges_valid
                || !first_pcurve_range_valid
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "spring context or conditional null-support ranges are invalid",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::SurfaceOffset {
            context,
            base_u_range,
            base_v_range,
            base_range,
            distance,
            shift,
            scale,
            ..
        } = &procedural.definition
        {
            let ranges = [base_u_range, base_v_range, base_range];
            if !support_context_is_finite(context)
                || ranges.iter().any(|range| {
                    !range.iter().all(|value| value.is_finite()) || range[0] > range[1]
                })
                || !distance.is_finite()
                || !shift.is_finite()
                || !scale.is_finite()
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "surface-offset fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Silhouette {
            context,
            silhouette,
            light_direction,
            ..
        } = &procedural.definition
        {
            let draft_finite = match silhouette {
                crate::geometry::SilhouetteKind::Taper { draft_factor } => draft_factor.is_finite(),
                _ => true,
            };
            if !support_context_is_finite(context)
                || !light_direction.x.is_finite()
                || !light_direction.y.is_finite()
                || !light_direction.z.is_finite()
                || light_direction.norm() <= f64::EPSILON
                || !draft_finite
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "silhouette fields are not finite or the light direction is degenerate",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::SurfaceCurve { context, .. } = &procedural.definition {
            if !support_context_is_finite(context) {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "surface-curve context is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::ThreeSurfaceIntersection { context, third, .. } =
            &procedural.definition
        {
            if !support_context_is_finite(context)
                || !support_side_mapping_is_finite(third)
                || (third.pcurve_parameter_range.is_some()
                    && context.parameter_range[0] == context.parameter_range[1])
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "three-surface intersection context is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Projection { context, tail, .. } = &procedural.definition
        {
            let tail_finite = match tail {
                crate::geometry::ProjectionTail::EarlyClose { .. } => true,
                crate::geometry::ProjectionTail::Ranged {
                    parameter_range, ..
                } => {
                    parameter_range.iter().all(|value| value.is_finite())
                        && parameter_range[0] <= parameter_range[1]
                }
            };
            if !support_context_is_finite(context) || !tail_finite {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "projection fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Intersection { context, .. } = &procedural.definition {
            if !support_context_is_finite(context) {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "intersection support context is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Offset {
            distance,
            direction,
            ..
        } = &procedural.definition
        {
            let direction_valid = direction.is_none_or(|direction| {
                direction.x.is_finite()
                    && direction.y.is_finite()
                    && direction.z.is_finite()
                    && direction.norm() > 0.0
            });
            if !distance.is_finite() || !direction_valid {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "offset curve distance or direction is invalid",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::TwoSidedOffset {
            context, offsets, ..
        } = &procedural.definition
        {
            let finite =
                support_context_is_finite(context) && offsets.iter().all(|value| value.is_finite());
            if !finite {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "two-sided offset fields are not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Compound {
            parameters,
            component_parameters,
            components,
        } = &procedural.definition
        {
            if components.is_empty() || component_parameters.len() != components.len() {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound components are empty or do not match component parameters",
                );
            }
            if parameters
                .iter()
                .chain(component_parameters)
                .any(|value| !value.is_finite())
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "compound parameters are not finite",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::Subset {
            parameter_range, ..
        } = &procedural.definition
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "subset-curve range is not finite and ordered",
                );
            }
            continue;
        }
        if let ProceduralCurveDefinition::VectorOffset {
            parameter_range,
            offset,
            ..
        } = &procedural.definition
        {
            if !parameter_range.iter().all(|value| value.is_finite())
                || parameter_range[0] > parameter_range[1]
                || !offset.x.is_finite()
                || !offset.y.is_finite()
                || !offset.z.is_finite()
            {
                bounds_err(
                    findings,
                    &procedural.id.0,
                    "vector-offset fields are not finite and ordered",
                );
            }
            continue;
        }
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        } = &procedural.definition
        else {
            continue;
        };
        let finite = angle_range.iter().all(|value| value.is_finite())
            && center.x.is_finite()
            && center.y.is_finite()
            && center.z.is_finite()
            && [major, minor, pitch, axis]
                .into_iter()
                .flat_map(|vector| [vector.x, vector.y, vector.z])
                .all(f64::is_finite)
            && apex_factor.is_finite();
        if !finite || angle_range[0] > angle_range[1] {
            bounds_err(
                findings,
                &procedural.id.0,
                "helix fields are not finite and ordered",
            );
        }
        if degenerate(major) || degenerate(minor) || degenerate(axis) {
            bounds_err(findings, &procedural.id.0, "helix frame is degenerate");
        }
        if (major.norm() - minor.norm()).abs() > 1e-9 {
            bounds_err(
                findings,
                &procedural.id.0,
                "helix major and minor radii differ",
            );
        }
    }
}

fn pcurve_basis_is_valid(geometry: &crate::geometry::PcurveGeometry) -> bool {
    use crate::geometry::PcurveGeometry;

    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    let point = |point: &crate::math::Point2| finite(&[point.u, point.v]);
    let direction = |value: &crate::math::Point2| point(value) && value.u.hypot(value.v) > 0.0;
    match geometry {
        PcurveGeometry::Line {
            origin,
            direction: d,
        } => point(origin) && direction(d),
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            point(center)
                && direction(x_axis)
                && direction(y_axis)
                && radius.is_finite()
                && *radius > 0.0
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        }
        | PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            point(center)
                && direction(x_axis)
                && direction(y_axis)
                && finite(&[*major_radius, *minor_radius])
                && *major_radius > 0.0
                && *minor_radius > 0.0
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => {
            point(vertex)
                && direction(x_axis)
                && direction(y_axis)
                && focal_distance.is_finite()
                && *focal_distance > 0.0
        }
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            point(radial_center)
                && point(radial_cos)
                && point(radial_sin)
                && (direction(radial_cos) || direction(radial_sin))
                && finite(&[*axial_origin, *axial_cos, *axial_sin])
        }
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            axial_control_points,
            weights,
            ..
        } => {
            *degree > 0
                && radial_control_points.len() > *degree as usize
                && axial_control_points.len() == radial_control_points.len()
                && knots.len() == radial_control_points.len() + *degree as usize + 1
                && finite(knots)
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && radial_control_points.iter().all(point)
                && finite(axial_control_points)
                && weights.as_ref().is_none_or(|weights| {
                    weights.len() == radial_control_points.len()
                        && weights
                            .iter()
                            .all(|weight| weight.is_finite() && *weight > 0.0)
                })
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => {
            *degree > 0
                && control_points.len() > *degree as usize
                && knots.len() == control_points.len() + *degree as usize + 1
                && finite(knots)
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && control_points.iter().all(point)
                && weights.as_ref().is_none_or(|weights| {
                    weights.len() == control_points.len()
                        && weights
                            .iter()
                            .all(|weight| weight.is_finite() && *weight > 0.0)
                })
        }
        PcurveGeometry::Trimmed {
            basis,
            parameter_range,
        } => {
            finite(parameter_range)
                && parameter_range[0] <= parameter_range[1]
                && pcurve_basis_is_valid(basis)
        }
        PcurveGeometry::Offset { basis, distance } => {
            distance.is_finite() && pcurve_basis_is_valid(basis)
        }
    }
}

fn valid_affine_transform(transform: crate::transform::Transform) -> bool {
    transform.rows.into_iter().flatten().all(f64::is_finite)
        && transform.rows[3] == [0.0, 0.0, 0.0, 1.0]
}

fn valid_surface_basis(geometry: &SurfaceGeometry) -> bool {
    match geometry {
        SurfaceGeometry::Plane { normal, u_axis, .. } => !degenerate(normal) && !degenerate(u_axis),
        SurfaceGeometry::Cylinder {
            axis,
            ref_direction,
            radius,
            ..
        } => !degenerate(axis) && !degenerate(ref_direction) && !nonpositive(*radius),
        SurfaceGeometry::Cone {
            axis,
            ref_direction,
            radius,
            ratio,
            ..
        } => {
            !degenerate(axis)
                && !degenerate(ref_direction)
                && *radius >= 0.0
                && ratio.is_finite()
                && *ratio > 0.0
        }
        SurfaceGeometry::Sphere {
            axis,
            ref_direction,
            radius,
            ..
        } => !degenerate(axis) && !degenerate(ref_direction) && radius.abs() > f64::EPSILON,
        SurfaceGeometry::Torus {
            axis,
            ref_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            !degenerate(axis)
                && !degenerate(ref_direction)
                && !nonpositive(*major_radius)
                && minor_radius.abs() > f64::EPSILON
        }
        SurfaceGeometry::Nurbs(n) => {
            n.control_points.len() == n.u_count as usize * n.v_count as usize
                && n.u_knots.windows(2).all(|w| w[0] <= w[1])
                && n.v_knots.windows(2).all(|w| w[0] <= w[1])
        }
        SurfaceGeometry::Polygonal {
            vertices,
            triangles,
            chordal_deflection,
        } => valid_polygonal_surface(vertices, triangles, *chordal_deflection),
        SurfaceGeometry::Transformed { basis, transform } => {
            valid_affine_transform(*transform) && valid_surface_basis(basis)
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => true,
    }
}

fn valid_curve_basis(geometry: &CurveGeometry) -> bool {
    match geometry {
        CurveGeometry::Line { direction, .. } => !degenerate(direction),
        CurveGeometry::Circle { axis, radius, .. } => !degenerate(axis) && !nonpositive(*radius),
        CurveGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => !nonpositive(*major_radius) && !nonpositive(*minor_radius),
        CurveGeometry::Parabola {
            axis,
            major_direction,
            focal_distance,
            ..
        } => !degenerate(axis) && !degenerate(major_direction) && !nonpositive(*focal_distance),
        CurveGeometry::Hyperbola {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => {
            !degenerate(axis)
                && !degenerate(major_direction)
                && !nonpositive(*major_radius)
                && !nonpositive(*minor_radius)
        }
        CurveGeometry::Degenerate { point } => {
            [point.x, point.y, point.z].into_iter().all(f64::is_finite)
        }
        CurveGeometry::Nurbs(n) => {
            n.control_points.len() > n.degree as usize && n.knots.windows(2).all(|w| w[0] <= w[1])
        }
        CurveGeometry::Polyline {
            points,
            parameters,
            chordal_deflection,
        } => valid_polyline(points, parameters.as_deref(), *chordal_deflection),
        CurveGeometry::Transformed { basis, transform } => {
            valid_affine_transform(*transform) && valid_curve_basis(basis)
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => true,
    }
}

fn valid_polyline(
    points: &[crate::math::Point3],
    parameters: Option<&[f64]>,
    deflection: f64,
) -> bool {
    points.len() >= 2
        && deflection.is_finite()
        && deflection >= 0.0
        && points
            .iter()
            .all(|point| [point.x, point.y, point.z].into_iter().all(f64::is_finite))
        && parameters.is_none_or(|parameters| {
            parameters.len() == points.len()
                && parameters.iter().all(|value| value.is_finite())
                && (parameters.windows(2).all(|window| window[0] < window[1])
                    || parameters.windows(2).all(|window| window[0] > window[1]))
        })
}

fn valid_polygonal_surface(
    vertices: &[crate::math::Point3],
    triangles: &[[u32; 3]],
    deflection: f64,
) -> bool {
    vertices.len() >= 3
        && !triangles.is_empty()
        && deflection.is_finite()
        && deflection >= 0.0
        && vertices
            .iter()
            .all(|point| [point.x, point.y, point.z].into_iter().all(f64::is_finite))
        && triangles
            .iter()
            .flatten()
            .all(|index| usize::try_from(*index).is_ok_and(|index| index < vertices.len()))
}

fn support_context_is_finite(context: &crate::geometry::IntcurveSupportContext) -> bool {
    context
        .parameter_range
        .iter()
        .all(|value| value.is_finite())
        && context.parameter_range[0] <= context.parameter_range[1]
        && (context.parameter_range[0] != context.parameter_range[1]
            || context
                .sides
                .iter()
                .all(|side| side.pcurve_parameter_range.is_none()))
        && context.sides.iter().all(support_side_mapping_is_finite)
        && context
            .discontinuities
            .iter()
            .flatten()
            .all(|value| value.is_finite())
}

fn support_side_mapping_is_finite(side: &crate::geometry::IntcurveSupportSide) -> bool {
    side.pcurve_parameter_range.is_none_or(|range| {
        side.pcurve.is_some() && range.iter().all(|value| value.is_finite()) && range[0] != range[1]
    })
}

pub(super) fn check_knots(findings: &mut Vec<Finding>, id: &str, knots: &[f64], dir: &str) {
    let issue = if knots.iter().any(|knot| !knot.is_finite()) {
        Some("knot vector contains a non-finite value")
    } else if knots.windows(2).any(|w| w[1] < w[0]) {
        Some("knot vector is not non-decreasing")
    } else {
        None
    };
    if let Some(issue) = issue {
        let label = if dir.is_empty() {
            issue.to_string()
        } else {
            format!("{dir}-{issue}")
        };
        bounds_err(findings, id, &label);
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
mod tests {
    use super::support_context_is_finite;
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry};
    use crate::math::Point2;

    fn context(pcurve: bool, pcurve_parameter_range: Option<[f64; 2]>) -> IntcurveSupportContext {
        IntcurveSupportContext {
            sides: [
                IntcurveSupportSide {
                    surface: None,
                    pcurve: pcurve.then_some(PcurveGeometry::Line {
                        origin: Point2::new(0.0, 0.0),
                        direction: Point2::new(1.0, 0.0),
                    }),
                    pcurve_parameter_range,
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                    pcurve_parameter_range: None,
                },
            ],
            parameter_range: [0.0, 1.0],
            discontinuities: std::array::from_fn(|_| Vec::new()),
        }
    }

    #[test]
    fn support_pcurve_mapping_requires_a_finite_nonzero_pcurve_interval() {
        let mapped = context(true, Some([5.0, 2.0]));
        assert!(support_context_is_finite(&mapped));
        assert_eq!(
            mapped.sides[0].pcurve_parameter(mapped.parameter_range, 0.25),
            Some(4.25)
        );
        assert!(!support_context_is_finite(&context(
            false,
            Some([5.0, 2.0])
        )));
        assert!(!support_context_is_finite(&context(true, Some([2.0, 2.0]))));
        assert!(!support_context_is_finite(&context(
            true,
            Some([f64::NAN, 2.0])
        )));
    }
}
