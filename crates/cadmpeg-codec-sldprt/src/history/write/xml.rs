// SPDX-License-Identifier: Apache-2.0
//! Native XML tag and operation-kind helpers for write.

use cadmpeg_ir::features::operation_settings::BodyRetentionMode;
use cadmpeg_ir::features::patterns::PatternTransform;
use cadmpeg_ir::features::sweeps::SweepMode;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

use crate::history::classify::extrude_op;

/// The source element name of a `SolidWorks` feature. Another format's tag
/// is a value only its own writer reads (§3.4).
pub(super) fn solidworks_source_tag(feature: &cadmpeg_ir::features::Feature) -> Option<&String> {
    feature
        .source_tag
        .as_ref()
        .filter(|tag| feature.id.as_str().starts_with("sldprt:") && valid_xml_name(tag))
}

pub(super) fn feature_xml_tag(feature: &cadmpeg_ir::features::Feature) -> String {
    if let Some(tag) = solidworks_source_tag(feature) {
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
        FeatureDefinition::Operation(
            FeatureOperation::Helix { .. } | FeatureOperation::HelixNativeAxis { .. },
        ) => "Helix",
        FeatureDefinition::Operation(FeatureOperation::Wrap { .. }) => "Wrap",
        FeatureDefinition::Operation(
            FeatureOperation::Sketch { .. } | FeatureOperation::SpatialSketch { .. },
        ) => "Sketch",
        FeatureDefinition::Operation(FeatureOperation::SketchBlockDefinition { .. }) => "Block",
        FeatureDefinition::Operation(FeatureOperation::SketchBlockInstance { .. }) => "Feature",
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}) => "Feature",
        FeatureDefinition::Operation(
            FeatureOperation::BaseFeature { .. }
            | FeatureOperation::InsertBodies { .. }
            | FeatureOperation::Form(_)
            | FeatureOperation::Coil { .. }
            | FeatureOperation::Sphere { .. }
            | FeatureOperation::Torus { .. }
            | FeatureOperation::SheetMetalBaseFlange { .. }
            | FeatureOperation::SheetMetalAttachedPlate { .. }
            | FeatureOperation::SheetMetalBend { .. }
            | FeatureOperation::SheetMetalCorner { .. },
        ) => "Feature",
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
        FeatureDefinition::Operation(
            FeatureOperation::Unresolved {
                family:
                    UnresolvedFamily::DatumPoint
                    | UnresolvedFamily::DatumCoordinateSystem
                    | UnresolvedFamily::Loft
                    | UnresolvedFamily::FreeformSurface
                    | UnresolvedFamily::Draft,
            }
            | FeatureOperation::Block { .. }
            | FeatureOperation::ExtractBody { .. }
            | FeatureOperation::FaceBlend { .. }
            | FeatureOperation::SewBodies { .. }
            | FeatureOperation::TrimBodies { .. },
        ) => "Feature",
        FeatureDefinition::Operation(_) => "Feature",
    };
    tag.into()
}

/// Whether `name` is an XML name without a colon, the only element or
/// attribute name a written Keywords text can hold.
///
/// The reader parses with namespaces: a colon starts a prefix, and the text
/// declares none. The ranges are the `NameStartChar` and `NameChar`
/// productions of XML 1.0, which the reader applies.
pub(crate) fn valid_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(name_start)
        && chars.all(|character| {
            name_start(character)
                || matches!(
                    character,
                    '-' | '.'
                        | '0'..='9'
                        | '\u{b7}'
                        | '\u{300}'..='\u{36f}'
                        | '\u{203f}'..='\u{2040}'
                )
        })
}

fn name_start(character: char) -> bool {
    matches!(
        character,
        'A'..='Z'
            | '_'
            | 'a'..='z'
            | '\u{c0}'..='\u{d6}'
            | '\u{d8}'..='\u{f6}'
            | '\u{f8}'..='\u{2ff}'
            | '\u{370}'..='\u{37d}'
            | '\u{37f}'..='\u{1fff}'
            | '\u{200c}'..='\u{200d}'
            | '\u{2070}'..='\u{218f}'
            | '\u{2c00}'..='\u{2fef}'
            | '\u{3001}'..='\u{d7ff}'
            | '\u{f900}'..='\u{fdcf}'
            | '\u{fdf0}'..='\u{fffd}'
            | '\u{10000}'..='\u{effff}'
    )
}

/// Whether an XML document can hold `character`, literally or as a character
/// reference (the `Char` production of XML 1.0).
pub(crate) fn xml_char(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..
    )
}

#[cfg(test)]
mod tests {
    use cadmpeg_ir::features::{
        evaluation::FeatureEvaluation, selections::DistinctMembers, Feature, FeatureContent,
        FeatureDefinition, FeatureId, FeatureOperation, UnresolvedFamily,
    };

    fn datum_plane(id: &str) -> Feature {
        Feature {
            product_definition: None,
            id: FeatureId::mint(id).expect("feature identity"),
            ordinal: 0,
            name: None,
            suppressed: Some(false),
            dependencies: DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: Some("WorkPlane".into()),
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Unresolved {
                    family: UnresolvedFamily::DatumPlane,
                },
            )),
            native_ref: None,
        }
    }

    #[test]
    fn only_a_solidworks_feature_keeps_its_source_tag() {
        assert_eq!(
            super::feature_xml_tag(&datum_plane("f3d:model:feature#plane")),
            "ReferencePlane"
        );
        assert_eq!(
            super::feature_xml_tag(&datum_plane("sldprt:model:feature#plane")),
            "WorkPlane"
        );
    }
}
