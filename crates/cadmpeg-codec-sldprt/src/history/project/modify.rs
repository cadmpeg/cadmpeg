// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, and body/face-edit projection.

use crate::records::{Feature, FeatureContent};
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, ChamferSpec, EdgeSelection, FaceMotion,
    FaceSelection, FeatureDefinition, FlexForm, FlexMode, Length, RadiusSpec, ScaleCenter,
    ScaleFactors, VariableRadius,
};
use cadmpeg_ir::math::Vector3;

use crate::history::classify::{indexed_name, is_fillet};
use crate::history::literals::{
    dimension_display, parse_angle_rad, parse_bool, parse_boolean_op, parse_bounded_angle_rad,
    parse_length_mm, parse_point3_mm, parse_positive_dimension_length_mm, parse_positive_length_mm,
    parse_valid_direction, parse_vector3,
};

pub(crate) fn project_fillet(feature: &Feature) -> FeatureDefinition {
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
        RadiusSpec::Constant {
            radius: Length(radius),
        }
    } else {
        let points = feature
            .parameters
            .iter()
            .filter_map(|(name, radius)| {
                let index = name.strip_prefix("Radius")?.parse::<usize>().ok()?;
                Some((index, radius))
            })
            .map(|(index, radius)| {
                let parameter = feature
                    .parameters
                    .get(&format!("Position{index}"))?
                    .trim()
                    .parse::<f64>()
                    .ok()?;
                let radius = parse_positive_length_mm(radius)?;
                (parameter.is_finite() && (0.0..=1.0).contains(&parameter)).then_some((
                    index,
                    VariableRadius {
                        parameter,
                        radius: Length(radius),
                    },
                ))
            })
            .collect::<Option<Vec<_>>>();
        points
            .and_then(|mut points| {
                points.sort_by_key(|(index, _)| *index);
                (points.len() >= 2
                    && points
                        .iter()
                        .enumerate()
                        .all(|(expected, (actual, _))| expected == *actual))
                .then_some(points)
            })
            .map_or_else(
                || {
                    if feature
                        .parameters
                        .keys()
                        .any(|name| indexed_name(name, "Radius"))
                    {
                        RadiusSpec::UnresolvedVariable
                    } else if feature
                        .parameters
                        .keys()
                        .any(|name| matches!(name.as_str(), "Radius" | "D1"))
                    {
                        RadiusSpec::UnresolvedConstant
                    } else {
                        RadiusSpec::Unresolved
                    }
                },
                |points| RadiusSpec::Variable {
                    points: points.into_iter().map(|(_, point)| point).collect(),
                },
            )
    };
    FeatureDefinition::Fillet {
        groups: vec![cadmpeg_ir::features::FilletGroup {
            edges: feature
                .properties
                .get("Edges")
                .cloned()
                .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
            radius,
            tangency_weight: None,
        }],
    }
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

pub(crate) fn variable_fillet(feature: &Feature) -> bool {
    feature.kind.eq_ignore_ascii_case("VarFillet")
        || feature
            .input_class
            .as_deref()
            .is_some_and(|class| class.eq_ignore_ascii_case("VarFillet_c"))
}

pub(crate) fn project_shell(feature: &Feature) -> FeatureDefinition {
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
    FeatureDefinition::Shell {
        bodies: None,
        removed_faces: feature
            .properties
            .get("RemovedFaces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness: thickness.map(Length),
        outward,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    }
}

pub(crate) fn project_thicken(feature: &Feature) -> FeatureDefinition {
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
    FeatureDefinition::Thicken {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness: thickness.map(Length),
        side,
    }
}

pub(crate) fn project_draft(feature: &Feature) -> FeatureDefinition {
    let pull_direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_vector3(value))
        .filter(|direction| direction.norm().is_finite() && direction.norm() > 0.0);
    let neutral_plane = feature
        .properties
        .get("NeutralPlane")
        .cloned()
        .map_or(FaceSelection::Unresolved, FaceSelection::Native);
    let pull = pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
        direction,
        plane: None,
    });
    let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
        plane: neutral_plane,
        pull,
    };
    FeatureDefinition::Draft {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        anchor,
        angle: feature
            .parameters
            .get("Angle")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_angle_rad(value))
            .map(Angle),
        outward: feature
            .properties
            .get("Outward")
            .and_then(|value| parse_bool(value)),
    }
}

pub(crate) fn project_combine(feature: &Feature) -> Option<FeatureDefinition> {
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))?
        .try_into()
        .ok()?;
    Some(FeatureDefinition::Combine {
        target: feature
            .properties
            .get("Target")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        tools: feature
            .properties
            .get("Tools")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        op,
        keep_tools: false,
    })
}

