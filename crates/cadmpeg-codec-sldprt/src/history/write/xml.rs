// SPDX-License-Identifier: Apache-2.0
//! Native XML tag and operation-kind helpers for write.

use cadmpeg_ir::features::{BodyRetentionMode, FeatureDefinition, PatternKind, SweepMode};

use crate::history::classify::extrude_op;

pub(crate) fn feature_xml_tag(feature: &cadmpeg_ir::features::Feature) -> String {
    if let Some(tag) = feature
        .source_tag
        .as_ref()
        .filter(|tag| valid_xml_name(tag))
    {
        return tag.clone();
    }
    let tag = match &feature.definition {
        FeatureDefinition::TreeNode { .. } => "Feature",
        FeatureDefinition::CosmeticThread { .. } => "Feature",
        FeatureDefinition::DatumPrincipalPlane { .. } => "Feature",
        FeatureDefinition::DatumPlane { .. } => "ReferencePlane",
        FeatureDefinition::DatumPlaneUnresolved => "ReferencePlane",
        FeatureDefinition::DatumOffsetPlane { .. } => "Feature",
        FeatureDefinition::DatumAxis { .. } => "ReferenceAxis",
        FeatureDefinition::DatumPoint { .. } => "ReferencePoint",
        FeatureDefinition::DatumCoordinateSystem { .. } => "CoordinateSystem",
        FeatureDefinition::EquationCurve { .. } => "EquationDrivenCurve",
        FeatureDefinition::ProjectedCurve { .. } => "ProjectedCurve",
        FeatureDefinition::CompositeCurve { .. } => "CompositeCurve",
        FeatureDefinition::Helix { .. } | FeatureDefinition::HelixNativeAxis { .. } => "Helix",
        FeatureDefinition::Wrap { .. } => "Wrap",
        FeatureDefinition::Sketch { .. } | FeatureDefinition::SpatialSketch { .. } => "Sketch",
        FeatureDefinition::SketchBlockDefinition { .. } => "Block",
        FeatureDefinition::SketchBlockInstance { .. } => "Feature",
        FeatureDefinition::StoredGeometry => "Feature",
        FeatureDefinition::BaseFeature { .. }
        | FeatureDefinition::InsertBodies { .. }
        | FeatureDefinition::Form { .. }
        | FeatureDefinition::Coil { .. }
        | FeatureDefinition::Sphere { .. }
        | FeatureDefinition::Torus { .. }
        | FeatureDefinition::SheetMetalBaseFlange { .. } => "Feature",
        FeatureDefinition::DerivedGeometry { .. } => "Feature",
        FeatureDefinition::ImportedGeometry { .. } => "Feature",
        FeatureDefinition::Primitive { .. } => "Primitive",
        FeatureDefinition::Extrude { .. } => "Extrusion",
        FeatureDefinition::Revolve { .. } => "Revolve",
        FeatureDefinition::Sweep {
            mode: SweepMode::Surface,
            ..
        } => "Surface-Sweep",
        FeatureDefinition::Sweep { .. } => "Sweep",
        FeatureDefinition::HelicalSweep { .. } => "Helix",
        FeatureDefinition::Binder { .. } => "Feature",
        FeatureDefinition::Loft { .. } => "Loft",
        FeatureDefinition::Rib { .. } => "Rib",
        FeatureDefinition::Fillet { .. } => "Fillet",
        FeatureDefinition::Chamfer { .. } => "Chamfer",
        FeatureDefinition::Shell { .. } => "Shell",
        FeatureDefinition::Thicken { .. } => "Thicken",
        FeatureDefinition::OffsetSurface { .. } => "OffsetSurface",
        FeatureDefinition::KnitSurface { .. } => "KnitSurface",
        FeatureDefinition::FilledSurface { .. } => "FilledSurface",
        FeatureDefinition::BoundarySurfaceUnresolved => "BoundarySurface",
        FeatureDefinition::TrimSurface { .. } => "TrimSurface",
        FeatureDefinition::ExtendSurface { .. } => "ExtendSurface",
        FeatureDefinition::RuledSurface { .. } => "RuledSurface",
        FeatureDefinition::Draft { .. } => "Draft",
        FeatureDefinition::Combine { .. } => "Combine",
        FeatureDefinition::CutWithSurface { .. } => "CutWithSurface",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::Unresolved,
            ..
        } => "Feature",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::DeleteSelected,
            ..
        } => "DeleteBody",
        FeatureDefinition::DeleteBody {
            mode: BodyRetentionMode::KeepSelected,
            ..
        } => "KeepBody",
        FeatureDefinition::DeleteFace { .. } => "DeleteFace",
        FeatureDefinition::ReplaceFace { .. } => "ReplaceFace",
        FeatureDefinition::MoveFace { .. } => "MoveFace",
        FeatureDefinition::MoveBody { .. } => "MoveBody",
        FeatureDefinition::Dome { .. } => "Dome",
        FeatureDefinition::Flex { .. } => "Flex",
        FeatureDefinition::Scale { .. } => "Scale",
        FeatureDefinition::OffsetShape { .. } => "Offset",
        FeatureDefinition::Compound { .. } => "Compound",
        FeatureDefinition::RefineShape { .. } => "Refine",
        FeatureDefinition::ReverseShape { .. } => "Reverse",
        FeatureDefinition::RuledBetweenCurves { .. } => "RuledSurface",
        FeatureDefinition::SectionShape { .. } => "Section",
        FeatureDefinition::MirrorShape { .. } => "Mirror",
        FeatureDefinition::ProjectOnSurface { .. } => "ProjectOnSurface",
        FeatureDefinition::Hole { .. } => "Hole",
        FeatureDefinition::Pattern {
            pattern: PatternKind::Mirror { .. },
            ..
        } => "Mirror",
        FeatureDefinition::Pattern { .. } => "Pattern",
        FeatureDefinition::PostProcess { .. } => "Feature",
        FeatureDefinition::PointGeometry { .. } => "Point",
        FeatureDefinition::LineSegment { .. } => "Line",
        FeatureDefinition::CircularArc { .. } => "Circle",
        FeatureDefinition::EllipticArc { .. } => "Ellipse",
        FeatureDefinition::Polyline { .. } => "Polyline",
        FeatureDefinition::RegularPolygonCurve { .. } => "Polygon",
        FeatureDefinition::PlanarPatch { .. } => "Plane",
        FeatureDefinition::FaceFromShapes { .. } => "Face",
        FeatureDefinition::Native { kind, .. } if extrude_op(kind.as_str()).is_some() => {
            "Extrusion"
        }
        FeatureDefinition::Native { kind, .. } if valid_xml_name(kind.as_str()) => kind.as_str(),
        FeatureDefinition::Native { .. } => "Feature",
        FeatureDefinition::DatumPointUnresolved
        | FeatureDefinition::DatumCoordinateSystemUnresolved
        | FeatureDefinition::Block { .. }
        | FeatureDefinition::ExtractBody { .. }
        | FeatureDefinition::LoftUnresolved
        | FeatureDefinition::FreeformSurfaceUnresolved
        | FeatureDefinition::DraftUnresolved
        | FeatureDefinition::FaceBlend { .. }
        | FeatureDefinition::SewBodies { .. }
        | FeatureDefinition::TrimBodies { .. } => "Feature",
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
