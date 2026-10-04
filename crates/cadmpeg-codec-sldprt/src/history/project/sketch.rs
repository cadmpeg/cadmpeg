// SPDX-License-Identifier: Apache-2.0
//! Split-face, cosmetic-thread, and sketch-block projection.

use super::copy_projected_feature_text;
use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    CosmeticThreadExtent, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
    SplitFaceTool,
};
use cadmpeg_ir::transform::Transform;

use crate::history::literals::{
    parse_angle_rad, parse_dimension_display_length, parse_point3_mm,
    parse_positive_dimension_length_mm, strip_diameter_modifier,
};

pub(super) fn project_split_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if feature.input_class.as_deref() != Some("moPLine_c")
        || feature
            .properties
            .get(crate::resolved_features::operations::SPLIT_LINE_MODE_PROPERTY)
            .map(String::as_str)
            != Some(crate::resolved_features::operations::SPLIT_LINE_PROJECTION_MODE)
    {
        return Ok(None);
    }
    let Some(native) = feature
        .properties
        .get(crate::resolved_features::operations::SPLIT_LINE_TOOL_PROPERTY)
        .map(String::as_str)
    else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitFace {
            targets: FaceSelection::Unresolved,
            tool: SplitFaceTool::Path(PathRef::Native(copy_projected_feature_text(ctx, native)?)),
        },
    )))
}

pub(super) fn project_cosmetic_thread(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(feature.parameters.len()),
        "scan SLDPRT cosmetic thread dimensions",
    )?;
    let diameter = feature
        .parameters
        .get("D2")
        .and_then(|value| parse_dimension_display_length(value))
        .or_else(|| {
            let mut tagged = feature
                .parameters
                .values()
                .filter(|value| strip_diameter_modifier(value).is_some())
                .filter_map(|value| parse_dimension_display_length(value));
            let diameter = tagged.next()?;
            tagged.next().is_none().then_some(diameter)
        })
        .and_then(|value| cadmpeg_ir::scalar::PositiveLength::try_from(value).ok());
    let extent = match feature.parameters.get("D1") {
        Some(value) => parse_positive_dimension_length_mm(value)
            .map(|length| CosmeticThreadExtent::Blind { length })
            .or_else(|| {
                (parse_angle_rad(value).is_some()
                    || parse_dimension_display_length(value)
                        == Some(cadmpeg_ir::scalar::Length::ZERO))
                .then_some(CosmeticThreadExtent::Through {})
            }),
        None => Some(CosmeticThreadExtent::Through {}),
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::CosmeticThread {
            face: feature
                .properties
                .get("Face")
                .map(|value| copy_projected_feature_text(ctx, value))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            diameter,
            extent,
        },
    ))
}

pub(in crate::history) fn sketch_block_placement(feature: &Feature) -> Option<Transform> {
    let origin = parse_point3_mm(feature.properties.get("BlockOrigin")?)?.get();
    Transform::affine([
        [1.0, 0.0, 0.0, origin.x],
        [0.0, 1.0, 0.0, origin.y],
        [0.0, 0.0, 1.0, origin.z],
    ])
}