pub(crate) fn body_retention_mode(feature: &Feature) -> Option<BodyRetentionMode> {
    let value = feature
        .properties
        .get("Mode")
        .map_or(feature.kind.as_str(), String::as_str);
    match value.to_ascii_lowercase().as_str() {
        "delete" | "deletebody" | "body-delete" => Some(BodyRetentionMode::DeleteSelected),
        "keep" | "keepbody" => Some(BodyRetentionMode::KeepSelected),
        _ if feature.xml_tag.eq_ignore_ascii_case("DeleteBody") => {
            Some(BodyRetentionMode::DeleteSelected)
        }
        _ if feature.xml_tag.eq_ignore_ascii_case("KeepBody") => {
            Some(BodyRetentionMode::KeepSelected)
        }
        _ if feature.kind.trim().eq_ignore_ascii_case("Body-Delete/Keep") => {
            Some(BodyRetentionMode::Unresolved)
        }
        _ => None,
    }
}

pub(crate) fn project_cut_with_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::CutWithSurface {
        targets: feature
            .properties
            .get("Targets")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        tools: feature
            .properties
            .get("Tools")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    }
}

pub(crate) fn project_delete_body(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DeleteBody {
        bodies: feature
            .properties
            .get("Bodies")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        mode: body_retention_mode(feature)?,
    })
}

pub(crate) fn project_delete_face(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::DeleteFace {
        faces: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        heal: parse_bool(feature.properties.get("Heal")?)?,
    })
}

pub(crate) fn project_replace_face(feature: &Feature) -> Option<FeatureDefinition> {
    Some(FeatureDefinition::ReplaceFace {
        targets: FaceSelection::Native(feature.properties.get("Faces")?.clone()),
        replacements: FaceSelection::Native(feature.properties.get("ReplacementFaces")?.clone()),
    })
}

pub(crate) fn project_move_face(feature: &Feature) -> Option<FeatureDefinition> {
    let distance = || {
        feature
            .parameters
            .get("Distance")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_length_mm(value))
            .map(Length)
    };
    let motion = match feature
        .properties
        .get("Mode")?
        .to_ascii_lowercase()
        .as_str()
    {
        "offset" => FaceMotion::Offset {
            distance: distance()?,
        },
        "translate" => FaceMotion::Translate {
            direction: parse_valid_direction(feature.properties.get("Direction")?)?,
            distance: distance()?,
        },
        "rotate" => FaceMotion::Rotate {
            axis_origin: parse_point3_mm(feature.properties.get("AxisOrigin")?)?,
            axis_dir: parse_valid_direction(feature.properties.get("AxisDirection")?)?,
            angle: Angle(
                feature
                    .parameters
                    .get("Angle")
                    .and_then(|value| parse_angle_rad(value))?,
            ),
        },
        _ => return None,
    };
    Some(FeatureDefinition::MoveFace {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        motion,
    })
}

pub(crate) fn project_move_body(feature: &Feature) -> Option<FeatureDefinition> {
    let bodies = feature
        .properties
        .get("Bodies")
        .cloned()
        .map_or(BodySelection::Unresolved, BodySelection::Native);
    let translation = parse_point3_mm(feature.properties.get("Translation")?)?;
    let translation = Vector3::new(translation.x, translation.y, translation.z);
    let rotation = match feature.parameters.get("Rotation") {
        Some(angle) => Some(AxisAngle {
            origin: parse_point3_mm(feature.properties.get("RotationOrigin")?)?,
            direction: parse_valid_direction(feature.properties.get("RotationAxis")?)?,
            angle: Angle(parse_angle_rad(angle)?),
        }),
        None => None,
    };
    let copies = feature
        .properties
        .get("Copies")
        .map_or(Some(0), |value| value.trim().parse::<u32>().ok())?;
    Some(FeatureDefinition::MoveBody {
        bodies,
        translation,
        rotation,
        copies,
    })
}

pub(crate) fn project_dome(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Dome {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        height: feature
            .parameters
            .get("Height")
            .or_else(|| feature.parameters.get("D1"))
            .and_then(|value| parse_positive_length_mm(value))
            .map(Length),
        elliptical: feature
            .properties
            .get("Elliptical")
            .and_then(|value| parse_bool(value)),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    }
}

