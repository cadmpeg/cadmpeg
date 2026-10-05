// SPDX-License-Identifier: Apache-2.0
//! Surface-feature projection.

use super::copy_projected_feature_text;
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

pub(super) fn project_offset_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    Ok(FeatureDefinition::Operation(
        FeatureOperation::OffsetSurface {
            faces: feature
                .properties
                .get("Faces")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            distance: feature
                .parameters
                .get("Distance")
                .or_else(|| feature.parameters.get("D1"))
                .and_then(|value| parse_length_mm(value)),
        },
    ))
}

pub(super) fn project_knit_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let gap_tolerance = match feature.parameters.get("GapTolerance") {
        Some(value) => parse_length_mm(value)
            .and_then(|value| cadmpeg_ir::scalar::NonNegativeLength::try_from(value).ok()),
        None => None,
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::KnitSurface {
            faces: feature
                .properties
                .get("Faces")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            merge_entities: feature
                .properties
                .get("MergeEntities")
                .and_then(|value| parse_bool(value)),
            create_solid: feature
                .properties
                .get("CreateSolid")
                .and_then(|value| parse_bool(value)),
            gap_tolerance,
        },
    ))
}

pub(super) fn project_filled_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let continuity = feature
        .properties
        .get("Continuity")
        .and_then(|value| crate::feature_schema::parse_surface_continuity(value));
    Ok(FeatureDefinition::Operation(
        FeatureOperation::FilledSurface {
            boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(
                feature
                    .properties
                    .get("Boundary")
                    .map(|value| copy_projected_feature_text(ctx, value))
                    .transpose()?
                    .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
            ),
            support_faces: feature
                .properties
                .get("SupportFaces")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            continuity: continuity.map_or_else(
                cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved,
                cadmpeg_ir::features::FilledSurfaceContinuityState::uniform,
            ),
            merge_result: feature
                .properties
                .get("MergeResult")
                .and_then(|value| parse_bool(value)),
        },
    ))
}

pub(super) fn project_trim_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> Result<FeatureDefinition, CodecError> {
    let tool = match feature.properties.get("Tool") {
        None => PathRef::Unresolved(ctx.format_retained(
            format_args!("{}:tool", feature.id),
            "retain SLDPRT trim surface tool",
        )?),
        Some(tool) => PathRef::Native(copy_projected_feature_text(
            ctx,
            ctx.get_hash_map(
                &(native_by_source),
                tool.as_str(),
                "look up SLDPRT hash key",
            )?
            .copied()
            .unwrap_or(tool.as_str()),
        )?),
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::TrimSurface {
            faces: feature
                .properties
                .get("Faces")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            tool,
            keep: feature
                .properties
                .get("Keep")
                .and_then(|value| crate::feature_schema::parse_trim_region(value))
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
            faces: feature
                .properties
                .get("Faces")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            distance: feature
                .parameters
                .get("Distance")
                .or_else(|| feature.parameters.get("D1"))
                .and_then(|value| parse_positive_length_mm(value)),
            method: feature
                .properties
                .get("Method")
                .and_then(|value| crate::feature_schema::parse_surface_extension(value))
                .unwrap_or(SurfaceExtension::Unresolved),
        },
    ))
}

pub(super) fn project_ruled_surface(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    let Some(mode) = (|| {
        let distance = parse_positive_length_mm(
            feature
                .parameters
                .get("Distance")
                .or_else(|| feature.parameters.get("D1"))?,
        )?;
        let mode_name = feature.properties.get("Mode")?;
        let mode = if mode_name.eq_ignore_ascii_case("normal") {
            RuledSurfaceMode::Normal { distance }
        } else if mode_name.eq_ignore_ascii_case("tangent") {
            RuledSurfaceMode::Tangent { distance }
        } else if mode_name.eq_ignore_ascii_case("direction") {
            RuledSurfaceMode::Direction {
                direction: parse_valid_direction(feature.properties.get("Direction")?)?,
                distance,
            }
        } else {
            return None;
        };
        Some(mode)
    })() else {
        return Ok(None);
    };
    let (Some(edges), Some(support_faces)) = (
        feature.properties.get("Edges"),
        feature.properties.get("SupportFaces"),
    ) else {
        return Ok(None);
    };
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
