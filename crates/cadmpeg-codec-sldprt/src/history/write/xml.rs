// SPDX-License-Identifier: Apache-2.0
//! Native XML tag and operation-kind helpers for write.

use cadmpeg_ir::features::{
    BodyRetentionMode, FeatureDefinition, FeatureOperation, PatternTransform, SweepMode,
    UnresolvedFamily,
};

use crate::history::classify::extrude_op;

pub(crate) fn feature_xml_tag(feature: &cadmpeg_ir::features::Feature) -> String {
    if let Some(tag) = feature
        .source_tag
        .as_ref()
        .filter(|tag| valid_xml_name(tag))
    {
        return tag.clone();
    }
    let tag = match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::TreeNode { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::CosmeticThread { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::DatumPlane { .. }) => "ReferencePlane",
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumPlane,
        }) => "ReferencePlane",
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::DatumAxis { .. }) => "ReferenceAxis",
        FeatureDefinition::Operation(FeatureOperation::DatumPoint { .. }) => "ReferencePoint",
        FeatureDefinition::Operation(FeatureOperation::DatumCoordinateSystem { .. }) => {
            "CoordinateSystem"
        }
        FeatureDefinition::Operation(FeatureOperation::EquationCurve { .. }) => {
            "EquationDrivenCurve"
        }
        FeatureDefinition::Operation(FeatureOperation::ProjectedCurve { .. }) => "ProjectedCurve",
        FeatureDefinition::Operation(FeatureOperation::CompositeCurve { .. }) => "CompositeCurve",
        FeatureDefinition::Operation(FeatureOperation::Helix { .. })
        | FeatureDefinition::Operation(FeatureOperation::HelixNativeAxis { .. }) => "Helix",
        FeatureDefinition::Operation(FeatureOperation::Wrap { .. }) => "Wrap",
        FeatureDefinition::Operation(FeatureOperation::Sketch { .. })
        | FeatureDefinition::Operation(FeatureOperation::SpatialSketch { .. }) => "Sketch",
        FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { .. }) => "Block",
        FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::BaseFeature { .. })
        | FeatureDefinition::Operation(FeatureOperation::InsertBodies { .. })
        | FeatureDefinition::Operation(FeatureOperation::Form { .. })
        | FeatureDefinition::Operation(FeatureOperation::Coil { .. })
        | FeatureDefinition::Operation(FeatureOperation::Sphere { .. })
        | FeatureDefinition::Operation(FeatureOperation::Torus { .. })
        | FeatureDefinition::Operation(FeatureOperation::SheetMetalBaseFlange { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::DerivedGeometry { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::ImportedGeometry { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::Primitive { .. }) => "Primitive",
        FeatureDefinition::Operation(FeatureOperation::Extrude { .. }) => "Extrusion",
        FeatureDefinition::Operation(FeatureOperation::Revolve { .. }) => "Revolve",
        FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. })
            if shape.mode() == SweepMode::Surface {} =>
        {
            "Surface-Sweep"
        }
        FeatureDefinition::Operation(FeatureOperation::Sweep { .. }) => "Sweep",
        FeatureDefinition::Operation(FeatureOperation::HelicalSweep { .. }) => "Helix",
        FeatureDefinition::Operation(FeatureOperation::Binder { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::Loft { .. }) => "Loft",
        FeatureDefinition::Operation(FeatureOperation::Rib { .. }) => "Rib",
        FeatureDefinition::Operation(FeatureOperation::Fillet { .. }) => "Fillet",
        FeatureDefinition::Operation(FeatureOperation::Chamfer { .. }) => "Chamfer",
        FeatureDefinition::Operation(FeatureOperation::Shell { .. }) => "Shell",
        FeatureDefinition::Operation(FeatureOperation::Thicken { .. }) => "Thicken",
        FeatureDefinition::Operation(FeatureOperation::OffsetSurface { .. }) => "OffsetSurface",
        FeatureDefinition::Operation(FeatureOperation::KnitSurface { .. }) => "KnitSurface",
        FeatureDefinition::Operation(FeatureOperation::FilledSurface { .. }) => "FilledSurface",
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::BoundarySurface,
        }) => "BoundarySurface",
        FeatureDefinition::Operation(FeatureOperation::TrimSurface { .. }) => "TrimSurface",
        FeatureDefinition::Operation(FeatureOperation::ExtendSurface { .. }) => "ExtendSurface",
        FeatureDefinition::Operation(FeatureOperation::RuledSurface { .. }) => "RuledSurface",
        FeatureDefinition::Operation(FeatureOperation::Draft { .. }) => "Draft",
        FeatureDefinition::Operation(FeatureOperation::Combine { .. }) => "Combine",
        FeatureDefinition::Operation(FeatureOperation::CutWithSurface { .. }) => "CutWithSurface",
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            mode: BodyRetentionMode::Unresolved,
            ..
        }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            mode: BodyRetentionMode::DeleteSelected,
            ..
        }) => "DeleteBody",
        FeatureDefinition::Operation(FeatureOperation::DeleteBody {
            mode: BodyRetentionMode::KeepSelected,
            ..
        }) => "KeepBody",
        FeatureDefinition::Operation(FeatureOperation::DeleteFace { .. }) => "DeleteFace",
        FeatureDefinition::Operation(FeatureOperation::ReplaceFace { .. }) => "ReplaceFace",
        FeatureDefinition::Operation(FeatureOperation::MoveFace { .. }) => "MoveFace",
        FeatureDefinition::Operation(FeatureOperation::MoveBody { .. }) => "MoveBody",
        FeatureDefinition::Operation(FeatureOperation::Dome { .. }) => "Dome",
        FeatureDefinition::Operation(FeatureOperation::Flex { .. }) => "Flex",
        FeatureDefinition::Operation(FeatureOperation::Scale { .. }) => "Scale",
        FeatureDefinition::Operation(FeatureOperation::OffsetShape { .. }) => "Offset",
        FeatureDefinition::Operation(FeatureOperation::Compound { .. }) => "Compound",
        FeatureDefinition::Operation(FeatureOperation::RefineShape { .. }) => "Refine",
        FeatureDefinition::Operation(FeatureOperation::ReverseShape { .. }) => "Reverse",
        FeatureDefinition::Operation(FeatureOperation::RuledBetweenCurves { .. }) => "RuledSurface",
        FeatureDefinition::Operation(FeatureOperation::SectionShape { .. }) => "Section",
        FeatureDefinition::Operation(FeatureOperation::MirrorShape { .. }) => "Mirror",
        FeatureDefinition::Operation(FeatureOperation::ProjectOnSurface { .. }) => {
            "ProjectOnSurface"
        }
        FeatureDefinition::Operation(FeatureOperation::Hole { .. }) => "Hole",
        FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. })
            if matches!(pattern.definition(), PatternTransform::Mirror { .. }) =>
        {
            "Mirror"
        }
        FeatureDefinition::Operation(FeatureOperation::Pattern { .. }) => "Pattern",
        FeatureDefinition::PostProcess { .. } => "Feature",
        FeatureDefinition::Operation(FeatureOperation::PointGeometry { .. }) => "Point",
        FeatureDefinition::Operation(FeatureOperation::LineSegment { .. }) => "Line",
        FeatureDefinition::Operation(FeatureOperation::CircularArc { .. }) => "Circle",
        FeatureDefinition::Operation(FeatureOperation::EllipticArc { .. }) => "Ellipse",
        FeatureDefinition::Operation(FeatureOperation::Polyline { .. }) => "Polyline",
        FeatureDefinition::Operation(FeatureOperation::RegularPolygonCurve { .. }) => "Polygon",
        FeatureDefinition::Operation(FeatureOperation::PlanarPatch { .. }) => "Plane",
        FeatureDefinition::Operation(FeatureOperation::FaceFromShapes { .. }) => "Face",
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
            if extrude_op(kind.as_str()).is_some() =>
        {
            "Extrusion"
        }
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. })
            if valid_xml_name(kind.as_str()) =>
        {
            kind.as_str()
        }
        FeatureDefinition::Operation(FeatureOperation::Native { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family:
                UnresolvedFamily::DatumPoint
                | UnresolvedFamily::DatumCoordinateSystem
                | UnresolvedFamily::Loft
                | UnresolvedFamily::FreeformSurface
                | UnresolvedFamily::Draft,
        })
        | FeatureDefinition::Operation(FeatureOperation::Block { .. })
        | FeatureDefinition::Operation(FeatureOperation::ExtractBody { .. })
        | FeatureDefinition::Operation(FeatureOperation::FaceBlend { .. })
        | FeatureDefinition::Operation(FeatureOperation::SewBodies { .. })
        | FeatureDefinition::Operation(FeatureOperation::TrimBodies { .. }) => "Feature",
        _ => "Feature",
    };
    tag.into()
}

pub(crate) fn valid_xml_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b':'))
        && bytes
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b':' | b'-' | b'.'))
}
