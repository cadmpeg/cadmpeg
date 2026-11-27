// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every design operation type.

mod assembly {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::assembly::*;
    use cadmpeg_ir::design::ops::shared::{
        Alignment, AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, Coupling, DimensionRun,
        DimensionTable, DriveAxis, DriveMode, Increment, InstanceOrientation, Joint,
        JointConnector, JointDrive, JointKind, JointMotion, JointOffset, LinearRun, Locus,
        PairElement, PathAxisControl, PathPosition, PathReference, PathRollControl, PatternLayout,
        PatternRun, RunSides, SlotPosition, Spacing, TableRow, TangentContact, UpToFill, WidthMode,
    };
    use cadmpeg_ir::scalar::{Angle, Length};

    use crate::generates;

    #[test]
    fn every_assembly_and_shared_layout_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Place>(&mut generator);
        generates::<PlacementBase>(&mut generator);
        generates::<PlaceSet>(&mut generator);
        generates::<Joint>(&mut generator);
        generates::<JointConnector>(&mut generator);
        generates::<Alignment>(&mut generator);
        generates::<JointKind>(&mut generator);
        generates::<JointMotion>(&mut generator);
        generates::<JointOffset>(&mut generator);
        generates::<AxisLimits>(&mut generator);
        generates::<Bounds<Length>>(&mut generator);
        generates::<Bounds<Angle>>(&mut generator);
        generates::<Coupling>(&mut generator);
        generates::<JointDrive>(&mut generator);
        generates::<DriveAxis>(&mut generator);
        generates::<DriveMode>(&mut generator);
        generates::<TangentContact>(&mut generator);
        generates::<WidthMode>(&mut generator);
        generates::<SlotPosition>(&mut generator);
        generates::<PathPosition>(&mut generator);
        generates::<PathAxisControl>(&mut generator);
        generates::<PathRollControl>(&mut generator);
        generates::<RigidGroup>(&mut generator);
        generates::<ComponentPattern>(&mut generator);
        generates::<ComponentInstance>(&mut generator);
        generates::<MirrorComponents>(&mut generator);
        generates::<MirrorResult>(&mut generator);
        generates::<MirrorOrientation>(&mut generator);
        generates::<MirrorReversal>(&mut generator);
        generates::<MirrorPivot>(&mut generator);
        generates::<AssemblyContext>(&mut generator);
        generates::<ContextEffect>(&mut generator);
        generates::<PairElement>(&mut generator);
        generates::<PatternLayout>(&mut generator);
        generates::<ChainLayout>(&mut generator);
        generates::<ChainLinks>(&mut generator);
        generates::<ChainLink>(&mut generator);
        generates::<LinearRun>(&mut generator);
        generates::<PatternRun<Length>>(&mut generator);
        generates::<PatternRun<Angle>>(&mut generator);
        generates::<RunSides<Length>>(&mut generator);
        generates::<Spacing<Angle>>(&mut generator);
        generates::<UpToFill<Length>>(&mut generator);
        generates::<Locus>(&mut generator);
        generates::<InstanceOrientation>(&mut generator);
        generates::<PathReference>(&mut generator);
        generates::<Increment>(&mut generator);
        generates::<DimensionRun>(&mut generator);
        generates::<DimensionTable>(&mut generator);
        generates::<TableRow>(&mut generator);
    }
}

mod body {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::dimension::Dimension;
    use cadmpeg_ir::design::ops::body::{
        AdjustAgainst, BodyMate, Combine, DeleteBodies, DeleteFaces, Extract, Flex, FlexKind,
        Graft, Heal, InstanceChange, InstanceOverride, Kept, MoveFaces, NewComponent, Pattern,
        PatternInstances, PatternMember, PatternMethod, PatternSeeds, PieceForm, ReplaceFaces,
        Retention, ScaleFactors, Split, SplitMethod, SplitMode, SplitTool, Template, TemplateInput,
        TemplateInstance, TemplateParameter, ToolExtension, TransformBodies, TransformMode,
        TransformStep, Transformation,
    };
    use cadmpeg_ir::design::ops::shared::{
        Alignment, AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, CircularLayout,
        CompositeStage, Coupling, DimensionRun, DimensionTable, DriveAxis, DriveMode, Increment,
        InstanceOrientation, JointConnector, JointDrive, JointKind, JointMotion, JointOffset,
        LinearLayout, LinearRun, Locus, MirrorLayout, PairElement, PathAxisControl, PathPosition,
        PathReference, PathRollControl, PatternLayout, PatternRun, RunSides, ScaleLayout,
        SlotPosition, Spacing, TableRow, TangentContact, ToolDisposition, UpToFill, WidthMode,
    };
    use cadmpeg_ir::products::ExternalDocument;
    use cadmpeg_ir::scalar::{Angle, Length};

