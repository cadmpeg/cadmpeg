// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, and body/face-edit projection.

use crate::records::{Feature, FeatureContent};
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
    dimension_display, parse_bool, parse_boolean_op, parse_bounded_angle_rad, parse_point3_mm,
    parse_valid_direction, parse_vector3, parse_vector3_mm,
};

fn property_text(feature: &Feature, name: &str) -> Option<String> {
    feature.properties.get(name).cloned()
}

pub(super) fn project_fillet(feature: &Feature) -> FeatureDefinition {
    let radius = if let Some(radius) =
        super::parameter_dimension(feature, &["Radius"]).or_else(|| {
            (!variable_fillet(feature))
                .then(|| super::parameter_dimension(feature, &["D1"]))
                .flatten()
        }) {
        RadiusSpec::Constant { radius }
    } else {
        let mut points = Vec::new();
        let mut valid = true;
        for name in feature.parameters.keys() {
            let Some(index) = name
                .as_str()
                .strip_prefix("Radius")
                .and_then(|index| index.parse::<usize>().ok())
            else {
                continue;
            };
            let parameter = feature.parameters.iter().find_map(|(name, value)| {
                let suffix = name.as_str().strip_prefix("Position")?;
                (suffix.bytes().all(|byte| byte.is_ascii_digit())
                    && (suffix.len() == 1 || !suffix.starts_with('0'))
                    && suffix.parse::<usize>().ok() == Some(index))
                .then_some(value)
            });
            let point = (|| {
                let parameter = parameter?.trim().parse::<f64>().ok()?;
                Some(VariableRadius {
                    parameter: Fraction::new(parameter)?,
                    radius: super::parameter_dimension::<NonNegativeLength>(
                        feature,
                        &[name.as_str()],
                    )?,
                })
            })();
            let Some(point) = point else {
                valid = false;
                break;
            };
            points.push((index, point));
        }
        points.sort_unstable_by_key(|(left, _)| *left);
        let points = if valid
            && points.len() >= 2
            && points
                .iter()
                .enumerate()
                .all(|(expected, (actual, _))| expected == *actual)
        {
            let radii = points.into_iter().map(|(_, point)| point).collect();
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
    FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::FilletGroup {
                edges: property_text(feature, "Edges")
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
                radius,
                tangency_weight: None,
            },
        ),
    })
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

pub(super) fn project_shell(feature: &Feature) -> FeatureDefinition {
    let thickness = super::parameter_dimension(feature, &["Thickness", "D1"]);
    let outward = feature
        .properties
        .get("Outward")
        .and_then(|value| parse_bool(value));
    FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: property_text(feature, "RemovedFaces")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness,
        outward,
        mode: None,
        join: None,
        resolve_intersections: None,
        allow_self_intersections: None,
    })
}

pub(super) fn project_thicken(feature: &Feature) -> FeatureDefinition {
    use cadmpeg_ir::features::ThickenSide;

    let thickness = super::parameter_dimension(feature, &["Thickness", "D1"]);
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
    FeatureDefinition::Operation(FeatureOperation::Thicken {
        faces: property_text(feature, "Faces")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness,
        side,
    })
}

pub(super) fn project_draft(feature: &Feature) -> FeatureDefinition {
    let pull_direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_vector3(value))
        .and_then(cadmpeg_ir::features::FeatureDirection3::new);
    let neutral_plane = property_text(feature, "NeutralPlane")
        .map_or(FaceSelection::Unresolved, FaceSelection::Native);
    let pull = pull_direction.map(|direction| cadmpeg_ir::features::DraftPull {
        direction,
        plane: None,
    });
    let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
        plane: neutral_plane,
        pull,
    };
    FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: property_text(feature, "Faces")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        anchor,
        angle: super::parameter_dimension(feature, &["Angle", "D1"]),
        outward: feature
            .properties
            .get("Outward")
            .and_then(|value| parse_bool(value)),
    })
}

