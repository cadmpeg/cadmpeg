use super::{
    ComponentTransferMode, FaceSelection, FeatureOperation, ThickenSide, UnresolvedFamily,
};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::scalar::PositiveLength;
use crate::transform::Transform;

impl FeatureOperation {
    #[doc = "Construct a Thicken with unspecified result and tangent-chain settings."]
    pub fn thicken(
        faces: FaceSelection,
        thickness: Option<PositiveLength>,
        side: Option<ThickenSide>,
    ) -> Self {
        Self::Thicken {
            faces,
            thickness,
            side,
            result: None,
            tangent_chain: None,
        }
    }

    #[doc = "Construct a component-transfer operation from its readable operands."]
    pub fn component_transfer(
        mode: ComponentTransferMode,
        occurrence: Option<OccurrenceId>,
        component: Option<ProductDefinitionId>,
        source_owner: Option<ProductDefinitionId>,
        target_owner: Option<ProductDefinitionId>,
        source_transform: Option<Transform>,
        target_transform: Option<Transform>,
    ) -> Self {
        Self::ComponentTransfer {
            mode,
            occurrence,
            component,
            source_owner,
            target_owner,
            source_transform,
            target_transform,
        }
    }

    /// Family name of an operation whose replay produces body geometry, and `None` for
    /// definitions that do not.
    ///
    /// A feature of one of these families carries current result bodies in its
    /// [`super::FeatureEvaluation::outputs`] list or intermediate result identities in a
    /// [`super::FeatureResultTopology`]. The returned name is the stable lowercase label for the
    /// family, suitable for grouping such features in a report.
    pub fn body_output_family(&self) -> Option<&'static str> {
        match self {
            Self::BaseFeature { .. } => Some("base feature"),
            Self::Unresolved { family } => match family {
                UnresolvedFamily::Brep => Some("brep"),
                UnresolvedFamily::ThroughCurveMesh => Some("through curve mesh"),
                UnresolvedFamily::ExtractFace => Some("extract face"),
                UnresolvedFamily::CopyFace => Some("copy face"),
                UnresolvedFamily::LinkedFace => Some("linked face"),
                UnresolvedFamily::FillHole => Some("fill hole"),
                UnresolvedFamily::MoveFace => Some("move face"),
                UnresolvedFamily::MirrorFace => Some("mirror face"),
                UnresolvedFamily::SubdivisionBody => Some("subdivision body"),
                UnresolvedFamily::TopologyOptimization => Some("topology optimization"),
                UnresolvedFamily::MoveObject => Some("move object"),
                UnresolvedFamily::Cylinder => Some("cylinder"),
                UnresolvedFamily::Cone => Some("cone"),
                UnresolvedFamily::Sphere => Some("sphere"),
                UnresolvedFamily::Thread => Some("thread"),
                UnresolvedFamily::DetailedThread => Some("detailed thread"),
                UnresolvedFamily::Extrude => Some("extrude"),
                UnresolvedFamily::Revolve => Some("revolve"),
                UnresolvedFamily::Combine => Some("combine"),
                UnresolvedFamily::Fillet => Some("fillet"),
                UnresolvedFamily::DeleteFace => Some("delete face"),
                UnresolvedFamily::DatumPlane
                | UnresolvedFamily::DatumAxis
                | UnresolvedFamily::DatumPoint
                | UnresolvedFamily::DatumCoordinateSystem
                | UnresolvedFamily::BridgeCurve
                | UnresolvedFamily::Loft
                | UnresolvedFamily::FreeformSurface
                | UnresolvedFamily::BoundarySurface
                | UnresolvedFamily::Draft => None,
            },
            Self::SheetMetalAttachedPlate { .. } => Some("sheet metal attached plate"),
            Self::SheetMetalBend { .. } => Some("sheet metal bend"),
            Self::SheetMetalUnfold { .. } => Some("sheet metal unfold"),
            Self::SheetMetalRefold { .. } => Some("sheet metal refold"),
            Self::Block { .. } => Some("block"),
            Self::Sphere { .. } => Some("sphere"),
            Self::ExtractBody { .. } => Some("extract body"),
            Self::Loft { .. } => Some("loft"),
            Self::TrimSurface { .. } => Some("trim surface"),
            Self::ExtendSurface { .. } => Some("extend surface"),
            Self::RuledSurface { .. } => Some("ruled surface"),
            Self::Hole { .. } => Some("hole"),
            Self::Rib { .. } => Some("rib"),
            Self::Chamfer { .. } => Some("chamfer"),
            Self::Fillet { .. } => Some("fillet"),
            Self::FullRoundFillet { .. } => Some("fillet"),
            Self::FaceBlend { .. } => Some("face blend"),
            Self::Shell { .. } => Some("shell"),
            Self::SewBodies { .. } => Some("sew bodies"),
            Self::TrimBodies { .. } | Self::TrimBodiesAtPlane { .. } => Some("trim bodies"),
            Self::Extrude { .. } => Some("extrude"),
            Self::Revolve { .. } => Some("revolve"),
            Self::Sweep { .. } => Some("sweep"),
            Self::OffsetSurface { .. } => Some("offset surface"),
            Self::Thicken { .. } => Some("thicken"),
            Self::Draft { .. } => Some("draft"),
            Self::Pattern { .. } => Some("pattern"),
            Self::Combine { .. } => Some("body combine"),
            Self::ReplaceFace { .. } => Some("replace face"),
            Self::DeleteFace { .. } => Some("delete face"),
            _ => None,
        }
    }
}