    use crate::generates;

    #[test]
    fn every_pattern_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Pattern>(&mut generator);
        generates::<PatternSeeds>(&mut generator);
        generates::<PatternMethod>(&mut generator);
        generates::<AdjustAgainst>(&mut generator);
        generates::<PatternLayout>(&mut generator);
        generates::<CompositeStage>(&mut generator);
        generates::<LinearLayout>(&mut generator);
        generates::<CircularLayout>(&mut generator);
        generates::<MirrorLayout>(&mut generator);
        generates::<ScaleLayout>(&mut generator);
        generates::<ChainLayout>(&mut generator);
        generates::<ChainLinks>(&mut generator);
        generates::<ChainLink>(&mut generator);
        generates::<LinearRun>(&mut generator);
        generates::<PatternRun<Length>>(&mut generator);
        generates::<PatternRun<Angle>>(&mut generator);
        generates::<RunSides<Length>>(&mut generator);
        generates::<RunSides<Angle>>(&mut generator);
        generates::<Spacing<Length>>(&mut generator);
        generates::<Spacing<Angle>>(&mut generator);
        generates::<UpToFill<Length>>(&mut generator);
        generates::<UpToFill<Angle>>(&mut generator);
        generates::<Locus>(&mut generator);
        generates::<InstanceOrientation>(&mut generator);
        generates::<PathReference>(&mut generator);
        generates::<Increment>(&mut generator);
        generates::<DimensionRun>(&mut generator);
        generates::<DimensionTable>(&mut generator);
        generates::<TableRow>(&mut generator);
        generates::<PatternInstances>(&mut generator);
        generates::<InstanceOverride>(&mut generator);
        generates::<InstanceChange>(&mut generator);
        generates::<PatternMember>(&mut generator);
    }

    #[test]
    fn every_template_and_body_operation_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<TemplateInstance>(&mut generator);
        generates::<Template>(&mut generator);
        generates::<TemplateInput>(&mut generator);
        generates::<TemplateParameter>(&mut generator);
        generates::<ExternalDocument>(&mut generator);
        generates::<Combine>(&mut generator);
        generates::<ToolDisposition>(&mut generator);
        generates::<NewComponent>(&mut generator);
        generates::<Split>(&mut generator);
        generates::<SplitMode>(&mut generator);
        generates::<SplitTool>(&mut generator);
        generates::<ToolExtension>(&mut generator);
        generates::<SplitMethod>(&mut generator);
        generates::<Kept<Dimension<bool>>>(&mut generator);
        generates::<Kept<Vec<Option<Dimension<bool>>>>>(&mut generator);
        generates::<PieceForm>(&mut generator);
        generates::<DeleteBodies>(&mut generator);
        generates::<Retention>(&mut generator);
        generates::<Extract>(&mut generator);
        generates::<TransformBodies>(&mut generator);
        generates::<TransformMode>(&mut generator);
        generates::<Transformation>(&mut generator);
        generates::<TransformStep>(&mut generator);
        generates::<BodyMate>(&mut generator);
        generates::<ScaleFactors>(&mut generator);
        generates::<MoveFaces>(&mut generator);
        generates::<DeleteFaces>(&mut generator);
        generates::<Heal>(&mut generator);
        generates::<ReplaceFaces>(&mut generator);
        generates::<Graft>(&mut generator);
        generates::<Flex>(&mut generator);
        generates::<FlexKind>(&mut generator);
    }

    #[test]
    fn every_joint_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<JointConnector>(&mut generator);
        generates::<Alignment>(&mut generator);
        generates::<JointKind>(&mut generator);
        generates::<JointMotion>(&mut generator);
        generates::<JointOffset>(&mut generator);
        generates::<AxisLimits>(&mut generator);
        generates::<Bounds<Length>>(&mut generator);
        generates::<Bounds<Angle>>(&mut generator);
        generates::<Coupling>(&mut generator);
        generates::<JointDrive>(&mut generator);
        generates::<DriveAxis>(&mut generator);
        generates::<DriveMode>(&mut generator);
        generates::<TangentContact>(&mut generator);
        generates::<WidthMode>(&mut generator);
        generates::<SlotPosition>(&mut generator);
        generates::<PathPosition>(&mut generator);
        generates::<PathAxisControl>(&mut generator);
        generates::<PathRollControl>(&mut generator);
        generates::<PairElement>(&mut generator);
    }
}

