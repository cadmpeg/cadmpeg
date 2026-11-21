// SPDX-License-Identifier: Apache-2.0
//! Every process type, written, read back strictly, and written again; the
//! two texts are equal.

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::fixtures::{
    self, axis_construction, box_point, choices, constant, dims, feature_id, frame_construction,
    group_id, length, lengths, machine_id, operation_id, parameter, point_constructions, preset_id,
    process_expressions, program_id, setup_id, text, tool_id, uuid,
};
use super::round_trip;
use crate::design::units::UnitSymbol;
use crate::process::cycle::{CycleReturn, PlaneAxis};
use crate::process::feature::MachiningFeatureClass;
use crate::process::groups::{GroupKind, ProcessDefinition, ProcessOwner};
use crate::process::limits::{
    AxialFrom, AxialRole, HeightFrom, HeightRole, LimitPoint, RadialLimit, RadiusRole,
};
use crate::process::operation::{
    DrillingClass, GeometryCombine, GeometryRole, MillingClass, ModifierKind, OperationClass,
    OperationKind, Phase, Tooling, TurningClass,
};
use crate::process::settings::{
    LeadAlong, LeadAngleFrom, Leads, PlungeKind, ProbeAt, RetractPolicy,
};
use crate::process::setup::{AxisClass, JobKind};
use crate::process::state::ProcessStateOf;
use crate::process::stock::{BoxSide, StockRule, WcsOrientation, WcsOrigin};
use crate::process::technology::{
    Compensation, Coolant, CornerMode, CutMode, FeedRole, HelixRise, JetCut, PassTransition,
    RestSource, SpindleDirection, StepRule, TabShape, TurningSide,
};
use crate::process::tool::{LibraryRef, RotaryAxes, ToolCatalog, ToolClass, ToolPocket, Uuid};
use crate::process::toolpath::{Heater, MotionClass, MovementClass, ToolpathContent};
use crate::process::Choice;
use crate::transform::Transform;

#[derive(Default)]
struct Report {
    failures: Vec<String>,
}

impl Report {
    fn check<T: Serialize + DeserializeOwned>(&mut self, name: &str, samples: Vec<T>) {
        assert!(!samples.is_empty(), "{name} has no sample");
        for sample in samples {
            if let Err(error) = round_trip(&sample) {
                self.failures.push(format!("{name}: {error}"));
            }
        }
    }

    fn finish(self) {
        assert!(self.failures.is_empty(), "{}", self.failures.join("\n"));
    }
}

#[test]
fn setups_and_programs_round_trip() {
    let mut report = Report::default();
    report.check(
        "Choice<StockRule>",
        choices(
            StockRule::PreviousSetup(setup_id(1)),
            StockRule::Native(text("f3d:x")),
        ),
    );
    report.check(
        "Choice<WcsOrientation>",
        choices(
            WcsOrientation::Model,
            WcsOrientation::Frame(frame_construction()),
        ),
    );
    report.check(
        "Choice<WcsOrigin>",
        choices(WcsOrigin::ModelOrigin, WcsOrigin::StockPoint(box_point())),
    );
    report.check(
        "Choice<HeightFrom>",
        choices(HeightFrom::StockTop, HeightFrom::Height(HeightRole::Feed)),
    );
    report.check(
        "Choice<SpindleDirection>",
        choices(SpindleDirection::Clockwise, SpindleDirection::Off),
    );
    report.check(
        "Choice<Coolant>",
        choices(Coolant::Air, Coolant::ThroughTool),
    );
    report.check("WorkpieceSelection", fixtures::workpiece_selections());
    report.check("ProcessConstruction<Point>", point_constructions());
    report.check("Setup", fixtures::setups());
    report.check(
        "JobKind",
        vec![
            JobKind::Milling,
            JobKind::Turning,
            JobKind::Jet,
            JobKind::WireEdm,
            JobKind::Additive,
            JobKind::Native(text("f3d:inspection")),
        ],
    );
    report.check(
        "AxisClass",
        vec![
            AxisClass::Planar,
            AxisClass::TwoAndHalf,
            AxisClass::Three,
            AxisClass::Indexed,
            AxisClass::Simultaneous,
        ],
    );
    report.check("WorkpieceSetup", fixtures::workpiece_setups());
    report.check("WorkOffset", fixtures::work_offsets());
    report.check("Step", fixtures::steps());
    report.check("FunctionStep", fixtures::function_steps());
    report.check("NcFunction", fixtures::nc_functions());
    report.check("Program", fixtures::programs());
    report.check("ProgramStep", fixtures::program_steps());
    report.check("ProgramOutput", fixtures::program_outputs());
    report.check(
        "Choice<UnitSymbol>",
        choices(UnitSymbol::Mm, UnitSymbol::In),
    );
    report.finish();
}

