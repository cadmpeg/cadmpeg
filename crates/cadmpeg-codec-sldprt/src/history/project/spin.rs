// SPDX-License-Identifier: Apache-2.0
//! Rib, loft, sweep, and revolve projection.

use super::{
    copy_projected_feature_text, either_parameter, parameter_literal, property_literal,
    property_value,
};
use crate::classification::{native_object_class, NativeClassKind};
use crate::records::{Feature, FeatureContent};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    AngularTermination, BooleanOp, FeatureDefinition, FeatureOperation, PartialRevolveConstruction,
    PathRef, PlanarProfileRef, ProfileRef, RevolutionAxis, RevolveConstruction, RevolveExtent,
    RibConstruction, RibDraft, RibSide, SweepMode,
};
use std::collections::HashMap;

use crate::history::classify::{feature_input_class, loft_op};
use crate::history::literals::{
    admit_literal, parse_angle_rad, parse_bool, parse_boolean_op, parse_point3_mm,
    parse_positive_angle_rad, parse_positive_length_mm, parse_valid_direction,
};

pub(super) fn project_rib(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<FeatureDefinition, CodecError> {
    let profile = property_value(ctx, feature, "Profile")?
        .map(|profile| native_ref(ctx, native_by_source, profile).map(PlanarProfileRef::native))
        .transpose()?;
    let direction = property_literal(ctx, feature, "Direction")?.and_then(parse_valid_direction);
    let draft = match parameter_literal(ctx, feature, "Draft")? {
        Some(value) => parse_angle_rad(value)
            .and_then(|angle| cadmpeg_ir::scalar::SlopeAngle::try_from(angle).ok())
            .map_or(RibDraft::Unresolved, RibDraft::Angle),
        None => RibDraft::None,
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Rib {
        construction: RibConstruction {
            profile,
            direction,
            thickness: either_parameter(ctx, feature, "Thickness", "D1")?
                .and_then(parse_positive_length_mm),
            side: property_value(ctx, feature, "BothSides")?
                .and_then(parse_bool)
                .map(|both_sides| {
                    if both_sides {
                        RibSide::Centered
                    } else {
                        RibSide::OneSided
                    }
                }),
            draft,
        },
        op: property_value(ctx, feature, "Operation")?
            .and_then(parse_boolean_op)
            .unwrap_or(BooleanOp::Unresolved),
    }))
}

/// A copy of the native record a source names, or of the source text itself.
fn native_ref(
    ctx: &DecodeContext<'_>,
    native_by_source: &HashMap<&str, &str>,
    source: &str,
) -> Result<String, CodecError> {
    copy_projected_feature_text(
        ctx,
        ctx.get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
            .copied()
            .unwrap_or(source),
    )
}

pub(super) fn project_loft(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(closed) = property_value(ctx, feature, "Closed")?.map_or(Some(false), parse_bool)
    else {
        return Ok(None);
    };
    let sections = project_native_refs(
        ctx,
        property_value(ctx, feature, "Profiles")?,
        native_by_source,
        |profile| {
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(
                PlanarProfileRef::Native(profile),
            ))
        },
    )?;
    let guides = project_native_refs(
        ctx,
        property_value(ctx, feature, "Guides")?,
        native_by_source,
        PathRef::Native,
    )?;
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
        op: property_value(ctx, feature, "Operation")?
            .and_then(parse_boolean_op)
            .or_else(|| {
                matches!(
                    feature.input_class.as_deref().map(native_object_class),
                    Some(NativeClassKind::LoftCut)
                )
                .then_some(BooleanOp::Cut)
            })
            .or_else(|| loft_op(&feature.kind))
            .unwrap_or(BooleanOp::Unresolved),
        closed,
        solid: !matches!(
            feature.input_class.as_deref().map(native_object_class),
            Some(NativeClassKind::SurfaceLoft)
        ),
        ruled: false,
        linearize: false,
        max_degree: None,
        allow_multi_profile_faces: None,
    })))
}

