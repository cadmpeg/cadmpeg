// SPDX-License-Identifier: Apache-2.0
//! Structural classification of native feature-history objects.

use crate::records::Feature;
use crate::records::{FeatureInputClassRole, FeatureInputRelationFamily};
use cadmpeg_ir::features::FeatureTreeNodeRole;

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

/// Semantic kind of a serialized native object class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeClassKind {
    Extrusion,
    Fillet,
    Chamfer,
    OriginProfileFeature,
    ProfileFeature,
    ReferencePlane,
    ReferenceAxis,
    Thicken,
    Sweep,
    SweepReferenceSurface,
    Helix,
    HoleWizard,
    Revolution,
    LinearPattern,
    CurvePattern,
    MirrorPattern,
    Combine,
    DeleteBody,
    TreeNode(FeatureTreeNodeRole),
    Sketch,
    SketchEntity,
    SketchRelation(FeatureInputRelationFamily),
    Dimension,
    LengthParameter,
    Reference,
    Auxiliary,
    Unknown,
}

/// Format semantics attached to a serialized native object class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeObjectClass {
    pub kind: NativeClassKind,
    pub role: FeatureInputClassRole,
    pub feature: Option<FeatureClass>,
    pub tree_node: Option<FeatureTreeNodeRole>,
}