#[test]
fn stock_and_work_coordinates_round_trip() {
    let mut report = Report::default();
    report.check("StockRule", fixtures::stock_rules());
    report.check("BoxOffsets", vec![fixtures::box_offsets()]);
    report.check("CylinderOffsets", fixtures::cylinder_offsets());
    report.check("StockShape", fixtures::stock_shapes());
    report.check("StockPlacement", fixtures::stock_placements());
    report.check("BoxAlignment", fixtures::box_alignments());
    report.check(
        "BoxSide",
        vec![BoxSide::Low, BoxSide::Center, BoxSide::High],
    );
    report.check("Wcs", fixtures::wcses());
    report.check("WcsOrientation", fixtures::wcs_orientations());
    report.check("AxisSpec", fixtures::axis_specs());
    report.check("WcsOrigin", fixtures::wcs_origins());
    report.check("BoxPoint", vec![box_point()]);
    report.finish();
}

#[test]
fn operations_round_trip() {
    let mut report = Report::default();
    report.check("Operation", fixtures::operations());
    report.check(
        "OperationKind",
        vec![
            OperationKind::Machining {
                class: OperationClass::Probing,
                phase: None,
            },
            OperationKind::Machining {
                class: OperationClass::Turning(TurningClass::Threading),
                phase: Some(Phase::Finish),
            },
            OperationKind::Modifier {
                modifier: ModifierKind::Tags,
                inputs: vec![operation_id(1), operation_id(2)],
            },
        ],
    );
    report.check(
        "Phase",
        vec![Phase::Rough, Phase::SemiFinish, Phase::Finish],
    );
    report.check(
        "OperationClass",
        vec![
            OperationClass::Milling(MillingClass::Side),
            OperationClass::Drilling(DrillingClass::BackBoring),
            OperationClass::Turning(TurningClass::Grooving),
            OperationClass::Probing,
            OperationClass::ThreadMilling,
            OperationClass::Engraving,
            OperationClass::Chamfering,
            OperationClass::Jet,
            OperationClass::WireEdm,
            OperationClass::Additive,
            OperationClass::Unclassified,
        ],
    );
    report.check(
        "MillingClass",
        vec![
            MillingClass::Plane,
            MillingClass::Side,
            MillingClass::BottomAndSide,
            MillingClass::Freeform,
        ],
    );
    report.check(
        "DrillingClass",
        vec![
            DrillingClass::Drilling,
            DrillingClass::CenterDrilling,
            DrillingClass::Countersinking,
            DrillingClass::Multistep,
            DrillingClass::Boring,
            DrillingClass::Reaming,
            DrillingClass::BackBoring,
            DrillingClass::Tapping,
            DrillingClass::ThreadDrilling,
        ],
    );
    report.check(
        "TurningClass",
        vec![
            TurningClass::Facing,
            TurningClass::Contouring,
            TurningClass::Grooving,
            TurningClass::CuttingIn,
            TurningClass::Threading,
            TurningClass::Knurling,
        ],
    );
    report.check(
        "ModifierKind",
        vec![
            ModifierKind::LeadInOut,
            ModifierKind::Ramp,
            ModifierKind::Tags,
            ModifierKind::Dogbone,
            ModifierKind::Trim,
            ModifierKind::Boundary,
            ModifierKind::Pattern,
            ModifierKind::Transform,
            ModifierKind::AxisMap,
            ModifierKind::DragKnife,
            ModifierKind::ProbeCorrection,
            ModifierKind::Unclassified,
        ],
    );
    report.check(
        "Tooling",
        vec![Tooling::Tool(tool_id(1)), Tooling::Preset(preset_id(1))],
    );
    report.check("ToolAxisControl", fixtures::tool_axis_controls());
    report.check("ProcessGeometry", fixtures::process_geometries());
    report.check(
        "GeometryRole",
        vec![
            GeometryRole::Machine,
            GeometryRole::Boundary,
            GeometryRole::Check,
            GeometryRole::Exclude,
            GeometryRole::Start,
            GeometryRole::Orientation,
            GeometryRole::Reference,
            GeometryRole::Native(text("nx:trim")),
        ],
    );
    report.check(
        "GeometryCombine",
        vec![GeometryCombine::Replace, GeometryCombine::Extend],
    );
    report.check("GeometrySource", fixtures::geometry_sources());
    report.check("ProfileCurve", fixtures::profile_curves());
    report.finish();
}