pub(crate) fn project_flex(feature: &Feature) -> FeatureDefinition {
    let axis = feature
        .properties
        .get("Axis")
        .or_else(|| feature.properties.get("AxisDirection"))
        .and_then(|value| parse_valid_direction(value));
    let angle = feature
        .parameters
        .get("Angle")
        .and_then(|value| parse_angle_rad(value))
        .filter(|value| value.is_finite())
        .map(Angle);
    let factor = feature
        .parameters
        .get("Factor")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0);
    let distance = feature
        .parameters
        .get("Distance")
        .and_then(|value| parse_length_mm(value))
        .filter(|value| value.is_finite())
        .map(Length);
    let form = feature.properties.get("Mode").and_then(|value| {
        match value.to_ascii_lowercase().as_str() {
            "bending" | "bend" => Some(FlexForm::Bending),
            "twisting" | "twist" => Some(FlexForm::Twisting),
            "tapering" | "taper" => Some(FlexForm::Tapering),
            "stretching" | "stretch" => Some(FlexForm::Stretching),
            _ => None,
        }
    });
    let mode = match (form, angle, factor, distance) {
        (Some(FlexForm::Bending), Some(angle), _, _) => FlexMode::Bending { angle },
        (Some(FlexForm::Twisting), Some(angle), _, _) => FlexMode::Twisting { angle },
        (Some(FlexForm::Tapering), _, Some(factor), _) => FlexMode::Tapering { factor },
        (Some(FlexForm::Stretching), _, _, Some(distance)) => FlexMode::Stretching { distance },
        (form, _, _, _) => FlexMode::Unresolved(form),
    };
    FeatureDefinition::Flex { axis, mode }
}

pub(crate) fn project_scale(feature: &Feature) -> FeatureDefinition {
    let center = match feature.properties.get("CenterType").map(String::as_str) {
        None | Some("Point") => feature
            .properties
            .get("Center")
            .and_then(|value| parse_point3_mm(value))
            .map(ScaleCenter::Point),
        Some("Centroid") => Some(ScaleCenter::Centroid),
        Some("Origin" | "ModelOrigin") => Some(ScaleCenter::ModelOrigin),
        Some("Reference" | "CoordinateSystem") => feature
            .properties
            .get("CenterRef")
            .filter(|value| !value.is_empty())
            .cloned()
            .map(ScaleCenter::Native),
        Some(_) => None,
    };
    let factor = |name| {
        feature
            .parameters
            .get(name)
            .and_then(|value| value.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value != 0.0)
    };
    let factors = match (
        factor("Factor"),
        factor("ScaleX"),
        factor("ScaleY"),
        factor("ScaleZ"),
    ) {
        (Some(uniform), None, None, None) => ScaleFactors::Uniform(uniform),
        (None, Some(x), Some(y), Some(z)) => ScaleFactors::PerAxis(Vector3::new(x, y, z)),
        _ => ScaleFactors::Unresolved,
    };
    FeatureDefinition::Scale {
        bodies: feature
            .properties
            .get("Bodies")
            .cloned()
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        center,
        factors,
    }
}

pub(crate) fn project_chamfer(feature: &Feature) -> FeatureDefinition {
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
            .map(Length)
    };
    let positional_angle = feature
        .parameters
        .get("D2")
        .filter(|value| parse_bounded_angle_rad(value).is_some());
    let ordered_dimensions = feature
        .content
        .iter()
        .filter_map(|content| match content {
            FeatureContent::Dimension(name) => feature.parameters.get(name),
            FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
        })
        .collect::<Vec<_>>();
    let ordered_spec = || match ordered_dimensions.as_slice() {
        [distance] => Some(ChamferSpec::Distance {
            distance: Length(parse_positive_dimension_length_mm(distance)?),
        }),
        [first, second] => {
            let first_length = parse_positive_dimension_length_mm(first).map(Length);
            let second_length = parse_positive_dimension_length_mm(second).map(Length);
            let first_angle = parse_bounded_angle_rad(first).map(Angle);
            let second_angle = parse_bounded_angle_rad(second).map(Angle);
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
                    angle: Angle(parse_bounded_angle_rad(value)?),
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
            ChamferSpec::UnresolvedDistanceAngle
        } else if feature.parameters.contains_key("Distance1")
            || feature.parameters.contains_key("Distance2")
        {
            ChamferSpec::UnresolvedTwoDistances
        } else if feature.parameters.contains_key("Distance")
            || (feature.parameters.contains_key("D1") && !feature.parameters.contains_key("D2"))
        {
            ChamferSpec::UnresolvedDistance
        } else {
            ChamferSpec::Unresolved
        }
    });
    FeatureDefinition::Chamfer {
        groups: vec![cadmpeg_ir::features::ChamferGroup {
            edges: feature
                .properties
                .get("Edges")
                .cloned()
                .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
            spec,
        }],
        flip_direction: false,
    }
}