pub(super) fn project_combine(feature: &Feature) -> Option<FeatureDefinition> {
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
        .and_then(|op| op.try_into().ok())?;
    let operands = cadmpeg_ir::features::CombineOperands::new(
        property_text(feature, "Target").map_or(BodySelection::Unresolved, BodySelection::Native),
        property_text(feature, "Tools").map_or(BodySelection::Unresolved, BodySelection::Native),
    )
    .ok()?;
    Some(FeatureDefinition::Operation(FeatureOperation::Combine {
        operands,
        op,
        keep_tools: false,
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

pub(super) fn project_cut_with_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::CutWithSurface {
        targets: property_text(feature, "Targets")
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        tools: property_text(feature, "Tools")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    })
}

pub(super) fn project_delete_body(feature: &Feature) -> Option<FeatureDefinition> {
    let mode = body_retention_mode(feature)?;
    Some(FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: property_text(feature, "Bodies")
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        mode,
    }))
}

pub(super) fn project_delete_face(feature: &Feature) -> Option<FeatureDefinition> {
    let heal = feature
        .properties
        .get("Heal")
        .and_then(|value| parse_bool(value))?;
    let faces = property_text(feature, "Faces")?;
    Some(FeatureDefinition::Operation(FeatureOperation::DeleteFace {
        faces: FaceSelection::Native(faces),
        heal,
    }))
}

pub(super) fn project_replace_face(feature: &Feature) -> Option<FeatureDefinition> {
    if !feature.properties.contains_key("Faces")
        || !feature.properties.contains_key("ReplacementFaces")
    {
        return None;
    }
    let faces = property_text(feature, "Faces")?;
    let replacement = property_text(feature, "ReplacementFaces")?;
    cadmpeg_ir::features::ReplaceFaceOperands::new(
        FaceSelection::Native(faces),
        FaceSelection::Native(replacement),
    )
    .ok()
    .map(|operands| FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands }))
}

pub(super) fn project_move_face(feature: &Feature) -> Option<FeatureDefinition> {
    let motion = (|| {
        let distance = || super::parameter_dimension(feature, &["Distance", "D1"]);
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
                angle: super::parameter_dimension(feature, &["Angle"])?,
            }
        } else {
            return None;
        };
        Some(motion)
    })()?;
    Some(FeatureDefinition::Operation(FeatureOperation::MoveFace {
        faces: property_text(feature, "Faces")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        motion,
    }))
}

pub(super) fn project_move_body(feature: &Feature) -> Option<FeatureDefinition> {
    let (translation, rotation, copies) = (|| {
        let vector = parse_vector3_mm(feature.properties.get("Translation")?)?.get();
        let translation = [
            super::parameter_dimension(feature, &["TranslationX", "DX"]).or_else(|| {
                cadmpeg_ir::scalar::Length::new(vector.x)
                    .map(cadmpeg_ir::features::Dimension::Literal)
            })?,
            super::parameter_dimension(feature, &["TranslationY", "DY"]).or_else(|| {
                cadmpeg_ir::scalar::Length::new(vector.y)
                    .map(cadmpeg_ir::features::Dimension::Literal)
            })?,
            super::parameter_dimension(feature, &["TranslationZ", "DZ"]).or_else(|| {
                cadmpeg_ir::scalar::Length::new(vector.z)
                    .map(cadmpeg_ir::features::Dimension::Literal)
            })?,
        ];
        let rotation = match super::parameter_dimension(feature, &["Rotation"]) {
            Some(angle) => Some(AxisAngle {
                origin: parse_point3_mm(feature.properties.get("RotationOrigin")?)?,
                direction: parse_valid_direction(feature.properties.get("RotationAxis")?)?,
                angle,
            }),
            None => None,
        };
        let copies = super::parameter_dimension(feature, &["Copies"]).or_else(|| {
            feature
                .properties
                .get("Copies")
                .map_or(Some(0), |value| value.trim().parse::<u32>().ok())
                .map(cadmpeg_ir::features::Dimension::Literal)
        })?;
        Some((translation, rotation, copies))
    })()?;
    Some(FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies: property_text(feature, "Bodies")
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        translation,
        rotation,
        copies,
    }))
}

