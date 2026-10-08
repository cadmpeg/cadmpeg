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

use crate::history::classify::is_fillet;
use crate::history::literals::{
    admit_literal, dimension_display, parse_angle_rad, parse_bool, parse_boolean_op,
    parse_bounded_angle_rad, parse_length_mm, parse_point3_mm, parse_positive_dimension_length_mm,
    parse_positive_length_mm, parse_valid_direction, parse_vector3, parse_vector3_mm,
};
use std::collections::BTreeMap;

use super::{either_parameter, parameter_literal, property_literal, property_text, property_value};

/// The parameters of a variable-radius fillet: `RadiusN` radii with their
/// `PositionN` fractions along the edge.
fn variable_radius_points(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<Vec<VariableRadius<Fraction, NonNegativeLength>>>, CodecError> {
    const RADII: &str = "scan SLDPRT variable fillet radii";
    const COLLECT: &str = "collect SLDPRT variable fillet radii";
    let mut scratch = ctx.reserve_scoped(0, COLLECT)?;
    // The first position parameter, in key order, naming each canonical index.
    let mut positions = BTreeMap::new();
    for (name, value) in
        ctx.admit_iter(&feature.parameters, "scan SLDPRT variable fillet positions")?
    {
        let Some(suffix) = name.as_str().strip_prefix("Position") else {
            continue;
        };
        if (suffix.len() != 1 && suffix.starts_with('0'))
            || !ctx.all_by(
                suffix.as_bytes(),
                |byte| Ok(byte.is_ascii_digit()),
                "scan SLDPRT fillet position digits",
            )?
        {
            continue;
        }
        let Ok(index) = ctx.parse_text::<usize>(suffix, "parse SLDPRT fillet position index")?
        else {
            continue;
        };
        if !ctx.contains_key_btree_map(&positions, &index, COLLECT)? {
            scratch.with_storage(|| ctx.insert_btree_map(&mut positions, index, value, COLLECT))?;
        }
    }
    let mut points = Vec::new();
    let mut parameters = feature.parameters.iter();
    while let Some((name, radius)) = ctx.next_charged(&mut parameters, RADII)? {
        let Some(suffix) = name.as_str().strip_prefix("Radius") else {
            continue;
        };
        let Ok(index) = ctx.parse_text::<usize>(suffix, "parse SLDPRT fillet radius index")? else {
            continue;
        };
        let Some(parameter) = ctx.get_btree_map(&positions, &index, COLLECT)?.copied() else {
            return Ok(None);
        };
        let parameter = ctx.trim_text(parameter, "trim SLDPRT fillet position")?;
        let Some(parameter) = ctx
            .parse_text::<f64>(parameter, "parse SLDPRT fillet position")?
            .ok()
        else {
            return Ok(None);
        };
        let Some(parameter) = Fraction::new(parameter) else {
            return Ok(None);
        };
        admit_literal(ctx, radius, RADII)?;
        let Some(radius) = parse_positive_length_mm(radius) else {
            return Ok(None);
        };
        let point = VariableRadius {
            parameter,
            radius: NonNegativeLength::from(radius),
        };
        ctx.push_scoped_vec(&mut scratch, &mut points, (index, point), COLLECT)?;
    }
    ctx.sort_unstable_by(
        &mut points,
        |value| &value.0,
        Ord::cmp,
        "sort SLDPRT variable fillet radii",
    )?;
    if points.len() < 2
        || !ctx.all_by(
            points.iter().enumerate(),
            |(expected, (actual, _))| Ok(expected == *actual),
            "scan SLDPRT project_fillet values",
        )?
    {
        return Ok(None);
    }
    let mut radii = Vec::new();
    for (_, point) in ctx.admit_iter(points, "collect SLDPRT variable fillet controls")? {
        ctx.push_vec(&mut radii, point, "collect SLDPRT variable fillet controls")?;
    }
    Ok(Some(radii))
}

pub(super) fn project_fillet(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let stated = match parameter_literal(ctx, feature, "Radius")?.and_then(parse_positive_length_mm)
    {
        Some(radius) => Some(radius),
        None if !variable_fillet(feature) => {
            parameter_literal(ctx, feature, "D1")?.and_then(parse_positive_dimension_length_mm)
        }
        None => None,
    };
    let radius = if let Some(radius) = stated {
        RadiusSpec::Constant { radius }
    } else {
        let (points, storage) =
            ctx.with_scoped_storage("collect SLDPRT variable fillet controls", || {
                Ok::<_, CodecError>(match variable_radius_points(ctx, feature)? {
                    Some(radii) => {
                        cadmpeg_ir::features::edge_treatments::VariableRadii::from_parts(
                            radii, ctx,
                        )?
                        .ok()
                    }
                    None => None,
                })
            })?;
        match points {
            None => {
                drop(storage);
                if ctx.any_by(
                    &feature.parameters,
                    |(name, _)| {
                        let Some(suffix) = name.as_str().strip_prefix("Radius") else {
                            return Ok(false);
                        };
                        Ok(!suffix.is_empty()
                            && ctx.all_by(
                                suffix.as_bytes(),
                                |byte| Ok(byte.is_ascii_digit()),
                                "scan SLDPRT fillet radius digits",
                            )?)
                    },
                    "scan SLDPRT project_fillet map keys",
                )? {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Variable),
                    }
                } else if ctx.contains_key_btree_map(
                    &feature.parameters,
                    "Radius",
                    "scan SLDPRT project_fillet map keys",
                )? || ctx.contains_key_btree_map(
                    &feature.parameters,
                    "D1",
                    "scan SLDPRT project_fillet map keys",
                )? {
                    RadiusSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::RadiusForm::Constant),
                    }
                } else {
                    RadiusSpec::Unresolved { form: None }
                }
            }
            Some(points) => {
                storage.commit()?;
                RadiusSpec::Variable { points }
            }
        }
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Result<bool, cadmpeg_core::CodecError> {
    Ok(is_fillet(feature)
        && if variable_fillet(feature) {
            crate::resolved_features::selections::variable_fillet_dimension_index_for_feature(
                ctx, feature, name,
            )?
            .is_some()
        } else {
            name == "D1"
        }
        && {
            admit_literal(ctx, expression, "classify SLDPRT fillet radius display")?;
            dimension_display(expression).is_some()
        })
}

