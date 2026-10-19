// SPDX-License-Identifier: Apache-2.0
//! Known native class declarations for synthesized neutral operations.

use cadmpeg_ir::features::{BooleanOp, FeatureOperation, FeatureTreeNodeRole};

pub(super) fn class(operation: &FeatureOperation) -> Option<&'static str> {
    Some(match operation {
        FeatureOperation::Extrude {
            op: BooleanOp::Cut, ..
        } => "moCut_c",
        FeatureOperation::Extrude { .. } => "moExtrusion_c",
        FeatureOperation::Revolve {
            op: BooleanOp::Cut, ..
        } => "moRevCut_c",
        FeatureOperation::Revolve { .. } => "moRevolution_c",
        FeatureOperation::Fillet { .. } => "Fillet_c",
        FeatureOperation::Chamfer { .. } => "Chamfer_c",
        FeatureOperation::Hole { .. } => "moHoleWzd_c",
        FeatureOperation::Sketch { .. } => "moProfileFeature_c",
        FeatureOperation::SpatialSketch { .. } => "mo3DProfileFeature_c",
        FeatureOperation::StoredGeometry {} => "moBaseBody_c",
        FeatureOperation::Shell { .. } => "moShell_c",
        FeatureOperation::Thicken { .. } => "moThicken_c",
        FeatureOperation::Draft { .. } => "moDraft_c",
        FeatureOperation::DatumPlane { .. }
        | FeatureOperation::DatumPrincipalPlane { .. }
        | FeatureOperation::DatumOffsetPlane { .. } => "moRefPlane_c",
        FeatureOperation::DatumAxis { .. } => "moRefAxis_c",
        FeatureOperation::DatumPoint { .. } => "moRefPoint_c",
        FeatureOperation::DatumCoordinateSystem { .. } => "moCoordSys_c",
        FeatureOperation::Sweep { .. } => "moSweep_c",
        FeatureOperation::Loft { .. } => "moBlend_c",
        FeatureOperation::Rib { .. } => "moRib_c",
        FeatureOperation::Helix { .. } => "moHelix_c",
        FeatureOperation::Combine { .. } => "moCombineBodies_c",
        FeatureOperation::DeleteBody { .. } => "moDeleteBody_c",
        FeatureOperation::DeleteFace { .. } => "moDelFace_c",
        FeatureOperation::MoveFace { .. } => "moMoveFace_c",
        FeatureOperation::MoveBody { .. } => "moMoveCopyBody_c",
        FeatureOperation::Dome { .. } => "moDome_c",
        FeatureOperation::OffsetSurface { .. } => "moOffsetRefSurface_c",
        FeatureOperation::KnitSurface { .. } => "moSewRefSurface_c",
        FeatureOperation::FilledSurface { .. } => "moFillRefSurface_c",
        FeatureOperation::TrimSurface { .. } => "moTrimRefSurface_c",
        FeatureOperation::ExtendSurface { .. } => "moExtendRefSurface_c",
        FeatureOperation::RuledSurface { .. } => "moRuledSrfFromEdge_c",
        FeatureOperation::CutWithSurface { .. } => "moSurfCut_c",
        FeatureOperation::CosmeticThread { .. } => "moCosmeticThread_c",
        FeatureOperation::Pattern { pattern, .. } => match pattern.definition() {
            cadmpeg_ir::features::patterns::PatternTransform::Linear { .. }
            | cadmpeg_ir::features::patterns::PatternTransform::LinearOffsets { .. } => {
                "moLPattern_c"
            }
            cadmpeg_ir::features::patterns::PatternTransform::Circular { .. }
            | cadmpeg_ir::features::patterns::PatternTransform::CircularAngles { .. } => {
                "moCirPattern_c"
            }
            cadmpeg_ir::features::patterns::PatternTransform::CurveDriven { .. } => {
                "moCurvePattern_c"
            }
            cadmpeg_ir::features::patterns::PatternTransform::Mirror { .. }
            | cadmpeg_ir::features::patterns::PatternTransform::MirrorReference { .. } => {
                "moMirrorPattern_c"
            }
            _ => return None,
        },
        FeatureOperation::TreeNode { role, .. } => match role {
            FeatureTreeNodeRole::Equations => "moEqnFolder_c",
            FeatureTreeNodeRole::History => "moHistoryFolder_c",
            FeatureTreeNodeRole::Annotations => "moDetailCabinet_c",
            FeatureTreeNodeRole::Comments => "moCommentsFolder_c",
            FeatureTreeNodeRole::FeatureFolder => "moFtrFolder_c",
            FeatureTreeNodeRole::SolidBodies => "moSolidBodyFolder_c",
            FeatureTreeNodeRole::SurfaceBodies => "moSurfaceBodyFolder_c",
            _ => return None,
        },
        _ => return None,
    })
}