#[test]
fn limits_and_settings_round_trip() {
    let mut report = Report::default();
    report.check(
        "HeightRole",
        vec![
            HeightRole::Clearance,
            HeightRole::Retract,
            HeightRole::Feed,
            HeightRole::Top,
            HeightRole::Bottom,
        ],
    );
    report.check("Height", fixtures::heights());
    report.check(
        "RadiusRole",
        vec![
            RadiusRole::Outer,
            RadiusRole::Inner,
            RadiusRole::OuterClearance,
            RadiusRole::InnerClearance,
            RadiusRole::OuterRetract,
        ],
    );
    report.check("RadialLimit", fixtures::radial_limits());
    report.check(
        "Choice<RadialLimit>",
        vec![constant(RadialLimit::From {
            base: fixtures::radius_froms().remove(4),
            offset: None,
        })],
    );
    report.check("RadiusFrom", fixtures::radius_froms());
    report.check("AxialRole", vec![AxialRole::Front, AxialRole::Back]);
    report.check("AxialLimit", fixtures::axial_limits());
    report.check(
        "AxialFrom",
        vec![
            AxialFrom::ModelFront,
            AxialFrom::ModelBack,
            AxialFrom::StockFront,
            AxialFrom::Native(text("f3d:selection")),
        ],
    );
    report.check(
        "LimitPoint",
        vec![LimitPoint::ContactPoint, LimitPoint::CuttingEdge],
    );
    report.check(
        "HeightFrom",
        vec![
            HeightFrom::Wcs,
            HeightFrom::StockTop,
            HeightFrom::StockBottom,
            HeightFrom::ModelTop,
            HeightFrom::ModelBottom,
            HeightFrom::Height(HeightRole::Clearance),
            HeightFrom::Selection(text("top")),
            HeightFrom::Geometry,
            HeightFrom::Native(text("f3d:hole-top")),
        ],
    );
    report.check(
        "Leads",
        vec![
            Leads {
                entry: Some(fixtures::leads().remove(0)),
                exit: Some(fixtures::leads().remove(1)),
            },
            Leads {
                entry: None,
                exit: None,
            },
        ],
    );
    report.check("Lead", fixtures::leads());
    report.check("LineTurn", fixtures::line_turns());
    report.check(
        "LeadAngleFrom",
        vec![LeadAngleFrom::Cut, LeadAngleFrom::SpindleAxis],
    );
    report.check("LeadAlong", vec![LeadAlong::Forward, LeadAlong::Backward]);
    report.check("PlungeStrategy", fixtures::plunge_strategies());
    report.check(
        "PlungeKind",
        vec![
            PlungeKind::ToolAxis,
            PlungeKind::Helix,
            PlungeKind::Ramp,
            PlungeKind::Native(text("f3d:profile")),
        ],
    );
    report.check("Linking", fixtures::linkings());
    report.check("PassClearance", fixtures::pass_clearances());
    report.check(
        "RetractPolicy",
        vec![
            RetractPolicy::Full,
            RetractPolicy::Minimum,
            RetractPolicy::Native(text("f3d:shortest")),
        ],
    );
    report.check("CycleSettings", fixtures::cycle_settings());
    report.check("ProbeAt", vec![ProbeAt::Nominal, ProbeAt::Picked]);
    report.check("HoleDepth", fixtures::hole_depths());
    report.finish();
}

