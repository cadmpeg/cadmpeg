// SPDX-License-Identifier: Apache-2.0
//! Structural classification of native feature-history objects.
#![deny(clippy::disallowed_methods)]

use crate::records::Feature;
use crate::records::FeatureSource;
use crate::records::{FeatureInputClassRole, FeatureInputRelationFamily};
use cadmpeg_ir::features::{FeatureTreeNodeRole, PrincipalPlane};

/// Semantic family established by native record identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FeatureClass {
    Sketch,
    SketchBlockDefinition,
    SketchBlockInstance,
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
    SplitFace,
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
    CosmeticThread,
}

/// Semantic kind of a serialized native object class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeClassKind {
    /// A native modeling operation identified by its semantic feature family.
    Operation(FeatureClass),
    /// A native planar-surface construction feature with defining selections.
    PlanarSurface,
    Extrusion,
    SurfaceExtrusion,
    Fillet,
    Chamfer,
    OriginProfileFeature,
    ProfileFeature,
    SketchBlockDefinition,
    SketchBlockInstance,
    ReferencePlane,
    ReferenceAxis,
    Thicken,
    Sweep,
    SweepCut,
    SweepReferenceSurface,
    Loft,
    LoftCut,
    SurfaceLoft,
    Helix,
    HoleWizard,
    Revolution,
    LinearPattern,
    CircularPattern,
    CurvePattern,
    MirrorPattern,
    Combine,
    DeleteBody,
    CosmeticThread,
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

impl NativeClassKind {
    pub(crate) fn role(self) -> FeatureInputClassRole {
        match self {
            Self::Operation(_)
            | Self::PlanarSurface
            | Self::Extrusion
            | Self::SurfaceExtrusion
            | Self::Fillet
            | Self::Chamfer
            | Self::OriginProfileFeature
            | Self::ProfileFeature
            | Self::SketchBlockDefinition
            | Self::SketchBlockInstance
            | Self::Thicken
            | Self::Sweep
            | Self::SweepCut
            | Self::SweepReferenceSurface
            | Self::Loft
            | Self::LoftCut
            | Self::SurfaceLoft
            | Self::Helix
            | Self::HoleWizard
            | Self::Revolution
            | Self::LinearPattern
            | Self::CircularPattern
            | Self::CurvePattern
            | Self::MirrorPattern
            | Self::Combine
            | Self::DeleteBody
            | Self::CosmeticThread => FeatureInputClassRole::Feature,
            Self::ReferencePlane | Self::ReferenceAxis | Self::Reference => {
                FeatureInputClassRole::Reference
            }
            Self::TreeNode(_) | Self::Auxiliary => FeatureInputClassRole::Auxiliary,
            Self::Sketch => FeatureInputClassRole::Sketch,
            Self::SketchEntity => FeatureInputClassRole::SketchEntity,
            Self::Dimension => FeatureInputClassRole::Dimension,
            Self::LengthParameter => FeatureInputClassRole::Parameter,
            Self::Unknown => FeatureInputClassRole::Native,
            Self::SketchRelation(_) => FeatureInputClassRole::SketchConstraint,
        }
    }

    pub(crate) fn feature(self) -> Option<FeatureClass> {
        use NativeClassKind::{
            Auxiliary, Chamfer, CircularPattern, Combine, CosmeticThread, CurvePattern, DeleteBody,
            Dimension, Extrusion, Fillet, Helix, HoleWizard, LengthParameter, LinearPattern, Loft,
            LoftCut, MirrorPattern, Operation, OriginProfileFeature, PlanarSurface, ProfileFeature,
            Reference, ReferenceAxis, ReferencePlane, Revolution, Sketch, SketchBlockDefinition,
            SketchBlockInstance, SketchEntity, SketchRelation, SurfaceExtrusion, SurfaceLoft,
            Sweep, SweepCut, SweepReferenceSurface, Thicken, TreeNode, Unknown,
        };
        Some(match self {
            Operation(feature) => feature,
            Extrusion | SurfaceExtrusion => FeatureClass::Extrude,
            Fillet => FeatureClass::Fillet,
            Chamfer => FeatureClass::Chamfer,
            ProfileFeature => FeatureClass::Sketch,
            SketchBlockDefinition => FeatureClass::SketchBlockDefinition,
            SketchBlockInstance => FeatureClass::SketchBlockInstance,
            ReferencePlane => FeatureClass::ReferencePlane,
            ReferenceAxis => FeatureClass::ReferenceAxis,
            Thicken => FeatureClass::Thicken,
            Sweep | SweepCut | SweepReferenceSurface => FeatureClass::Sweep,
            Loft | LoftCut | SurfaceLoft => FeatureClass::Loft,
            Helix => FeatureClass::Helix,
            HoleWizard => FeatureClass::Hole,
            Revolution => FeatureClass::Revolve,
            LinearPattern | CircularPattern | CurvePattern | MirrorPattern => FeatureClass::Pattern,
            Combine => FeatureClass::Combine,
            DeleteBody => FeatureClass::DeleteBody,
            CosmeticThread => FeatureClass::CosmeticThread,
            PlanarSurface | OriginProfileFeature | TreeNode(_) | Sketch | SketchEntity
            | SketchRelation(_) | Dimension | LengthParameter | Reference | Auxiliary | Unknown => {
                return None
            }
        })
    }

