// SPDX-License-Identifier: Apache-2.0
//! Structural classification of native feature-history objects.

use crate::records::Feature;

/// Semantic family established by native record identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureClass {
    Sketch,
    ReferencePlane,
    ReferenceAxis,
    ReferencePoint,
    CoordinateSystem,
    EquationCurve,
    ProjectedCurve,
    CompositeCurve,
    Helix,
    Wrap,
    Extrude,
    Fillet,
    Chamfer,
    Shell,
    Thicken,
    OffsetSurface,
    KnitSurface,
    FilledSurface,
    TrimSurface,
    ExtendSurface,
    RuledSurface,
    Draft,
    Combine,
    CutWithSurface,
    DeleteBody,
    DeleteFace,
    ReplaceFace,
    MoveFace,
    MoveBody,
    Dome,
    Flex,
    Scale,
    Hole,
    Revolve,
    Pattern,
    Sweep,
    Loft,
    Rib,
}

/// Classify a feature from serialized object identity, never its display name.
pub(crate) fn classify(feature: &Feature) -> Option<FeatureClass> {
    let evidence = [
        classify_input_class(feature.input_class.as_deref()),
        classify_xml_element(&feature.xml_tag),
        classify_type_token(&feature.kind),
    ];
    let mut classes = evidence.into_iter().flatten();
    let first = classes.next()?;
    classes.all(|class| class == first).then_some(first)
}

fn classify_input_class(class: Option<&str>) -> Option<FeatureClass> {
    Some(match class? {
        "Fillet_c" => FeatureClass::Fillet,
        "Chamfer_c" => FeatureClass::Chamfer,
        "moRefPlane_c" => FeatureClass::ReferencePlane,
        "moThicken_c" => FeatureClass::Thicken,
        "moSweep_c" => FeatureClass::Sweep,
        "moHelix_c" => FeatureClass::Helix,
        "moLPattern_c" | "moCurvePattern_c" => FeatureClass::Pattern,
        "moDeleteBody_c" => FeatureClass::DeleteBody,
        _ => return None,
    })
}

fn classify_xml_element(tag: &str) -> Option<FeatureClass> {
    Some(match tag {
        "Sketch" => FeatureClass::Sketch,
        "Plane" | "ReferencePlane" => FeatureClass::ReferencePlane,
        "ReferenceAxis" => FeatureClass::ReferenceAxis,
        "ReferencePoint" => FeatureClass::ReferencePoint,
        "CoordinateSystem" | "ReferenceCoordinateSystem" => FeatureClass::CoordinateSystem,
        "EquationDrivenCurve" | "EquationCurve" => FeatureClass::EquationCurve,
        "ProjectedCurve" | "ProjectionCurve" => FeatureClass::ProjectedCurve,
        "CompositeCurve" => FeatureClass::CompositeCurve,
        "Helix" | "HelixSpiral" | "Helix/Spiral" => FeatureClass::Helix,
        "Wrap" => FeatureClass::Wrap,
        "Extrusion" => FeatureClass::Extrude,
        "Fillet" => FeatureClass::Fillet,
        "Chamfer" => FeatureClass::Chamfer,
        "Shell" => FeatureClass::Shell,
        "Thicken" | "Thickness" => FeatureClass::Thicken,
        "OffsetSurface" => FeatureClass::OffsetSurface,
        "KnitSurface" | "Knit" => FeatureClass::KnitSurface,
        "FilledSurface" | "FillSurface" => FeatureClass::FilledSurface,
        "TrimSurface" | "SurfaceTrim" => FeatureClass::TrimSurface,
        "ExtendSurface" | "SurfaceExtend" => FeatureClass::ExtendSurface,
        "RuledSurface" | "SurfaceRuled" => FeatureClass::RuledSurface,
        "Draft" => FeatureClass::Draft,
        "Combine" => FeatureClass::Combine,
        "CutWithSurface" | "SurfaceCut" => FeatureClass::CutWithSurface,
        "DeleteBody" | "KeepBody" => FeatureClass::DeleteBody,
        "DeleteFace" => FeatureClass::DeleteFace,
        "ReplaceFace" => FeatureClass::ReplaceFace,
        "MoveFace" => FeatureClass::MoveFace,
        "MoveBody" | "MoveCopyBody" => FeatureClass::MoveBody,
        "Dome" => FeatureClass::Dome,
        "Flex" => FeatureClass::Flex,
        "Scale" => FeatureClass::Scale,
        "Hole" => FeatureClass::Hole,
        "Revolve" | "Revolution" => FeatureClass::Revolve,
        "Pattern" | "Mirror" => FeatureClass::Pattern,
        "Sweep" | "Surface-Sweep" => FeatureClass::Sweep,
        "Loft" | "Boundary" => FeatureClass::Loft,
        "Rib" => FeatureClass::Rib,
        _ => return None,
    })
}

fn classify_type_token(kind: &str) -> Option<FeatureClass> {
    Some(match kind {
        "BossExtrude" | "CutExtrude" => FeatureClass::Extrude,
        "LinearPattern" | "CircularPattern" | "CrvPattern" | "CurvePattern"
        | "CurveDrivenPattern" | "Mirror" => FeatureClass::Pattern,
        "BossLoft" | "CutLoft" | "BoundaryBoss" | "BoundaryCut" => FeatureClass::Loft,
        "Body-Delete/Keep " | "Body-Delete/Keep" => FeatureClass::DeleteBody,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn feature(xml_tag: &str, name: &str, kind: &str, input_class: Option<&str>) -> Feature {
        Feature {
            id: "feature".into(),
            parent: "history".into(),
            xml_tag: xml_tag.into(),
            tree_parent: None,
            source_id: None,
            parent_source_id: None,
            ordinal: 0,
            name: name.into(),
            kind: kind.into(),
            input_class: input_class.map(str::to_owned),
            suppressed: false,
            parameters: BTreeMap::new(),
            dimension_properties: BTreeMap::new(),
            properties: BTreeMap::new(),
            text: None,
            content: Vec::new(),
        }
    }

    #[test]
    fn display_name_does_not_classify_an_object() {
        assert_eq!(classify(&feature("Feature", "Plane", "Custom", None)), None);
        assert_eq!(classify(&feature("Feature", "Plano", "Custom", None)), None);
    }

    #[test]
    fn native_identity_is_independent_of_display_name() {
        assert_eq!(
            classify(&feature(
                "Feature",
                "arbitrary",
                "Custom",
                Some("moRefPlane_c")
            )),
            Some(FeatureClass::ReferencePlane)
        );
        assert_eq!(
            classify(&feature("Extrusion", "arbitrary", "Custom", None)),
            Some(FeatureClass::Extrude)
        );
    }

    #[test]
    fn conflicting_native_identities_are_not_classified() {
        assert_eq!(
            classify(&feature(
                "Extrusion",
                "arbitrary",
                "BossExtrude",
                Some("moSweep_c")
            )),
            None
        );
    }
}