#[test]
fn machining_features_and_technology_round_trip() {
    let mut report = Report::default();
    report.check("MachiningFeature", fixtures::machining_features());
    report.check(
        "MachiningFeatureClass",
        vec![
            MachiningFeatureClass::PlanarFace {},
            MachiningFeatureClass::ClosedPocket {},
            MachiningFeatureClass::OpenPocket {},
            MachiningFeatureClass::Slot {},
            MachiningFeatureClass::Step {},
            MachiningFeatureClass::RoundHole {},
            MachiningFeatureClass::Boss {},
            MachiningFeatureClass::OutsideProfile {},
            MachiningFeatureClass::Region {},
            MachiningFeatureClass::Replicate {
                base: feature_id(1),
            },
            MachiningFeatureClass::Compound {
                members: vec![feature_id(1), feature_id(2)],
            },
            MachiningFeatureClass::Unclassified {},
        ],
    );
    report.check("Technology", fixtures::technologies());
    report.check(
        "FeedRole",
        vec![
            FeedRole::Cutting,
            FeedRole::Plunge,
            FeedRole::Ramp,
            FeedRole::Entry,
            FeedRole::Exit,
            FeedRole::Retract,
            FeedRole::Approach,
            FeedRole::Departure,
            FeedRole::Link,
            FeedRole::Transition,
            FeedRole::Finishing,
            FeedRole::Reduced,
            FeedRole::Native(text("f3d:feedPerRevolution")),
        ],
    );
    report.check("FeedRate", fixtures::feed_rates());
    report.check("Spindle", fixtures::spindles());
    report.check(
        "SpindleDirection",
        vec![
            SpindleDirection::Clockwise,
            SpindleDirection::CounterClockwise,
            SpindleDirection::Off,
        ],
    );
    report.check(
        "Coolant",
        vec![
            Coolant::Off,
            Coolant::Flood,
            Coolant::Mist,
            Coolant::Air,
            Coolant::ThroughTool,
            Coolant::Native(text("f3d:suction")),
        ],
    );
    report.check(
        "CutMode",
        vec![
            CutMode::Climb,
            CutMode::Conventional,
            CutMode::Both,
            CutMode::Native(text("f3d:auto")),
        ],
    );
    report.check(
        "StepRule",
        vec![
            StepRule::Fixed,
            StepRule::Even,
            StepRule::ConstantEngagement,
        ],
    );
    report.check("FinishingSteps", vec![fixtures::finishing_steps()]);
    report.check(
        "Compensation",
        vec![
            Compensation::Left,
            Compensation::Right,
            Compensation::Center,
            Compensation::Native(text("f3d:wear")),
        ],
    );
    report.check("CornerMode", vec![CornerMode::Sharp, CornerMode::Rounded]);
    report.check(
        "PassTransition",
        vec![
            PassTransition::Arc,
            PassTransition::Line,
            PassTransition::None,
        ],
    );
    report.check("Tabs", vec![fixtures::tabs()]);
    report.check(
        "TabShape",
        vec![TabShape::Rectangular, TabShape::Triangular],
    );
    report.check("TabPlacement", fixtures::tab_placements());
    report.check("ChamferCut", vec![fixtures::chamfer_cut()]);
    report.check(
        "JetCut",
        vec![
            JetCut::Through,
            JetCut::Mark,
            JetCut::Native(text("f3d:etch")),
        ],
    );
    report.check(
        "RestSource",
        vec![
            RestSource::EarlierOperations,
            RestSource::SetupStock,
            RestSource::Native(text("f3d:from-file")),
        ],
    );
    report.check(
        "HelixRise",
        vec![
            HelixRise::Pitch(lengths().remove(0)),
            HelixRise::Angle(dims(fixtures::angle(0.1)).remove(1)),
        ],
    );
    report.check(
        "TurningSide",
        vec![TurningSide::Outside, TurningSide::Inside],
    );
    report.finish();
}

#[test]
fn tools_presets_and_machines_round_trip() {
    let mut report = Report::default();
    report.check("Tool", fixtures::tools());
    report.check(
        "ToolClass",
        vec![
            ToolClass::FlatEndmill,
            ToolClass::BallEndmill,
            ToolClass::BullnoseEndmill,
            ToolClass::ChamferMill,
            ToolClass::FaceMill,
            ToolClass::SlotMill,
            ToolClass::DovetailMill,
            ToolClass::ThreadMill,
            ToolClass::Drill,
            ToolClass::CenterDrill,
            ToolClass::SpotDrill,
            ToolClass::Countersink,
            ToolClass::Counterbore,
            ToolClass::Reamer,
            ToolClass::Tap,
            ToolClass::BoringBar,
            ToolClass::TurningGeneral,
            ToolClass::TurningBoring,
            ToolClass::TurningGrooving,
            ToolClass::TurningThreading,
            ToolClass::Probe,
            ToolClass::JetCutter,
            ToolClass::Unclassified,
        ],
    );
    report.check(
        "LibraryRef",
        vec![
            LibraryRef::Guid(uuid()),
            LibraryRef::Uri(text("Bit/5mm_Endmill.fctb")),
        ],
    );
    report.check("Uuid", vec![uuid(), Uuid([0xff; 16])]);
    report.check(
        "ToolCatalog",
        vec![
            fixtures::tools().remove(0).catalog.unwrap(),
            ToolCatalog {
                vendor: None,
                product_id: None,
                link: None,
            },
        ],
    );
    report.check("NcNumbers", fixtures::nc_numbers());
    report.check("Cutter", fixtures::cutters());
    report.check("TurningInsert", fixtures::turning_inserts());
    report.check("HolderProfile", vec![fixtures::holder_profile()]);
    report.check("HolderSection", fixtures::holder_profile().sections);
    report.check("Preset", fixtures::presets());
    report.check("Machine", fixtures::machines());
    report.check(
        "RotaryAxes",
        vec![
            RotaryAxes::Absent {},
            RotaryAxes::Present {
                count: Some(fixtures::count(1)),
            },
            RotaryAxes::Present { count: None },
        ],
    );
    report.check(
        "ToolPocket",
        vec![ToolPocket {
            tool: tool_id(2),
            nc: fixtures::nc_numbers().remove(1),
        }],
    );
    report.finish();
}

