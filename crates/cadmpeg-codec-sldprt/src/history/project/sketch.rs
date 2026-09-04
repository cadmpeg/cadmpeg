// SPDX-License-Identifier: Apache-2.0
//! Split-face, cosmetic-thread, and sketch-block projection.

use crate::records::Feature;
use cadmpeg_ir::features::{
    CosmeticThreadExtent, FaceSelection, FeatureDefinition, Length, PathRef, SplitFaceTool,
};
use cadmpeg_ir::transform::Transform;

use crate::history::literals::{
    parse_angle_rad, parse_dimension_display_length, parse_point3_mm,
    parse_positive_dimension_length_mm, strip_diameter_modifier,
};

pub(crate) fn project_split_face(feature: &Feature) -> Option<FeatureDefinition> {
    if feature.input_class.as_deref() != Some("moPLine_c")
        || feature
            .properties
            .get(crate::resolved_features::operations::SPLIT_LINE_MODE_PROPERTY)
            .map(String::as_str)
            != Some(crate::resolved_features::operations::SPLIT_LINE_PROJECTION_MODE)
    {
        return None;
    }
    let native = feature
        .properties
        .get(crate::resolved_features::operations::SPLIT_LINE_TOOL_PROPERTY)
        .map(String::as_str)?;
    Some(FeatureDefinition::SplitFace {
        targets: FaceSelection::Unresolved,
        tool: SplitFaceTool::Path(PathRef::Native(native.into())),
    })
}

pub(crate) fn project_cosmetic_thread(feature: &Feature) -> FeatureDefinition {
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
        .filter(|value| *value > 0.0)
        .map(Length);
    let extent = match feature.parameters.get("D1") {
        Some(value) => parse_positive_dimension_length_mm(value)
            .map(|length| CosmeticThreadExtent::Blind {
                length: Length(length),
            })
            .or_else(|| {
                (parse_angle_rad(value).is_some()
                    || parse_dimension_display_length(value) == Some(0.0))
                .then_some(CosmeticThreadExtent::Through)
            }),
        None => Some(CosmeticThreadExtent::Through),
    };
    FeatureDefinition::CosmeticThread {
        face: feature
            .properties
            .get("Face")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        diameter,
        extent,
    }
}

pub(crate) fn sketch_block_placement(feature: &Feature) -> Option<Transform> {
    let origin = parse_point3_mm(feature.properties.get("BlockOrigin")?)?;
    Transform::affine([
        [1.0, 0.0, 0.0, origin.x],
        [0.0, 1.0, 0.0, origin.y],
        [0.0, 0.0, 1.0, origin.z],
    ])
}
