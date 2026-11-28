// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every process type.

use cadmpeg_core::serde_erase::json;
use schemars::{schema_for, SchemaGenerator};

use cadmpeg_ir::design::construction::kinds;
use cadmpeg_ir::document::Process;
use cadmpeg_ir::process::cycle::{Cycle, CycleKind, CycleReturn, PlaneAxis, ProbeTarget};
use cadmpeg_ir::process::feature::{MachiningFeature, MachiningFeatureClass};
use cadmpeg_ir::process::groups::{
    GroupKind, ProcessDefinition, ProcessGroup, ProcessOwner, ProcessParameter,
};
use cadmpeg_ir::process::limits::{
    AxialFrom, AxialLimit, AxialRole, Height, HeightFrom, HeightRole, LimitPoint, RadialLimit,
    RadiusFrom, RadiusRole,
};
use cadmpeg_ir::process::operation::{
    DrillingClass, GeometryCombine, GeometryRole, GeometrySource, MillingClass, ModifierKind,
    Operation, OperationClass, OperationKind, Phase, ProcessGeometry, ProfileCurve,
    ToolAxisControl, Tooling, TurningClass,
};
use cadmpeg_ir::process::settings::{
    CycleSettings, HoleDepth, Lead, LeadAlong, LeadAngleFrom, Leads, LineTurn, Linking,
    PassClearance, PlungeKind, PlungeStrategy, ProbeAt, RetractPolicy,
};
use cadmpeg_ir::process::setup::{
    AxisClass, FunctionStep, JobKind, NcFunction, Program, ProgramOutput, ProgramStep, Setup, Step,
    WorkOffset, WorkpieceSetup,
};
use cadmpeg_ir::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};
use cadmpeg_ir::process::stock::{
    AxisSpec, BoxAlignment, BoxOffsets, BoxPoint, BoxSide, CylinderOffsets, StockPlacement,
    StockRule, StockShape, Wcs, WcsOrientation, WcsOrigin,
};
use cadmpeg_ir::process::technology::{
    ChamferCut, Compensation, Coolant, CornerMode, CutMode, FeedRate, FeedRole, FinishingSteps,
    HelixRise, JetCut, PassTransition, RestSource, Spindle, SpindleDirection, StepRule,
    TabPlacement, TabShape, Tabs, Technology, TurningSide,
};
use cadmpeg_ir::process::tool::{
    Cutter, HolderProfile, HolderSection, LibraryRef, Machine, NcNumbers, Preset, RotaryAxes, Tool,
    ToolCatalog, ToolClass, ToolPocket, TurningInsert, Uuid,
};
use cadmpeg_ir::process::toolpath::{
    Heater, Motion, MotionClass, MotionValue, MovementClass, Toolpath, ToolpathContent,
};
use cadmpeg_ir::process::{Choice, ProcessConstruction, WorkpieceSelection};

use crate::generates;