/// Resolve a serialized class name through the format-wide object taxonomy.
pub(crate) fn native_object_class(name: &str) -> NativeObjectClass {
    use FeatureInputClassRole::{
        Auxiliary, Dimension, Feature, Native, Parameter, Reference, Sketch, SketchEntity,
    };

    let (kind, role, feature, tree_node) = match name {
        "moExtrusion_c" | "moICE_c" | "moCut_c" => (
            NativeClassKind::Extrusion,
            Feature,
            Some(FeatureClass::Extrude),
            None,
        ),
        "Fillet_c" => (
            NativeClassKind::Fillet,
            Feature,
            Some(FeatureClass::Fillet),
            None,
        ),
        "Chamfer_c" => (
            NativeClassKind::Chamfer,
            Feature,
            Some(FeatureClass::Chamfer),
            None,
        ),
        "moOriginProfileFeature_c" => (
            NativeClassKind::OriginProfileFeature,
            Feature,
            None,
            Some(FeatureTreeNodeRole::ModelOrigin),
        ),
        "moProfileFeature_c" | "mo3DProfileFeature_c" => (
            NativeClassKind::ProfileFeature,
            Feature,
            Some(FeatureClass::Sketch),
            None,
        ),
        "moRefPlane_c" => (
            NativeClassKind::ReferencePlane,
            Reference,
            Some(FeatureClass::ReferencePlane),
            None,
        ),
        "moRefAxis_c" => (
            NativeClassKind::ReferenceAxis,
            Reference,
            Some(FeatureClass::ReferenceAxis),
            None,
        ),
        "moThicken_c" => (
            NativeClassKind::Thicken,
            Feature,
            Some(FeatureClass::Thicken),
            None,
        ),
        "moSweep_c" => (
            NativeClassKind::Sweep,
            Feature,
            Some(FeatureClass::Sweep),
            None,
        ),
        "moSweepRefSurface_c" => (
            NativeClassKind::SweepReferenceSurface,
            Feature,
            Some(FeatureClass::Sweep),
            None,
        ),
        "moHelix_c" => (
            NativeClassKind::Helix,
            Feature,
            Some(FeatureClass::Helix),
            None,
        ),
        "moHoleWzd_c" => (
            NativeClassKind::HoleWizard,
            Feature,
            Some(FeatureClass::Hole),
            None,
        ),
        "moRevolution_c" | "moRevCut_c" => (
            NativeClassKind::Revolution,
            Feature,
            Some(FeatureClass::Revolve),
            None,
        ),
        "moLPattern_c" => (
            NativeClassKind::LinearPattern,
            Feature,
            Some(FeatureClass::Pattern),
            None,
        ),
        "moCurvePattern_c" => (
            NativeClassKind::CurvePattern,
            Feature,
            Some(FeatureClass::Pattern),
            None,
        ),
        "moMirrorPattern_c" => (
            NativeClassKind::MirrorPattern,
            Feature,
            Some(FeatureClass::Pattern),
            None,
        ),
        "moCombineBodies_c" => (
            NativeClassKind::Combine,
            Feature,
            Some(FeatureClass::Combine),
            None,
        ),
        "moDeleteBody_c" => (
            NativeClassKind::DeleteBody,
            Feature,
            Some(FeatureClass::DeleteBody),
            None,
        ),

        "moDetailCabinet_c" => tree_node_class(FeatureTreeNodeRole::Annotations),
        "moCommentsFolder_c" => tree_node_class(FeatureTreeNodeRole::Comments),
        "moDocsFolder_c" => tree_node_class(FeatureTreeNodeRole::DesignBinder),
        "moEqnFolder_c" => tree_node_class(FeatureTreeNodeRole::Equations),
        "moFavoriteFolder_c" => tree_node_class(FeatureTreeNodeRole::Favorites),
        "moHistoryFolder_c" => tree_node_class(FeatureTreeNodeRole::History),
        "moInkMarkupFolder_c" => tree_node_class(FeatureTreeNodeRole::Markups),
        "moMaterialFolder_c" => tree_node_class(FeatureTreeNodeRole::Materials),
        "moNotesAreaFtrFolder_c" => tree_node_class(FeatureTreeNodeRole::Notes),
        "moSelectionSetFolder_c" => tree_node_class(FeatureTreeNodeRole::SelectionSets),
        "moSensorFolder_c" => tree_node_class(FeatureTreeNodeRole::Sensors),
        "moSolidBodyFolder_c" => tree_node_class(FeatureTreeNodeRole::SolidBodies),
        "moSurfaceBodyFolder_c" => tree_node_class(FeatureTreeNodeRole::SurfaceBodies),

        "sgSketch" => (NativeClassKind::Sketch, Sketch, None, None),
        "sgArcHandle" | "sgEntHandle" | "sgLineHandle" | "sgPointHandle" | "sgSplineHandle" => {
            (NativeClassKind::SketchEntity, SketchEntity, None, None)
        }
        "sgLLDist" => relation_class(FeatureInputRelationFamily::LineLineDistance),
        "sgPntPntDist" => relation_class(FeatureInputRelationFamily::PointPointDistance),
        "sgPntLineDist" => relation_class(FeatureInputRelationFamily::PointLineDistance),
        "sgPntPntHorDist" => {
            relation_class(FeatureInputRelationFamily::PointPointHorizontalDistance)
        }
        "sgPntPntVertDist" => {
            relation_class(FeatureInputRelationFamily::PointPointVerticalDistance)
        }
        "sgAnglDim" => relation_class(FeatureInputRelationFamily::Angle),
        "sgCircleDim" => relation_class(FeatureInputRelationFamily::CircleDiameter),
        "ParallelPlaneDistanceDim_c"
        | "ThreeDRadiusDim_c"
        | "faceRadiusObject_c"
        | "moDisplayDistanceDim_c"
        | "moDisplayRadialDim_c"
        | "moFeatureDimHandle_c"
        | "moSkDimHandleRadial_c"
        | "moSkDimHandleValG2_c"
        | "moSkDimHandleOffset_c"
        | "moSkDimHandleLinearPattCnt_c"
        | "moDisplayAngularDim_c"
        | "moDisplayDim_c"
        | "moDisplayLinearPattCntDim_c"
        | "moNumberDim_c"
        | "moScalerDim_c"
        | "AngleDim_c" => (NativeClassKind::Dimension, Dimension, None, None),
        "sgDimEntityHelpData_c" | "sgLinearPattCntDim" | "sgOffsetDim" | "sgSkOffsetDim" => {
            (NativeClassKind::Dimension, Dimension, None, None)
        }
        "moLengthParameter_c" => (NativeClassKind::LengthParameter, Parameter, None, None),
        "moCompEdge_c"
        | "moCompFace_c"
        | "moCompFeature_c"
        | "moCompRefPlane_c"
        | "moCompReferenceCurve_c"
        | "moCompSketchEntHandle_c"
        | "moCompSolidBody_c"
        | "moCompSurfaceBody_c"
        | "moCompVertex_c"
        | "moConstSurfRef_w"
        | "moEdgeRef_c"
        | "moEndPointRef_w"
        | "moFaceRef_c"
        | "moGeneralCurveRef_w"
        | "moLineRef_w"
        | "moSingleFaceRef_w"
        | "moSolidRef_w"
        | "moVertexRef_c" => (NativeClassKind::Reference, Reference, None, None),
        "moBBoxCenterData_c"
        | "moDefaultRefPlnData_c"
        | "moEndFace3IntSurfIdRep_c"
        | "moEndFaceSurfIdRep_c"
        | "moEndSpec_c"
        | "moExtObject_c"
        | "moFavoriteHandle_c"
        | "moFilletSurfIdRep_c"
        | "moFR_c"
        | "moFromEndSpec_c"
        | "moFromSktEnt3IntSurfIdRep_c"
        | "moFromSktEntSurfIdRep_c"
        | "moLineBackedUpData_c"
        | "moPerBodyChooserData_c"
        | "moPointBackedUpData_c"
        | "moSketchChain_c"
        | "moSketchExtRef_w"
        | "moSketchRegion_c"
        | "moSurfaceIdRep_c"
        | "sgExtEnt_c" => (NativeClassKind::Auxiliary, Auxiliary, None, None),
        _ => (NativeClassKind::Unknown, Native, None, None),
    };
    NativeObjectClass {
        kind,
        role,
        feature,
        tree_node,
    }
}

