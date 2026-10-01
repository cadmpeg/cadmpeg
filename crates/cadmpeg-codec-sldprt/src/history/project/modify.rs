// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, and body/face-edit projection.

use crate::records::{Feature, FeatureContent};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        edge_treatments::{ChamferSpec, RadiusSpec, VariableRadius},
        AxisAngle, BodyRetentionMode, BodySelection, EdgeSelection, FaceMotion, FaceSelection,
        FeatureDefinition, FeatureOperation, FlexForm, FlexMode, ScaleCenter, ScaleFactors,
    },
    scalar::{Fraction, NonNegativeLength},
};

use crate::history::classify::{indexed_name, is_fillet};
use crate::history::literals::{
    dimension_display, parse_angle_rad, parse_bool, parse_boolean_op, parse_bounded_angle_rad,
    parse_length_mm, parse_point3_mm, parse_positive_dimension_length_mm, parse_positive_length_mm,
    parse_valid_direction, parse_vector3, parse_vector3_mm,
};

fn property_text(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<String>, CodecError> {
    feature
        .properties
        .get(name)
        .map(|value| {
            ctx.format_retained(
                format_args!("{value}"),
                "retain SLDPRT edit selection reference",
            )
        })
        .transpose()
}

pub(super) fn project_fillet(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let radius = if let Some(radius) = feature
        .parameters
        .get("Radius")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            if variable_fillet(feature) {
                None
            } else {
                feature
                    .parameters
                    .get("D1")
                    .and_then(|value| parse_positive_dimension_length_mm(value))
            }
        }) {
        RadiusSpec::Constant { radius }
    } else {
        let mut points = Vec::new();
        let mut valid = true;
        for (name, radius) in &feature.parameters {
            ctx.charge_work(1, "scan SLDPRT variable fillet radii")?;
            let Some(index) = name
                .as_str()
                .strip_prefix("Radius")
                .and_then(|index| index.parse::<usize>().ok())
            else {
                continue;
            };
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(feature.parameters.len()),
                "scan SLDPRT variable fillet positions",
            )?;
            let parameter = feature.parameters.iter().find_map(|(name, value)| {
                let suffix = name.as_str().strip_prefix("Position")?;
                (suffix.bytes().all(|byte| byte.is_ascii_digit())
                    && (suffix.len() == 1 || !suffix.starts_with('0'))
                    && suffix.parse::<usize>().ok() == Some(index))
                .then_some(value)
            });
            let point = (|| {
                let parameter = parameter?.trim().parse::<f64>().ok()?;
                let radius = parse_positive_length_mm(radius)?;
                Some(VariableRadius {
                    parameter: Fraction::new(parameter)?,
                    radius: NonNegativeLength::from(radius),
                })
            })();
            let Some(point) = point else {
                valid = false;
                break;
            };
            ctx.reserve_vec(&mut points, 1, "collect SLDPRT variable fillet radii")?;
            points.push((index, point));
        }
        ctx.sort_unstable_by(
            &mut points,
            |(left, _), (right, _)| left.cmp(right),
            |_| 0,
            "sort SLDPRT variable fillet radii",
        )?;
        let points = if valid
            && points.len() >= 2
            && points
                .iter()
                .enumerate()
                .all(|(expected, (actual, _))| expected == *actual)
        {
            let mut radii = Vec::new();
            ctx.reserve_vec(
                &mut radii,
                points.len(),
                "collect SLDPRT variable fillet controls",
            )?;
            radii.extend(points.into_iter().map(|(_, point)| point));
            cadmpeg_ir::features::edge_treatments::VariableRadii::from_parts(radii).ok()
        } else {
            None
        };
        points.map_or_else(
            || {
                if feature
                    .parameters
                    .keys()
                    .any(|name| indexed_name(name.as_str(), "Radius"))
                {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
                    }
                } else if feature
                    .parameters
                    .keys()
                    .any(|name| matches!(name.as_str(), "Radius" | "D1"))
                {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                    }
                } else {
                    RadiusSpec::Unresolved { form: None }
                }
            },
            |points| RadiusSpec::Variable { points },
        )
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges: property_text(ctx, feature, "Edges")?
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
                radius,
                tangency_weight: None,
            },
        ),
    }))
}

pub(crate) fn fillet_radius_parameter_has_native_display(
    feature: &Feature,
    name: &str,
    expression: &str,
) -> bool {
    is_fillet(feature)
        && if variable_fillet(feature) {
            crate::resolved_features::selections::variable_fillet_dimension_index_for_feature(
                feature, name,
            )
            .is_some()
        } else {
            name == "D1"
        }
        && dimension_display(expression).is_some()
}

