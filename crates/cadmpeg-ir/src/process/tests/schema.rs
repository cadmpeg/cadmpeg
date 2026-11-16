// SPDX-License-Identifier: Apache-2.0
//! The schema feature generates a schema for every process type.

use schemars::{schema_for, JsonSchema};

use crate::design::construction::kinds;
use crate::process::cycle::{Cycle, CycleKind, CycleReturn, PlaneAxis, ProbeTarget};
use crate::process::feature::{MachiningFeature, MachiningFeatureClass};
use crate::process::groups::{GroupKind, ProcessDefinition, ProcessGroup, ProcessOwner};
use crate::process::limits::{
    AxialFrom, AxialLimit, AxialRole, Height, HeightFrom, HeightRole, LimitPoint, RadialLimit,
    RadiusFrom, RadiusRole,
};
use crate::process::operation::{
    DrillingClass, GeometryCombine, GeometryRole, GeometrySource, MillingClass, ModifierKind,
    Operation, OperationClass, OperationKind, Phase, ProcessGeometry, ProfileCurve,
    ToolAxisControl, Tooling, TurningClass,
};
use crate::process::settings::{
    CycleSettings, HoleDepth, Lead, LeadAlong, LeadAngleFrom, Leads, LineTurn, Linking,
    PassClearance, PlungeKind, PlungeStrategy, ProbeAt, RetractPolicy,
};
use crate::process::setup::{
    AxisClass, FunctionStep, JobKind, NcFunction, Program, ProgramOutput, ProgramStep, Setup, Step,
    WorkOffset, WorkpieceSetup,
};
use crate::process::state::ProcessStateOf;
use crate::process::stock::{
    AxisSpec, BoxAlignment, BoxOffsets, BoxPoint, BoxSide, CylinderOffsets, StockPlacement,
    StockRule, StockShape, Wcs, WcsOrientation, WcsOrigin,
};
use crate::process::technology::{
    ChamferCut, Compensation, Coolant, CornerMode, CutMode, FeedRate, FeedRole, FinishingSteps,
    HelixRise, JetCut, PassTransition, RestSource, Spindle, SpindleDirection, StepRule,
    TabPlacement, TabShape, Tabs, Technology, TurningSide,
};
use crate::process::tool::{
    Cutter, HolderProfile, HolderSection, LibraryRef, Machine, NcNumbers, Preset, RotaryAxes, Tool,
    ToolCatalog, ToolClass, ToolPocket, TurningInsert, Uuid,
};
use crate::process::toolpath::{
    Heater, Motion, MotionClass, MotionValue, MovementClass, ToolpathContent,
};
use crate::process::{Choice, ProcessConstruction, WorkpieceSelection};

fn generates<T: JsonSchema>() {
    let schema = serde_json::to_value(schema_for!(T)).unwrap();
    assert!(schema.is_object(), "{}", T::schema_name());
}

#[test]
fn every_process_type_has_a_schema() {
    generates::<Choice<StockRule>>();
    generates::<Choice<WcsOrientation>>();
    generates::<Choice<WcsOrigin>>();
    generates::<Choice<HeightFrom>>();
    generates::<Choice<SpindleDirection>>();
    generates::<Choice<Coolant>>();
    generates::<WorkpieceSelection>();
    generates::<ProcessConstruction<kinds::Point>>();
    generates::<ProcessConstruction<kinds::Direction>>();
    generates::<ProcessConstruction<kinds::Axis>>();
    generates::<ProcessConstruction<kinds::Frame>>();
    generates::<Setup>();
    generates::<JobKind>();
    generates::<AxisClass>();
    generates::<WorkpieceSetup>();
    generates::<WorkOffset>();
    generates::<Step>();
    generates::<FunctionStep>();
    generates::<NcFunction>();
    generates::<Program>();
    generates::<ProgramStep>();
    generates::<ProgramOutput>();
    generates::<StockRule>();
    generates::<BoxOffsets>();
    generates::<CylinderOffsets>();
    generates::<StockShape>();
    generates::<StockPlacement>();
    generates::<BoxAlignment>();
    generates::<BoxSide>();
    generates::<Wcs>();
    generates::<WcsOrientation>();
    generates::<AxisSpec>();
    generates::<WcsOrigin>();
    generates::<BoxPoint>();
    generates::<Operation>();
    generates::<OperationKind>();
    generates::<Phase>();
    generates::<OperationClass>();
    generates::<MillingClass>();
    generates::<DrillingClass>();
    generates::<TurningClass>();
    generates::<ModifierKind>();
    generates::<Tooling>();
    generates::<ToolAxisControl>();
    generates::<ProcessGeometry>();
    generates::<GeometryRole>();
    generates::<GeometryCombine>();
    generates::<GeometrySource>();
    generates::<ProfileCurve>();
    generates::<HeightRole>();
    generates::<Height>();
    generates::<RadiusRole>();
    generates::<RadialLimit>();
    generates::<RadiusFrom>();
    generates::<AxialRole>();
    generates::<AxialLimit>();
    generates::<AxialFrom>();
    generates::<LimitPoint>();
    generates::<HeightFrom>();
    generates::<Leads>();
    generates::<Lead>();
    generates::<LineTurn>();
    generates::<LeadAngleFrom>();
    generates::<LeadAlong>();
    generates::<PlungeStrategy>();
    generates::<PlungeKind>();
    generates::<Linking>();
    generates::<PassClearance>();
    generates::<RetractPolicy>();
    generates::<CycleSettings>();
    generates::<ProbeAt>();
    generates::<HoleDepth>();
    generates::<MachiningFeature>();
    generates::<MachiningFeatureClass>();
    generates::<Technology>();
    generates::<FeedRole>();
    generates::<FeedRate>();
    generates::<Spindle>();
    generates::<SpindleDirection>();
    generates::<Coolant>();
    generates::<CutMode>();
    generates::<StepRule>();
    generates::<FinishingSteps>();
    generates::<Compensation>();
    generates::<CornerMode>();
    generates::<PassTransition>();
    generates::<Tabs>();
    generates::<TabShape>();
    generates::<TabPlacement>();
    generates::<ChamferCut>();
    generates::<JetCut>();
    generates::<RestSource>();
    generates::<HelixRise>();
    generates::<TurningSide>();
    generates::<Tool>();
    generates::<ToolClass>();
    generates::<LibraryRef>();
    generates::<Uuid>();
    generates::<ToolCatalog>();
    generates::<NcNumbers>();
    generates::<Cutter>();
    generates::<TurningInsert>();
    generates::<HolderProfile>();
    generates::<HolderSection>();
    generates::<Preset>();
    generates::<Machine>();
    generates::<RotaryAxes>();
    generates::<ToolPocket>();
    generates::<ProcessDefinition>();
    generates::<ProcessOwner>();
    generates::<ProcessGroup>();
    generates::<GroupKind>();
    generates::<ToolpathContent>();
    generates::<Motion>();
    generates::<Heater>();
    generates::<MotionClass>();
    generates::<MovementClass>();
    generates::<MotionValue>();
    generates::<Cycle>();
    generates::<CycleKind>();
    generates::<ProbeTarget>();
    generates::<PlaneAxis>();
    generates::<CycleReturn>();
    generates::<ProcessStateOf>();
}

#[test]
fn a_uuid_schema_states_the_lower_case_pattern() {
    let schema = serde_json::to_value(schema_for!(Uuid)).unwrap();
    assert_eq!(
        schema["pattern"],
        "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
    );
}