mod dressup {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::dressup::*;
    use cadmpeg_ir::design::ops::shared::{Interpolation, OffsetJoin};
    use cadmpeg_ir::scalar::{NonNegativeLength, SlopeAngle};

    use crate::generates;

    #[test]
    fn every_dressup_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Stations<NonNegativeLength>>(&mut generator);
        generates::<Stations<SlopeAngle>>(&mut generator);
        generates::<ValueStation<NonNegativeLength>>(&mut generator);
        generates::<ValueStation<SlopeAngle>>(&mut generator);
        generates::<StationAt>(&mut generator);
        generates::<Along>(&mut generator);
        generates::<Interpolation>(&mut generator);
        generates::<Fillet>(&mut generator);
        generates::<FilletSet>(&mut generator);
        generates::<FaceSide>(&mut generator);
        generates::<RoundSide>(&mut generator);
        generates::<FilletSection>(&mut generator);
        generates::<FilletSize>(&mut generator);
        generates::<FilletProfile>(&mut generator);
        generates::<ConicShape>(&mut generator);
        generates::<Chamfer>(&mut generator);
        generates::<ChamferSet>(&mut generator);
        generates::<ChamferSize>(&mut generator);
        generates::<ChamferMeasure>(&mut generator);
        generates::<BlendControls>(&mut generator);
        generates::<CornerStyle>(&mut generator);
        generates::<Corner>(&mut generator);
        generates::<Setbacks>(&mut generator);
        generates::<EdgeSetback>(&mut generator);
        generates::<BlendLimit>(&mut generator);
        generates::<LimitAt>(&mut generator);
        generates::<Transition>(&mut generator);
        generates::<TransitionKind>(&mut generator);
        generates::<Overflow>(&mut generator);
        generates::<ShellFeature>(&mut generator);
        generates::<FaceThickness>(&mut generator);
        generates::<OffsetJoin>(&mut generator);
        generates::<FaceMeeting>(&mut generator);
        generates::<Thicken>(&mut generator);
        generates::<Offset>(&mut generator);
        generates::<OffsetSet>(&mut generator);
        generates::<OffsetResult>(&mut generator);
        generates::<Draft>(&mut generator);
        generates::<DraftSet>(&mut generator);
        generates::<DraftAngle>(&mut generator);
        generates::<StepFaces>(&mut generator);
        generates::<Hole>(&mut generator);
        generates::<SeriesStage>(&mut generator);
        generates::<StageHole>(&mut generator);
        generates::<HolePositions>(&mut generator);
        generates::<CenterSource>(&mut generator);
        generates::<HoleShape>(&mut generator);
        generates::<SlotLength>(&mut generator);
        generates::<Bore>(&mut generator);
        generates::<HoleHead>(&mut generator);
        generates::<Cone>(&mut generator);
        generates::<HoleBottom>(&mut generator);
        generates::<DepthTo>(&mut generator);
        generates::<StandardSize>(&mut generator);
        generates::<Thread>(&mut generator);
        generates::<ThreadSpec>(&mut generator);
        generates::<ThreadLength>(&mut generator);
        generates::<Emboss>(&mut generator);
        generates::<EmbossEffect>(&mut generator);
        generates::<EmbossMapping>(&mut generator);
        generates::<Dome>(&mut generator);
        generates::<DomeShape>(&mut generator);
        generates::<Decal>(&mut generator);
        generates::<DecalPlacement>(&mut generator);
    }
}

mod shared {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::shared::*;
    use cadmpeg_ir::design::ops::sketch::{
        TextAlign, TextContent, TextHorizontalBox, TextVerticalBox,
    };
    use cadmpeg_ir::scalar::{Angle, Fraction, Length, PositiveLength};

    use crate::generates;

