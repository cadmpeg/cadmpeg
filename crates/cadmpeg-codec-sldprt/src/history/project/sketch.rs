// SPDX-License-Identifier: Apache-2.0
//! Split-face, cosmetic-thread, and sketch-block projection.

use crate::records::Feature;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    CosmeticThreadExtent, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
    SplitFaceTool,
};
use cadmpeg_ir::transform::Transform;

use crate::history::literals::{
    admit_literal, named_literal, parse_angle_rad, parse_dimension_display_length, parse_point3_mm,
    parse_positive_dimension_length_mm, strip_diameter_modifier,
};

const OPERATION: &str = "project SLDPRT sketch-derived feature";

pub(super) fn project_split_face(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<Option<FeatureDefinition>, CodecError> {
    if feature.input_class.as_deref() != Some("moPLine_c")
        || ctx
            .get_btree_map(
                &feature.properties,
                crate::resolved_features::operations::SPLIT_LINE_MODE_PROPERTY,
                OPERATION,
            )?
            .map(String::as_str)
            != Some(crate::resolved_features::operations::SPLIT_LINE_PROJECTION_MODE)
    {
        return Ok(None);
    }
    let Some(native) = ctx.get_btree_map(
        &feature.properties,
        crate::resolved_features::operations::SPLIT_LINE_TOOL_PROPERTY,
        OPERATION,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(FeatureDefinition::Operation(
        FeatureOperation::SplitFace {
            targets: FaceSelection::Unresolved,
            tool: SplitFaceTool::Path(PathRef::Native(ctx.copy_retained_text(native, OPERATION)?)),
        },
    )))
}

pub(super) fn project_cosmetic_thread(
    ctx: &DecodeContext<'_>,
    feature: &Feature,
) -> Result<FeatureDefinition, CodecError> {
    let diameter = match named_literal(ctx, &feature.parameters, "D2", OPERATION)?
        .and_then(parse_dimension_display_length)
    {
        Some(diameter) => Some(diameter),
        None => {
            // The one parameter tagged as a diameter, when exactly one parses.
            let mut tagged = None;
            for (_, value) in ctx.admit_iter(&feature.parameters, OPERATION)? {
                admit_literal(ctx, value, OPERATION)?;
                if strip_diameter_modifier(value).is_none() {
                    continue;
                }
                let Some(diameter) = parse_dimension_display_length(value) else {
                    continue;
                };
                if tagged.is_some() {
                    tagged = None;
                    break;
                }
                tagged = Some(diameter);
            }
            tagged
        }
    }
    .and_then(|value| cadmpeg_ir::scalar::PositiveLength::try_from(value).ok());
    let extent = match named_literal(ctx, &feature.parameters, "D1", OPERATION)? {
        Some(value) => match parse_positive_dimension_length_mm(value) {
            Some(length) => Some(CosmeticThreadExtent::Blind { length }),
            None => (parse_angle_rad(value).is_some()
                || parse_dimension_display_length(value) == Some(cadmpeg_ir::scalar::Length::ZERO))
            .then_some(CosmeticThreadExtent::Through {}),
        },
        None => Some(CosmeticThreadExtent::Through {}),
    };
    Ok(FeatureDefinition::Operation(
        FeatureOperation::CosmeticThread {
            face: ctx
                .get_btree_map(&feature.properties, "Face", OPERATION)?
                .map(|value| ctx.copy_retained_text(value, OPERATION))
                .transpose()?
                .map_or(FaceSelection::Unresolved, FaceSelection::Native),
            diameter,
            extent,
        },
    ))
}

/// The placement a sketch-block instance's `BlockOrigin` literal states.
pub(super) fn block_placement(origin: &str) -> Option<Transform> {
    let origin = parse_point3_mm(origin)?.get();
    Transform::affine([
        [1.0, 0.0, 0.0, origin.x],
        [0.0, 1.0, 0.0, origin.y],
        [0.0, 0.0, 1.0, origin.z],
    ])
}

/// The placement a sketch-block instance record states, for the writer.
pub(in crate::history) fn sketch_block_placement(feature: &Feature) -> Option<Transform> {
    block_placement(feature.properties.get("BlockOrigin")?)
}
