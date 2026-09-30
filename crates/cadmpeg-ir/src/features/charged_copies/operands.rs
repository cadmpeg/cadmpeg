// SPDX-License-Identifier: Apache-2.0
//! Caller-admitted copies of feature operands.

use super::super::{
    AngularTermination, AxisAngle, BinderConstruction, BinderCopyOnChange, BinderLifecycle,
    BinderOffset, BinderPlacement, BinderSource, BinderTarget, BodyRetentionMode, BodySelection,
    BodyTrimSide, BooleanKind, BooleanOp, CoilConstruction, CoilExtent, CoilPlacement, CoilResult,
    CoilSection, CoilSectionPlacement, CombineOperands, CosmeticThreadExtent,
    CurveProjectionDirection, DatumPlaneReference, DatumPointConstruction, DecalMapping,
    DraftAnchor, DraftPull, EdgeSelection, ExtrudeDirection, ExtrudeExtent, ExtrudeSide,
    ExtrudeStart, ExtrusionDirectionSource, FaceBlendOperands, FaceMaker, FaceMotion,
    FaceSelection, FeatureCircularArc, FeatureContent, FeatureCoordinateFrame, FeatureEllipticArc,
    FeatureEquationCurve, FeatureImageBounds, FeatureLineSegment, FeaturePolyline,
    FeatureSourceContent, FeatureTreeNodeRole, FeatureUnitPlaneFrame, FilledSurfaceContinuity,
    FilledSurfaceContinuityState, FlexMode, FuzzyTolerance, GeneratedBodyRef, GeneratedEdgeRef,
    GeneratedSweepSection, GeometryImportFormat, GeometryImportPath, HelicalSweepConstruction,
    HelicalSweepLaw, HelicalSweepTravel, HelixConstructionStyle, HelixShape, InnerWireTaper,
    InsertedBodies, LinearTermination, LoftGuidance, LoftPointSection, LoftSection,
    NativeFeatureKind, NativeSelections, NoGeneratedSection, PartialRevolveConstruction, PathRef,
    PlanarProfileRef, PolygonSideCount, PrimitiveSolid, PrimitiveSolidKind, PrincipalPlane,
    ProfileRef, ReplaceFaceOperands, RevolutionAxis, RevolutionFuseOrder, RevolveConstruction,
    RevolveExtent, RibConstruction, RibDraft, RibSide, RuledCurveOrientation, RuledSurfaceCorner,
    RuledSurfaceMode, ScaleCenter, ScaleFactors, SectionOperands, SelectionReference,
    SewBodySelection, SheetMetalBendPosition, SheetMetalFlangeEdgeWidths, SheetMetalFlangeHeight,
    SheetMetalFlangeHeightTarget, SheetMetalFlangeTwoSidedWidth, SheetMetalFlangeWidth,
    SheetMetalHeightDatum, SheetMetalHemDirection, SheetMetalHemForm, SheetMetalThicknessSide,
    ShellJoin, ShellMode, SketchFeatureBinding, SketchPointSelection, SolidSweepOperation,
    SplitFacePlanes, SplitFaceTool, SurfaceBoundary, SurfaceContinuity, SurfaceExtension,
    SurfaceProjectionMode, SweepCircularRegion, SweepGuideRail, SweepOrientation, SweepPathExtent,
    SweepSection, SweepShape, SweepTransformation, SweepTransition, ThickenSide,
    ThreePointSelection, TreeChildren, TrimBodyOperands, TrimCellSelection, TrimRegion,
    UnresolvedFamily, VertexSelection, WrapMode,
};
use super::FeatureCopy;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