    #[test]
    fn every_shared_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<TextContent>(&mut generator);
        generates::<TextAlign>(&mut generator);
        generates::<TextHorizontalBox>(&mut generator);
        generates::<TextVerticalBox>(&mut generator);
        generates::<ToolDisposition>(&mut generator);
        generates::<PairElement>(&mut generator);
        generates::<SheetCutNormal>(&mut generator);
        generates::<Joint>(&mut generator);
        generates::<JointConnector>(&mut generator);
        generates::<Alignment>(&mut generator);
        generates::<JointKind>(&mut generator);
        generates::<JointMotion>(&mut generator);
        generates::<JointOffset>(&mut generator);
        generates::<AxisLimits>(&mut generator);
        generates::<Coupling>(&mut generator);
        generates::<JointDrive>(&mut generator);
        generates::<DriveAxis>(&mut generator);
        generates::<DriveMode>(&mut generator);
        generates::<TangentContact>(&mut generator);
        generates::<WidthMode>(&mut generator);
        generates::<SlotPosition>(&mut generator);
        generates::<PathPosition>(&mut generator);
        generates::<PathAxisControl>(&mut generator);
        generates::<PathRollControl>(&mut generator);
        generates::<RadialDimension<PositiveLength>>(&mut generator);
        generates::<Law<Length>>(&mut generator);
        generates::<Bounds<Length>>(&mut generator);
        generates::<Bounds<Angle>>(&mut generator);
        generates::<Law<Fraction>>(&mut generator);
        generates::<Law<Angle>>(&mut generator);
        generates::<Span<Length>>(&mut generator);
        generates::<Limit<Length>>(&mut generator);
    }
}

mod sheet_metal {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::sheet_metal::*;

    use crate::generates;

    #[test]
    fn every_sheet_metal_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<SheetMetal>(&mut generator);
        generates::<SheetMetalRule>(&mut generator);
        generates::<BendAllowance>(&mut generator);
        generates::<KFactorTable>(&mut generator);
        generates::<BendTable>(&mut generator);
        generates::<BendTableBlock>(&mut generator);
        generates::<BendTableAngle>(&mut generator);
        generates::<BendTableValue>(&mut generator);
        generates::<FlangeMeasure>(&mut generator);
        generates::<BendTableLookup>(&mut generator);
        generates::<AxisLookup>(&mut generator);
        generates::<OutsideGrid>(&mut generator);
        generates::<BendEquations>(&mut generator);
        generates::<BendQuantity>(&mut generator);
        generates::<AngleRange>(&mut generator);
        generates::<TableLookup>(&mut generator);
        generates::<KFactorRow>(&mut generator);
        generates::<SheetMetalKind>(&mut generator);
        generates::<AttachedRelief>(&mut generator);
        generates::<AttachedReliefForm>(&mut generator);
        generates::<TransitionForm>(&mut generator);
        generates::<ConnectBetween>(&mut generator);
        generates::<Rip>(&mut generator);
        generates::<RipPath>(&mut generator);
        generates::<SketchedRipDepth>(&mut generator);
        generates::<RipWidth>(&mut generator);
        generates::<KerfSide>(&mut generator);
        generates::<Convert>(&mut generator);
        generates::<ConvertFrom>(&mut generator);
        generates::<ConvertRip>(&mut generator);
        generates::<ConvertSide>(&mut generator);
        generates::<CornerBreak>(&mut generator);
        generates::<ConvertBend>(&mut generator);
        generates::<NormalSide>(&mut generator);
        generates::<VertexBend>(&mut generator);
        generates::<StampOutline>(&mut generator);
        generates::<StampForm>(&mut generator);
        generates::<BendSet>(&mut generator);
        generates::<BendSpec>(&mut generator);
        generates::<RadiusSide>(&mut generator);
        generates::<Relief>(&mut generator);
        generates::<ReliefShape>(&mut generator);
        generates::<Flange>(&mut generator);
        generates::<FlangeHeight>(&mut generator);
        generates::<FlangeWidth>(&mut generator);
        generates::<HeightDatum>(&mut generator);
        generates::<BendPosition>(&mut generator);
        generates::<BendExtension>(&mut generator);
        generates::<BendZone>(&mut generator);
        generates::<BendLineFace>(&mut generator);
        generates::<LineSide>(&mut generator);
        generates::<DoubleBend>(&mut generator);
        generates::<Hem>(&mut generator);
        generates::<HemSide>(&mut generator);
        generates::<HemForm>(&mut generator);
    }
}

mod sketch_datum {
    use schemars::{JsonSchema, SchemaGenerator};

