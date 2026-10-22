// SPDX-License-Identifier: Apache-2.0
//! Surface-feature projection.

use crate::records::Feature;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef, RuledSurfaceMode,
    SurfaceExtension, TrimRegion,
};
use std::collections::HashMap;

use crate::history::literals::{parse_bool, parse_valid_direction};

pub(super) fn project_offset_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::OffsetSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        distance: super::parameter_dimension(feature, &["Distance", "D1"]),
    })
}

pub(super) fn project_knit_surface(feature: &Feature) -> FeatureDefinition {
    let gap_tolerance = super::parameter_dimension(feature, &["GapTolerance"]);
    FeatureDefinition::Operation(FeatureOperation::KnitSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
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
    })
}

pub(super) fn project_filled_surface(feature: &Feature) -> FeatureDefinition {
    let continuity = feature
        .properties
        .get("Continuity")
        .and_then(|value| crate::feature_schema::parse_surface_continuity(value));
    FeatureDefinition::Operation(FeatureOperation::FilledSurface {
        boundary: cadmpeg_ir::features::SurfaceBoundary::Edges(
            feature
                .properties
                .get("Boundary")
                .cloned()
                .map_or(EdgeSelection::Unresolved, EdgeSelection::Native),
        ),
        support_faces: feature
            .properties
            .get("SupportFaces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        continuity: continuity.map_or_else(
            cadmpeg_ir::features::FilledSurfaceContinuityState::unresolved,
            cadmpeg_ir::features::FilledSurfaceContinuityState::uniform,
        ),
        merge_result: feature
            .properties
            .get("MergeResult")
            .and_then(|value| parse_bool(value)),
    })
}

pub(super) fn project_trim_surface(
    feature: &Feature,
    native_by_source: &HashMap<String, &str>,
) -> FeatureDefinition {
    let tool = match feature.properties.get("Tool") {
        None => PathRef::Unresolved(format!("{}:tool", feature.id)),
        Some(tool) => PathRef::Native(
            (native_by_source
                .get(tool.as_str())
                .copied()
                .unwrap_or(tool.as_str()))
            .to_owned(),
        ),
    };
    FeatureDefinition::Operation(FeatureOperation::TrimSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        tool,
        keep: feature
            .properties
            .get("Keep")
            .and_then(|value| crate::feature_schema::parse_trim_region(value))
            .unwrap_or(TrimRegion::Unresolved),
    })
}

pub(super) fn project_extend_surface(feature: &Feature) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::ExtendSurface {
        faces: feature
            .properties
            .get("Faces")
            .cloned()
            .map_or(FaceSelection::Unresolved, FaceSelection::Native),
        distance: super::parameter_dimension(feature, &["Distance", "D1"]),
        method: feature
            .properties
            .get("Method")
            .and_then(|value| crate::feature_schema::parse_surface_extension(value))
            .unwrap_or(SurfaceExtension::Unresolved),
    })
}

pub(super) fn project_ruled_surface(feature: &Feature) -> Option<FeatureDefinition> {
    let mode = (|| {
        let distance = super::parameter_dimension(feature, &["Distance", "D1"])?;
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
    })()?;
    let (Some(edges), Some(support_faces)) = (
        feature.properties.get("Edges"),
        feature.properties.get("SupportFaces"),
    ) else {
        return None;
    };
    Some(FeatureDefinition::Operation(
        FeatureOperation::RuledSurface {
            edges: EdgeSelection::Native(edges.to_owned()),
            support_faces: FaceSelection::Native(support_faces.to_owned()),
            mode,
            angle: None,
            alternate_face: None,
            corner: None,
        },
    ))
}