fn variable_fillet(feature: &Feature) -> bool {
    feature.kind.eq_ignore_ascii_case("VarFillet")
        || feature
            .input_class
            .as_deref()
            .is_some_and(|class| class.eq_ignore_ascii_case("VarFillet_c"))
}

/// A positive thickness from a named or positional dimension.
fn thickness(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<cadmpeg_ir::scalar::PositiveLength>, CodecError> {
    Ok(
        match parameter_literal(ctx, feature, "Thickness")?.and_then(parse_positive_length_mm) {
            Some(thickness) => Some(thickness),
            None => {
                parameter_literal(ctx, feature, "D1")?.and_then(parse_positive_dimension_length_mm)
            }
        },
    )
}

/// A named property read as a boolean literal.
fn property_bool(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<bool>, CodecError> {
    Ok(property_value(ctx, feature, name)?.and_then(parse_bool))
}

pub(super) fn project_shell(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(FeatureOperation::Shell {
        bodies: None,
        removed_faces: property_text(ctx, feature, "RemovedFaces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        thickness: thickness(ctx, feature)?,
        outward: property_bool(ctx, feature, "Outward")?,
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

    let thickness = thickness(ctx, feature)?;
    let both_sides = property_value(ctx, feature, "BothSides")?.map(parse_bool);
    let reverse = property_value(ctx, feature, "Reverse")?.map(parse_bool);
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
    let pull = property_literal(ctx, feature, "Direction")?
        .and_then(parse_vector3)
        .and_then(cadmpeg_ir::features::FeatureDirection3::new)
        .map(|direction| cadmpeg_ir::features::DraftPull {
            direction,
            plane: None,
        });
    let neutral_plane = property_text(ctx, feature, "NeutralPlane")?
        .map_or(FaceSelection::Unresolved, FaceSelection::Native);
    let anchor = cadmpeg_ir::features::DraftAnchor::NeutralPlane {
        plane: neutral_plane,
        pull,
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Draft {
        faces: property_text(ctx, feature, "Faces")?
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        anchor,
        angle: either_parameter(ctx, feature, "Angle", "D1")?
            .and_then(parse_angle_rad)
            .and_then(|angle| cadmpeg_ir::scalar::SlopeAngle::try_from(angle).ok()),
        outward: property_bool(ctx, feature, "Outward")?,
    }))
}

pub(super) fn project_combine(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(op) = property_value(ctx, feature, "Operation")?
        .and_then(parse_boolean_op)
        .and_then(|op| op.try_into().ok())
    else {
        return Ok(None);
    };
    let operands = cadmpeg_ir::features::CombineOperands::new(
        property_text(ctx, feature, "Target")?
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        property_text(ctx, feature, "Tools")?
            .map_or(BodySelection::Unresolved, BodySelection::Native),
        ctx,
    )?
    .ok();
    Ok(operands.map(|operands| {
        FeatureDefinition::Operation(FeatureOperation::Combine {
            operands,
            op,
            keep_tools: false,
        })
    }))
}

/// The retention mode a body-delete record states through its `Mode` property
/// or, without one, its kind token, for the writer.
pub(in crate::history) fn body_retention_mode(feature: &Feature) -> Option<BodyRetentionMode> {
    match retention_mode(
        feature,
        feature.properties.get("Mode").map(String::as_str),
        || Ok::<_, std::convert::Infallible>(feature.kind.trim()),
    ) {
        Ok(mode) => mode,
        Err(never) => match never {},
    }
}

/// Resolve fixed tokens before reading the trimmed kind fallback.
fn retention_mode<'f, E>(
    feature: &'f Feature,
    mode: Option<&str>,
    trimmed_kind: impl FnOnce() -> Result<&'f str, E>,
) -> Result<Option<BodyRetentionMode>, E> {
    let value = mode.unwrap_or(feature.kind.as_str());
    Ok(
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
        } else if trimmed_kind()?.eq_ignore_ascii_case("Body-Delete/Keep") {
            Some(BodyRetentionMode::Unresolved)
        } else {
            None
        },
    )
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
            reverse: property_bool(ctx, feature, "Reverse")?,
        },
    ))
}