    use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
    use cadmpeg_ir::design::ops::shared::{Locus, RunSides, Spacing, UpToFill};
    use cadmpeg_ir::design::ops::sketch::{
        AngleOperands, AngleSector, BlockInstance, Contact, DefiningRole, Derivation,
        DerivationMode, DistanceAxis, EntityDefinition, EntityForm, EntityRole, InterpolatedSpline,
        Operand, Planar, PlanarCurve, RadialMeasure, Sketch, SketchConstraint, SketchEntity,
        SketchOffset, SketchPattern, SketchPatternAxis, SketchPatternDirection,
        SketchPatternInstance, SketchPatternLayout, SketchPlane, SketchPolygon, SketchRelation,
        SketchSlot, SketchSolve, SketchSpace, SketchText, SlotKind, SolveState, Spatial,
        SpatialCurve, SpatialSketch, SplineHandle, TextAlign, TextContent, TextHorizontalBox,
        TextLayout, TextVerticalBox,
    };
    use cadmpeg_ir::scalar::{Angle, Length};
    use cadmpeg_ir::units::UnitVector2;

    use crate::generates;

    fn generates_in<S>(generator: &mut SchemaGenerator)
    where
        S: SketchSpace + JsonSchema,
        S::Sketch: JsonSchema,
        S::Entity: JsonSchema,
        S::Constraint: JsonSchema,
        S::Direction: JsonSchema,
        S::Curve: JsonSchema,
    {
        generates::<SketchEntity<S>>(generator);
        generates::<EntityDefinition<S>>(generator);
        generates::<EntityForm<S>>(generator);
        generates::<EntityRole<S>>(generator);
        generates::<InterpolatedSpline<S>>(generator);
        generates::<SplineHandle<S>>(generator);
        generates::<SketchText<S>>(generator);
        generates::<TextLayout<S>>(generator);
        generates::<SketchConstraint<S>>(generator);
        generates::<Operand<S>>(generator);
        generates::<SketchRelation<S>>(generator);
        generates::<Contact<S>>(generator);
        generates::<DistanceAxis<S>>(generator);
        generates::<AngleOperands<S>>(generator);
        generates::<SketchOffset<S>>(generator);
        generates::<SketchPolygon<S>>(generator);
        generates::<SketchSlot<S>>(generator);
        generates::<SketchPattern<S>>(generator);
        generates::<SketchPatternLayout<S>>(generator);
        generates::<SketchPatternAxis<S>>(generator);
        generates::<SketchPatternDirection<S>>(generator);
        generates::<SketchPatternInstance<S>>(generator);
    }

    #[test]
    fn every_sketch_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Sketch>(&mut generator);
        generates::<SketchPlane>(&mut generator);
        generates::<SpatialSketch>(&mut generator);
        generates::<SketchSolve>(&mut generator);
        generates::<SolveState>(&mut generator);
        generates::<Planar>(&mut generator);
        generates::<Spatial>(&mut generator);
        generates::<DefiningRole>(&mut generator);
        generates::<PlanarCurve>(&mut generator);
        generates::<SpatialCurve>(&mut generator);
        generates::<UnitVector2>(&mut generator);
        generates::<Derivation>(&mut generator);
        generates::<DerivationMode>(&mut generator);
        generates::<TextContent>(&mut generator);
        generates::<TextAlign>(&mut generator);
        generates::<TextHorizontalBox>(&mut generator);
        generates::<TextVerticalBox>(&mut generator);
        generates::<BlockInstance>(&mut generator);
        generates::<AngleSector>(&mut generator);
        generates::<RadialMeasure>(&mut generator);
        generates::<SlotKind>(&mut generator);
        generates_in::<Planar>(&mut generator);
        generates_in::<Spatial>(&mut generator);
    }

    #[test]
    fn every_datum_and_shared_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Datum>(&mut generator);
        generates::<DatumElement>(&mut generator);
        generates::<RunSides<Length>>(&mut generator);
        generates::<RunSides<Angle>>(&mut generator);
        generates::<Spacing<Length>>(&mut generator);
        generates::<Spacing<Angle>>(&mut generator);
        generates::<UpToFill<Length>>(&mut generator);
        generates::<UpToFill<Angle>>(&mut generator);
        generates::<Locus>(&mut generator);
    }
}

mod surface {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::surface::*;

    use crate::generates;