    pub(crate) fn tree_node(self) -> Option<FeatureTreeNodeRole> {
        match self {
            NativeClassKind::TreeNode(role) => Some(role),
            NativeClassKind::OriginProfileFeature => Some(FeatureTreeNodeRole::ModelOrigin),
            _ => None,
        }
    }
}

/// Resolve a serialized class name through the format-wide object taxonomy.
pub(crate) fn native_object_class(name: &str) -> NativeClassKind {
    match name {
        "moExtrusion_c" | "moICE_c" | "moCut_c" => NativeClassKind::Extrusion,
        "moExtruRefSurface_c" => NativeClassKind::SurfaceExtrusion,
        "Fillet_c" => NativeClassKind::Fillet,
        "Chamfer_c" => NativeClassKind::Chamfer,
        "moOriginProfileFeature_c" => NativeClassKind::OriginProfileFeature,
        "moProfileFeature_c" | "mo3DProfileFeature_c" => NativeClassKind::ProfileFeature,
        "moSketchBlockDef_c" => NativeClassKind::SketchBlockDefinition,
        "moSketchBlockInst_c" => NativeClassKind::SketchBlockInstance,
        "moRefPlane_c" => NativeClassKind::ReferencePlane,
        "moRefAxis_c" => NativeClassKind::ReferenceAxis,
        "moThicken_c" => NativeClassKind::Thicken,
        "moPLine_c" => NativeClassKind::Operation(FeatureClass::SplitFace),
        "moPLineProject_c"
        | "moPLineProjIdRep_c"
        | "moPLineSurfIdRep_c"
        | "moPerBodyChooserDataWithFileName_c" => NativeClassKind::Auxiliary,
        "moSweep_c" => NativeClassKind::Sweep,
        "moSweepCut_c" => NativeClassKind::SweepCut,
        "moSweepRefSurface_c" => NativeClassKind::SweepReferenceSurface,
        "moBlend_c" => NativeClassKind::Loft,
        "moBlendCut_c" => NativeClassKind::LoftCut,
        "moBlendRefSurface_c" => NativeClassKind::SurfaceLoft,
        "moHelix_c" => NativeClassKind::Helix,
        "moHoleWzd_c" => NativeClassKind::HoleWizard,
        "moRevolution_c" | "moRevCut_c" => NativeClassKind::Revolution,
        "moLPattern_c" => NativeClassKind::LinearPattern,
        "moCirPattern_c" => NativeClassKind::CircularPattern,
        "moCurvePattern_c" => NativeClassKind::CurvePattern,
        "moMirrorPattern_c" | "moMirrorSolid_c" => NativeClassKind::MirrorPattern,
        "moCombineBodies_c" => NativeClassKind::Combine,
        "moDeleteBody_c" => NativeClassKind::DeleteBody,
        "moDome_c" => NativeClassKind::Operation(FeatureClass::Dome),
        "moRib_c" => NativeClassKind::Operation(FeatureClass::Rib),
        "moShell_c" => NativeClassKind::Operation(FeatureClass::Shell),
        "moDraft_c" => NativeClassKind::Operation(FeatureClass::Draft),
        "moOffsetRefSurface_c" => NativeClassKind::Operation(FeatureClass::OffsetSurface),
        "moSewRefSurface_c" => NativeClassKind::Operation(FeatureClass::KnitSurface),
        "moFillRefSurface_c" => NativeClassKind::Operation(FeatureClass::FilledSurface),
        "moTrimRefSurface_c" => NativeClassKind::Operation(FeatureClass::TrimSurface),
        "moExtendRefSurface_c" => NativeClassKind::Operation(FeatureClass::ExtendSurface),
        "moRuledSrfFromEdge_c" => NativeClassKind::Operation(FeatureClass::RuledSurface),
        "moSurfCut_c" => NativeClassKind::Operation(FeatureClass::CutWithSurface),
        "moPlanarSurface_c" => NativeClassKind::PlanarSurface,
        "moDelFace_c" => NativeClassKind::Operation(FeatureClass::DeleteFace),
        "moMoveFace_c" => NativeClassKind::Operation(FeatureClass::MoveFace),
        "moMoveCopyBody_c" => NativeClassKind::Operation(FeatureClass::MoveBody),
        "VarFillet_c" => NativeClassKind::Fillet,
        "moRefPoint_c" => NativeClassKind::Operation(FeatureClass::ReferencePoint),
        "moCoordSys_c" => NativeClassKind::Operation(FeatureClass::CoordinateSystem),

        "moDetailCabinet_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Annotations),
        "moDetailFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Details),
        "moCommentsFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Comments),
        "moCosmeticThread_c" | "moDerivedCosmeticThread_c" => NativeClassKind::CosmeticThread,
        "moDocsFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::DesignBinder),
        "moEnvFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::LightsAndCameras),
        "moEqnFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Equations),
        "moFavoriteFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Favorites),
        "moFtrFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::FeatureFolder),
        "moHistoryFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::History),
        "moInkMarkupFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Markups),
        "moMaterialFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Materials),
        "moNotesAreaFtrFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Notes),
        "moSelectionSetFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::SelectionSets),
        "moSensorFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Sensors),
        "moSolidBodyFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::SolidBodies),
        "moSurfaceBodyFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::SurfaceBodies),
        "moTableFolder_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::Tables),
        "moAmbientLight_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::AmbientLight),
        "moDirectionLight_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::DirectionalLight),
        "moPointLight_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::PointLight),
        "moSpotLight_c" => NativeClassKind::TreeNode(FeatureTreeNodeRole::SpotLight),

        "sgSketch" => NativeClassKind::Sketch,
        "sgArcHandle" | "sgEntHandle" | "sgLineHandle" | "sgPointHandle" | "sgSplineHandle" => {
            NativeClassKind::SketchEntity
        }
        "sgLLDist" => NativeClassKind::SketchRelation(FeatureInputRelationFamily::LineLineDistance),
        "sgPntPntDist" => {
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::PointPointDistance)
        }
        "sgPntLineDist" => {
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::PointLineDistance)
        }
        "sgPntPntHorDist" => NativeClassKind::SketchRelation(
            FeatureInputRelationFamily::PointPointHorizontalDistance,
        ),
        "sgPntPntVertDist" => {
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::PointPointVerticalDistance)
        }
        "sgAnglDim" => NativeClassKind::SketchRelation(FeatureInputRelationFamily::Angle),
        "sgCircleDim" => {
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::CircleDiameter)
        }
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
        | "AngleDim_c" => NativeClassKind::Dimension,
        "sgDimEntityHelpData_c" | "sgLinearPattCntDim" | "sgOffsetDim" | "sgSkOffsetDim" => {
            NativeClassKind::Dimension
        }
        "moLengthParameter_c" => NativeClassKind::LengthParameter,
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
        | "moVertexRef_c" => NativeClassKind::Reference,
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
        | "moMirPatternSurfIdRep_c"
        | "moWzdHoleSurfIdRep_c"
        | "sgExtEnt_c" => NativeClassKind::Auxiliary,
        _ => NativeClassKind::Unknown,
    }
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