fn tree_node_class(
    role: FeatureTreeNodeRole,
) -> (
    NativeClassKind,
    FeatureInputClassRole,
    Option<FeatureClass>,
    Option<FeatureTreeNodeRole>,
) {
    (
        NativeClassKind::TreeNode(role),
        FeatureInputClassRole::Auxiliary,
        None,
        Some(role),
    )
}

fn relation_class(
    family: FeatureInputRelationFamily,
) -> (
    NativeClassKind,
    FeatureInputClassRole,
    Option<FeatureClass>,
    Option<FeatureTreeNodeRole>,
) {
    (
        NativeClassKind::SketchRelation(family),
        FeatureInputClassRole::SketchConstraint,
        None,
        None,
    )
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
    native_object_class(class?).feature
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
        "Extrusion" | "Cut" => FeatureClass::Extrude,
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
        "Helix" | "HelixSpiral" | "Helix/Spiral" => FeatureClass::Helix,
        "Surface-Sweep" => FeatureClass::Sweep,
        "Thicken" | "Thickness" => FeatureClass::Thicken,
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
    fn operation_classes_classify_without_localized_type_tokens() {
        for (class, expected) in [
            ("moHoleWzd_c", FeatureClass::Hole),
            ("moRevolution_c", FeatureClass::Revolve),
            ("moRevCut_c", FeatureClass::Revolve),
            ("moRefAxis_c", FeatureClass::ReferenceAxis),
            ("moMirrorPattern_c", FeatureClass::Pattern),
        ] {
            assert_eq!(
                classify(&feature("Feature", "localized", "localized", Some(class))),
                Some(expected),
                "{class}"
            );
        }
    }

    #[test]
    fn serialized_type_tokens_classify_generic_feature_elements() {
        for (kind, class) in [
            ("Helix/Spiral", FeatureClass::Helix),
            ("Surface-Sweep", FeatureClass::Sweep),
            ("Thicken", FeatureClass::Thicken),
        ] {
            assert_eq!(
                classify(&feature("Feature", "localized display name", kind, None)),
                Some(class),
                "{kind}"
            );
        }
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

    #[test]
    fn native_taxonomy_carries_orthogonal_object_semantics() {
        let plane = native_object_class("moRefPlane_c");
        assert_eq!(plane.kind, NativeClassKind::ReferencePlane);
        assert_eq!(plane.role, FeatureInputClassRole::Reference);
        assert_eq!(plane.feature, Some(FeatureClass::ReferencePlane));
        assert_eq!(plane.tree_node, None);

        let folder = native_object_class("moSolidBodyFolder_c");
        assert_eq!(folder.role, FeatureInputClassRole::Auxiliary);
        assert_eq!(folder.feature, None);
        assert_eq!(folder.tree_node, Some(FeatureTreeNodeRole::SolidBodies));

        let markup = native_object_class("moInkMarkupFolder_c");
        assert_eq!(markup.role, FeatureInputClassRole::Auxiliary);
        assert_eq!(markup.tree_node, Some(FeatureTreeNodeRole::Markups));

        let origin = native_object_class("moOriginProfileFeature_c");
        assert_eq!(origin.kind, NativeClassKind::OriginProfileFeature);
        assert_eq!(origin.role, FeatureInputClassRole::Feature);
        assert_eq!(origin.feature, None);
        assert_eq!(origin.tree_node, Some(FeatureTreeNodeRole::ModelOrigin));

        for name in [
            "moCompReferenceCurve_c",
            "moCompSurfaceBody_c",
            "moConstSurfRef_w",
            "moEndPointRef_w",
            "moGeneralCurveRef_w",
            "moLineRef_w",
            "moSingleFaceRef_w",
            "moSolidRef_w",
        ] {
            let reference = native_object_class(name);
            assert_eq!(reference.kind, NativeClassKind::Reference, "{name}");
            assert_eq!(reference.role, FeatureInputClassRole::Reference, "{name}");
        }

        let relation = native_object_class("sgPntPntDist");
        assert_eq!(
            relation.kind,
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::PointPointDistance)
        );
        let diameter = native_object_class("sgCircleDim");
        assert_eq!(
            diameter.kind,
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::CircleDiameter)
        );
        assert_eq!(diameter.role, FeatureInputClassRole::SketchConstraint);

        for name in [
            "sgArcHandle",
            "sgEntHandle",
            "sgLineHandle",
            "sgPointHandle",
            "sgSplineHandle",
        ] {
            let entity = native_object_class(name);
            assert_eq!(entity.kind, NativeClassKind::SketchEntity, "{name}");
            assert_eq!(entity.role, FeatureInputClassRole::SketchEntity, "{name}");
        }

        for name in [
            "AngleDim_c",
            "moDisplayAngularDim_c",
            "moDisplayDim_c",
            "moDisplayLinearPattCntDim_c",
            "moNumberDim_c",
            "moScalerDim_c",
            "moSkDimHandleLinearPattCnt_c",
            "moSkDimHandleOffset_c",
            "sgDimEntityHelpData_c",
            "sgLinearPattCntDim",
            "sgOffsetDim",
            "sgSkOffsetDim",
        ] {
            let dimension = native_object_class(name);
            assert_eq!(dimension.kind, NativeClassKind::Dimension, "{name}");
            assert_eq!(dimension.role, FeatureInputClassRole::Dimension, "{name}");
        }

        assert_eq!(
            native_object_class("futureClass_c").kind,
            NativeClassKind::Unknown
        );
    }

    #[test]
    fn every_known_feature_class_has_projection_role() {
        for name in [
            "moExtrusion_c",
            "moICE_c",
            "moCut_c",
            "Fillet_c",
            "Chamfer_c",
            "moOriginProfileFeature_c",
            "moProfileFeature_c",
            "mo3DProfileFeature_c",
            "moRefPlane_c",
            "moThicken_c",
            "moSweep_c",
            "moSweepRefSurface_c",
            "moHelix_c",
            "moLPattern_c",
            "moCurvePattern_c",
            "moCombineBodies_c",
            "moDeleteBody_c",
        ] {
            let class = native_object_class(name);
            assert!(
                class.feature.is_some() || class.tree_node.is_some(),
                "missing projection role for {name}"
            );
            assert_ne!(
                class.role,
                FeatureInputClassRole::Native,
                "missing role for {name}"
            );
        }
    }
}