    #[test]
    fn every_surface_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Patch>(&mut generator);
        generates::<PatchBounds>(&mut generator);
        generates::<PatchEdge>(&mut generator);
        generates::<Knit>(&mut generator);
        generates::<Healing>(&mut generator);
        generates::<Unsew>(&mut generator);
        generates::<UnsewFaces>(&mut generator);
        generates::<Extend>(&mut generator);
        generates::<ExtendLimit>(&mut generator);
        generates::<ExtendMethod>(&mut generator);
        generates::<ImplicitSweep>(&mut generator);
        generates::<ImplicitSection>(&mut generator);
        generates::<LineMode>(&mut generator);
        generates::<FaceReference>(&mut generator);
        generates::<SurfaceRelation>(&mut generator);
        generates::<DirectionRelation>(&mut generator);
        generates::<CurveRelation>(&mut generator);
        generates::<LineLength>(&mut generator);
        generates::<CircleMode>(&mut generator);
        generates::<BoundaryFill>(&mut generator);
        generates::<RegionKind>(&mut generator);
        generates::<CurveFeature>(&mut generator);
        generates::<CurveDefinition>(&mut generator);
        generates::<EquationCoordinates>(&mut generator);
        generates::<SplineCurve>(&mut generator);
        generates::<SplinePoint>(&mut generator);
        generates::<BridgeEnd>(&mut generator);
        generates::<TextCurve>(&mut generator);
        generates::<TextPlacement>(&mut generator);
        generates::<CombineOperand>(&mut generator);
        generates::<CurveOffset>(&mut generator);
        generates::<OffsetMeasure>(&mut generator);
        generates::<Form>(&mut generator);
        generates::<MeshFeature>(&mut generator);
        generates::<MeshKind>(&mut generator);
        generates::<MeshCut>(&mut generator);
        generates::<MeshFill>(&mut generator);
        generates::<ReduceTarget>(&mut generator);
        generates::<MeshConversion>(&mut generator);
    }
}

mod swept {
    use schemars::SchemaGenerator;

    use cadmpeg_ir::design::ops::shared::{Interpolation, OffsetJoin, SheetCutNormal};
    use cadmpeg_ir::design::ops::swept::*;

    use crate::generates;

    #[test]
    fn every_swept_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<Extrude>(&mut generator);
        generates::<ExtrudeDirection>(&mut generator);
        generates::<ExtrudeOrigin>(&mut generator);
        generates::<ExtrudeDraft>(&mut generator);
        generates::<DraftNeutral>(&mut generator);
        generates::<HoleDraft>(&mut generator);
        generates::<ProfileFit>(&mut generator);
        generates::<Revolve>(&mut generator);
        generates::<Sweep>(&mut generator);
        generates::<CornerTransition>(&mut generator);
        generates::<SweepSection>(&mut generator);
        generates::<SectionTurn>(&mut generator);
        generates::<SectionShape>(&mut generator);
        generates::<Apex>(&mut generator);
        generates::<SweepControls>(&mut generator);
        generates::<SweepOrientation>(&mut generator);
        generates::<GuideFrame>(&mut generator);
        generates::<Twist>(&mut generator);
        generates::<ScalingLaw>(&mut generator);
        generates::<SweepGuide>(&mut generator);
        generates::<PathExtent>(&mut generator);
        generates::<ProfilePlacement>(&mut generator);
        generates::<ProfilePosition>(&mut generator);
        generates::<SpineLimits>(&mut generator);
        generates::<HelicalSweep>(&mut generator);
        generates::<HelicalSection>(&mut generator);
        generates::<SectionPlacement>(&mut generator);
        generates::<Helix>(&mut generator);
        generates::<HelixStart>(&mut generator);
        generates::<HelixShape>(&mut generator);
        generates::<HelixLaw>(&mut generator);
        generates::<HelixTaper>(&mut generator);
        generates::<SpiralLaw>(&mut generator);
        generates::<Rib>(&mut generator);
        generates::<RibGrowth>(&mut generator);
        generates::<RibEnd>(&mut generator);
        generates::<Primitive>(&mut generator);
        generates::<PrimitiveShape>(&mut generator);
        generates::<Loft>(&mut generator);
        generates::<Section>(&mut generator);
        generates::<SectionForm>(&mut generator);
        generates::<Seam>(&mut generator);
        generates::<PointSection>(&mut generator);
        generates::<PointCondition>(&mut generator);
        generates::<Guide>(&mut generator);
        generates::<GuideInfluence>(&mut generator);
        generates::<Spine>(&mut generator);
        generates::<AreaStation>(&mut generator);
        generates::<CenterlineAt>(&mut generator);
        generates::<Correspondence>(&mut generator);
        generates::<CouplingMode>(&mut generator);
        generates::<Connector>(&mut generator);
        generates::<LoftShape>(&mut generator);
        generates::<LoftSmoothing>(&mut generator);
        generates::<LoftStyle>(&mut generator);
        generates::<Rebuild>(&mut generator);
        generates::<Interpolation>(&mut generator);
        generates::<OffsetJoin>(&mut generator);
        generates::<SheetCutNormal>(&mut generator);
    }
}
