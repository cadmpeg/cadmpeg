// SPDX-License-Identifier: Apache-2.0
//! The v11 operation family of a timeline item, from the operation the v10
//! projection resolved for it (part 6).

use cadmpeg_ir::design::feature::OperationFamily;
use cadmpeg_ir::features::{FeatureOperation as LegacyOperation, UnresolvedFamily};

/// The family a v10 operation establishes, or the native kind of one whose
/// family part 6 does not establish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Family {
    /// The §6 family of the operation.
    Typed(OperationFamily),
    /// The source kind of an operation with no established family.
    Native(String),
}

pub(super) fn family(operation: &LegacyOperation) -> Family {
    let family = match operation {
        LegacyOperation::TreeNode { .. } => return Family::Native("tree_node".to_owned()),
        LegacyOperation::BaseFeature { .. } => return Family::Native("base_feature".to_owned()),
        LegacyOperation::MeshImport { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshGenerateFaceGroups { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshCombineFaceGroups { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshSection { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshModify { .. } => OperationFamily::Mesh,
        LegacyOperation::MeshConvert(..) => OperationFamily::Mesh,
        LegacyOperation::InsertBodies { .. } => return Family::Native("insert_bodies".to_owned()),
        LegacyOperation::InsertComponent { .. } => OperationFamily::Place,
        LegacyOperation::InsertComponents { .. } => OperationFamily::PlaceSet,
        LegacyOperation::ComponentTransfer { .. } => {
            return Family::Native("component_transfer".to_owned())
        }
        LegacyOperation::CapturedComponentPositions { .. } => {
            return Family::Native("captured_component_positions".to_owned())
        }
        LegacyOperation::AssemblyJoint { .. } => OperationFamily::Joint,
        LegacyOperation::Form(..) => OperationFamily::Form,
        LegacyOperation::CosmeticThread { .. } => OperationFamily::Thread,
        LegacyOperation::ReferenceImage { .. } => {
            return Family::Native("reference_image".to_owned())
        }
        LegacyOperation::Decal { .. } => OperationFamily::Decal,
        LegacyOperation::DatumPrincipalPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumThreePointPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumOffsetPlane { .. } => OperationFamily::Datum,
        LegacyOperation::DatumAxis { .. } => OperationFamily::Datum,
        LegacyOperation::DatumPoint { .. } => OperationFamily::Datum,
        LegacyOperation::PointGeometry { .. } => OperationFamily::Datum,
        LegacyOperation::LineSegment { .. } => OperationFamily::Curve,
        LegacyOperation::CircularArc { .. } => OperationFamily::Curve,
        LegacyOperation::EllipticArc { .. } => OperationFamily::Curve,
        LegacyOperation::Polyline { .. } => OperationFamily::Curve,
        LegacyOperation::RegularPolygonCurve { .. } => OperationFamily::Curve,
        LegacyOperation::PlanarPatch { .. } => OperationFamily::Patch,
        LegacyOperation::FaceFromShapes { .. } => OperationFamily::Patch,
        LegacyOperation::DatumCoordinateSystem { .. } => OperationFamily::Datum,
        LegacyOperation::Block { .. } => OperationFamily::Primitive,
        LegacyOperation::EquationCurve { .. } => OperationFamily::Curve,
        LegacyOperation::ProjectedCurve { .. } => OperationFamily::Curve,
        LegacyOperation::ProjectOnSurface { .. } => OperationFamily::Curve,
        LegacyOperation::CompositeCurve { .. } => OperationFamily::Curve,
        LegacyOperation::Helix { .. } => OperationFamily::Curve,
        LegacyOperation::HelixNativeAxis { .. } => OperationFamily::Curve,
        LegacyOperation::Coil { .. } => OperationFamily::HelicalSweep,
        LegacyOperation::Sphere { .. } => OperationFamily::Primitive,
        LegacyOperation::Torus { .. } => OperationFamily::Primitive,
        LegacyOperation::Wrap { .. } => OperationFamily::Emboss,
        LegacyOperation::Sketch { .. } => OperationFamily::Sketch,
        LegacyOperation::SpatialSketch { .. } => OperationFamily::SpatialSketch,
        LegacyOperation::SketchBlockDefinition { .. } => OperationFamily::Sketch,
        LegacyOperation::SketchBlockInstance { .. } => OperationFamily::Sketch,
        LegacyOperation::StoredGeometry { .. } => {
            return Family::Native("stored_geometry".to_owned())
        }
        LegacyOperation::ExtractBody { .. } => OperationFamily::Extract,
        LegacyOperation::DerivedGeometry { .. } => {
            return Family::Native("derived_geometry".to_owned())
        }
        LegacyOperation::ImportedGeometry { .. } => {
            return Family::Native("imported_geometry".to_owned())
        }
        LegacyOperation::Primitive { .. } => OperationFamily::Primitive,
        LegacyOperation::Revolve { .. } => OperationFamily::Revolve,
        LegacyOperation::Sweep { .. } => OperationFamily::Sweep,
        LegacyOperation::HelicalSweep { .. } => OperationFamily::HelicalSweep,
        LegacyOperation::Binder { .. } => return Family::Native("binder".to_owned()),
        LegacyOperation::Rib { .. } => OperationFamily::Rib,
        LegacyOperation::SheetMetalBaseFlange { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalAttachedPlate { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalBend { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalContourFlange { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalUnfold { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalRefold { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalBendLines { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalCorner { .. } => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalEdgeFlange(..) => OperationFamily::SheetMetal,
        LegacyOperation::SheetMetalHem { .. } => OperationFamily::SheetMetal,
        LegacyOperation::Fillet { .. } => OperationFamily::Fillet,
        LegacyOperation::FullRoundFillet { .. } => OperationFamily::Fillet,
        LegacyOperation::FaceBlend { .. } => OperationFamily::Fillet,
        LegacyOperation::Chamfer { .. } => OperationFamily::Chamfer,
        LegacyOperation::Shell { .. } => OperationFamily::Shell,
        LegacyOperation::OffsetShape { .. } => OperationFamily::Offset,
        LegacyOperation::Compound { .. } => return Family::Native("compound".to_owned()),
        LegacyOperation::RefineShape { .. } => return Family::Native("refine_shape".to_owned()),
        LegacyOperation::ReverseShape { .. } => return Family::Native("reverse_shape".to_owned()),
        LegacyOperation::RuledBetweenCurves { .. } => OperationFamily::ImplicitSweep,
        LegacyOperation::SectionShape { .. } => OperationFamily::Curve,
        LegacyOperation::MirrorShape { .. } => OperationFamily::Pattern,
        LegacyOperation::Thicken { .. } => OperationFamily::Thicken,
        LegacyOperation::OffsetSurface { .. } => OperationFamily::Offset,
        LegacyOperation::KnitSurface { .. } => OperationFamily::Knit,
        LegacyOperation::SewBodies { .. } => OperationFamily::Knit,
        LegacyOperation::UnstitchFaces { .. } => OperationFamily::UnsewFaces,
        LegacyOperation::FilledSurface { .. } => OperationFamily::Patch,
        LegacyOperation::TrimSurface { .. } => OperationFamily::Split,
        LegacyOperation::ExtendSurface { .. } => OperationFamily::Extend,
        LegacyOperation::UntrimSurface { .. } => OperationFamily::Extend,
        LegacyOperation::RuledSurface { .. } => OperationFamily::ImplicitSweep,
        LegacyOperation::Draft { .. } => OperationFamily::Draft,
        LegacyOperation::Combine { .. } => OperationFamily::Combine,
        LegacyOperation::BoundaryFill { .. } => OperationFamily::BoundaryFill,
        LegacyOperation::CutWithSurface { .. } => OperationFamily::Split,
        LegacyOperation::TrimBodies { .. } => OperationFamily::Split,
        LegacyOperation::TrimBodiesAtPlane { .. } => OperationFamily::Split,
        LegacyOperation::SplitBody { .. } => OperationFamily::Split,
        LegacyOperation::SilhouetteSplit { .. } => OperationFamily::Split,
        LegacyOperation::SplitFace { .. } => OperationFamily::Split,
        LegacyOperation::DeleteBody { .. } => OperationFamily::DeleteBodies,
        LegacyOperation::DeleteFace { .. } => OperationFamily::DeleteFaces,
        LegacyOperation::ReplaceFace { .. } => OperationFamily::ReplaceFaces,
        LegacyOperation::MoveFace { .. } => OperationFamily::MoveFaces,
        LegacyOperation::MoveBody { .. } => OperationFamily::TransformBodies,
        LegacyOperation::Dome { .. } => OperationFamily::Dome,
        LegacyOperation::Flex { .. } => OperationFamily::Flex,
        LegacyOperation::Scale { .. } => OperationFamily::TransformBodies,
        LegacyOperation::Hole { .. } => OperationFamily::Hole,
        LegacyOperation::Pattern { .. } => OperationFamily::Pattern,
        LegacyOperation::Unresolved { family } => match family {
            UnresolvedFamily::DatumPlane => OperationFamily::Datum,
            UnresolvedFamily::DatumAxis => OperationFamily::Datum,
            UnresolvedFamily::DatumPoint => OperationFamily::Datum,
            UnresolvedFamily::DatumCoordinateSystem => OperationFamily::Datum,
            UnresolvedFamily::BridgeCurve => OperationFamily::Curve,
            UnresolvedFamily::Brep => return Family::Native("brep".to_owned()),
            UnresolvedFamily::Cylinder => OperationFamily::Primitive,
            UnresolvedFamily::Cone => OperationFamily::Primitive,
            UnresolvedFamily::Sphere => OperationFamily::Primitive,
            UnresolvedFamily::Thread => OperationFamily::Thread,
            UnresolvedFamily::DetailedThread => OperationFamily::Thread,
            UnresolvedFamily::Loft => OperationFamily::Loft,
            UnresolvedFamily::ThroughCurveMesh => OperationFamily::Loft,
            UnresolvedFamily::FreeformSurface => OperationFamily::Patch,
            UnresolvedFamily::Extrude => OperationFamily::Extrude,
            UnresolvedFamily::Revolve => OperationFamily::Revolve,
            UnresolvedFamily::Combine => OperationFamily::Combine,
            UnresolvedFamily::Fillet => OperationFamily::Fillet,
            UnresolvedFamily::ExtractFace => OperationFamily::Extract,
            UnresolvedFamily::CopyFace => OperationFamily::Extract,
            UnresolvedFamily::LinkedFace => OperationFamily::Extract,
            UnresolvedFamily::FillHole => OperationFamily::Patch,
            UnresolvedFamily::BoundarySurface => OperationFamily::Patch,
            UnresolvedFamily::Draft => OperationFamily::Draft,
            UnresolvedFamily::DeleteFace => OperationFamily::DeleteFaces,
            UnresolvedFamily::MoveFace => OperationFamily::MoveFaces,
            UnresolvedFamily::MirrorFace => OperationFamily::Pattern,
            UnresolvedFamily::SubdivisionBody => OperationFamily::Form,
            UnresolvedFamily::TopologyOptimization => {
                return Family::Native("topology_optimization".to_owned())
            }
            UnresolvedFamily::MoveObject => OperationFamily::TransformBodies,
        },
        LegacyOperation::Native { kind, .. } => return Family::Native(kind.as_str().to_owned()),
        LegacyOperation::Extrude { .. } => OperationFamily::Extrude,
        LegacyOperation::Loft { .. } => OperationFamily::Loft,
    };
    Family::Typed(family)
}

/// Timeline item types that hold no parameter scope, with the name of the
/// operation each stores: a new component's group scope, whose members are
/// the component's creation and its occurrence step; the grounding of an
/// occurrence; and a paste or body-to-component object.
const ITEM_TYPES: &[(&str, &str)] = &[
    ("54F9ACE8-B5B8-4A6E-B582-64F629511DE4", "NewComponent"),
    ("21E03EC1-0E53-406D-A852-6D088D4D1E95", "Ground"),
    ("56225BC7-69B7-446A-9AEA-6AA25BC7806E", "BodyToComponent"),
    ("BA3059E4-4B23-4D02-9D18-E7BB95C991F3", "Paste"),
];

/// The family of a timeline item the v10 projection did not resolve, from
/// its record's type GUID: the family of the feature kind the type stores,
/// or the native kind the type names.
pub(super) fn kind_family(guid: &str) -> Family {
    use crate::records::feature::scope::DesignFeatureKind as Kind;
    let Some(kind) = crate::design::decode::scopes::parameter_scope::family::from_type_guid(guid)
    else {
        let named = ITEM_TYPES
            .iter()
            .find(|(registered, _)| registered.eq_ignore_ascii_case(guid))
            .map_or(guid, |(_, name)| name);
        return Family::Native(named.to_owned());
    };
    let family = match &kind {
        Kind::Sketch => OperationFamily::Sketch,
        Kind::Assemble | Kind::AsBuilt => OperationFamily::Joint,
        Kind::ComponentInsert => OperationFamily::Place,
        Kind::Extrude => OperationFamily::Extrude,
        Kind::Revolve => OperationFamily::Revolve,
        Kind::Sweep | Kind::Pipe => OperationFamily::Sweep,
        Kind::Loft => OperationFamily::Loft,
        Kind::SpirePrimitive | Kind::CoilPrimitive => OperationFamily::HelicalSweep,
        Kind::SpherePrimitive
        | Kind::TorusPrimitive
        | Kind::BoxPrimitive
        | Kind::CylinderPrimitive => OperationFamily::Primitive,
        Kind::Fillet => OperationFamily::Fillet,
        Kind::Chamfer => OperationFamily::Chamfer,
        Kind::Shell => OperationFamily::Shell,
        Kind::Thicken => OperationFamily::Thicken,
        Kind::Draft => OperationFamily::Draft,
        Kind::Hole => OperationFamily::Hole,
        Kind::Thread => OperationFamily::Thread,
        Kind::Emboss => OperationFamily::Emboss,
        Kind::Decal => OperationFamily::Decal,
        Kind::CPattern
        | Kind::CircularPattern
        | Kind::RPattern
        | Kind::RectangularPattern
        | Kind::PathPattern
        | Kind::Mirror => OperationFamily::Pattern,
        Kind::Combine => OperationFamily::Combine,
        Kind::Split | Kind::SplitFace | Kind::SilhouetteSplit | Kind::SurfaceTrim => {
            OperationFamily::Split
        }
        Kind::RemoveBody => OperationFamily::DeleteBodies,
        Kind::Move | Kind::Scale => OperationFamily::TransformBodies,
        Kind::MoveFace => OperationFamily::MoveFaces,
        Kind::DeleteFace | Kind::SurfaceDeleteFace => OperationFamily::DeleteFaces,
        Kind::ReplaceFace => OperationFamily::ReplaceFaces,
        Kind::SurfacePatch => OperationFamily::Patch,
        Kind::SurfaceStitch => OperationFamily::Knit,
        Kind::UnStitch => OperationFamily::UnsewFaces,
        Kind::SurfaceExtend | Kind::SurfaceUntrim => OperationFamily::Extend,
        Kind::SurfaceOffset => OperationFamily::Offset,
        Kind::SurfaceRuled => OperationFamily::ImplicitSweep,
        Kind::BoundaryFill => OperationFamily::BoundaryFill,
        Kind::Form => OperationFamily::Form,
        Kind::BaseFlange
        | Kind::Face
        | Kind::EdgeFlange
        | Kind::ContourFlange
        | Kind::Hem
        | Kind::Bend
        | Kind::Unfold
        | Kind::Refold => OperationFamily::SheetMetal,
        Kind::GenerateFaceGroups | Kind::CombineFaceGroups => OperationFamily::Mesh,
        Kind::WorkPlane | Kind::WorkAxis | Kind::WorkPoint | Kind::JointOrigin => {
            OperationFamily::Datum
        }
        Kind::Align
        | Kind::Modify
        | Kind::OffsetFaces
        | Kind::MotionLink
        | Kind::CopyPaste
        | Kind::CopyPasteBodies
        | Kind::DerivedInstance
        | Kind::BaseFeature
        | Kind::BaseMeshFeature
        | Kind::ReverseNormal
        | Kind::Canvas
        | Kind::CustomFeature
        | Kind::Native(_) => return Family::Native(kind.as_str().to_owned()),
    };
    Family::Typed(family)
}