fn variable_fillet(feature: &Feature) -> bool {
    feature.kind.eq_ignore_ascii_case("VarFillet")
        || feature
            .input_class
            .as_deref()
            .is_some_and(|class| class.eq_ignore_ascii_case("VarFillet_c"))
}

pub(super) fn project_shell(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let thickness = feature
        .parameters
        .get("Thickness")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            feature
                .parameters
                .get("D1")
                .and_then(|value| parse_positive_dimension_length_mm(value))
        });
    let outward = feature
        .properties
        .get("Outward")
        .and_then(|value| parse_bool(value));
    Ok(FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: property_text(ctx, feature, "RemovedFaces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness,
        outward,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    }))
}

pub(super) fn project_thicken(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    use cadmpeg_ir::features::ThickenSide;

    let thickness = feature
        .parameters
        .get("Thickness")
        .and_then(|value| parse_positive_length_mm(value))
        .or_else(|| {
            feature
                .parameters
                .get("D1")
                .and_then(|value| parse_positive_dimension_length_mm(value))
        });
    let both_sides = feature
        .properties
        .get("BothSides")
        .map(|value| parse_bool(value));
    let reverse = feature
        .properties
        .get("Reverse")
        .map(|value| parse_bool(value));
    let side = match (both_sides, reverse) {
        (Some(Some(true)), Some(Some(true))) | (Some(None), _) | (_, Some(None)) => None,
        (Some(Some(true)), _) => Some(ThickenSide::Both),
        (_, Some(Some(true))) => Some(ThickenSide::Reverse),
        (Some(Some(false)), _) | (_, Some(Some(false))) => Some(ThickenSide::Forward),
        (None, None) => Some(ThickenSide::Forward),
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces: property_text(ctx, feature, "Faces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness,
        side,
    }))
}

pub(super) fn project_draft(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let pull_direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_vector3(value))
        .and_then(cadmpeg_ir::features::FeatureDirection3::new);
    let neutral_plane = property_text(ctx, feature, "NeutralPlane")?
        .map_or(FaceSelection::Unresolved, FaceSelection::Native);
    let pull = pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
        direction,
        plane: None,
    });
    let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
        plane: neutral_plane,
        pull,
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: property_text(ctx, feature, "Faces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        anchor,
        angle: feature
            .parameters
            .get("Angle")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_angle_rad(value))
            .and_then(|angle| cadmpeg_ir::scalar::SlopeAngle::try_from(angle).ok()),
        outward: feature
            .properties
            .get("Outward")
            .and_then(|value| parse_bool(value)),
    }))
}

pub(super) fn project_combine(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(op) = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .and_then(|op| op.try_into().ok())
    else {
        return Ok(None);
    };
    let operands = cadmpeg_ir::features::CombineOperands::new(
        property_text(ctx, feature, "Target")?
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        property_text(ctx, feature, "Tools")?
            .map_or(BodySelection::Unresolved, BodySelection::Native),
    )
    .ok();
    Ok(operands.map(|operands| {
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands,
            op,
            keep_tools: false,
        })
    }))
}

pub(in crate::history) fn body_retention_mode(feature: &Feature) -> Option<BodyRetentionMode> {
    let value = feature
        .properties
        .get("Mode")
        .map_or(feature.kind.as_str(), String::as_str);
    if ["delete", "deletebody", "body-delete"]
        .iter()
        .any(|name| value.eq_ignore_ascii_case(name))
    {
        Some(BodyRetentionMode::DeleteSelected)
    } else if ["keep", "keepbody"]
        .iter()
        .any(|name| value.eq_ignore_ascii_case(name))
    {
        Some(BodyRetentionMode::KeepSelected)
    } else if feature.xml_tag.eq_ignore_ascii_case("DeleteBody") {
        Some(BodyRetentionMode::DeleteSelected)
    } else if feature.xml_tag.eq_ignore_ascii_case("KeepBody") {
        Some(BodyRetentionMode::KeepSelected)
    } else if feature.kind.trim().eq_ignore_ascii_case("Body-Delete/Keep") {
        Some(BodyRetentionMode::Unresolved)
    } else {
        None
    }
}

pub(super) fn project_cut_with_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(
        FeatureOperation::CutWithSurface {
            targets: property_text(ctx, feature, "Targets")?
                .map_or(BodySelection::Unresolved, BodySelection::Native),
            tools: property_text(ctx, feature, "Tools")?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            reverse: feature
                .properties
                .get("Reverse")
                .and_then(|value| parse_bool(value)),
        },
    ))
}

pub(super) fn project_delete_body(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(mode) = body_retention_mode(feature) else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteBody {
            bodies: property_text(ctx, feature, "Bodies")?
                .map_or(BodySelection::Unresolved, BodySelection::Native),
            mode,
        },
    )))
}