fn project_native_refs<T>(
    ctx: &DecodeContext<'_>,
    value: Option<&str>,
    native_by_source: &HashMap<&str, &str>,
    mut wrap: impl FnMut(String) -> T,
) -> Result<Vec<T>, CodecError> {
    const OPERATION: &str = "project SLDPRT loft references";
    let mut references = Vec::new();
    let Some(value) = value else {
        return Ok(references);
    };
    let mut characters = value.char_indices();
    let mut start = 0;
    let mut finished = false;
    while !finished {
        let delimiter = ctx.find_map(
            &mut characters,
            |(offset, character)| Ok((character == ',').then_some(offset)),
            OPERATION,
        )?;
        let end = delimiter.unwrap_or(value.len());
        finished = delimiter.is_none();
        let source = ctx.trim_text(&value[start..end], OPERATION)?;
        start = end + usize::from(!finished);
        if source.is_empty() {
            continue;
        }
        let reference = ctx
            .get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
            .copied()
            .unwrap_or(source);
        let reference = ctx.copy_retained_text(reference, "retain SLDPRT loft reference")?;
        ctx.push_vec(&mut references, wrap(reference), OPERATION)?;
    }
    Ok(references)
}

pub(super) fn project_sweep(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let mode = if feature_input_class(feature, NativeClassKind::SweepReferenceSurface)
        || feature.xml_tag == "Surface-Sweep"
        || feature.kind == "Surface-Sweep"
    {
        SweepMode::Surface {}
    } else if feature_input_class(feature, NativeClassKind::Sweep)
        || feature_input_class(feature, NativeClassKind::SweepCut)
    {
        sweep_mode(feature_sweep_operation(ctx, feature)?)
    } else if let Some(op) = property_value(ctx, feature, "Operation")?.and_then(parse_boolean_op) {
        sweep_mode(op)
    } else {
        SweepMode::Unresolved {}
    };
    let twist = match parameter_literal(ctx, feature, "Twist")? {
        Some(value) => match parse_angle_rad(value) {
            Some(twist) => Some(twist),
            None => return Ok(None),
        },
        None => None,
    };
    let scale = match ctx.get_btree_map(&feature.parameters, "Scale", super::FEATURE_LITERAL)? {
        Some(value) => match ctx
            .parse_text::<f64>(
                ctx.trim_text(value, super::FEATURE_LITERAL)?,
                super::FEATURE_LITERAL,
            )?
            .ok()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)
        {
            Some(scale) => Some(scale),
            None => return Ok(None),
        },
        None => None,
    };
    let profile = property_value(ctx, feature, "Profile")?
        .map(|source| native_ref(ctx, native_by_source, source).map(PlanarProfileRef::native))
        .transpose()?;
    let path = property_value(ctx, feature, "Path")?
        .map(|source| native_ref(ctx, native_by_source, source).map(PathRef::Native))
        .transpose()?;
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::Sweep {
            shape: cadmpeg_ir::features::SweepShape::sheet_sections(
                mode,
                profile.map_or(
                    cadmpeg_ir::features::SweepSection::Unresolved(None),
                    cadmpeg_ir::features::SweepSection::Profile,
                ),
                Vec::new(),
            ),

            path,

            orientation: None,
            transition: None,
            transformation: None,
            path_tangent: false,
            linearize: false,
            twist,
            path_extent: None,
            guide_rail: None,
            taper: None,
            scale,
            allow_multi_profile_faces: None,
        },
    )))
}

fn sweep_mode(op: BooleanOp) -> SweepMode {
    match op {
        BooleanOp::Unresolved => SweepMode::Unresolved {},
        BooleanOp::NewBody => SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
        },
        BooleanOp::Join => SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::Join,
        },
        BooleanOp::Cut => SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::Cut,
        },
        BooleanOp::Intersect => SweepMode::Solid {
            op: cadmpeg_ir::features::SolidSweepOperation::Intersect,
        },
    }
}

