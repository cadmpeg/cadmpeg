// SPDX-License-Identifier: Apache-2.0
//! Surface-feature projection.

use super::{
    copy_projected_feature_text, either_parameter, parameter_literal, property_literal,
    property_text, property_value,
};
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef, RuledSurfaceMode,
    SurfaceExtension, TrimRegion,
};
use std::collections::HashMap;

use crate::history::literals::{
    parse_bool, parse_length_mm, parse_positive_length_mm, parse_valid_direction,
};

fn faces(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<FaceSelection, CodecError> {
    Ok(property_text(ctx, feature, name)?.map_or(FaceSelection::Unresolved, FaceSelection::Native))
}

fn property_bool(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    name: &str,
) -> Result<Option<bool>, CodecError> {
    Ok(property_value(ctx, feature, name)?.and_then(parse_bool))
}

pub(super) fn project_offset_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(
        FeatureOperation::OffsetSurface {
            faces: faces(ctx, feature, "Faces")?,
            distance: either_parameter(ctx, feature, "Distance", "D1")?.and_then(parse_length_mm),
        },
    ))
}

pub(super) fn project_knit_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let gap_tolerance = parameter_literal(ctx, feature, "GapTolerance")?
        .and_then(parse_length_mm)
        .and_then(|value| cadmpeg_ir::scalar::NonNegativeLength::try_from(value).ok());
    Ok(FeatureDefinition::Operation(
        FeatureOperation::KnitSurface {
            faces: faces(ctx, feature, "Faces")?,
            merge_entities: property_bool(ctx, feature, "MergeEntities")?,
            create_solid: property_bool(ctx, feature, "CreateSolid")?,
            gap_tolerance,
        },
    ))
}

pub(super) fn project_filled_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let continuity = property_value(ctx, feature, "Continuity")?
        .and_then(crate::feature_schema::parse_surface_continuity);
    Ok(FeatureDefinition::Operation(
        FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(
                property_text(ctx, feature, "Boundary")?
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
            ),
            support_faces: faces(ctx, feature, "SupportFaces")?,
            continuity: continuity.map_or_else(
                cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved,
                cadmpeg_ir::features::FilledSurfaceContinuityState::uniform,
            ),
            merge_result: property_bool(ctx, feature, "MergeResult")?,
        },
    ))
}

pub(super) fn project_trim_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<FeatureDefinition, CodecError> {
    let tool = match property_value(ctx, feature, "Tool")? {
        None => PathRef::Unresolved(ctx.format_retained(
            format_args!("{}:tool", feature.id),
            "retain SLDPRT trim surface tool",
        )?),
        Some(tool) => PathRef::Native(copy_projected_feature_text(
            ctx,
            ctx.get_hash_map(native_by_source, tool, "look up SLDPRT hash key")?
                .copied()
                .unwrap_or(tool),
        )?),
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::TrimSurface {
            faces: faces(ctx, feature, "Faces")?,
            tool,
            keep: property_value(ctx, feature, "Keep")?
                .and_then(crate::feature_schema::parse_trim_region)
                .unwrap_or(TrimRegion::Unresolved),
        },
    ))
}

pub(super) fn project_extend_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(
        FeatureOperation::ExtendSurface {
            faces: faces(ctx, feature, "Faces")?,
            distance: either_parameter(ctx, feature, "Distance", "D1")?
                .and_then(parse_positive_length_mm),
            method: property_value(ctx, feature, "Method")?
                .and_then(crate::feature_schema::parse_surface_extension)
                .unwrap_or(SurfaceExtension::Unresolved),
        },
    ))
}

pub(super) fn project_ruled_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let distance = require!(
        either_parameter(ctx, feature, "Distance", "D1")?.and_then(parse_positive_length_mm)
    );
    let mode_name = require!(property_value(ctx, feature, "Mode")?);
    let mode = if mode_name.eq_ignore_ascii_case("normal") {
        RuledSurfaceMode::Normal { distance }
    } else if mode_name.eq_ignore_ascii_case("tangent") {
        RuledSurfaceMode::Tangent { distance }
    } else if mode_name.eq_ignore_ascii_case("direction") {
        RuledSurfaceMode::Direction {
            direction: require!(
                property_literal(ctx, feature, "Direction")?.and_then(parse_valid_direction)
            ),
            distance,
        }
    } else {
        return Ok(None);
    };
    let edges = require!(property_value(ctx, feature, "Edges")?);
    let support_faces = require!(property_value(ctx, feature, "SupportFaces")?);
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::RuledSurface {
            edges: EdgeSelection::Native(copy_projected_feature_text(ctx, edges)?),
            support_faces: FaceSelection::Native(copy_projected_feature_text(ctx, support_faces)?),
            mode,
            angle: None,
            alternate_face: None,
            corner: None,
        },
    )))
}