pub(super) fn project_delete_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(heal) = feature
        .properties
        .get("Heal")
        .and_then(|value| parse_bool(value))
    else {
        return Ok(None);
    };
    let Some(faces) = property_text(ctx, feature, "Faces")? else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::DeleteFace {
            faces: FaceSelection::Native(faces),
            heal,
        },
    )))
}

pub(super) fn project_replace_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if !feature.properties.contains_key("Faces")
        || !feature.properties.contains_key("ReplacementFaces")
    {
        return Ok(None);
    }
    let (Some(faces), Some(replacement)) = (
        property_text(ctx, feature, "Faces")?,
        property_text(ctx, feature, "ReplacementFaces")?,
    ) else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::features::ReplaceFaceOperands::new(
        FaceSelection::Native(faces),
        FaceSelection::Native(replacement),
    )
    .ok()
    .map(|operands| FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands })))
}

pub(super) fn project_move_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(motion) = (|| {
        let distance = || {
            feature
                .parameters
                .get("Distance")
                .or_else(|| feature.parameters.get("D1"))
                .and_then(|value| parse_length_mm(value))
        };
        let mode = feature.properties.get("Mode")?;
        let motion = if mode.eq_ignore_ascii_case("offset") {
            FaceMotion::Offset {
                distance: distance()?,
            }
        } else if mode.eq_ignore_ascii_case("translate") {
            FaceMotion::Translate {
                direction: parse_valid_direction(feature.properties.get("Direction")?)?,
                distance: distance()?,
            }
        } else if mode.eq_ignore_ascii_case("rotate") {
            FaceMotion::Rotate {
                axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
                axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
                angle: feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            }
        } else {
            return None;
        };
        Some(motion)
    })() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveFace {
            faces: property_text(ctx, feature, "Faces")?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            motion,
        },
    )))
}

pub(super) fn project_move_body(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some((translation, rotation, copies)) = (|| {
        let translation = parse_vector3_mm(feature.properties.get("Translation")?)?;
        let rotation = match feature.parameters.get("Rotation") {
            Some(angle) => Some(AxisAngle {
                origin: parse_point3_mm(feature.properties.get("RotationOrigin")?)?,
                direction: parse_valid_direction(feature.properties.get("RotationAxis")?)?,
                angle: parse_angle_rad(angle)?,
            }),
            None => None,
        };
        let copies = feature
            .properties
            .get("Copies")
            .map_or(Some(0), |value| value.trim().parse::<u32>().ok())?;
        Some((translation, rotation, copies))
    })() else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::MoveBody {
            bodies: property_text(ctx, feature, "Bodies")?
                .map_or(BodySelection::Unresolved, BodySelection::Native),
            translation,
            rotation,
            copies,
        },
    )))
}

pub(super) fn project_dome(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(FeatureOperation::Dome {
        faces: property_text(ctx, feature, "Faces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        height: feature
            .parameters
            .get("Height")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_positive_length_mm(value)),
        elliptical: feature
            .properties
            .get("Elliptical")
            .and_then(|value| parse_bool(value)),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    }))
}