/// Classify a built-in principal plane from its native class and reserved identity.
pub(crate) fn principal_plane(feature: &Feature) -> Option<PrincipalPlane> {
    if native_object_class(feature.input_class.as_deref()?) != NativeClassKind::ReferencePlane
        || !feature.parameters.is_empty()
        || !feature.properties.is_empty()
    {
        return None;
    }
    match feature.source_value()? {
        2 => Some(PrincipalPlane::Front),
        3 => Some(PrincipalPlane::Top),
        4 => Some(PrincipalPlane::Right),
        _ => None,
    }
}

/// Classify a built-in principal plane from a complete reserved-identity triplet.
pub(crate) fn principal_plane_with_siblings(
    feature: &Feature,
    siblings: &[Feature],
) -> Option<PrincipalPlane> {
    let is_builtin_plane = |candidate: &Feature| {
        classify(candidate) == Some(FeatureClass::ReferencePlane)
            && candidate.parameters.is_empty()
            && candidate.properties.is_empty()
    };
    let complete_triplet = |start: u32| {
        (start..start + 3).all(|source| {
            siblings.iter().any(|candidate| {
                candidate.source_value() == Some(source) && is_builtin_plane(candidate)
            })
        })
    };
    let start = if complete_triplet(2) {
        2
    } else if complete_triplet(3) {
        return None;
    } else {
        return principal_plane(feature);
    };
    if !is_builtin_plane(feature) {
        return None;
    }
    match feature.source_value()?.checked_sub(start)? {
        0 => Some(PrincipalPlane::Front),
        1 => Some(PrincipalPlane::Top),
        2 => Some(PrincipalPlane::Right),
        _ => None,
    }
}