pub(super) fn project_delete_body(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let mode = property_value(ctx, feature, "Mode")?;
    let Some(mode) = retention_mode(feature, mode, || {
        ctx.trim_text(&feature.kind, "trim SLDPRT body retention kind")
    })?
    else {
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
    let Some(heal) = property_bool(ctx, feature, "Heal")? else {
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
    let (Some(faces), Some(replacement)) = (
        property_value(ctx, feature, "Faces")?,
        property_value(ctx, feature, "ReplacementFaces")?,
    ) else {
        return Ok(None);
    };
    Ok(cadmpeg_ir::features::ReplaceFaceOperands::new(
        FaceSelection::Native(ctx.copy_retained_text(faces, "retain SLDPRT feature property")?),
        FaceSelection::Native(
            ctx.copy_retained_text(replacement, "retain SLDPRT feature property")?,
        ),
        ctx,
    )?
    .ok()
    .map(|operands| FeatureDefinition::Operation(FeatureOperation::ReplaceFace { operands })))
}

pub(super) fn project_move_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let distance = || -> Result<_, CodecError> {
        Ok(either_parameter(ctx, feature, "Distance", "D1")?.and_then(parse_length_mm))
    };
    let Some(mode) = property_value(ctx, feature, "Mode")? else {
        return Ok(None);
    };
    let motion = if mode.eq_ignore_ascii_case("offset") {
        distance()?.map(|distance| FaceMotion::Offset { distance })
    } else if mode.eq_ignore_ascii_case("translate") {
        match property_literal(ctx, feature, "Direction")?.and_then(parse_valid_direction) {
            Some(direction) => distance()?.map(|distance| FaceMotion::Translate {
                direction,
                distance,
            }),
            None => None,
        }
    } else if mode.eq_ignore_ascii_case("rotate") {
        let axis_origin = property_literal(ctx, feature, "AxisOrigin")?.and_then(parse_point3_mm);
        let axis_dir = match axis_origin {
            Some(_) => {
                property_literal(ctx, feature, "AxisDirection")?.and_then(parse_valid_direction)
            }
            None => None,
        };
        let angle = match axis_dir {
            Some(_) => parameter_literal(ctx, feature, "Angle")?.and_then(parse_angle_rad),
            None => None,
        };
        match (axis_origin, axis_dir, angle) {
            (Some(axis_origin), Some(axis_dir), Some(angle)) => Some(FaceMotion::Rotate {
                axis_origin,
                axis_dir,
                angle,
            }),
            _ => None,
        }
    } else {
        None
    };
    let Some(motion) = motion else {
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
    let Some(translation) =
        property_literal(ctx, feature, "Translation")?.and_then(parse_vector3_mm)
    else {
        return Ok(None);
    };
    let rotation = match parameter_literal(ctx, feature, "Rotation")? {
        Some(angle) => {
            let Some(origin) =
                property_literal(ctx, feature, "RotationOrigin")?.and_then(parse_point3_mm)
            else {
                return Ok(None);
            };
            let Some(direction) =
                property_literal(ctx, feature, "RotationAxis")?.and_then(parse_valid_direction)
            else {
                return Ok(None);
            };
            let Some(angle) = parse_angle_rad(angle) else {
                return Ok(None);
            };
            Some(AxisAngle {
                origin,
                direction,
                angle,
            })
        }
        None => None,
    };
    let copies = match property_value(ctx, feature, "Copies")? {
        Some(value) => match ctx.parse_text::<u32>(
            ctx.trim_text(value, "trim SLDPRT body copy count")?,
            "parse SLDPRT body copy count",
        )? {
            Ok(copies) => copies,
            Err(_) => return Ok(None),
        },
        None => 0,
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
        height: either_parameter(ctx, feature, "Height", "D1")?.and_then(parse_positive_length_mm),
        elliptical: property_bool(ctx, feature, "Elliptical")?,
        reverse: property_bool(ctx, feature, "Reverse")?,
    }))
}