pub(super) fn project_flex(feature: &Feature) -> FeatureDefinition {
    let axis = feature
        .properties
        .get("Axis")
        .or_else(|| feature.properties.get("AxisDirection"))
        .and_then(|value| parse_valid_direction(value));
    let angle = feature
        .parameters
        .get("Angle")
        .and_then(|value| parse_angle_rad(value));
    let factor = feature
        .parameters
        .get("Factor")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .and_then(cadmpeg_ir::scalar::PositiveReal::new);
    let distance = feature
        .parameters
        .get("Distance")
        .and_then(|value| parse_length_mm(value));
    let form = feature.properties.get("Mode").and_then(|value| {
        if ["bending", "bend"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
        {
            Some(FlexForm::Bending)
        } else if ["twisting", "twist"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
        {
            Some(FlexForm::Twisting)
        } else if ["tapering", "taper"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
        {
            Some(FlexForm::Tapering)
        } else if ["stretching", "stretch"]
            .iter()
            .any(|name| value.eq_ignore_ascii_case(name))
        {
            Some(FlexForm::Stretching)
        } else {
            None
        }
    });
    let mode = match (form, angle, factor, distance) {
        (Some(FlexForm::Bending), Some(angle), _, _) => FlexMode::Bending { angle },
        (Some(FlexForm::Twisting), Some(angle), _, _) => FlexMode::Twisting { angle },
        (Some(FlexForm::Tapering), _, Some(factor), _) => FlexMode::Tapering { factor },
        (Some(FlexForm::Stretching), _, _, Some(distance)) => FlexMode::Stretching { distance },
        (form, _, _, _) => FlexMode::Unresolved { form },
    };
    FeatureDefinition::Operation(FeatureOperation::Flex { axis, mode })
}

pub(super) fn project_scale(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let center = match feature.properties.get("CenterType").map(String::as_str) {
        None | Some("Point") => feature
            .properties
            .get("Center")
            .and_then(|value| parse_point3_mm(value))
            .map(ScaleCenter::Point),
        Some("Centroid") => Some(ScaleCenter::Centroid),
        Some("Origin" | "ModelOrigin") => Some(ScaleCenter::ModelOrigin),
        Some("Reference" | "CoordinateSystem") => property_text(ctx, feature, "CenterRef")?
            .filter(|value| !value.is_empty())
            .map(ScaleCenter::Native),
        Some(_) => None,
    };
    let factor = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .and_then(cadmpeg_ir::scalar::NonZeroReal::new)
    };
    let factors = match (
        factor("Factor"),
        factor("ScaleX"),
        factor("ScaleY"),
        factor("ScaleZ"),
    ) {
        (Some(uniform), None, None, None) => ScaleFactors::Uniform { factor: uniform },
        (None, Some(x), Some(y), Some(z)) => ScaleFactors::PerAxis { factors: [x, y, z] },
        _ => ScaleFactors::Unresolved {},
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Scale {
        bodies: property_text(ctx, feature, "Bodies")?
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        center,
        factors,
    }))
}

pub(super) fn project_chamfer(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let length = |name, positional| {
        feature
            .parameters
            .get(name)
            .and_then(|value| parse_positive_length_mm(value))
            .or_else(|| {
                feature
                    .parameters
                    .get(positional)
                    .and_then(|value| parse_positive_dimension_length_mm(value))
            })
    };
    let positional_angle = feature
        .parameters
        .get("D2")
        .filter(|value| parse_bounded_angle_rad(value).is_some());
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(feature.content.len()),
        "scan SLDPRT chamfer dimension order",
    )?;
    let mut ordered_dimensions = feature.content.iter().filter_map(|content| match content {
        FeatureContent::Dimension(name) => feature.parameters.get(name.as_str()),
        FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
    });
    let ordered_dimensions = (
        ordered_dimensions.next(),
        ordered_dimensions.next(),
        ordered_dimensions.next(),
    );
    let ordered_spec = || match ordered_dimensions {
        (Some(distance), None, None) => Some(ChamferSpec::Distance {
            distance: parse_positive_dimension_length_mm(distance)?,
        }),
        (Some(first), Some(second), None) => {
            let first_length = parse_positive_dimension_length_mm(first);
            let second_length = parse_positive_dimension_length_mm(second);
            let first_angle = parse_bounded_angle_rad(first);
            let second_angle = parse_bounded_angle_rad(second);
            match (first_length, second_length, first_angle, second_angle) {
                (Some(distance), None, None, Some(angle))
                | (None, Some(distance), Some(angle), None) => {
                    Some(ChamferSpec::DistanceAngle { distance, angle })
                }
                (Some(first), Some(second), None, None) => {
                    Some(ChamferSpec::TwoDistances { first, second })
                }
                _ => None,
            }
        }
        _ => None,
    };
    let spec = (|| {
        Some(
            if let Some(value) = feature.parameters.get("Angle").or(positional_angle) {
                ChamferSpec::DistanceAngle {
                    distance: length("Distance", "D1")?,
                    angle: parse_bounded_angle_rad(value)?,
                }
            } else if let (Some(first), Some(second)) =
                (length("Distance1", "D1"), length("Distance2", "D2"))
            {
                ChamferSpec::TwoDistances { first, second }
            } else {
                ChamferSpec::Distance {
                    distance: length("Distance", "D1")?,
                }
            },
        )
    })()
    .or_else(ordered_spec)
    .unwrap_or_else(|| {
        if feature.parameters.contains_key("Angle") {
            ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::DistanceAngle),
            }
        } else if feature.parameters.contains_key("Distance1")
            || feature.parameters.contains_key("Distance2")
        {
            ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::TwoDistances),
            }
        } else if feature.parameters.contains_key("Distance")
            || (feature.parameters.contains_key("D1") && !feature.parameters.contains_key("D2"))
        {
            ChamferSpec::Unresolved {
                form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::Distance),
            }
        } else {
            ChamferSpec::Unresolved { form: None }
        }
    });
    Ok(FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::ChamferGroup {
                edges: property_text(ctx, feature, "Edges")?
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
                spec,
            },
        ),
        flip_direction: false,
    }))
}
