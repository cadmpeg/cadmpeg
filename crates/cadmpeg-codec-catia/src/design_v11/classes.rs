// SPDX-License-Identifier: Apache-2.0
//! The operation family of each compact feature class (`catia.md` §7.3.2,
//! Feature classes).

use cadmpeg_ir::design::feature::OperationFamily;

/// The class of a sketch record.
pub(crate) const SKETCH: &str = "PRTSketch";

/// The class of the part's own planes and of a plane its tree lists with no
/// construction kind.
pub(crate) const PLANE: &str = "GSMPlane";

/// The classes of plane records.
pub(crate) const PLANES: [&str; 9] = [
    PLANE,
    "GSMPlane2Lines",
    "GSMPlaneAngle",
    "GSMPlaneBetween",
    "GSMPlaneEquation",
    "GSMPlaneNormal",
    "GSMPlaneOffset",
    "GSMPlaneOffsetPt",
    "GSMPlaneTangent",
];

/// Each feature class and the §6 family of its operation.
const FAMILIES: &[(&str, OperationFamily)] = &[
    (SKETCH, OperationFamily::Sketch),
    ("GSMPoint", OperationFamily::Datum),
    ("GSMPointBetween", OperationFamily::Datum),
    ("GSMPointCenter", OperationFamily::Datum),
    ("GSMPointCoord", OperationFamily::Datum),
    ("GSMPointOnCurve", OperationFamily::Datum),
    ("GSMPointOnPlane", OperationFamily::Datum),
    ("GSMPointOnSurface", OperationFamily::Datum),
    (PLANE, OperationFamily::Datum),
    ("GSMPlane2Lines", OperationFamily::Datum),
    ("GSMPlaneAngle", OperationFamily::Datum),
    ("GSMPlaneBetween", OperationFamily::Datum),
    ("GSMPlaneEquation", OperationFamily::Datum),
    ("GSMPlaneNormal", OperationFamily::Datum),
    ("GSMPlaneOffset", OperationFamily::Datum),
    ("GSMPlaneOffsetPt", OperationFamily::Datum),
    ("GSMPlaneTangent", OperationFamily::Datum),
    ("AxisSystem", OperationFamily::Datum),
    ("Pad", OperationFamily::Extrude),
    ("Pocket", OperationFamily::Extrude),
    ("GSMExtrude", OperationFamily::Extrude),
    ("Shaft", OperationFamily::Revolve),
    ("Groove", OperationFamily::Revolve),
    ("Rib", OperationFamily::Sweep),
    ("Slot", OperationFamily::Sweep),
    ("Stiffener", OperationFamily::Rib),
    ("GSMLoft", OperationFamily::Loft),
    ("EdgeFillet", OperationFamily::Fillet),
    ("TritangentFillet", OperationFamily::Fillet),
    ("Chamfer", OperationFamily::Chamfer),
    ("Shell", OperationFamily::Shell),
    ("Draft", OperationFamily::Draft),
    ("ThickSurface", OperationFamily::Thicken),
    ("GSMOffset", OperationFamily::Offset),
    ("Thickness", OperationFamily::Offset),
    ("Hole", OperationFamily::Hole),
    ("CATSm_Hole3D", OperationFamily::Hole),
    ("Thread", OperationFamily::Thread),
    ("RectPattern", OperationFamily::Pattern),
    ("CircPattern", OperationFamily::Pattern),
    ("UserPattern", OperationFamily::Pattern),
    ("Mirror", OperationFamily::Pattern),
    ("UdfFeature", OperationFamily::TemplateInstance),
    ("Add", OperationFamily::Combine),
    ("Remove", OperationFamily::Combine),
    ("Assemble", OperationFamily::Combine),
    ("Intersect", OperationFamily::Combine),
    ("Split", OperationFamily::Split),
    ("GSMSplit", OperationFamily::Split),
    ("GSMTrim", OperationFamily::Split),
    ("RemoveFace", OperationFamily::DeleteFaces),
    ("GSMFill", OperationFamily::Patch),
    ("GSMAssemble", OperationFamily::Knit),
    ("CloseSurface", OperationFamily::Knit),
    ("SewSurface", OperationFamily::Graft),
    ("GSMExtract", OperationFamily::Extract),
    ("GSMExtrapol", OperationFamily::Extend),
    ("GSMTranslate", OperationFamily::TransformBodies),
    ("GSMRotate", OperationFamily::TransformBodies),
    ("GSMSymmetry", OperationFamily::TransformBodies),
    ("GSMLine", OperationFamily::Curve),
    ("GSMLinePtDir", OperationFamily::Curve),
    ("GSMLinePtPt", OperationFamily::Curve),
    ("GSMAxisLine", OperationFamily::Curve),
    ("GSMHelix", OperationFamily::Curve),
    ("GSMSpiral", OperationFamily::Curve),
    ("GSMIntersect", OperationFamily::Curve),
    ("GSMProject", OperationFamily::Curve),
    ("GSMCombine", OperationFamily::Curve),
    ("GSMBoundary", OperationFamily::Curve),
    ("ShePart", OperationFamily::SheetMetal),
    ("PredefinedFlange", OperationFamily::SheetMetal),
    ("CATSmd_Stamp", OperationFamily::SheetMetal),
    ("CATSmCorner", OperationFamily::SheetMetal),
    ("CATSmCutOut", OperationFamily::SheetMetal),
    ("CATSmd_Extrusion", OperationFamily::SheetMetal),
    ("CATSmd_Wall", OperationFamily::SheetMetal),
    ("CATSmd_NewWallOnEdge", OperationFamily::SheetMetal),
    ("CATSm_ExtrudedHolePunch", OperationFamily::SheetMetal),
    ("CATSmd_BendExtremity", OperationFamily::SheetMetal),
];

/// Classes the part root lists that hold no operation: its properties and
/// its parameter, relation, and annotation sets.
const NOT_OPERATIONS: [&str; 7] = [
    "MechanicalPartProperties",
    "TTRSSet",
    "RelationSet",
    "ParameterSet",
    "String",
    "GSMWSupportSet",
    "StDynAnalysisRootNodeFeature",
];

/// The §6 family of a feature class's operation.
pub(super) fn family(class: &str) -> Option<OperationFamily> {
    FAMILIES
        .iter()
        .find(|(name, _)| *name == class)
        .map(|(_, family)| *family)
}

/// Whether a class the part root lists holds no operation.
pub(super) fn is_not_operation(class: &str) -> bool {
    NOT_OPERATIONS.contains(&class)
}
