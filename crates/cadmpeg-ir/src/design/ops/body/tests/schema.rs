// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every pattern, template, body
//! operation, and shared type of §6.5.

use schemars::{JsonSchema, SchemaGenerator};

use crate::design::dimension::Dimension;
use crate::design::ops::body::{
    AdjustAgainst, BodyMate, Combine, DeleteBodies, DeleteFaces, ExternalDocument, Extract, Flex,
    FlexKind, Graft, Heal, InstanceChange, InstanceOverride, Kept, MoveFaces, NewComponent,
    Pattern, PatternInstances, PatternMember, PatternMethod, PatternSeeds, PieceForm, ReplaceFaces,
    Retention, ScaleFactors, Split, SplitMethod, SplitMode, SplitTool, Template, TemplateInput,
    TemplateInstance, TemplateParameter, TransformBodies, TransformMode, TransformStep,
    Transformation,
};
use crate::design::ops::shared::{
    Alignment, AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, CircularLayout,
    CompositeStage, Coupling, DimensionRun, DimensionTable, DriveAxis, DriveMode, Increment,
    InstanceOrientation, JointConnector, JointDrive, JointKind, JointMotion, JointOffset,
    LinearLayout, LinearRun, Locus, MirrorLayout, PairElement, PathAxisControl, PathPosition,
    PathReference, PathRollControl, PatternLayout, PatternRun, RunSides, ScaleLayout, SlotPosition,
    Spacing, TableRow, TangentContact, ToolDisposition, UpToFill, WidthMode,
};
use crate::scalar::{Angle, Length};

fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
    let schema = T::json_schema(generator);
    assert!(schema.as_object().is_some(), "{}", T::schema_name());
}

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