fn feature_sweep_operation(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<BooleanOp, CodecError> {
    Ok(property_value(ctx, feature, "Operation")?
        .and_then(parse_boolean_op)
        .or_else(|| {
            matches!(
                feature.input_class.as_deref().map(native_object_class),
                Some(NativeClassKind::SweepCut)
            )
            .then_some(BooleanOp::Cut)
        })
        .or_else(|| {
            feature
                .kind
                .eq_ignore_ascii_case("Cut-Sweep")
                .then_some(BooleanOp::Cut)
        })
        .unwrap_or(BooleanOp::Unresolved))
}

pub(super) fn project_revolve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<&str, &str>,
) -> Result<FeatureDefinition, CodecError> {
    let mut content = feature.content.iter();
    let mut ordered = [None; 2];
    let mut found = 0;
    let mut exhausted = false;
    let mut ordered_angle =
        |ordinal: usize| -> Result<Option<cadmpeg_ir::scalar::PositiveAngle>, CodecError> {
            const OPERATION: &str = "scan SLDPRT project_revolve values";
            while found <= ordinal && !exhausted {
                let angle = ctx.find_map(
                    content.by_ref(),
                    |content| {
                        let FeatureContent::Dimension(name) = content else {
                            return Ok(None);
                        };
                        let Some(value) = ctx.get_btree_map(
                            &feature.parameters,
                            name.as_str(),
                            "look up SLDPRT ordered key",
                        )?
                        else {
                            return Ok(None);
                        };
                        admit_literal(ctx, value, OPERATION)?;
                        Ok(parse_positive_angle_rad(value))
                    },
                    OPERATION,
                )?;
                match angle {
                    Some(angle) => {
                        ordered[found] = Some(angle);
                        found += 1;
                    }
                    None => exhausted = true,
                }
            }
            Ok(ordered[ordinal])
        };
    let mut angle = |name, ordinal| -> Result<_, CodecError> {
        let positional = match name {
            "Angle" => "D1",
            _ => "D2",
        };
        let value =
            either_parameter(ctx, feature, name, positional)?.and_then(parse_positive_angle_rad);
        match value {
            Some(value) => Ok(Some(value)),
            None => ordered_angle(ordinal),
        }
    };
    let extent = match property_value(ctx, feature, "EndCondition")? {
        None | Some("OneSided") => angle("Angle", 0)?.map(|angle| RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle },
        }),
        Some("Symmetric") => angle("Angle", 0)?.map(|angle| RevolveExtent::Symmetric {
            termination: AngularTermination::Angle { angle },
        }),
        Some("TwoSided") => angle("Angle", 0)?
            .zip(angle("Angle2", 1)?)
            .map(|(first, second)| RevolveExtent::TwoSided {
                first: AngularTermination::Angle { angle: first },
                second: AngularTermination::Angle { angle: second },
            }),
        Some(_) => None,
    };
    let profile = match property_value(ctx, feature, "Profile")? {
        Some(source) => ctx
            .get_hash_map(native_by_source, source, "look up SLDPRT hash key")?
            .map(|id| copy_projected_feature_text(ctx, id).map(PlanarProfileRef::native))
            .transpose()?,
        None => None,
    };
    let axis_origin = property_literal(ctx, feature, "AxisOrigin")?.and_then(parse_point3_mm);
    let axis_direction =
        property_literal(ctx, feature, "AxisDirection")?.and_then(parse_valid_direction);
    let axis = axis_origin
        .zip(axis_direction)
        .map(|(origin, direction)| RevolutionAxis {
            origin,
            direction,
            reference: None,
        });
    let op = property_value(ctx, feature, "Operation")?
        .and_then(parse_boolean_op)
        .or_else(|| {
            (feature.input_class.as_deref() == Some("moRevCut_c")).then_some(BooleanOp::Cut)
        })
        .unwrap_or(BooleanOp::Unresolved);
    let solid = Some(true);
    Ok(FeatureDefinition::Operation(FeatureOperation::Revolve {
        construction: match (profile, axis, extent) {
            (None, axis, extent) => {
                RevolveConstruction::Unresolved(PartialRevolveConstruction::Profile {
                    axis,
                    extent,
                    solid,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                })
            }
            (Some(profile), None, extent) => {
                RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
                    profile,
                    extent,
                    solid,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                })
            }
            (Some(profile), Some(axis), None) => {
                RevolveConstruction::Unresolved(PartialRevolveConstruction::Extent {
                    profile,
                    axis,
                    solid,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                })
            }
            (Some(profile), Some(axis), Some(extent)) => RevolveConstruction::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker: None,
                fuse_order: None,
                allow_multi_profile_faces: None,
            },
        },
        op,
    }))
}
