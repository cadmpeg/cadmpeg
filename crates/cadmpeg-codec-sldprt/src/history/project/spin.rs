// SPDX-License-Identifier: Apache-2.0
//! Rib, loft, sweep, and revolve projection.

use super::copy_projected_feature_text;
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
    parse_angle_rad, parse_bool, parse_boolean_op, parse_point3_mm, parse_positive_angle_rad,
    parse_positive_length_mm, parse_valid_direction,
};

pub(super) fn project_rib(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<FeatureDefinition, CodecError> {
    let profile = feature
        .properties
        .get("Profile")
        .map(|profile| {
            copy_projected_feature_text(
                ctx,
                native_by_source
                    .get(profile.as_str())
                    .copied()
                    .unwrap_or(profile.as_str()),
            )
            .map(PlanarProfileRef::native)
        })
        .transpose()?;
    let direction = feature
        .properties
        .get("Direction")
        .and_then(|value| parse_valid_direction(value));
    let draft = match feature.parameters.get("Draft") {
        Some(value) => parse_angle_rad(value)
            .and_then(|angle| cadmpeg_ir::scalar::SlopeAngle::try_from(angle).ok())
            .map_or(RibDraft::Unresolved, RibDraft::Angle),
        None => RibDraft::None,
    };
    Ok(FeatureDefinition::Operation(FeatureOperation::Rib {
        construction: RibConstruction {
            profile,
            direction,
            thickness: feature
                .parameters
                .get("Thickness")
                .or_else(|| feature.parameters.get("D1"))
                .and_then(|value| parse_positive_length_mm(value)),
            side: feature
                .properties
                .get("BothSides")
                .and_then(|value| parse_bool(value))
                .map(|both_sides| {
                    if both_sides {
                        RibSide::Centered
                    } else {
                        RibSide::OneSided
                    }
                }),
            draft,
        },
        op: feature
            .properties
            .get("Operation")
            .and_then(|value| parse_boolean_op(value))
            .unwrap_or(BooleanOp::Unresolved),
    }))
}

pub(super) fn project_loft(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(closed) = feature
        .properties
        .get("Closed")
        .map_or(Some(false), |closed| parse_bool(closed))
    else {
        return Ok(None);
    };
    let sections = project_native_refs(
        ctx,
        feature.properties.get("Profiles").map(String::as_str),
        native_by_source,
        |profile| {
            cadmpeg_ir::features::LoftSection::Profile(ProfileRef::Planar(
                PlanarProfileRef::Native(profile),
            ))
        },
    )?;
    let guides = project_native_refs(
        ctx,
        feature.properties.get("Guides").map(String::as_str),
        native_by_source,
        PathRef::Native,
    )?;
    Ok(Some(FeatureDefinition::Operation(FeatureOperation::Loft {
        sections,
        guidance: cadmpeg_ir::features::LoftGuidance::Guides(guides),
        op: feature
            .properties
            .get("Operation")
            .and_then(|operation| parse_boolean_op(operation))
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
    native_by_source: &HashMap<String, &str>,
    mut wrap: impl FnMut(String) -> T,
) -> Result<Vec<T>, CodecError> {
    let mut references = Vec::new();
    let Some(value) = value else {
        return Ok(references);
    };
    ctx.charge_work(value.len() as u64, "project SLDPRT loft references")?;
    for source in value
        .split(',')
        .map(str::trim)
        .filter(|source| !source.is_empty())
    {
        ctx.reserve_vec(&mut references, 1, "project SLDPRT loft references")?;
        let reference = native_by_source.get(source).copied().unwrap_or(source);
        let reference =
            ctx.format_retained(format_args!("{reference}"), "retain SLDPRT loft reference")?;
        references.push(wrap(reference));
    }
    Ok(references)
}

pub(super) fn project_sweep(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let native_ref = |source: &String| {
        copy_projected_feature_text(
            ctx,
            native_by_source
                .get(source.as_str())
                .copied()
                .unwrap_or(source.as_str()),
        )
    };
    let profile = feature
        .properties
        .get("Profile")
        .map(|source| native_ref(source).map(PlanarProfileRef::native))
        .transpose()?;
    let path = feature
        .properties
        .get("Path")
        .map(|source| native_ref(source).map(PathRef::Native))
        .transpose()?;
    let mode = if feature_input_class(feature, NativeClassKind::SweepReferenceSurface)
        || feature.xml_tag == "Surface-Sweep"
        || feature.kind == "Surface-Sweep"
    {
        SweepMode::Surface {}
    } else if feature_input_class(feature, NativeClassKind::Sweep)
        || feature_input_class(feature, NativeClassKind::SweepCut)
    {
        sweep_mode(feature_sweep_operation(feature))
    } else if let Some(op) = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
    {
        sweep_mode(op)
    } else {
        SweepMode::Unresolved {}
    };
    let Some((twist, scale)) = (|| {
        let twist = match feature.parameters.get("Twist") {
            Some(value) => Some(parse_angle_rad(value)?),
            None => None,
        };
        let scale = match feature.parameters.get("Scale") {
            Some(value) => Some(
                value
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .and_then(cadmpeg_ir::scalar::PositiveReal::new)?,
            ),
            None => None,
        };
        Some((twist, scale))
    })() else {
        return Ok(None);
    };
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

fn feature_sweep_operation(feature: &Feature) -> BooleanOp {
    feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
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
        .unwrap_or(BooleanOp::Unresolved)
}

pub(super) fn project_revolve(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<FeatureDefinition, CodecError> {
    ctx.charge_work(
        feature.content.len() as u64,
        "scan SLDPRT revolve dimension order",
    )?;
    if feature
        .properties
        .get("EndCondition")
        .is_some_and(|condition| condition == "TwoSided")
    {
        ctx.charge_work(
            feature.content.len() as u64,
            "scan SLDPRT revolve dimension order",
        )?;
    }
    let ordered_angle = |ordinal| {
        feature
            .content
            .iter()
            .filter_map(|content| match content {
                FeatureContent::Dimension(name) => feature.parameters.get(name.as_str()),
                FeatureContent::Feature(_) | FeatureContent::Text(_) => None,
            })
            .filter_map(|value| parse_positive_angle_rad(value))
            .nth(ordinal)
    };
    let angle = |name, ordinal| {
        feature
            .parameters
            .get(name)
            .or_else(|| match name {
                "Angle" => feature.parameters.get("D1"),
                "Angle2" => feature.parameters.get("D2"),
                _ => None,
            })
            .and_then(|value| parse_positive_angle_rad(value))
            .or_else(|| ordered_angle(ordinal))
    };
    let extent = match feature.properties.get("EndCondition").map(String::as_str) {
        None | Some("OneSided") => angle("Angle", 0).map(|angle| RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle },
        }),
        Some("Symmetric") => angle("Angle", 0).map(|angle| RevolveExtent::Symmetric {
            termination: AngularTermination::Angle { angle },
        }),
        Some("TwoSided") => angle("Angle", 0)
            .zip(angle("Angle2", 1))
            .map(|(first, second)| RevolveExtent::TwoSided {
                first: AngularTermination::Angle { angle: first },
                second: AngularTermination::Angle { angle: second },
            }),
        Some(_) => None,
    };
    let profile = feature
        .properties
        .get("Profile")
        .and_then(|source| native_by_source.get(source.as_str()))
        .map(|id| copy_projected_feature_text(ctx, id).map(PlanarProfileRef::native))
        .transpose()?;
    let axis = feature
        .properties
        .get("AxisOrigin")
        .and_then(|value| parse_point3_mm(value))
        .zip(
            feature
                .properties
                .get("AxisDirection")
                .and_then(|value| parse_valid_direction(value)),
        )
        .map(|(origin, direction)| RevolutionAxis {
            origin,
            direction,
            reference: None,
        });
    let op = feature
        .properties
        .get("Operation")
        .and_then(|value| parse_boolean_op(value))
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