impl FeatureCopy for AngularTermination {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved {} => Ok(Self::Unresolved {}),
            Self::ThroughAll {} => Ok(Self::ThroughAll {}),
            Self::ThroughNext {} => Ok(Self::ThroughNext {}),
            Self::ToFirst {} => Ok(Self::ToFirst {}),
            Self::ToLast {} => Ok(Self::ToLast {}),
            Self::ToFace { face, offset } => Ok(Self::ToFace {
                face: face.copy_feature(ctx, purpose)?,
                offset: offset.copy_feature(ctx, purpose)?,
            }),
            Self::ToVertex { vertex } => Ok(Self::ToVertex {
                vertex: vertex.copy_feature(ctx, purpose)?,
            }),
            Self::OffsetFromFace { face, offset } => Ok(Self::OffsetFromFace {
                face: face.copy_feature(ctx, purpose)?,
                offset: offset.copy_feature(ctx, purpose)?,
            }),
            Self::ToShape { target } => Ok(Self::ToShape {
                target: target.copy_feature(ctx, purpose)?,
            }),
            Self::Angle { angle } => Ok(Self::Angle {
                angle: angle.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for AxisAngle {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            origin: self.origin.copy_feature(ctx, purpose)?,
            direction: self.direction.copy_feature(ctx, purpose)?,
            angle: self.angle.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for BinderConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Shape { trace_support } => Ok(Self::Shape {
                trace_support: trace_support.copy_feature(ctx, purpose)?,
            }),
            Self::SubShape {
                lifecycle,
                placement,
                copy_on_change,
                claim_children,
                fuse,
                make_face,
                partial_load,
                refine,
                offset,
                context,
            } => Ok(Self::SubShape {
                lifecycle: lifecycle.copy_feature(ctx, purpose)?,
                placement: placement.copy_feature(ctx, purpose)?,
                copy_on_change: copy_on_change.copy_feature(ctx, purpose)?,
                claim_children: claim_children.copy_feature(ctx, purpose)?,
                fuse: fuse.copy_feature(ctx, purpose)?,
                make_face: make_face.copy_feature(ctx, purpose)?,
                partial_load: partial_load.copy_feature(ctx, purpose)?,
                refine: refine.copy_feature(ctx, purpose)?,
                offset: offset.copy_feature(ctx, purpose)?,
                context: context.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for BinderCopyOnChange {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BinderLifecycle {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BinderOffset {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BinderPlacement {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BinderSource {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            target: self.target.copy_feature(ctx, purpose)?,
            subelements: self.subelements.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for BinderTarget {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Feature { feature } => Ok(Self::Feature {
                feature: feature.copy_feature(ctx, purpose)?,
            }),
            Self::External { document, object } => Ok(Self::External {
                document: document.copy_feature(ctx, purpose)?,
                object: object.copy_feature(ctx, purpose)?,
            }),
            Self::Native { reference } => Ok(Self::Native {
                reference: reference.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for BodyRetentionMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BodySelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::Bodies(value_0) => Ok(Self::Bodies(value_0.copy_feature(ctx, purpose)?)),
            Self::Resolved { bodies, native } => Ok(Self::Resolved {
                bodies: bodies.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::ResolvedSet { members } => Ok(Self::ResolvedSet {
                members: members.copy_feature(ctx, purpose)?,
            }),
            Self::Historical {
                state,
                bodies,
                native,
            } => Ok(Self::Historical {
                state: state.copy_feature(ctx, purpose)?,
                bodies: bodies.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::HistoricalSet { state, members } => Ok(Self::HistoricalSet {
                state: state.copy_feature(ctx, purpose)?,
                members: members.copy_feature(ctx, purpose)?,
            }),
            Self::Generated { bodies, native } => Ok(Self::Generated {
                bodies: bodies.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Local { bodies, native } => Ok(Self::Local {
                bodies: bodies.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
            Self::NativeSet(value_0) => Ok(Self::NativeSet(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for BodyTrimSide {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BooleanKind {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for BooleanOp {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for CoilConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            placement: self.placement.copy_feature(ctx, purpose)?,
            diameter: self.diameter.copy_feature(ctx, purpose)?,
            extent: self.extent.copy_feature(ctx, purpose)?,
            section: self.section.copy_feature(ctx, purpose)?,
            section_placement: self.section_placement.copy_feature(ctx, purpose)?,
            clockwise: self.clockwise.copy_feature(ctx, purpose)?,
            taper: self.taper.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for CoilExtent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for CoilPlacement {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Explicit { frame } => Ok(Self::Explicit {
                frame: frame.copy_feature(ctx, purpose)?,
            }),
            Self::Native { native_ref } => Ok(Self::Native {
                native_ref: native_ref.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for CoilResult {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::NewBody {} => Ok(Self::NewBody {}),
            Self::Boolean { operation, targets } => Ok(Self::Boolean {
                operation: operation.copy_feature(ctx, purpose)?,
                targets: targets.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for CoilSection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for CoilSectionPlacement {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for CombineOperands {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            target: self.target.copy_feature(ctx, purpose)?,
            tools: self.tools.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for CosmeticThreadExtent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for CurveProjectionDirection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for DatumPlaneReference {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Feature { feature } => Ok(Self::Feature {
                feature: feature.copy_feature(ctx, purpose)?,
            }),
            Self::Face { face } => Ok(Self::Face {
                face: face.copy_feature(ctx, purpose)?,
            }),
            Self::ResolvedPlane { frame } => Ok(Self::ResolvedPlane {
                frame: frame.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for DatumPointConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::CircleCenter { edge } => Ok(Self::CircleCenter {
                edge: edge.copy_feature(ctx, purpose)?,
            }),
            Self::TwoEdgeIntersection { edges } => Ok(Self::TwoEdgeIntersection {
                edges: edges.copy_feature(ctx, purpose)?,
            }),
            Self::ThreePlaneIntersection { planes } => Ok(Self::ThreePlaneIntersection {
                planes: planes.copy_feature(ctx, purpose)?,
            }),
            Self::Vertex { vertex } => Ok(Self::Vertex {
                vertex: vertex.copy_feature(ctx, purpose)?,
            }),
            Self::SketchPoint { point } => Ok(Self::SketchPoint {
                point: point.copy_feature(ctx, purpose)?,
            }),
            Self::EdgePlaneIntersection { edge, plane } => Ok(Self::EdgePlaneIntersection {
                edge: edge.copy_feature(ctx, purpose)?,
                plane: plane.copy_feature(ctx, purpose)?,
            }),
            Self::DistanceOnEdge { edge, fraction } => Ok(Self::DistanceOnEdge {
                edge: edge.copy_feature(ctx, purpose)?,
                fraction: fraction.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for DecalMapping {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for DraftAnchor {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::NeutralPlane { plane, pull } => Ok(Self::NeutralPlane {
                plane: plane.copy_feature(ctx, purpose)?,
                pull: pull.copy_feature(ctx, purpose)?,
            }),
            Self::PartingLine { tool, pull } => Ok(Self::PartingLine {
                tool: tool.copy_feature(ctx, purpose)?,
                pull: pull.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for DraftPull {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            direction: self.direction.copy_feature(ctx, purpose)?,
            plane: self.plane.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for EdgeSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::All => Ok(Self::All),
            Self::Edges(value_0) => Ok(Self::Edges(value_0.copy_feature(ctx, purpose)?)),
            Self::Resolved { edges, native } => Ok(Self::Resolved {
                edges: edges.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Historical {
                state,
                edges,
                native,
            } => Ok(Self::Historical {
                state: state.copy_feature(ctx, purpose)?,
                edges: edges.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::HistoricalPartial {
                state,
                edges,
                unresolved,
                native,
            } => Ok(Self::HistoricalPartial {
                state: state.copy_feature(ctx, purpose)?,
                edges: edges.copy_feature(ctx, purpose)?,
                unresolved: unresolved.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Generated { edges, native } => Ok(Self::Generated {
                edges: edges.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for ExtrudeDirection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved {} => Ok(Self::Unresolved {}),
            Self::ProfileNormal {} => Ok(Self::ProfileNormal {}),
            Self::ReversedProfileNormal {} => Ok(Self::ReversedProfileNormal {}),
            Self::Explicit { vector, source } => Ok(Self::Explicit {
                vector: vector.copy_feature(ctx, purpose)?,
                source: source.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for ExtrudeExtent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::OneSided { side } => Ok(Self::OneSided {
                side: side.copy_feature(ctx, purpose)?,
            }),
            Self::TwoSided { first, second } => Ok(Self::TwoSided {
                first: first.copy_feature(ctx, purpose)?,
                second: second.copy_feature(ctx, purpose)?,
            }),
            Self::Symmetric { side } => Ok(Self::Symmetric {
                side: side.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for ExtrudeSide {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            termination: self.termination.copy_feature(ctx, purpose)?,
            draft: self.draft.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for ExtrudeStart {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved {} => Ok(Self::Unresolved {}),
            Self::ProfilePlane {} => Ok(Self::ProfilePlane {}),
            Self::OffsetProfilePlane { offset } => Ok(Self::OffsetProfilePlane {
                offset: offset.copy_feature(ctx, purpose)?,
            }),
            Self::FromFace { face, offset } => Ok(Self::FromFace {
                face: face.copy_feature(ctx, purpose)?,
                offset: offset.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for ExtrusionDirectionSource {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Custom {} => Ok(Self::Custom {}),
            Self::Edge { reference } => Ok(Self::Edge {
                reference: reference.copy_feature(ctx, purpose)?,
            }),
            Self::ProfileNormal {} => Ok(Self::ProfileNormal {}),
        }
    }
}

impl FeatureCopy for FaceBlendOperands {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            first_faces: self.first_faces.copy_feature(ctx, purpose)?,
            second_faces: self.second_faces.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for FaceMaker {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Simple => Ok(Self::Simple),
            Self::Cheese => Ok(Self::Cheese),
            Self::Extrusion => Ok(Self::Extrusion),
            Self::Bullseye => Ok(Self::Bullseye),
            Self::Unified => Ok(Self::Unified),
            Self::Other(value_0) => Ok(Self::Other(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for FaceMotion {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Offset { distance } => Ok(Self::Offset {
                distance: distance.copy_feature(ctx, purpose)?,
            }),
            Self::Translate {
                direction,
                distance,
            } => Ok(Self::Translate {
                direction: direction.copy_feature(ctx, purpose)?,
                distance: distance.copy_feature(ctx, purpose)?,
            }),
            Self::Rotate {
                axis_origin,
                axis_dir,
                angle,
            } => Ok(Self::Rotate {
                axis_origin: axis_origin.copy_feature(ctx, purpose)?,
                axis_dir: axis_dir.copy_feature(ctx, purpose)?,
                angle: angle.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for FaceSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.try_clone_charged(ctx, purpose)
    }
}

impl FeatureCopy for FeatureCircularArc {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeatureContent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for FeatureCoordinateFrame {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeatureEllipticArc {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeatureEquationCurve {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            parameter: self.parameter.copy_feature(ctx, purpose)?,
            x_expression: self.x_expression.copy_feature(ctx, purpose)?,
            y_expression: self.y_expression.copy_feature(ctx, purpose)?,
            z_expression: self.z_expression.copy_feature(ctx, purpose)?,
            domain: self.domain.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for FeatureImageBounds {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeatureLineSegment {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeaturePolyline {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            points: self.points.copy_feature(ctx, purpose)?,
            closed: self.closed.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for FeatureSourceContent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Text(value_0) => Ok(Self::Text(value_0.copy_feature(ctx, purpose)?)),
            Self::Parameter(value_0) => Ok(Self::Parameter(value_0.copy_feature(ctx, purpose)?)),
            Self::Feature(value_0) => Ok(Self::Feature(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for FeatureTreeNodeRole {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FeatureUnitPlaneFrame {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FilledSurfaceContinuity {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            conditions: self.conditions.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for FilledSurfaceContinuityState {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for FlexMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for FuzzyTolerance {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for GeneratedBodyRef {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            feature: self.feature.copy_feature(ctx, purpose)?,
            local_id: self.local_id.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for GeneratedEdgeRef {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            feature: self.feature.copy_feature(ctx, purpose)?,
            local_id: self.local_id.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for GeneratedSweepSection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::CircularRegion { region } => Ok(Self::CircularRegion {
                region: region.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for GeometryImportFormat {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for GeometryImportPath {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for HelicalSweepConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            profile: self.profile.copy_feature(ctx, purpose)?,
            axis_origin: self.axis_origin.copy_feature(ctx, purpose)?,
            axis_direction: self.axis_direction.copy_feature(ctx, purpose)?,
            law: self.law.copy_feature(ctx, purpose)?,
            pitch: self.pitch.copy_feature(ctx, purpose)?,
            travel: self.travel.copy_feature(ctx, purpose)?,
            turns: self.turns.copy_feature(ctx, purpose)?,
            cone_angle: self.cone_angle.copy_feature(ctx, purpose)?,
            left_handed: self.left_handed.copy_feature(ctx, purpose)?,
            reversed: self.reversed.copy_feature(ctx, purpose)?,
            tolerance: self.tolerance.copy_feature(ctx, purpose)?,
            allow_multi_profile_faces: self.allow_multi_profile_faces.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for HelicalSweepLaw {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HelicalSweepTravel {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HelixConstructionStyle {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for HelixShape {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for InnerWireTaper {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for InsertedBodies {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
            Self::Resolved { native } => Ok(Self::Resolved {
                native: native.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for LinearTermination {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.try_clone_charged(ctx, purpose)
    }
}

impl FeatureCopy for LoftGuidance {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Guides(value_0) => Ok(Self::Guides(value_0.copy_feature(ctx, purpose)?)),
            Self::Centerline(value_0) => Ok(Self::Centerline(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for LoftPointSection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
            Self::Point(value_0) => Ok(Self::Point(value_0.copy_feature(ctx, purpose)?)),
            Self::Vertex(value_0) => Ok(Self::Vertex(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for LoftSection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Profile(value_0) => Ok(Self::Profile(value_0.copy_feature(ctx, purpose)?)),
            Self::Point(value_0) => Ok(Self::Point(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for NativeFeatureKind {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Canvas => Ok(Self::Canvas),
            Self::Decal => Ok(Self::Decal),
            Self::Draft => Ok(Self::Draft),
            Self::Fillet => Ok(Self::Fillet),
            Self::Chamfer => Ok(Self::Chamfer),
            Self::Extrude => Ok(Self::Extrude),
            Self::DeleteFace => Ok(Self::DeleteFace),
            Self::SurfaceDeleteFace => Ok(Self::SurfaceDeleteFace),
            Self::Other(value_0) => Ok(Self::Other(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for NativeSelections {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for NoGeneratedSection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for PartialRevolveConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Profile {
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            } => Ok(Self::Profile {
                axis: axis.copy_feature(ctx, purpose)?,
                extent: extent.copy_feature(ctx, purpose)?,
                solid: solid.copy_feature(ctx, purpose)?,
                face_maker: face_maker.copy_feature(ctx, purpose)?,
                fuse_order: fuse_order.copy_feature(ctx, purpose)?,
                allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)?,
            }),
            Self::Axis {
                profile,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            } => Ok(Self::Axis {
                profile: profile.copy_feature(ctx, purpose)?,
                extent: extent.copy_feature(ctx, purpose)?,
                solid: solid.copy_feature(ctx, purpose)?,
                face_maker: face_maker.copy_feature(ctx, purpose)?,
                fuse_order: fuse_order.copy_feature(ctx, purpose)?,
                allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)?,
            }),
            Self::Extent {
                profile,
                axis,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            } => Ok(Self::Extent {
                profile: profile.copy_feature(ctx, purpose)?,
                axis: axis.copy_feature(ctx, purpose)?,
                solid: solid.copy_feature(ctx, purpose)?,
                face_maker: face_maker.copy_feature(ctx, purpose)?,
                fuse_order: fuse_order.copy_feature(ctx, purpose)?,
                allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for PathRef {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved(value_0) => Ok(Self::Unresolved(value_0.copy_feature(ctx, purpose)?)),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
            Self::Sketch(value_0) => Ok(Self::Sketch(value_0.copy_feature(ctx, purpose)?)),
            Self::SketchCurves { sketch, curves } => Ok(Self::SketchCurves {
                sketch: sketch.copy_feature(ctx, purpose)?,
                curves: curves.copy_feature(ctx, purpose)?,
            }),
            Self::SpatialSketchSelection { sketch, selections } => {
                Ok(Self::SpatialSketchSelection {
                    sketch: sketch.copy_feature(ctx, purpose)?,
                    selections: selections.copy_feature(ctx, purpose)?,
                })
            }
            Self::SpatialSketchCurves { sketch, curves } => Ok(Self::SpatialSketchCurves {
                sketch: sketch.copy_feature(ctx, purpose)?,
                curves: curves.copy_feature(ctx, purpose)?,
            }),
            Self::Edges(value_0) => Ok(Self::Edges(value_0.copy_feature(ctx, purpose)?)),
            Self::Curves(value_0) => Ok(Self::Curves(value_0.copy_feature(ctx, purpose)?)),
            Self::HistoricalEdges {
                state,
                edges,
                native,
            } => Ok(Self::HistoricalEdges {
                state: state.copy_feature(ctx, purpose)?,
                edges: edges.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for PlanarProfileRef {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.try_clone_charged(ctx, purpose)
    }
}

impl FeatureCopy for PolygonSideCount {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for PrimitiveSolid {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for PrimitiveSolidKind {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Box {
                length,
                width,
                height,
            } => Ok(Self::Box {
                length: length.copy_feature(ctx, purpose)?,
                width: width.copy_feature(ctx, purpose)?,
                height: height.copy_feature(ctx, purpose)?,
            }),
            Self::Cylinder {
                radius,
                height,
                angle,
            } => Ok(Self::Cylinder {
                radius: radius.copy_feature(ctx, purpose)?,
                height: height.copy_feature(ctx, purpose)?,
                angle: angle.copy_feature(ctx, purpose)?,
            }),
            Self::Cone {
                radius1,
                radius2,
                height,
                angle,
            } => Ok(Self::Cone {
                radius1: radius1.copy_feature(ctx, purpose)?,
                radius2: radius2.copy_feature(ctx, purpose)?,
                height: height.copy_feature(ctx, purpose)?,
                angle: angle.copy_feature(ctx, purpose)?,
            }),
            Self::Sphere {
                radius,
                latitude1,
                latitude2,
                longitude,
            } => Ok(Self::Sphere {
                radius: radius.copy_feature(ctx, purpose)?,
                latitude1: latitude1.copy_feature(ctx, purpose)?,
                latitude2: latitude2.copy_feature(ctx, purpose)?,
                longitude: longitude.copy_feature(ctx, purpose)?,
            }),
            Self::Ellipsoid {
                x_radius,
                y_radius,
                z_radius,
                latitude1,
                latitude2,
                longitude,
            } => Ok(Self::Ellipsoid {
                x_radius: x_radius.copy_feature(ctx, purpose)?,
                y_radius: y_radius.copy_feature(ctx, purpose)?,
                z_radius: z_radius.copy_feature(ctx, purpose)?,
                latitude1: latitude1.copy_feature(ctx, purpose)?,
                latitude2: latitude2.copy_feature(ctx, purpose)?,
                longitude: longitude.copy_feature(ctx, purpose)?,
            }),
            Self::Torus {
                major_radius,
                minor_radius,
                latitude1,
                latitude2,
                longitude,
            } => Ok(Self::Torus {
                major_radius: major_radius.copy_feature(ctx, purpose)?,
                minor_radius: minor_radius.copy_feature(ctx, purpose)?,
                latitude1: latitude1.copy_feature(ctx, purpose)?,
                latitude2: latitude2.copy_feature(ctx, purpose)?,
                longitude: longitude.copy_feature(ctx, purpose)?,
            }),
            Self::Prism {
                sides,
                circumradius,
                height,
            } => Ok(Self::Prism {
                sides: sides.copy_feature(ctx, purpose)?,
                circumradius: circumradius.copy_feature(ctx, purpose)?,
                height: height.copy_feature(ctx, purpose)?,
            }),
            Self::Wedge {
                xmin,
                ymin,
                zmin,
                x2min,
                z2min,
                xmax,
                ymax,
                zmax,
                x2max,
                z2max,
            } => Ok(Self::Wedge {
                xmin: xmin.copy_feature(ctx, purpose)?,
                ymin: ymin.copy_feature(ctx, purpose)?,
                zmin: zmin.copy_feature(ctx, purpose)?,
                x2min: x2min.copy_feature(ctx, purpose)?,
                z2min: z2min.copy_feature(ctx, purpose)?,
                xmax: xmax.copy_feature(ctx, purpose)?,
                ymax: ymax.copy_feature(ctx, purpose)?,
                zmax: zmax.copy_feature(ctx, purpose)?,
                x2max: x2max.copy_feature(ctx, purpose)?,
                z2max: z2max.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for PrincipalPlane {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for ProfileRef {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::SpatialSketchProfiles { sketch, profiles } => Ok(Self::SpatialSketchProfiles {
                sketch: sketch.copy_feature(ctx, purpose)?,
                profiles: profiles.copy_feature(ctx, purpose)?,
            }),
            Self::SpatialSketchSelection { sketch, selections } => {
                Ok(Self::SpatialSketchSelection {
                    sketch: sketch.copy_feature(ctx, purpose)?,
                    selections: selections.copy_feature(ctx, purpose)?,
                })
            }
            Self::Planar(value_0) => Ok(Self::Planar(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for ReplaceFaceOperands {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            targets: self.targets.copy_feature(ctx, purpose)?,
            replacements: self.replacements.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for RevolutionAxis {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            origin: self.origin.copy_feature(ctx, purpose)?,
            direction: self.direction.copy_feature(ctx, purpose)?,
            reference: self.reference.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for RevolutionFuseOrder {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RevolveConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved(value_0) => Ok(Self::Unresolved(value_0.copy_feature(ctx, purpose)?)),
            Self::Resolved {
                profile,
                axis,
                extent,
                solid,
                face_maker,
                fuse_order,
                allow_multi_profile_faces,
            } => Ok(Self::Resolved {
                profile: profile.copy_feature(ctx, purpose)?,
                axis: axis.copy_feature(ctx, purpose)?,
                extent: extent.copy_feature(ctx, purpose)?,
                solid: solid.copy_feature(ctx, purpose)?,
                face_maker: face_maker.copy_feature(ctx, purpose)?,
                fuse_order: fuse_order.copy_feature(ctx, purpose)?,
                allow_multi_profile_faces: allow_multi_profile_faces.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for RevolveExtent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::OneSided { termination } => Ok(Self::OneSided {
                termination: termination.copy_feature(ctx, purpose)?,
            }),
            Self::TwoSided { first, second } => Ok(Self::TwoSided {
                first: first.copy_feature(ctx, purpose)?,
                second: second.copy_feature(ctx, purpose)?,
            }),
            Self::Symmetric { termination } => Ok(Self::Symmetric {
                termination: termination.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for RibConstruction {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            profile: self.profile.copy_feature(ctx, purpose)?,
            direction: self.direction.copy_feature(ctx, purpose)?,
            thickness: self.thickness.copy_feature(ctx, purpose)?,
            side: self.side.copy_feature(ctx, purpose)?,
            draft: self.draft.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for RibDraft {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RibSide {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RuledCurveOrientation {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RuledSurfaceCorner {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for RuledSurfaceMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Normal { distance } => Ok(Self::Normal {
                distance: distance.copy_feature(ctx, purpose)?,
            }),
            Self::Tangent { distance } => Ok(Self::Tangent {
                distance: distance.copy_feature(ctx, purpose)?,
            }),
            Self::Direction {
                direction,
                distance,
            } => Ok(Self::Direction {
                direction: direction.copy_feature(ctx, purpose)?,
                distance: distance.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for ScaleCenter {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Centroid => Ok(Self::Centroid),
            Self::ModelOrigin => Ok(Self::ModelOrigin),
            Self::Point(value_0) => Ok(Self::Point(value_0.copy_feature(ctx, purpose)?)),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for ScaleFactors {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SectionOperands {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            first: self.first.copy_feature(ctx, purpose)?,
            second: self.second.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for SelectionReference {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for SewBodySelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for SheetMetalBendPosition {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SheetMetalFlangeEdgeWidths {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for SheetMetalFlangeHeight {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Distance(value_0) => Ok(Self::Distance(value_0.copy_feature(ctx, purpose)?)),
            Self::ToObject { target, offset } => Ok(Self::ToObject {
                target: target.copy_feature(ctx, purpose)?,
                offset: offset.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SheetMetalFlangeHeightTarget {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Feature(value_0) => Ok(Self::Feature(value_0.copy_feature(ctx, purpose)?)),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for SheetMetalFlangeTwoSidedWidth {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            first: self.first.copy_feature(ctx, purpose)?,
            second: self.second.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for SheetMetalFlangeWidth {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::FullEdge => Ok(Self::FullEdge),
            Self::Symmetric { width } => Ok(Self::Symmetric {
                width: width.copy_feature(ctx, purpose)?,
            }),
            Self::TwoSides { first, second } => Ok(Self::TwoSides {
                first: first.copy_feature(ctx, purpose)?,
                second: second.copy_feature(ctx, purpose)?,
            }),
            Self::TwoSidesPerEdge { widths } => Ok(Self::TwoSidesPerEdge {
                widths: widths.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SheetMetalHeightDatum {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SheetMetalHemDirection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SheetMetalHemForm {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Flat { length } => Ok(Self::Flat {
                length: length.copy_feature(ctx, purpose)?,
            }),
            Self::Open { gap, length } => Ok(Self::Open {
                gap: gap.copy_feature(ctx, purpose)?,
                length: length.copy_feature(ctx, purpose)?,
            }),
            Self::GapLength { gap, length } => Ok(Self::GapLength {
                gap: gap.copy_feature(ctx, purpose)?,
                length: length.copy_feature(ctx, purpose)?,
            }),
            Self::Rolled { radius, angle } => Ok(Self::Rolled {
                radius: radius.copy_feature(ctx, purpose)?,
                angle: angle.copy_feature(ctx, purpose)?,
            }),
            Self::Teardrop {
                gap,
                length,
                radius,
            } => Ok(Self::Teardrop {
                gap: gap.copy_feature(ctx, purpose)?,
                length: length.copy_feature(ctx, purpose)?,
                radius: radius.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SheetMetalThicknessSide {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for ShellJoin {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for ShellMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SketchFeatureBinding {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::Planar(value_0) => Ok(Self::Planar(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for SketchPointSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::Planar {
                sketch,
                point,
                native,
            } => Ok(Self::Planar {
                sketch: sketch.copy_feature(ctx, purpose)?,
                point: point.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Spatial {
                sketch,
                point,
                native,
            } => Ok(Self::Spatial {
                sketch: sketch.copy_feature(ctx, purpose)?,
                point: point.copy_feature(ctx, purpose)?,
                native: native.copy_feature(ctx, purpose)?,
            }),
            Self::Native(value_0) => Ok(Self::Native(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for SolidSweepOperation {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SplitFacePlanes {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for SplitFaceTool {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Path(value_0) => Ok(Self::Path(value_0.copy_feature(ctx, purpose)?)),
            Self::Plane { plane } => Ok(Self::Plane {
                plane: plane.copy_feature(ctx, purpose)?,
            }),
            Self::Planes { planes } => Ok(Self::Planes {
                planes: planes.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SurfaceBoundary {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Edges(value_0) => Ok(Self::Edges(value_0.copy_feature(ctx, purpose)?)),
            Self::Path(value_0) => Ok(Self::Path(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for SurfaceContinuity {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SurfaceExtension {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SurfaceProjectionMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SweepCircularRegion {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SweepGuideRail {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            path: self.path.copy_feature(ctx, purpose)?,
            extent: self.extent.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for SweepOrientation {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::CorrectedFrenet {} => Ok(Self::CorrectedFrenet {}),
            Self::Fixed {} => Ok(Self::Fixed {}),
            Self::Frenet {} => Ok(Self::Frenet {}),
            Self::Auxiliary {
                path,
                tangent,
                curvilinear,
            } => Ok(Self::Auxiliary {
                path: path.copy_feature(ctx, purpose)?,
                tangent: tangent.copy_feature(ctx, purpose)?,
                curvilinear: curvilinear.copy_feature(ctx, purpose)?,
            }),
            Self::GuideSurface { faces } => Ok(Self::GuideSurface {
                faces: faces.copy_feature(ctx, purpose)?,
            }),
            Self::Binormal { direction } => Ok(Self::Binormal {
                direction: direction.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SweepPathExtent {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl<G: FeatureCopy> FeatureCopy for SweepSection<G> {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved(value_0) => Ok(Self::Unresolved(value_0.copy_feature(ctx, purpose)?)),
            Self::Profile(value_0) => Ok(Self::Profile(value_0.copy_feature(ctx, purpose)?)),
            Self::Generated(value_0) => Ok(Self::Generated(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for SweepShape {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved { section, sections } => Ok(Self::Unresolved {
                section: section.copy_feature(ctx, purpose)?,
                sections: sections.copy_feature(ctx, purpose)?,
            }),
            Self::Solid {
                op,
                section,
                sections,
            } => Ok(Self::Solid {
                op: op.copy_feature(ctx, purpose)?,
                section: section.copy_feature(ctx, purpose)?,
                sections: sections.copy_feature(ctx, purpose)?,
            }),
            Self::Surface { section, sections } => Ok(Self::Surface {
                section: section.copy_feature(ctx, purpose)?,
                sections: sections.copy_feature(ctx, purpose)?,
            }),
        }
    }
}

impl FeatureCopy for SweepTransformation {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for SweepTransition {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for ThickenSide {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for ThreePointSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self(self.0.copy_feature(ctx, purpose)?))
    }
}

impl FeatureCopy for TreeChildren {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            children: self.children.copy_feature(ctx, purpose)?,
            active_child: self.active_child.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for TrimBodyOperands {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            targets: self.targets.copy_feature(ctx, purpose)?,
            tools: self.tools.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for TrimCellSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(Self {
            removed: self.removed.copy_feature(ctx, purpose)?,
            total: self.total.copy_feature(ctx, purpose)?,
        })
    }
}

impl FeatureCopy for TrimRegion {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        match self {
            Self::Unresolved => Ok(Self::Unresolved),
            Self::Inside => Ok(Self::Inside),
            Self::Outside => Ok(Self::Outside),
            Self::Cells(value_0) => Ok(Self::Cells(value_0.copy_feature(ctx, purpose)?)),
        }
    }
}

impl FeatureCopy for UnresolvedFamily {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}

impl FeatureCopy for VertexSelection {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        self.try_clone_charged(ctx, purpose)
    }
}

impl FeatureCopy for WrapMode {
    fn copy_feature(
        &self,
        ctx: &DecodeContext<'_>,
        purpose: &'static str,
    ) -> Result<Self, CodecError> {
        ctx.charge_work(u64_from_index(std::mem::size_of::<Self>()), purpose)?;
        Ok(*self)
    }
}