pub(super) fn project_dome(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Dome {
        faces: property_text(feature, "Faces")
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        height: super::parameter_dimension(feature, &["Height", "D1"]),
        elliptical: feature
            .properties
            .get("Elliptical")
            .and_then(|value| parse_bool(value)),
        reverse: feature
            .properties
            .get("Reverse")
            .and_then(|value| parse_bool(value)),
    })
}

pub(super) fn project_flex(feature: &Feature) -> FeatureDefinition {
    let axis = feature
        .properties
        .get("Axis")
        .or_else(|| feature.properties.get("AxisDirection"))
        .and_then(|value| parse_valid_direction(value));
    let angle = super::parameter_dimension(feature, &["Angle"]);
    let factor = super::parameter_dimension(feature, &["Factor"]);
    let distance = super::parameter_dimension(feature, &["Distance"]);
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

pub(super) fn project_scale(feature: &Feature) -> FeatureDefinition {
    let center = match feature.properties.get("CenterType").map(String::as_str) {
        None | Some("Point") => feature
            .properties
            .get("Center")
            .and_then(|value| parse_point3_mm(value))
            .map(ScaleCenter::Point),
        Some("Centroid") => Some(ScaleCenter::Centroid),
        Some("Origin" | "ModelOrigin") => Some(ScaleCenter::ModelOrigin),
        Some("Reference" | "CoordinateSystem") => property_text(feature, "CenterRef")
            .filter(|value| !value.is_empty())
            .map(ScaleCenter::Native),
        Some(_) => None,
    };
    let factor = |name| super::parameter_dimension(feature, &[name]);
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
    FeatureDefinition::Operation(FeatureOperation::Scale {
        bodies: property_text(feature, "Bodies")
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        center,
        factors,
    })
}

pub(super) fn project_chamfer(feature: &Feature) -> FeatureDefinition {
    let length = |name, positional| super::parameter_dimension(feature, &[name, positional]);
    let positional_angle = feature
        .parameters
        .get("D2")
        .filter(|value| parse_bounded_angle_rad(value).is_some())
        .map(|_| "D2");
    let mut ordered_dimensions = feature.content.iter().filter_map(|content| match content {
        FeatureContent::Dimension(name) => feature
            .parameters
            .get(name.as_str())
            .map(|value| (name.as_str(), value)),
        FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
    });
    let ordered_dimensions = (
        ordered_dimensions.next(),
        ordered_dimensions.next(),
        ordered_dimensions.next(),
    );
    let ordered_spec = || match ordered_dimensions {
        (Some((name, _)), None, None) => Some(ChamferSpec::Distance {
            distance: super::parameter_dimension(feature, &[name])?,
        }),
        (Some((first_name, first)), Some((second_name, second)), None) => {
            let first_angle = parse_bounded_angle_rad(first).is_some();
            let second_angle = parse_bounded_angle_rad(second).is_some();
            match (first_angle, second_angle) {
                (false, true) => Some(ChamferSpec::DistanceAngle {
                    distance: super::parameter_dimension(feature, &[first_name])?,
                    angle: super::parameter_dimension(feature, &[second_name])?,
                }),
                (true, false) => Some(ChamferSpec::DistanceAngle {
                    distance: super::parameter_dimension(feature, &[second_name])?,
                    angle: super::parameter_dimension(feature, &[first_name])?,
                }),
                (false, false) => Some(ChamferSpec::TwoDistances {
                    first: super::parameter_dimension(feature, &[first_name])?,
                    second: super::parameter_dimension(feature, &[second_name])?,
                }),
                (true, true) => None,
            }
        }
        _ => None,
    };
    let spec = (|| {
        Some(
            if let Some(name) = feature
                .parameters
                .contains_key("Angle")
                .then_some("Angle")
                .or(positional_angle)
            {
                ChamferSpec::DistanceAngle {
                    distance: length("Distance", "D1")?,
                    angle: super::parameter_dimension(feature, &[name])?,
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
    FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: cadmpeg_ir::features::NonEmptyMembers::one(
            cadmpeg_ir::features::edge_treatments::ChamferGroup {
                edges: property_text(feature, "Edges")
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
                spec,
            },
        ),
        flip_direction: false,
    })
}