pub(super) fn project_flex(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let axis = match property_literal(ctx, feature, "Axis")? {
        Some(axis) => Some(axis),
        None => property_literal(ctx, feature, "AxisDirection")?,
    }
    .and_then(parse_valid_direction);
    let form = property_value(ctx, feature, "Mode")?.and_then(|value| {
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
    let mode = match form {
        Some(FlexForm::Bending | FlexForm::Twisting) => {
            match parameter_literal(ctx, feature, "Angle")?.and_then(parse_angle_rad) {
                Some(angle) if form == Some(FlexForm::Bending) => FlexMode::Bending { angle },
                Some(angle) => FlexMode::Twisting { angle },
                None => FlexMode::Unresolved { form },
            }
        }
        Some(FlexForm::Tapering) => {
            let factor = match ctx.get_btree_map(&feature.parameters, "Factor", super::FEATURE_LITERAL)? {
                Some(value) => ctx.parse_text::<f64>(
                    ctx.trim_text(value, super::FEATURE_LITERAL)?,
                    super::FEATURE_LITERAL,
                )?.ok().and_then(cadmpeg_ir::scalar::PositiveReal::new),
                None => None,
            };
            factor.map_or(FlexMode::Unresolved { form }, |factor| FlexMode::Tapering { factor })
        }
        Some(FlexForm::Stretching) => parameter_literal(ctx, feature, "Distance")?
            .and_then(parse_length_mm)
            .map_or(FlexMode::Unresolved { form }, |distance| FlexMode::Stretching { distance }),
        None => FlexMode::Unresolved { form },
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Flex {
        axis,
        mode,
    }))
}

pub(super) fn project_scale(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let center = match property_value(ctx, feature, "CenterType")? {
        None | Some("Point") => property_literal(ctx, feature, "Center")?
            .and_then(parse_point3_mm)
            .map(ScaleCenter::Point),
        Some("Centroid") => Some(ScaleCenter::Centroid),
        Some("Origin" | "ModelOrigin") => Some(ScaleCenter::ModelOrigin),
        Some("Reference" | "CoordinateSystem") => property_text(ctx, feature, "CenterRef")?
            .filter(|value| !value.is_empty())
            .map(ScaleCenter::Native),
        Some(_) => None,
    };
    let factor = |name| -> Result<_, CodecError> {
        let Some(value) = ctx.get_btree_map(&feature.parameters, name, super::FEATURE_LITERAL)?
        else {
            return Ok(None);
        };
        Ok(ctx
            .parse_text::<f64>(
                ctx.trim_text(value, super::FEATURE_LITERAL)?,
                super::FEATURE_LITERAL,
            )?
            .ok()
            .and_then(cadmpeg_ir::scalar::NonZeroReal::new))
    };
    let factors = match (
        factor("Factor")?,
        factor("ScaleX")?,
        factor("ScaleY")?,
        factor("ScaleZ")?,
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

/// The first three dimension children naming a parameter, in content order.
fn ordered_dimensions<'f>(
    ctx: &DecodeContext<'_>,
    feature: &'f Feature,
) -> Result<[Option<&'f str>; 3], CodecError> {
    const OPERATION: &str = "scan SLDPRT chamfer dimension order";
    let mut ordered = [None; 3];
    let mut found = 0;
    ctx.any_by(
        &feature.content,
        |content| {
            let FeatureContent::Dimension(name) = content else {
                return Ok(false);
            };
            let Some(value) = ctx.get_btree_map(&feature.parameters, name.as_str(), OPERATION)?
            else {
                return Ok(false);
            };
            ordered[found] = Some(value.as_str());
            found += 1;
            Ok(found == ordered.len())
        },
        OPERATION,
    )?;
    Ok(ordered)
}

pub(super) fn project_chamfer(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let length = |name, positional| -> Result<_, CodecError> {
        Ok(
            match parameter_literal(ctx, feature, name)?.and_then(parse_positive_length_mm) {
                Some(value) => Some(value),
                None => parameter_literal(ctx, feature, positional)?
                    .and_then(parse_positive_dimension_length_mm),
            },
        )
    };
    let ordered_spec = |ordered: [Option<&str>; 3]| -> Result<Option<ChamferSpec>, CodecError> {
        match ordered {
            [Some(distance), None, None] => {
                admit_literal(ctx, distance, super::FEATURE_LITERAL)?;
                Ok(parse_positive_dimension_length_mm(distance)
                    .map(|distance| ChamferSpec::Distance { distance }))
            }
            [Some(first), Some(second), None] => {
                admit_literal(ctx, first, super::FEATURE_LITERAL)?;
                admit_literal(ctx, second, super::FEATURE_LITERAL)?;
                let first_length = parse_positive_dimension_length_mm(first);
                let second_length = parse_positive_dimension_length_mm(second);
                let first_angle = parse_bounded_angle_rad(first);
                let second_angle = parse_bounded_angle_rad(second);
                Ok(
                    match (first_length, second_length, first_angle, second_angle) {
                        (Some(distance), None, None, Some(angle))
                        | (None, Some(distance), Some(angle), None) => {
                            Some(ChamferSpec::DistanceAngle { distance, angle })
                        }
                        (Some(first), Some(second), None, None) => {
                            Some(ChamferSpec::TwoDistances { first, second })
                        }
                        _ => None,
                    },
                )
            }
            _ => Ok(None),
        }
    };
    let angle = match parameter_literal(ctx, feature, "Angle")? {
        Some(angle) => Some(parse_bounded_angle_rad(angle)),
        None => parameter_literal(ctx, feature, "D2")?
            .and_then(parse_bounded_angle_rad)
            .map(Some),
    };
    let stated = if let Some(angle) = angle {
        match (length("Distance", "D1")?, angle) {
            (Some(distance), Some(angle)) => Some(ChamferSpec::DistanceAngle { distance, angle }),
            _ => None,
        }
    } else if let (Some(first), Some(second)) =
        (length("Distance1", "D1")?, length("Distance2", "D2")?)
    {
        Some(ChamferSpec::TwoDistances { first, second })
    } else {
        length("Distance", "D1")?.map(|distance| ChamferSpec::Distance { distance })
    };
    let spec = match stated {
        Some(spec) => spec,
        None => match ordered_spec(ordered_dimensions(ctx, feature)?)? {
            Some(spec) => spec,
            None => {
                let has = |name| {
                    ctx.contains_key_btree_map(&feature.parameters, name, "test SLDPRT map key")
                };
                if has("Angle")? {
                    ChamferSpec::Unresolved {
                        form: Some(
                            cadmpeg_ir::features::edge_treatments::ChamferForm::DistanceAngle,
                        ),
                    }
                } else if has("Distance1")? || has("Distance2")? {
                    ChamferSpec::Unresolved {
                        form: Some(
                            cadmpeg_ir::features::edge_treatments::ChamferForm::TwoDistances,
                        ),
                    }
                } else if has("Distance")? || (has("D1")? && !has("D2")?) {
                    ChamferSpec::Unresolved {
                        form: Some(cadmpeg_ir::features::edge_treatments::ChamferForm::Distance),
                    }
                } else {
                    ChamferSpec::Unresolved { form: None }
                }
            }
        },
    };
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