#[test]
fn every_process_type_has_a_schema() {
    let mut generator = SchemaGenerator::default();
    generates::<Process>(&mut generator);
    generates::<ProcessParameter>(&mut generator);
    generates::<Toolpath>(&mut generator);
    generates::<ProcessState>(&mut generator);
    generates::<ProcessContent>(&mut generator);
    generates::<ProcessBase>(&mut generator);
    generates::<Choice<StockRule>>(&mut generator);
    generates::<Choice<WcsOrientation>>(&mut generator);
    generates::<Choice<WcsOrigin>>(&mut generator);
    generates::<Choice<HeightFrom>>(&mut generator);
    generates::<Choice<SpindleDirection>>(&mut generator);
    generates::<Choice<Coolant>>(&mut generator);
    generates::<WorkpieceSelection>(&mut generator);
    generates::<ProcessConstruction<kinds::Point>>(&mut generator);
    generates::<ProcessConstruction<kinds::Direction>>(&mut generator);
    generates::<ProcessConstruction<kinds::Axis>>(&mut generator);
    generates::<ProcessConstruction<kinds::Frame>>(&mut generator);
    generates::<Setup>(&mut generator);
    generates::<JobKind>(&mut generator);
    generates::<AxisClass>(&mut generator);
    generates::<WorkpieceSetup>(&mut generator);
    generates::<WorkOffset>(&mut generator);
    generates::<Step>(&mut generator);
    generates::<FunctionStep>(&mut generator);
    generates::<NcFunction>(&mut generator);
    generates::<Program>(&mut generator);
    generates::<ProgramStep>(&mut generator);
    generates::<ProgramOutput>(&mut generator);
    generates::<StockRule>(&mut generator);
    generates::<BoxOffsets>(&mut generator);
    generates::<CylinderOffsets>(&mut generator);
    generates::<StockShape>(&mut generator);
    generates::<StockPlacement>(&mut generator);
    generates::<BoxAlignment>(&mut generator);
    generates::<BoxSide>(&mut generator);
    generates::<Wcs>(&mut generator);
    generates::<WcsOrientation>(&mut generator);
    generates::<AxisSpec>(&mut generator);
    generates::<WcsOrigin>(&mut generator);
    generates::<BoxPoint>(&mut generator);
    generates::<Operation>(&mut generator);
    generates::<OperationKind>(&mut generator);
    generates::<Phase>(&mut generator);
    generates::<OperationClass>(&mut generator);
    generates::<MillingClass>(&mut generator);
    generates::<DrillingClass>(&mut generator);
    generates::<TurningClass>(&mut generator);
    generates::<ModifierKind>(&mut generator);
    generates::<Tooling>(&mut generator);
    generates::<ToolAxisControl>(&mut generator);
    generates::<ProcessGeometry>(&mut generator);
    generates::<GeometryRole>(&mut generator);
    generates::<GeometryCombine>(&mut generator);
    generates::<GeometrySource>(&mut generator);
    generates::<ProfileCurve>(&mut generator);
    generates::<HeightRole>(&mut generator);
    generates::<Height>(&mut generator);
    generates::<RadiusRole>(&mut generator);
    generates::<RadialLimit>(&mut generator);
    generates::<RadiusFrom>(&mut generator);
    generates::<AxialRole>(&mut generator);
    generates::<AxialLimit>(&mut generator);
    generates::<AxialFrom>(&mut generator);
    generates::<LimitPoint>(&mut generator);
    generates::<HeightFrom>(&mut generator);
    generates::<Leads>(&mut generator);
    generates::<Lead>(&mut generator);
    generates::<LineTurn>(&mut generator);
    generates::<LeadAngleFrom>(&mut generator);
    generates::<LeadAlong>(&mut generator);
    generates::<PlungeStrategy>(&mut generator);
    generates::<PlungeKind>(&mut generator);
    generates::<Linking>(&mut generator);
    generates::<PassClearance>(&mut generator);
    generates::<RetractPolicy>(&mut generator);
    generates::<CycleSettings>(&mut generator);
    generates::<ProbeAt>(&mut generator);
    generates::<HoleDepth>(&mut generator);
    generates::<MachiningFeature>(&mut generator);
    generates::<MachiningFeatureClass>(&mut generator);
    generates::<Technology>(&mut generator);
    generates::<FeedRole>(&mut generator);
    generates::<FeedRate>(&mut generator);
    generates::<Spindle>(&mut generator);
    generates::<SpindleDirection>(&mut generator);
    generates::<Coolant>(&mut generator);
    generates::<CutMode>(&mut generator);
    generates::<StepRule>(&mut generator);
    generates::<FinishingSteps>(&mut generator);
    generates::<Compensation>(&mut generator);
    generates::<CornerMode>(&mut generator);
    generates::<PassTransition>(&mut generator);
    generates::<Tabs>(&mut generator);
    generates::<TabShape>(&mut generator);
    generates::<TabPlacement>(&mut generator);
    generates::<ChamferCut>(&mut generator);
    generates::<JetCut>(&mut generator);
    generates::<RestSource>(&mut generator);
    generates::<HelixRise>(&mut generator);
    generates::<TurningSide>(&mut generator);
    generates::<Tool>(&mut generator);
    generates::<ToolClass>(&mut generator);
    generates::<LibraryRef>(&mut generator);
    generates::<Uuid>(&mut generator);
    generates::<ToolCatalog>(&mut generator);
    generates::<NcNumbers>(&mut generator);
    generates::<Cutter>(&mut generator);
    generates::<TurningInsert>(&mut generator);
    generates::<HolderProfile>(&mut generator);
    generates::<HolderSection>(&mut generator);
    generates::<Preset>(&mut generator);
    generates::<Machine>(&mut generator);
    generates::<RotaryAxes>(&mut generator);
    generates::<ToolPocket>(&mut generator);
    generates::<ProcessDefinition>(&mut generator);
    generates::<ProcessOwner>(&mut generator);
    generates::<ProcessGroup>(&mut generator);
    generates::<GroupKind>(&mut generator);
    generates::<ToolpathContent>(&mut generator);
    generates::<Motion>(&mut generator);
    generates::<Heater>(&mut generator);
    generates::<MotionClass>(&mut generator);
    generates::<MovementClass>(&mut generator);
    generates::<MotionValue>(&mut generator);
    generates::<Cycle>(&mut generator);
    generates::<CycleKind>(&mut generator);
    generates::<ProbeTarget>(&mut generator);
    generates::<PlaneAxis>(&mut generator);
    generates::<CycleReturn>(&mut generator);
    generates::<ProcessStateOf>(&mut generator);
}

#[test]
fn a_uuid_schema_states_the_lower_case_pattern() {
    let schema = json::to_value(schema_for!(Uuid)).unwrap();
    assert_eq!(
        schema["pattern"],
        "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
    );
}
