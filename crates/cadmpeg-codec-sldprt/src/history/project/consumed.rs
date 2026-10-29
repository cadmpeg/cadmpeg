// SPDX-License-Identifier: Apache-2.0
//! Native property keys whose facts have typed neutral homes.

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

/// Feature state and reference keys with canonical homes outside the definition.
pub(crate) const FEATURE_KEYS: &[&str] = &["Scope", "Dependency", "Dependencies", "ParentFeatures"];

/// Keys consumed by the current typed operation, excluded from source detail.
pub(crate) fn keys(definition: &FeatureDefinition) -> &'static [&'static str] {
    let operation = match definition {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    match operation {
        FeatureOperation::Extrude { .. } => &[
            "Direction",
            "DissectableChildren",
            "DissectableRoot",
            "EndCondition",
            "Face",
            "Operation",
            "Profile",
            "Vertex",
        ],
        FeatureOperation::Revolve { .. } => &[
            "AxisDirection",
            "AxisOrigin",
            "EndCondition",
            "Operation",
            "Profile",
        ],
        FeatureOperation::Rib { .. } => &["BothSides", "Direction", "Operation", "Profile"],
        FeatureOperation::Sweep { .. } => &["Operation", "Path", "Profile"],
        FeatureOperation::Loft { .. } => &["Closed", "Guides", "Operation", "Profiles"],
        FeatureOperation::Pattern { .. } => &[
            "AxisDirection",
            "AxisOrigin",
            "Direction",
            "Direction2",
            "Path",
            "PatternType",
            "PlaneNormal",
            "PlaneOrigin",
            "Seeds",
        ],
        FeatureOperation::TreeNode { .. } => &["DissectableChildren", "DissectableRoot"],
        FeatureOperation::DatumCoordinateSystem { .. } => &["Origin", "XAxis", "YAxis", "ZAxis"],
        FeatureOperation::DatumPlane { .. } => &["Origin", "Normal", "UAxis"],
        FeatureOperation::DatumPoint { .. } => &["Position"],
        FeatureOperation::DatumAxis { .. } => &["Origin", "Direction"],
        FeatureOperation::DatumOffsetPlane { .. } => &[
            "Plane",
            "Reference",
            "ReferenceFaceNative",
            "ReferenceFaceNormal",
            "ReferenceFaceOrigin",
            "ReferenceFaceUAxis",
        ],
        FeatureOperation::ProjectedCurve { .. } => {
            &["Source", "TargetFaces", "Direction", "Bidirectional"]
        }
        FeatureOperation::CompositeCurve { .. } => &["Segments", "Closed"],
        FeatureOperation::EquationCurve { .. } => &[
            "Parameter",
            "XEquation",
            "YEquation",
            "ZEquation",
            "Start",
            "End",
        ],
        FeatureOperation::Helix { .. } => &["AxisOrigin", "AxisDirection", "Clockwise"],
        FeatureOperation::Fillet { .. } => &["Edges"],
        FeatureOperation::Chamfer { .. } => &["Edges"],
        FeatureOperation::Shell { .. } => &["RemovedFaces", "Outward"],
        FeatureOperation::Thicken { .. } => &["Faces", "BothSides", "Reverse"],
        FeatureOperation::Draft { .. } => &["Faces", "Direction", "NeutralPlane", "Outward"],
        FeatureOperation::Hole { .. } => &[
            "Face",
            "Position",
            "Direction",
            "EndCondition",
            "DissectableChildren",
        ],
        FeatureOperation::MoveFace { .. } => {
            &["Faces", "Mode", "Direction", "AxisOrigin", "AxisDirection"]
        }
        FeatureOperation::MoveBody { .. } => &[
            "Bodies",
            "Translation",
            "RotationOrigin",
            "RotationAxis",
            "Copies",
        ],
        FeatureOperation::Dome { .. } => &["Faces", "Elliptical", "Reverse"],
        FeatureOperation::Flex { .. } => &["Axis", "AxisDirection", "Mode"],
        FeatureOperation::Scale { .. } => &["Bodies", "Center", "CenterType", "CenterRef"],
        FeatureOperation::CutWithSurface { .. } => &["Targets", "Tools", "Reverse"],
        FeatureOperation::DeleteBody { .. } => &["Bodies", "Mode"],
        FeatureOperation::DeleteFace { .. } => &["Faces", "Heal"],
        FeatureOperation::ReplaceFace { .. } => &["Faces", "ReplacementFaces"],
        FeatureOperation::Wrap { .. } => &["Profile", "Face", "Mode"],
        FeatureOperation::HelixNativeAxis { .. } => &["Clockwise"],
        FeatureOperation::CosmeticThread { .. } => &["Face"],
        FeatureOperation::OffsetSurface { .. } => &["Faces"],
        FeatureOperation::ExtendSurface { .. } => &["Faces", "Method"],
        FeatureOperation::KnitSurface { .. } => &["Faces", "MergeEntities", "CreateSolid"],
        FeatureOperation::FilledSurface { .. } => {
            &["Boundary", "SupportFaces", "Continuity", "MergeResult"]
        }
        FeatureOperation::TrimSurface { .. } => &["Faces", "Tool", "Keep"],
        FeatureOperation::RuledSurface { .. } => &["Mode", "Direction", "Edges", "SupportFaces"],
        _ => &[],
    }
}

pub(super) fn properties(
    source: &crate::records::Feature,
    definition: &FeatureDefinition,
) -> std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let consumed = keys(definition);
    source
        .properties
        .iter()
        .filter(|(key, _)| {
            !consumed.contains(&key.as_str()) && !FEATURE_KEYS.contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}