fn classify_input_class(class: Option<&str>) -> Option<FeatureClass> {
    native_object_class(class?).feature()
}

pub(crate) fn classify_xml_element(tag: &str) -> Option<FeatureClass> {
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
        "Hole" | "HoleWizard" => FeatureClass::Hole,
        "Revolve" | "Revolution" => FeatureClass::Revolve,
        "Pattern" | "Mirror" => FeatureClass::Pattern,
        "Sweep" | "Surface-Sweep" => FeatureClass::Sweep,
        "Loft" | "Boundary" => FeatureClass::Loft,
        "Rib" => FeatureClass::Rib,
        _ => return None,
    })
}

pub(crate) fn classify_type_token(kind: &str) -> Option<FeatureClass> {
    Some(match kind {
        "Plane" => FeatureClass::ReferencePlane,
        "BossExtrude" | "CutExtrude" | "Extrude" | "Surface-Extrude" => FeatureClass::Extrude,
        "Helix" | "HelixSpiral" | "Helix/Spiral" => FeatureClass::Helix,
        "Surface-Sweep" | "Sweep" | "Cut-Sweep" => FeatureClass::Sweep,
        "Thicken" | "Thickness" => FeatureClass::Thicken,
        "LinearPattern" | "CircularPattern" | "CrvPattern" | "CurvePattern"
        | "CurveDrivenPattern" | "CirPattern" | "Mirror" => FeatureClass::Pattern,
        "BossLoft" | "CutLoft" | "BoundaryBoss" | "BoundaryCut" | "Loft" | "Cut-Loft"
        | "Surface-Loft" => FeatureClass::Loft,
        "Body-Delete" | "Body-Delete/Keep " | "Body-Delete/Keep" => FeatureClass::DeleteBody,
        "Body-Move/Copy" => FeatureClass::MoveBody,
        "Dome" => FeatureClass::Dome,
        "Rib" => FeatureClass::Rib,
        "Shell" => FeatureClass::Shell,
        "Draft" => FeatureClass::Draft,
        "Split Line" => FeatureClass::SplitFace,
        "Surface-Offset" => FeatureClass::OffsetSurface,
        "Surface-Knit" => FeatureClass::KnitSurface,
        "Surface-Fill" => FeatureClass::FilledSurface,
        "Surface-Trim" => FeatureClass::TrimSurface,
        "Surface-Extend" => FeatureClass::ExtendSurface,
        "Ruled Surface" => FeatureClass::RuledSurface,
        "SurfaceCut" => FeatureClass::CutWithSurface,
        "DeleteFace" => FeatureClass::DeleteFace,
        "Move Face" => FeatureClass::MoveFace,
        "VarFillet" => FeatureClass::Fillet,
        "3DPoint" => FeatureClass::ReferencePoint,
        "Coordinate System" => FeatureClass::CoordinateSystem,
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
        assert_eq!(
            classify(&feature("Feature", "arbitrary", "Plane", None)),
            Some(FeatureClass::ReferencePlane)
        );
    }

    #[test]
    fn classless_plane_triplet_has_principal_plane_identity() {
        let mut planes = [
            feature("Feature", "localized front", "Plane", None),
            feature("Feature", "localized top", "Plane", None),
            feature("Feature", "localized right", "Plane", None),
        ];
        for (plane, source) in planes.iter_mut().zip([2, 3, 4]) {
            plane.source_id = FeatureSource::from_value(source);
        }

        assert_eq!(
            principal_plane_with_siblings(&planes[0], &planes),
            Some(PrincipalPlane::Front)
        );
        assert_eq!(
            principal_plane_with_siblings(&planes[1], &planes),
            Some(PrincipalPlane::Top)
        );
        assert_eq!(
            principal_plane_with_siblings(&planes[2], &planes),
            Some(PrincipalPlane::Right)
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
            ("moMirrorSolid_c", FeatureClass::Pattern),
            ("moDome_c", FeatureClass::Dome),
            ("moRib_c", FeatureClass::Rib),
            ("moBlendRefSurface_c", FeatureClass::Loft),
            ("moExtruRefSurface_c", FeatureClass::Extrude),
            ("moCirPattern_c", FeatureClass::Pattern),
            ("moCoordSys_c", FeatureClass::CoordinateSystem),
            ("moPLine_c", FeatureClass::SplitFace),
        ] {
            assert_eq!(
                classify(&feature("Feature", "localized", "localized", Some(class))),
                Some(expected),
                "{class}"
            );
        }
    }

    #[test]
    fn operation_tokens_classify_generic_feature_elements() {
        for (kind, expected) in [
            ("Dome", FeatureClass::Dome),
            ("Rib", FeatureClass::Rib),
            ("Surface-Trim", FeatureClass::TrimSurface),
            ("Surface-Knit", FeatureClass::KnitSurface),
            ("Surface-Fill", FeatureClass::FilledSurface),
            ("Surface-Extend", FeatureClass::ExtendSurface),
            ("Surface-Offset", FeatureClass::OffsetSurface),
            ("SurfaceCut", FeatureClass::CutWithSurface),
            ("Body-Move/Copy", FeatureClass::MoveBody),
            ("3DPoint", FeatureClass::ReferencePoint),
            ("Coordinate System", FeatureClass::CoordinateSystem),
            ("Split Line", FeatureClass::SplitFace),
        ] {
            assert_eq!(
                classify(&feature("Feature", "localized", kind, None)),
                Some(expected),
                "{kind}"
            );
        }
    }

    #[test]
    fn operation_class_taxonomy_preserves_family_identity() {
        for (name, kind, feature) in [
            (
                "moDome_c",
                NativeClassKind::Operation(FeatureClass::Dome),
                FeatureClass::Dome,
            ),
            ("moBlendCut_c", NativeClassKind::LoftCut, FeatureClass::Loft),
            (
                "moBlendRefSurface_c",
                NativeClassKind::SurfaceLoft,
                FeatureClass::Loft,
            ),
            (
                "moExtruRefSurface_c",
                NativeClassKind::SurfaceExtrusion,
                FeatureClass::Extrude,
            ),
            (
                "moCirPattern_c",
                NativeClassKind::CircularPattern,
                FeatureClass::Pattern,
            ),
            (
                "moPLine_c",
                NativeClassKind::Operation(FeatureClass::SplitFace),
                FeatureClass::SplitFace,
            ),
        ] {
            let class = native_object_class(name);
            assert_eq!(class, kind, "{name}");
            assert_eq!(class.role(), FeatureInputClassRole::Feature, "{name}");
            assert_eq!(class.feature(), Some(feature), "{name}");
            assert_eq!(class.tree_node(), None, "{name}");
        }

        let planar_surface = native_object_class("moPlanarSurface_c");
        assert_eq!(planar_surface, NativeClassKind::PlanarSurface);
        assert_eq!(planar_surface.role(), FeatureInputClassRole::Feature);
        assert_eq!(planar_surface.feature(), None);
        assert_eq!(planar_surface.tree_node(), None);
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
    fn hole_wizard_element_is_a_hole_independent_of_display_language() {
        assert_eq!(
            classify(&feature("HoleWizard", "localized", "localized", None)),
            Some(FeatureClass::Hole)
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

    #[test]
    fn native_taxonomy_carries_orthogonal_object_semantics() {
        let plane = native_object_class("moRefPlane_c");
        assert_eq!(plane, NativeClassKind::ReferencePlane);
        assert_eq!(plane.role(), FeatureInputClassRole::Reference);
        assert_eq!(plane.feature(), Some(FeatureClass::ReferencePlane));
        assert_eq!(plane.tree_node(), None);

        let folder = native_object_class("moSolidBodyFolder_c");
        assert_eq!(folder.role(), FeatureInputClassRole::Auxiliary);
        assert_eq!(folder.feature(), None);
        assert_eq!(folder.tree_node(), Some(FeatureTreeNodeRole::SolidBodies));

        let markup = native_object_class("moInkMarkupFolder_c");
        assert_eq!(markup.role(), FeatureInputClassRole::Auxiliary);
        assert_eq!(markup.tree_node(), Some(FeatureTreeNodeRole::Markups));

        for class in ["moMirPatternSurfIdRep_c", "moWzdHoleSurfIdRep_c"] {
            let output = native_object_class(class);
            assert_eq!(output.role(), FeatureInputClassRole::Auxiliary, "{class}");
            assert_eq!(output.feature(), None, "{class}");
            assert_eq!(output.tree_node(), None, "{class}");
        }

        for class in ["moCosmeticThread_c", "moDerivedCosmeticThread_c"] {
            let thread = native_object_class(class);
            assert_eq!(thread, NativeClassKind::CosmeticThread, "{class}");
            assert_eq!(thread.role(), FeatureInputClassRole::Feature, "{class}");
            assert_eq!(
                thread.feature(),
                Some(FeatureClass::CosmeticThread),
                "{class}"
            );
            assert_eq!(thread.tree_node(), None, "{class}");
        }

        for (class, role) in [
            ("moAmbientLight_c", FeatureTreeNodeRole::AmbientLight),
            ("moDetailFolder_c", FeatureTreeNodeRole::Details),
            ("moDirectionLight_c", FeatureTreeNodeRole::DirectionalLight),
            ("moEnvFolder_c", FeatureTreeNodeRole::LightsAndCameras),
            ("moFtrFolder_c", FeatureTreeNodeRole::FeatureFolder),
            ("moPointLight_c", FeatureTreeNodeRole::PointLight),
            ("moSpotLight_c", FeatureTreeNodeRole::SpotLight),
            ("moTableFolder_c", FeatureTreeNodeRole::Tables),
        ] {
            let folder = native_object_class(class);
            assert_eq!(folder.role(), FeatureInputClassRole::Auxiliary, "{class}");
            assert_eq!(folder.feature(), None, "{class}");
            assert_eq!(folder.tree_node(), Some(role), "{class}");
        }

        let origin = native_object_class("moOriginProfileFeature_c");
        assert_eq!(origin, NativeClassKind::OriginProfileFeature);
        assert_eq!(origin.role(), FeatureInputClassRole::Feature);
        assert_eq!(origin.feature(), None);
        assert_eq!(origin.tree_node(), Some(FeatureTreeNodeRole::ModelOrigin));

        for (name, kind, feature) in [
            (
                "moSketchBlockDef_c",
                NativeClassKind::SketchBlockDefinition,
                FeatureClass::SketchBlockDefinition,
            ),
            (
                "moSketchBlockInst_c",
                NativeClassKind::SketchBlockInstance,
                FeatureClass::SketchBlockInstance,
            ),
        ] {
            let block = native_object_class(name);
            assert_eq!(block, kind, "{name}");
            assert_eq!(block.role(), FeatureInputClassRole::Feature, "{name}");
            assert_eq!(block.feature(), Some(feature), "{name}");
        }

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
            assert_eq!(reference, NativeClassKind::Reference, "{name}");
            assert_eq!(reference.role(), FeatureInputClassRole::Reference, "{name}");
        }

        let relation = native_object_class("sgPntPntDist");
        assert_eq!(
            relation,
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::PointPointDistance)
        );
        let diameter = native_object_class("sgCircleDim");
        assert_eq!(
            diameter,
            NativeClassKind::SketchRelation(FeatureInputRelationFamily::CircleDiameter)
        );
        assert_eq!(diameter.role(), FeatureInputClassRole::SketchConstraint);

        for name in [
            "sgArcHandle",
            "sgEntHandle",
            "sgLineHandle",
            "sgPointHandle",
            "sgSplineHandle",
        ] {
            let entity = native_object_class(name);
            assert_eq!(entity, NativeClassKind::SketchEntity, "{name}");
            assert_eq!(entity.role(), FeatureInputClassRole::SketchEntity, "{name}");
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
            assert_eq!(dimension, NativeClassKind::Dimension, "{name}");
            assert_eq!(dimension.role(), FeatureInputClassRole::Dimension, "{name}");
        }

        assert_eq!(
            native_object_class("futureClass_c"),
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
            "VarFillet_c",
            "Chamfer_c",
            "moOriginProfileFeature_c",
            "moProfileFeature_c",
            "mo3DProfileFeature_c",
            "moSketchBlockDef_c",
            "moSketchBlockInst_c",
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
                class.feature().is_some() || class.tree_node().is_some(),
                "missing projection role for {name}"
            );
            assert_ne!(
                class.role(),
                FeatureInputClassRole::Native,
                "missing role for {name}"
            );
        }
    }
}
