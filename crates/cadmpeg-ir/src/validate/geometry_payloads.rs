// SPDX-License-Identifier: Apache-2.0
//! Focused validation checks for geometry payloads.
#![allow(clippy::wildcard_imports)]

use super::*;
const EPS_SPATIAL_CURVE_DIRECTION: f64 = 1.0e-9;
const EPS_HELIX_RADIUS: f64 = 1.0e-9;

const EPS_GEOMETRY_PAYLOADS_LAW_VALID_4_E9: f64 = 1.0e-9;
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

pub(super) fn degenerate(v: &Vector3) -> bool {
    v.norm() <= f64::EPSILON
}

fn variable_blend_value_valid(value: &crate::geometry::VariableBlendValue) -> bool {
    use crate::geometry::VariableBlendValuePayload;
    let finite = |values: &[f64]| values.iter().all(|value| value.is_finite());
    match &value.payload {
        VariableBlendValuePayload::TwoEnds {
            parameters, radii, ..
        } => finite(parameters) && finite(radii),
        VariableBlendValuePayload::FixedWidth {
            parameters, width, ..
        } => finite(parameters) && width.is_finite(),
        VariableBlendValuePayload::EdgeOffset {
            scalars, lengths, ..
        } => finite(scalars) && finite(lengths),
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            terminal,
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && !matches!(terminal, crate::geometry::VariableBlendTerminal::Double(v) if !v.is_finite())
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
            ..
        } => {
            parameter.is_finite()
                && radius.is_finite()
                && points.iter().all(|point| {
                    point.parameter.is_finite()
                        && point.radius.is_finite()
                        && point
                            .tangents
                            .iter()
                            .flatten()
                            .all(|value| value.is_finite())
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
        if let ProceduralSurfaceDefinition::Extrusion {
            parameter_interval,
            direction,
            native_position,
            ..
        } = procedural.definition()
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
                    procedural.id.as_str(),
                    "extrusion interval, direction, or native position is non-finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::LinearSweep { direction, .. } = procedural.definition()
        {
            if ![direction.x, direction.y, direction.z]
                .into_iter()
                .all(f64::is_finite)
                || degenerate(direction)
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "invalid linear-sweep direction",
                );
            }
        }
        if let ProceduralSurfaceDefinition::ParallelOffset { distance, .. } =
            procedural.definition()
        {
            if !distance.is_finite() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "non-finite parallel offset",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Exact { spline } = procedural.definition() {
            let valid = match spline {
                crate::geometry::ExactSpline::Legacy { ranges, .. } => ranges.iter().all(|range| {
                    range.iter().all(|value| value.is_finite()) && range[0] <= range[1]
                }),
                crate::geometry::ExactSpline::Revision { intervals, .. } => intervals
                    .iter()
                    .flatten()
                    .flatten()
                    .all(|value| value.is_finite()),
            };
            if !valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "exact spline surface parameter fields are invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Compound { components } = procedural.definition() {
            if components.iter().any(|item| !item.parameter.is_finite()) {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "compound surface parameters and components are inconsistent",
                );
            }
        }
        if let ProceduralSurfaceDefinition::SubSurface {
            parameter_ranges, ..
        } = procedural.definition()
        {
            if !parameter_ranges
                .iter()
                .flatten()
                .all(|value| value.is_finite())
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "sub-surface parameter interval is not finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Taper {
            parameter, taper, ..
        } = procedural.definition()
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
                    procedural.id.as_str(),
                    "taper surface parameter or subtype tail is not finite",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Loft {
            sections,
            parameters,
            bridge,
            ..
        } = procedural.definition()
        {
            let parameters_valid = match parameters {
                crate::geometry::SplineSurfaceParameters::OrderedRanges { ranges } => {
                    ranges.iter().all(|range| {
                        range[0].is_finite() && range[1].is_finite() && range[0] <= range[1]
                    })
                }
                crate::geometry::SplineSurfaceParameters::RevisionRanges { intervals } => intervals
                    .iter()
                    .flatten()
                    .flatten()
                    .all(|value| value.is_finite()),
            };
            let sections_valid =
                sections
                    .iter()
                    .flat_map(|section| &section.entries)
                    .all(|entry| {
                        entry.parameter.is_finite()
                            && entry.profile.iter().all(|member| {
                                let table = member.form.subdata();
                                table.row_values_are_finite()
                            })
                    });
            let bridge_valid = bridge.iter().all(|token| match token {
                crate::geometry::LoftBridgeToken::Double(value) => value.is_finite(),
                _ => true,
            });
            if !parameters_valid || !sections_valid || !bridge_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::CompoundLoft { construction } = procedural.definition()
        {
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
                    table.row_values_are_finite()
                        && data.direction.as_ref().is_none_or(&vector_finite)
                })
            });
            if !leading_scale_shape_valid || !tail_valid || !scales_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "compound loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } =
            procedural.definition()
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
                crate::geometry::ScaledCompoundLoftBranch::Direct { direction, .. } => {
                    match direction {
                        crate::geometry::CompoundLoftDirection::Vector { value } => {
                            vector_finite(value)
                        }
                        crate::geometry::CompoundLoftDirection::Curve { .. } => true,
                    }
                }
            };
            let scales_valid = scales.iter().all(|scale| {
                scale.members.iter().all(|member| {
                    let data = &member.data;
                    let table = &data.subdata;
                    table.row_values_are_finite()
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
                    procedural.id.as_str(),
                    "scaled compound loft construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Law { construction } = procedural.definition() {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Text { value } => !value.is_empty(),
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
                    crate::geometry::LawExpression::TransformVec { vectors, scale, .. } => {
                        scale.is_finite()
                            && vectors.iter().all(|value| {
                                value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                            })
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
                formula.variables().iter().all(|value| law_valid(value, 0))
            };
            let tail_valid = match &construction.tail {
                crate::geometry::LawSurfaceTail::Summary { parameters, .. } => {
                    parameters.iter().flatten().all(|value| value.is_finite())
                }
                crate::geometry::LawSurfaceTail::None {
                    parameter_ranges, ..
                } => parameter_ranges
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite()),
                _ => true,
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
                    procedural.id.as_str(),
                    "law surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Skin { construction } = procedural.definition() {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null => true,
                    crate::geometry::LawExpression::Text { value } => !value.is_empty(),
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
                    crate::geometry::LawExpression::TransformVec { vectors, scale, .. } => {
                        scale.is_finite()
                            && vectors.iter().all(|value| {
                                value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                            })
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
                    profiles.iter().all(|profile| {
                        let table = &profile.data.subdata;
                        table.row_values_are_finite()
                            && profile.data.direction.as_ref().is_none_or(&vector_finite)
                    })
                }
                crate::geometry::SkinSurfaceLayout::Compact { subdata, .. } => {
                    subdata.row_values_are_finite()
                }
            };
            let formula_valid = construction
                .formula
                .variables()
                .iter()
                .all(|variable| law_valid(variable, 0));
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
                    procedural.id.as_str(),
                    "skin surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Net { construction } = procedural.definition() {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Text { value } => !value.is_empty(),
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
                    crate::geometry::LawExpression::TransformVec { vectors, scale, .. } => {
                        scale.is_finite()
                            && vectors.iter().all(|value| {
                                value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                            })
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
                            let table = member.form.subdata();
                            table.row_values_are_finite()
                        })
                })
            });
            let formulas_valid = construction.formulas.iter().all(|formula| {
                formula
                    .variables()
                    .iter()
                    .all(|variable| law_valid(variable, 0))
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
                    procedural.id.as_str(),
                    "net surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Sweep {
            native: Some(construction),
            ..
        } = procedural.definition()
        {
            fn law_valid(expression: &crate::geometry::LawExpression, depth: usize) -> bool {
                if depth > 64 {
                    return false;
                }
                match expression {
                    crate::geometry::LawExpression::Null
                    | crate::geometry::LawExpression::Integer { .. } => true,
                    crate::geometry::LawExpression::Text { value } => !value.is_empty(),
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
                    crate::geometry::LawExpression::TransformVec { vectors, scale, .. } => {
                        scale.is_finite()
                            && vectors.iter().all(|value| {
                                value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
                            })
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
                formula
                    .variables()
                    .iter()
                    .all(|variable| law_valid(variable, 0))
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
                    procedural.id.as_str(),
                    "sweep surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::TSpline { construction } = procedural.definition() {
            let ranges_valid = construction
                .parameter_ranges
                .iter()
                .flatten()
                .chain(construction.discontinuities.iter().flatten())
                .all(|value| value.is_finite());
            let source_valid = construction.subtransform.inline().is_some();
            if !ranges_valid || !source_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "T-spline surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Helix { construction } = procedural.definition() {
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
                && (major_length - minor_length).abs()
                    <= EPS_GEOMETRY_PAYLOADS_LAW_VALID_4_E9 * major_length.max(1.0);
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
                    procedural.id.as_str(),
                    "helix surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Deformable { construction } = procedural.definition() {
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
                crate::geometry::DeformableSurfaceData::RevisionMode3 {
                    leading_vectors,
                    leading_parameter,
                    trailing_point,
                    trailing_vectors,
                    frame_parameter,
                    parameters,
                    trailing_parameter,
                    ..
                } => {
                    leading_vectors.iter().all(vector_finite)
                        && leading_parameter.is_finite()
                        && trailing_point.x.is_finite()
                        && trailing_point.y.is_finite()
                        && trailing_point.z.is_finite()
                        && trailing_vectors.iter().all(vector_finite)
                        && frame_parameter.is_finite()
                        && parameters.iter().all(|value| value.is_finite())
                        && trailing_parameter.is_finite()
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
                    procedural.id.as_str(),
                    "deformable surface construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::G2Blend { construction } = procedural.definition() {
            let direction_finite = |direction: &Vector3| {
                direction.x.is_finite() && direction.y.is_finite() && direction.z.is_finite()
            };
            let first_shape_valid = match &construction.first_shape {
                crate::geometry::G2BlendFirstShape::Full { .. } => true,
                crate::geometry::G2BlendFirstShape::None {
                    coefficients,
                    extension,
                    ..
                } => {
                    coefficients.iter().all(|value| value.is_finite())
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
                    procedural.id.as_str(),
                    "G2 blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::VariableBlend { construction } = procedural.definition()
        {
            let ranges_valid = construction.u_range.iter().all(|value| value.is_finite())
                && construction.u_range[0] <= construction.u_range[1]
                && construction.v_lower.is_none_or(f64::is_finite)
                && [&construction.post_range, &construction.slice_range]
                    .into_iter()
                    .chain(
                        construction
                            .secondary_curve
                            .as_ref()
                            .map(|curve| &curve.parameter_range),
                    )
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
            let values_valid = match &construction.radii {
                crate::geometry::VariableBlendRadii::Single { value } => {
                    variable_blend_value_valid(value)
                }
                crate::geometry::VariableBlendRadii::Two { first, second } => {
                    variable_blend_value_valid(first) && variable_blend_value_valid(second)
                }
            } && construction.cross_section.as_ref().is_none_or(
                |cross_section| match cross_section {
                    crate::geometry::VariableBlendCrossSection::Circular => true,
                    crate::geometry::VariableBlendCrossSection::Thumbweights { parameters }
                    | crate::geometry::VariableBlendCrossSection::G2Round { parameters } => {
                        parameters.iter().all(|value| value.is_finite())
                    }
                    crate::geometry::VariableBlendCrossSection::RoundedChamfer { radius } => {
                        radius.as_deref().is_none_or(variable_blend_value_valid)
                    }
                    crate::geometry::VariableBlendCrossSection::UnclassifiedBare { .. } => true,
                },
            );
            let scalar_tail_valid = construction.offsets.iter().all(|value| value.is_finite())
                && construction.shape_parameter.is_finite()
                && construction.shape_length.is_finite();
            if !ranges_valid || !sides_valid || !values_valid || !scalar_tail_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "variable blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::VertexBlend { construction } = procedural.definition() {
            let point_finite = |point: &crate::math::Point3| {
                point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
            };
            let vector_finite = |vector: &Vector3| {
                vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
            };
            let boundaries_valid = construction.boundaries.iter().all(|boundary| {
                vector_finite(&boundary.magic)
                    && boundary.fullness.is_finite()
                    && match &boundary.geometry {
                        crate::geometry::VertexBlendBoundaryGeometry::Circle {
                            twists,
                            parameters,
                            ..
                        } => {
                            twists.entries().iter().all(&point_finite)
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
                        crate::geometry::VertexBlendBoundaryGeometry::Pcurve { .. } => true,
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
            if !boundaries_valid {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "vertex blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Blend {
            native: Some(construction),
            ..
        } = procedural.definition()
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
                    procedural.id.as_str(),
                    "rolling-ball blend construction payload is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Offset { distance, .. } = procedural.definition() {
            if !distance.is_finite() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "offset spline surface distance is invalid",
                );
            }
        }
        if let ProceduralSurfaceDefinition::Subset {
            parameter_ranges, ..
        } = procedural.definition()
        {
            if !parameter_ranges
                .iter()
                .all(|range| range[0].is_finite() && range[1].is_finite() && range[0] != range[1])
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "surface subset ranges are not finite and non-zero",
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
        if let ProceduralCurveDefinition::Compound {
            parameters,
            components,
        } = procedural.definition()
        {
            if components.is_empty() {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "compound components are empty",
                );
            }
            if parameters
                .iter()
                .chain(components.iter().map(|item| &item.parameter))
                .any(|value| !value.is_finite())
            {
                bounds_err(
                    findings,
                    procedural.id.as_str(),
                    "compound parameters are not finite",
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
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        } = procedural.definition()
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
                procedural.id.as_str(),
                "helix fields are not finite and ordered",
            );
        }
        if degenerate(major) || degenerate(minor) || degenerate(axis) {
            bounds_err(
                findings,
                procedural.id.as_str(),
                "helix frame is degenerate",
            );
        }
        if (major.norm() - minor.norm()).abs() > EPS_HELIX_RADIUS {
            bounds_err(
                findings,
                procedural.id.as_str(),
                "helix major and minor radii differ",
            );
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