#[test]
fn parameters_and_groups_round_trip() {
    let mut report = Report::default();
    let mut definitions = process_expressions()
        .into_iter()
        .map(ProcessDefinition::Expression)
        .collect::<Vec<_>>();
    definitions.push(ProcessDefinition::Computed);
    report.check("ProcessDefinition", definitions);
    report.check(
        "ProcessOwner",
        vec![
            ProcessOwner::Setup(setup_id(1)),
            ProcessOwner::Operation(operation_id(1)),
            ProcessOwner::Feature(feature_id(1)),
            ProcessOwner::Group(group_id(1)),
            ProcessOwner::Tool(tool_id(1)),
            ProcessOwner::Preset(preset_id(1)),
            ProcessOwner::Machine(machine_id(1)),
            ProcessOwner::Program(program_id(1)),
        ],
    );
    report.check("ProcessGroup", fixtures::process_groups());
    report.check(
        "GroupKind",
        vec![
            GroupKind::Method,
            GroupKind::Geometry {
                geometry: fixtures::process_geometries(),
            },
            GroupKind::Native(text("nx:program")),
        ],
    );
    report.finish();
}

#[test]
fn toolpaths_and_states_round_trip() {
    let mut report = Report::default();
    report.check(
        "ToolpathContent",
        vec![
            ToolpathContent::Motions {
                frame: Transform::identity(),
                motions: fixtures::motions(),
            },
            ToolpathContent::Native("toolpath-1".to_owned()),
        ],
    );
    report.check("Motion", fixtures::motions());
    report.check(
        "Heater",
        vec![Heater::Extruder { index: None }, Heater::Bed {}],
    );
    report.check(
        "MotionClass",
        vec![MotionClass {
            class: MovementClass::Cutting,
            native: None,
        }],
    );
    report.check(
        "MovementClass",
        vec![
            MovementClass::Approach,
            MovementClass::Cutting,
            MovementClass::Finishing,
            MovementClass::Retreat,
            MovementClass::Transition,
            MovementClass::Link,
            MovementClass::Ramp,
            MovementClass::Plunge,
            MovementClass::Reduced,
            MovementClass::Unclassified,
        ],
    );
    report.check("MotionValue", fixtures::motion_values());
    report.check("Cycle", fixtures::cycles());
    report.check("CycleKind", fixtures::cycle_kinds());
    report.check("ProbeTarget", fixtures::probe_targets());
    report.check("PlaneAxis", vec![PlaneAxis::X, PlaneAxis::Y]);
    report.check(
        "CycleReturn",
        vec![CycleReturn::Initial, CycleReturn::Retract],
    );
    report.check(
        "ProcessStateOf",
        vec![
            ProcessStateOf::Stock(setup_id(1)),
            ProcessStateOf::After(operation_id(1)),
            ProcessStateOf::Removal(operation_id(2)),
        ],
    );
    report.finish();
}

#[test]
fn choices_name_their_parameters() {
    let mut report = Report::default();
    report.check(
        "Choice<AxialFrom>",
        vec![Choice {
            value: Some(AxialFrom::StockFront),
            parameters: [parameter(1)].into(),
        }],
    );
    report.check("ProcessConstruction<Axis>", vec![axis_construction()]);
    report.check(
        "Choice<HelixRise>",
        choices(
            HelixRise::Pitch(fixtures::lit(length(1.0))),
            HelixRise::Angle(fixtures::lit(fixtures::angle(0.2))),
        ),
    );
    report.finish();
}
