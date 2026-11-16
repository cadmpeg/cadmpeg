// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every pattern, template, body
//! operation, and shared type of §6.5.

use schemars::{schema_for, JsonSchema};

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
    Alignment, AxisLimits, Bounds, ChainLayout, ChainLink, ChainLinks, Coupling, DimensionRun,
    DimensionTable, DriveAxis, DriveMode, Increment, InstanceOrientation, JointConnector,
    JointDrive, JointKind, JointMotion, JointOffset, LinearRun, Locus, PairElement,
    PathAxisControl, PathPosition, PathReference, PathRollControl, PatternLayout, PatternRun,
    RunSides, SlotPosition, Spacing, TableRow, TangentContact, ToolDisposition, UpToFill,
    WidthMode,
};
use crate::scalar::{Angle, Length};

fn generates<T: JsonSchema>() {
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    assert!(schema.is_object(), "{}", T::schema_name());
}

#[test]
fn every_pattern_type_has_a_schema() {
    generates::<Pattern>();
    generates::<PatternSeeds>();
    generates::<PatternMethod>();
    generates::<AdjustAgainst>();
    generates::<PatternLayout>();
    generates::<ChainLayout>();
    generates::<ChainLinks>();
    generates::<ChainLink>();
    generates::<LinearRun>();
    generates::<PatternRun<Length>>();
    generates::<PatternRun<Angle>>();
    generates::<RunSides<Length>>();
    generates::<RunSides<Angle>>();
    generates::<Spacing<Length>>();
    generates::<Spacing<Angle>>();
    generates::<UpToFill<Length>>();
    generates::<UpToFill<Angle>>();
    generates::<Locus>();
    generates::<InstanceOrientation>();
    generates::<PathReference>();
    generates::<Increment>();
    generates::<DimensionRun>();
    generates::<DimensionTable>();
    generates::<TableRow>();
    generates::<PatternInstances>();
    generates::<InstanceOverride>();
    generates::<InstanceChange>();
    generates::<PatternMember>();
}

#[test]
fn every_template_and_body_operation_type_has_a_schema() {
    generates::<TemplateInstance>();
    generates::<Template>();
    generates::<TemplateInput>();
    generates::<TemplateParameter>();
    generates::<ExternalDocument>();
    generates::<Combine>();
    generates::<ToolDisposition>();
    generates::<NewComponent>();
    generates::<Split>();
    generates::<SplitMode>();
    generates::<SplitTool>();
    generates::<SplitMethod>();
    generates::<Kept<Dimension<bool>>>();
    generates::<Kept<Vec<Option<Dimension<bool>>>>>();
    generates::<PieceForm>();
    generates::<DeleteBodies>();
    generates::<Retention>();
    generates::<Extract>();
    generates::<TransformBodies>();
    generates::<TransformMode>();
    generates::<Transformation>();
    generates::<TransformStep>();
    generates::<BodyMate>();
    generates::<ScaleFactors>();
    generates::<MoveFaces>();
    generates::<DeleteFaces>();
    generates::<Heal>();
    generates::<ReplaceFaces>();
    generates::<Graft>();
    generates::<Flex>();
    generates::<FlexKind>();
}

#[test]
fn every_joint_type_has_a_schema() {
    generates::<JointConnector>();
    generates::<Alignment>();
    generates::<JointKind>();
    generates::<JointMotion>();
    generates::<JointOffset>();
    generates::<AxisLimits>();
    generates::<Bounds<Length>>();
    generates::<Bounds<Angle>>();
    generates::<Coupling>();
    generates::<JointDrive>();
    generates::<DriveAxis>();
    generates::<DriveMode>();
    generates::<TangentContact>();
    generates::<WidthMode>();
    generates::<SlotPosition>();
    generates::<PathPosition>();
    generates::<PathAxisControl>();
    generates::<PathRollControl>();
    generates::<PairElement>();
}
