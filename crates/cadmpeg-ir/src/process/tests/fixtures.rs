// SPDX-License-Identifier: Apache-2.0
//! Hand-built values of the process types, one or more per variant.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::num::NonZeroU32;

use cadmpeg_core::text::NonBlankString;

use crate::design::construction::{
    kinds, AxisValue, Construction, ConstructionKind, FrameAxis, FrameValue,
};
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{
    BinaryOp, Expression, ExpressionDialect, ExpressionToken, Neutral, Node,
};
use crate::design::ids::{
    MachineId, MachiningFeatureId, OperationId, PresetId, ProcessGroupId, ProcessParameterId,
    ProgramId, SetupId, ToolId,
};
use crate::design::names::{NameKind, NativeName, NativeNameValue, TopoName};
use crate::design::ops::shared::RadialDimension;
use crate::design::ops::sketch::BoundedCurve;
use crate::design::options::Hand;
use crate::design::selection::{
    Anchor, MemberOrder, Members, PickTarget, Picked, Picks, Selection,
};
use crate::design::units::{AngularSpeed, Duration, Feed, UnitSymbol};
use crate::features::geometry::FinitePoint3;
use crate::features::{FeatureId, ParameterId};
use crate::geometry::analytic::LineCurve;
use crate::geometry::SolvedCurveGeometry;
use crate::math::{Point3, Vector3};
use crate::process::cycle::{Cycle, CycleKind, CycleReturn, PlaneAxis, ProbeTarget};
use crate::process::feature::{MachiningFeature, MachiningFeatureClass};
use crate::process::groups::{GroupKind, ProcessGroup};
use crate::process::limits::{
    AxialFrom, AxialLimit, AxialRole, Height, HeightFrom, HeightRole, LimitPoint, RadialLimit,
    RadiusFrom, RadiusRole,
};
use crate::process::operation::{
    GeometryCombine, GeometryRole, GeometrySource, MillingClass, ModifierKind, Operation,
    OperationClass, OperationKind, Phase, ProcessGeometry, ProfileCurve, ToolAxisControl, Tooling,
};
use crate::process::settings::{
    CycleSettings, HoleDepth, Lead, LeadAlong, LeadAngleFrom, Leads, LineTurn, Linking,
    PassClearance, PlungeKind, PlungeStrategy, ProbeAt, RetractPolicy,
};
use crate::process::setup::{
    AxisClass, FunctionStep, JobKind, NcFunction, Program, ProgramOutput, ProgramStep, Setup, Step,
    WorkOffset, WorkpieceSetup,
};
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
use crate::process::toolpath::{Heater, Motion, MotionClass, MotionValue, MovementClass};
use crate::process::{Choice, ProcessConstruction, ProcessDimension, WorkpieceSelection};
use crate::scalar::{Angle, FiniteReal, Fraction, Length};
use crate::topology::IncreasingParameterInterval;
use crate::transform::Transform;
use crate::units::UnitVector3;

/// An identity of kind `kind` with key `key`.
pub(super) fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:process:{kind}#{key}")).unwrap()
}

pub(super) fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

pub(super) fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

pub(super) fn angle(value: f64) -> Angle {
    Angle::new(value).unwrap()
}

pub(super) fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

pub(super) fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3 { x, y, z }).unwrap()
}

pub(super) fn feed(value: f64) -> Feed {
    Feed::new(value).unwrap()
}

pub(super) fn speed(value: f64) -> AngularSpeed {
    AngularSpeed::new(value).unwrap()
}

pub(super) fn duration(value: f64) -> Duration {
    Duration::new(value).unwrap()
}

pub(super) fn count(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}

pub(super) fn uuid() -> Uuid {
    Uuid([
        0x12, 0x3e, 0x45, 0x67, 0xe8, 0x9b, 0x12, 0xd3, 0xa4, 0x56, 0x42, 0x66, 0x14, 0x17, 0x40,
        0x00,
    ])
}

pub(super) fn setup_id(key: u32) -> SetupId {
    id("setup", key)
}

pub(super) fn operation_id(key: u32) -> OperationId {
    id("operation", key)
}

pub(super) fn feature_id(key: u32) -> MachiningFeatureId {
    id("machining-feature", key)
}

pub(super) fn tool_id(key: u32) -> ToolId {
    id("tool", key)
}

pub(super) fn preset_id(key: u32) -> PresetId {
    id("preset", key)
}

pub(super) fn machine_id(key: u32) -> MachineId {
    id("machine", key)
}

pub(super) fn program_id(key: u32) -> ProgramId {
    id("program", key)
}

pub(super) fn group_id(key: u32) -> ProcessGroupId {
    id("process-group", key)
}

pub(super) fn parameter(key: u32) -> ProcessParameterId {
    id("process-parameter", key)
}

pub(super) fn design_ref(key: u32) -> ParamRef {
    ParamRef::Design(ParameterRef {
        target: RefTarget::Parameter(id::<ParameterId>("parameter", key)),
        through: None,
    })
}

pub(super) fn native_name(value: &str) -> NativeName {
    NativeName {
        codec: crate::codec_id!("f3d"),
        value: NativeNameValue::Text(text(value)),
    }
}

/// A literal process dimension.
pub(super) fn lit<T>(value: T) -> ProcessDimension<T> {
    Dimension::Literal(value)
}

/// One dimension of each form: a literal, a process parameter, and a negated
/// design parameter.
pub(super) fn dims<T>(value: T) -> Vec<ProcessDimension<T>> {
    vec![
        Dimension::Literal(value),
        Dimension::Parameter(ParamRef::Process(parameter(1))),
        Dimension::Negated(design_ref(1)),
    ]
}

pub(super) fn lengths() -> Vec<ProcessDimension<Length>> {
    dims(length(2.5))
}

/// A choice the source holds as a constant.
pub(super) fn constant<T>(value: T) -> Choice<T> {
    Choice {
        value: Some(value),
        parameters: BTreeSet::new(),
    }
}

/// A constant choice, a choice read from parameters, and a choice whose
/// parameters hold no value.
pub(super) fn choices<T>(value: T, read: T) -> Vec<Choice<T>> {
    vec![
        constant(value),
        Choice {
            value: Some(read),
            parameters: BTreeSet::from([parameter(1), parameter(2)]),
        },
        Choice {
            value: None,
            parameters: BTreeSet::from([parameter(3)]),
        },
    ]
}

fn picked(key: u32) -> Picked {
    Picked {
        target: PickTarget::Entity(TopoName::Generated {
            kind: NameKind::Face,
            feature: id::<FeatureId>("feature", key),
            role: None,
            generator: None,
            index: None,
        }),
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

pub(super) fn selection<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Named(Members::new(vec![picked(1)]).unwrap()),
        native: None,
    }
}

pub(super) fn workpiece_selections() -> Vec<WorkpieceSelection> {
    vec![
        WorkpieceSelection {
            workpiece: Some(0),
            selection: selection(),
        },
        WorkpieceSelection {
            workpiece: None,
            selection: selection(),
        },
    ]
}

pub(super) fn construction<K: ConstructionKind>(resolved: Option<K::Value>) -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }
}

/// A construction on a workpiece, resolved in setup space, and one in the
/// definition's frame with nothing resolved.
pub(super) fn process_constructions<K: ConstructionKind>(
    resolved: K::Value,
) -> Vec<ProcessConstruction<K>> {
    vec![
        ProcessConstruction {
            workpiece: Some(0),
            construction: construction(Some(resolved)),
        },
        ProcessConstruction {
            workpiece: None,
            construction: construction(None),
        },
    ]
}

pub(super) fn frame_value() -> FrameValue {
    FrameValue {
        origin: point(1.0, 2.0, 3.0),
        x_axis: UnitVector3::X_AXIS,
        y_axis: UnitVector3::Y_AXIS,
        z_axis: UnitVector3::Z_AXIS,
    }
}

pub(super) fn axis_value() -> AxisValue {
    AxisValue {
        origin: point(0.0, 0.0, 0.0),
        direction: UnitVector3::Z_AXIS,
    }
}

pub(super) fn point_constructions() -> Vec<ProcessConstruction<kinds::Point>> {
    process_constructions(point(1.0, 2.0, 3.0))
}

pub(super) fn direction_construction() -> ProcessConstruction<kinds::Direction> {
    process_constructions(UnitVector3::X_AXIS).remove(0)
}

pub(super) fn axis_construction() -> ProcessConstruction<kinds::Axis> {
    process_constructions(axis_value()).remove(0)
}

pub(super) fn frame_construction() -> ProcessConstruction<kinds::Frame> {
    process_constructions(frame_value()).remove(0)
}

pub(super) fn process_expressions() -> Vec<Expression<ParamRef>> {
    vec![
        Expression::Neutral(Neutral(Node::Binary {
            op: BinaryOp::Subtract,
            left: Box::new(Node::Reference(design_ref(1))),
            right: Box::new(Node::Reference(ParamRef::Process(parameter(1)))),
        })),
        Expression::Source {
            dialect: ExpressionDialect::FusionCam,
            tokens: vec![
                ExpressionToken::Reference(ParamRef::Process(parameter(2))),
                ExpressionToken::Text(" * 2".to_owned()),
            ],
        },
    ]
}

pub(super) fn work_offsets() -> Vec<WorkOffset> {
    vec![WorkOffset::Default, WorkOffset::Number(count(54))]
}

pub(super) fn function_steps() -> Vec<FunctionStep> {
    vec![
        FunctionStep {
            function: NcFunction::OptionalStop,
            suppressed: Some(true),
            native_ref: Some("nc-function-1".to_owned()),
        },
        FunctionStep {
            function: NcFunction::Message("change fixture".to_owned()),
            suppressed: None,
            native_ref: None,
        },
    ]
}

pub(super) fn nc_functions() -> Vec<NcFunction> {
    vec![
        NcFunction::Stop,
        NcFunction::OptionalStop,
        NcFunction::Message("check part".to_owned()),
        NcFunction::WorkOffset(WorkOffset::Number(count(2))),
        NcFunction::Code("M8".to_owned()),
        NcFunction::Native(text("f3d:pass-through")),
    ]
}

pub(super) fn steps() -> Vec<Step> {
    vec![
        Step::Operation(operation_id(1)),
        Step::Folder {
            name: Some("Roughing".to_owned()),
            steps: vec![
                Step::Operation(operation_id(2)),
                Step::Folder {
                    name: None,
                    steps: Vec::new(),
                },
            ],
        },
        Step::Function(function_steps().remove(0)),
    ]
}

pub(super) fn program_steps() -> Vec<ProgramStep> {
    vec![
        ProgramStep::Setup(setup_id(1)),
        ProgramStep::Operation(operation_id(1)),
        ProgramStep::Function(function_steps().remove(1)),
        ProgramStep::Group {
            name: Some("PROGRAM".to_owned()),
            steps: vec![ProgramStep::Operation(operation_id(2))],
        },
    ]
}

pub(super) fn program_outputs() -> Vec<ProgramOutput> {
    vec![
        ProgramOutput {
            filename: Some(text("1001")),
            folder: Some(String::new()),
            extension: Some(text("nc")),
            unit: Some(choices(UnitSymbol::Mm, UnitSymbol::In).remove(1)),
        },
        ProgramOutput {
            filename: None,
            folder: None,
            extension: None,
            unit: None,
        },
    ]
}

pub(super) fn programs() -> Vec<Program> {
    let [full, empty] = <[ProgramOutput; 2]>::try_from(program_outputs()).unwrap();
    vec![
        Program {
            id: program_id(1),
            name: Some("1001".to_owned()),
            steps: program_steps(),
            post: Some(text("fanuc.cps")),
            machine: Some(machine_id(1)),
            output: full,
            native_ref: Some("nc-program-1".to_owned()),
        },
        Program {
            id: program_id(2),
            name: None,
            steps: Vec::new(),
            post: None,
            machine: None,
            output: empty,
            native_ref: None,
        },
    ]
}

pub(super) fn box_offsets() -> BoxOffsets {
    BoxOffsets {
        low: [Some(lit(length(1.0))), None, Some(lengths().remove(1))],
        high: [None, Some(lit(length(2.0))), Some(lengths().remove(2))],
    }
}

pub(super) fn cylinder_offsets() -> Vec<CylinderOffsets> {
    vec![
        CylinderOffsets {
            axis: Some(axis_construction()),
            radial: Some(lit(length(1.0))),
            front: Some(lit(length(0.5))),
            back: Some(lengths().remove(1)),
        },
        CylinderOffsets {
            axis: None,
            radial: None,
            front: None,
            back: None,
        },
    ]
}

pub(super) fn stock_shapes() -> Vec<StockShape> {
    vec![
        StockShape::Box {
            size: [lit(length(100.0)), lit(length(50.0)), lengths().remove(1)],
        },
        StockShape::Cylinder {
            diameter: lit(length(40.0)),
            length: lit(length(80.0)),
        },
        StockShape::Tube {
            outer: lit(length(40.0)),
            inner: lit(length(20.0)),
            length: lengths().remove(2),
        },
    ]
}

pub(super) fn box_alignments() -> Vec<BoxAlignment> {
    vec![
        BoxAlignment {
            side: Some(BoxSide::Center),
            offset: Some(lit(length(1.5))),
        },
        BoxAlignment {
            side: None,
            offset: None,
        },
    ]
}

pub(super) fn stock_placements() -> Vec<StockPlacement> {
    let [aligned, open] = <[BoxAlignment; 2]>::try_from(box_alignments()).unwrap();
    vec![
        StockPlacement::Aligned([aligned.clone(), open, aligned]),
        StockPlacement::Transform(Transform::identity()),
    ]
}

pub(super) fn stock_rules() -> Vec<StockRule> {
    vec![
        StockRule::Relative(box_offsets()),
        StockRule::RelativeCylinder(cylinder_offsets().remove(0)),
        StockRule::Fixed {
            shape: stock_shapes().remove(0),
            placement: stock_placements().remove(0),
        },
        StockRule::Solid(selection()),
        StockRule::PreviousSetup(setup_id(1)),
        StockRule::Native(text("f3d:from-mesh")),
    ]
}

pub(super) fn box_point() -> BoxPoint {
    BoxPoint {
        x: BoxSide::Low,
        y: BoxSide::Center,
        z: BoxSide::High,
    }
}

pub(super) fn axis_specs() -> Vec<AxisSpec> {
    vec![
        AxisSpec {
            axis: FrameAxis::Z,
            direction: direction_construction(),
        },
        AxisSpec {
            axis: FrameAxis::X,
            direction: process_constructions(UnitVector3::Y_AXIS).remove(1),
        },
    ]
}

pub(super) fn wcs_orientations() -> Vec<WcsOrientation> {
    let [primary, secondary] = <[AxisSpec; 2]>::try_from(axis_specs()).unwrap();
    vec![
        WcsOrientation::Model,
        WcsOrientation::Axes {
            primary: Box::new(primary),
            secondary: Box::new(secondary),
            flips: [Some(lit(true)), None, Some(dims(false).remove(2))],
        },
        WcsOrientation::Frame(frame_construction()),
        WcsOrientation::Native(text("f3d:view")),
    ]
}

pub(super) fn wcs_origins() -> Vec<WcsOrigin> {
    vec![
        WcsOrigin::ModelOrigin,
        WcsOrigin::FrameOrigin,
        WcsOrigin::StockPoint(box_point()),
        WcsOrigin::ModelPoint(box_point()),
        WcsOrigin::Point(point_constructions().remove(0)),
        WcsOrigin::Native(text("f3d:selected-point")),
    ]
}

pub(super) fn wcses() -> Vec<Wcs> {
    vec![
        Wcs {
            orientation: Some(constant(WcsOrientation::Model)),
            origin: Some(
                choices(WcsOrigin::ModelOrigin, WcsOrigin::StockPoint(box_point())).remove(1),
            ),
            placement: Some(Transform::identity()),
        },
        Wcs {
            orientation: None,
            origin: None,
            placement: None,
        },
    ]
}

pub(super) fn workpiece_setups() -> Vec<WorkpieceSetup> {
    vec![
        WorkpieceSetup {
            workpiece: selection(),
            placement: Some(Transform::identity()),
        },
        WorkpieceSetup {
            workpiece: selection(),
            placement: None,
        },
    ]
}

pub(super) fn setups() -> Vec<Setup> {
    let [wcs, open] = <[Wcs; 2]>::try_from(wcses()).unwrap();
    vec![
        Setup {
            id: setup_id(1),
            scope: id("product-definition", 1),
            name: Some("Setup1".to_owned()),
            job: Some(choices(JobKind::Milling, JobKind::Turning).remove(1)),
            axes: Some(AxisClass::Three),
            steps: steps(),
            workpieces: workpiece_setups(),
            fixtures: Some(selection()),
            stock: Some(constant(StockRule::Relative(box_offsets()))),
            wcs,
            work_offsets: vec![constant(WorkOffset::Number(count(1)))],
            machine: Some(machine_id(1)),
            native_ref: Some("setup-1".to_owned()),
        },
        Setup {
            id: setup_id(2),
            scope: id("product-definition", 1),
            name: None,
            job: None,
            axes: None,
            steps: Vec::new(),
            workpieces: Vec::new(),
            fixtures: None,
            stock: None,
            wcs: open,
            work_offsets: Vec::new(),
            machine: None,
            native_ref: None,
        },
    ]
}

pub(super) fn tool_axis_controls() -> Vec<ToolAxisControl> {
    let line = || workpiece_selections().remove(0);
    vec![
        ToolAxisControl::InPlane { plane: line() },
        ToolAxisControl::ThroughLine { line: line() },
        ToolAxisControl::NormalToSurface { surfaces: line() },
        ToolAxisControl::AlongRuling { surfaces: line() },
        ToolAxisControl::Native(text("f3d:lead-lag")),
    ]
}

pub(super) fn profile_curves() -> Vec<ProfileCurve> {
    let line = SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    );
    vec![
        ProfileCurve {
            geometry: Some(BoundedCurve {
                carrier: line.clone(),
                range: IncreasingParameterInterval::new([0.0, 2.5]),
            }),
            source: Some(selection()),
        },
        ProfileCurve {
            geometry: Some(BoundedCurve {
                carrier: line,
                range: IncreasingParameterInterval::new([-1.0, 1.0]),
            }),
            source: None,
        },
        ProfileCurve {
            geometry: None,
            source: Some(selection()),
        },
    ]
}

pub(super) fn geometry_sources() -> Vec<GeometrySource> {
    vec![
        GeometrySource::Selection(workpiece_selections()),
        GeometrySource::Points(point_constructions()),
        GeometrySource::Direction(direction_construction()),
        GeometrySource::Axis(axis_construction()),
        GeometrySource::Profile {
            plane: construction(Some(frame_value())),
            curves: profile_curves(),
        },
    ]
}

pub(super) fn process_geometries() -> Vec<ProcessGeometry> {
    vec![
        ProcessGeometry {
            role: GeometryRole::Machine,
            source: GeometrySource::Selection(workpiece_selections()),
            slot: Some(text("contours")),
            combine: Some(GeometryCombine::Extend),
        },
        ProcessGeometry {
            role: GeometryRole::Native(text("nx:cut-area")),
            source: GeometrySource::Axis(axis_construction()),
            slot: None,
            combine: None,
        },
    ]
}

pub(super) fn heights() -> Vec<Height> {
    vec![
        Height {
            from: Some(constant(HeightFrom::Height(HeightRole::Retract))),
            offset: Some(lit(length(5.0))),
        },
        Height {
            from: None,
            offset: None,
        },
    ]
}

pub(super) fn radius_froms() -> Vec<RadiusFrom> {
    vec![
        RadiusFrom::StockOuter,
        RadiusFrom::StockInner,
        RadiusFrom::SelectionMin(text("turning")),
        RadiusFrom::SelectionMax(text("turning")),
        RadiusFrom::Positions {
            slot: text("limits"),
            side: constant(AxialRole::Front),
        },
        RadiusFrom::Limit(RadiusRole::Outer),
        RadiusFrom::Native(text("f3d:tool-radius")),
    ]
}

pub(super) fn radial_limits() -> Vec<RadialLimit> {
    vec![
        RadialLimit::Absolute(RadialDimension::Diameter(lit(length(40.0)))),
        RadialLimit::Absolute(RadialDimension::Radius(lengths().remove(1))),
        RadialLimit::From {
            base: RadiusFrom::StockOuter,
            offset: Some(lit(length(2.0))),
        },
        RadialLimit::From {
            base: RadiusFrom::Limit(RadiusRole::Inner),
            offset: None,
        },
    ]
}

pub(super) fn axial_limits() -> Vec<AxialLimit> {
    vec![
        AxialLimit {
            base: constant(AxialFrom::StockFront),
            offset: Some(lit(length(1.0))),
            point: Some(constant(LimitPoint::CuttingEdge)),
        },
        AxialLimit {
            base: choices(
                AxialFrom::ModelBack,
                AxialFrom::Native(text("f3d:selection")),
            )
            .remove(2),
            offset: None,
            point: None,
        },
    ]
}

pub(super) fn leads() -> Vec<Lead> {
    vec![
        Lead {
            enabled: Some(lit(true)),
            arc_radius: Some(lit(length(2.0))),
            arc_sweep: Some(lit(angle(1.5))),
            line_length: Some(lit(length(1.0))),
            vertical_radius: Some(lengths().remove(1)),
            line_turn: Some(constant(LineTurn::Perpendicular {})),
            from_axis: Some(lit(false)),
            helical: Some(lit(true)),
        },
        Lead {
            enabled: None,
            arc_radius: None,
            arc_sweep: None,
            line_length: None,
            vertical_radius: None,
            line_turn: None,
            from_axis: None,
            helical: None,
        },
    ]
}

pub(super) fn line_turns() -> Vec<LineTurn> {
    vec![
        LineTurn::Tangent {},
        LineTurn::Perpendicular {},
        LineTurn::Angle {
            angle: lit(angle(0.5)),
            from: Some(constant(LeadAngleFrom::SpindleAxis)),
            along: Some(LeadAlong::Backward),
        },
        LineTurn::Angle {
            angle: dims(angle(0.5)).remove(1),
            from: None,
            along: None,
        },
    ]
}

pub(super) fn plunge_strategies() -> Vec<PlungeStrategy> {
    vec![
        PlungeStrategy {
            kind: Some(constant(PlungeKind::Helix)),
            angle: Some(lit(angle(0.05))),
            helix_diameter: Some(lit(length(8.0))),
            helix_taper: Some(lit(angle(0.0))),
            start_above: Some(lit(length(1.0))),
        },
        PlungeStrategy {
            kind: None,
            angle: None,
            helix_diameter: None,
            helix_taper: None,
            start_above: None,
        },
    ]
}

pub(super) fn pass_clearances() -> Vec<PassClearance> {
    vec![
        PassClearance {
            radial: Some(lit(length(0.5))),
            axial: Some(lit(length(0.5))),
        },
        PassClearance {
            radial: None,
            axial: None,
        },
    ]
}

pub(super) fn linkings() -> Vec<Linking> {
    vec![
        Linking {
            stay_down: Some(lit(true)),
            stay_down_distance: Some(lit(length(10.0))),
            lift: Some(lit(length(0.2))),
            retract: Some(constant(RetractPolicy::Minimum)),
            pull_out: Some(lit(length(1.0))),
            pass_clearance: Some(pass_clearances().remove(0)),
        },
        Linking {
            stay_down: None,
            stay_down_distance: None,
            lift: None,
            retract: None,
            pull_out: None,
            pass_clearance: None,
        },
    ]
}

pub(super) fn hole_depths() -> Vec<HoleDepth> {
    vec![
        HoleDepth {
            tip_through: Some(lit(true)),
            break_through: Some(lit(length(1.0))),
        },
        HoleDepth {
            tip_through: None,
            break_through: None,
        },
    ]
}

pub(super) fn cycle_settings() -> Vec<CycleSettings> {
    vec![
        CycleSettings {
            kind: Some(constant(CycleKind::Deep)),
            increment: Some(lit(length(2.0))),
            increment_reduction: Some(lit(length(0.2))),
            minimum_increment: Some(lit(length(0.5))),
            full_retract_after: Some(lit(length(6.0))),
            chip_break_retract: Some(lit(length(0.1))),
            dwell: Some(lit(duration(0.5))),
            depth: Some(hole_depths().remove(0)),
            probe_at: Some(constant(ProbeAt::Nominal)),
        },
        CycleSettings {
            kind: None,
            increment: None,
            increment_reduction: None,
            minimum_increment: None,
            full_retract_after: None,
            chip_break_retract: None,
            dwell: None,
            depth: None,
            probe_at: None,
        },
    ]
}

pub(super) fn feed_rates() -> Vec<FeedRate> {
    vec![
        FeedRate::PerTime(lit(feed(16.6))),
        FeedRate::PerRevolution(lit(length(0.1))),
        FeedRate::PerTooth(lengths().remove(1)),
    ]
}

pub(super) fn spindles() -> Vec<Spindle> {
    vec![
        Spindle::Rotation(lit(speed(1047.2))),
        Spindle::SurfaceSpeed {
            speed: lit(feed(3000.0)),
            limit: Some(lit(speed(400.0))),
        },
        Spindle::SurfaceSpeed {
            speed: dims(feed(3000.0)).remove(1),
            limit: None,
        },
    ]
}

pub(super) fn finishing_steps() -> FinishingSteps {
    FinishingSteps {
        count: lit(2),
        step: lit(length(0.25)),
    }
}

pub(super) fn tab_placements() -> Vec<TabPlacement> {
    vec![
        TabPlacement::PerContour(lit(4)),
        TabPlacement::AtPoints(point_constructions()),
        TabPlacement::Spacing(lit(length(50.0))),
        TabPlacement::Native(text("f3d:by-distance")),
    ]
}

pub(super) fn tabs() -> Tabs {
    Tabs {
        height: lit(length(2.0)),
        width: lit(length(5.0)),
        shape: constant(TabShape::Triangular),
        placement: TabPlacement::Spacing(lit(length(50.0))),
    }
}

pub(super) fn chamfer_cut() -> ChamferCut {
    ChamferCut {
        width: lit(length(0.5)),
        tip_offset: lit(length(0.2)),
    }
}

pub(super) fn technologies() -> Vec<Technology> {
    vec![full_technology(), empty_technology()]
}

pub(super) fn empty_technology() -> Technology {
    Technology {
        feeds: BTreeMap::new(),
        spindle: None,
        spindle_direction: None,
        coolant: None,
        stepover: None,
        stepdown: None,
        stock_to_leave_radial: None,
        stock_to_leave_axial: None,
        tolerance: None,
        cut_mode: None,
        stepdown_rule: None,
        stepover_rule: None,
        multiple_depths: None,
        finishing_stepdown: None,
        finishing_stepover: None,
        compensation: None,
        repeat_final_pass: None,
        closed_pass_overlap: None,
        corners: None,
        roughing_passes: None,
        finishing_passes: None,
        pass_transition: None,
        reduce_feed: None,
        pass_angle: None,
        roughing_pass_count: None,
        tabs: None,
        chamfer: None,
        jet_cut: None,
        rest: None,
        helix_rise: None,
        thread_hand: None,
        thread_depth: None,
        thread_infeed_angle: None,
        angular_stepover: None,
        axial_offset: None,
        intermediate_stepdown: None,
        stepdown_count: None,
        bottom_up: None,
        turning_side: None,
    }
}

pub(super) fn full_technology() -> Technology {
    Technology {
        feeds: BTreeMap::from([
            (FeedRole::Cutting, FeedRate::PerTime(lit(feed(16.6)))),
            (FeedRole::Plunge, FeedRate::PerTooth(lit(length(0.05)))),
            (
                FeedRole::Native(text("f3d:feedPerRevolution")),
                FeedRate::PerRevolution(lit(length(0.1))),
            ),
        ]),
        spindle: Some(Spindle::Rotation(lit(speed(1047.2)))),
        spindle_direction: Some(constant(SpindleDirection::Clockwise)),
        coolant: Some(choices(Coolant::Flood, Coolant::Mist).remove(1)),
        stepover: Some(lit(length(4.0))),
        stepdown: Some(lit(length(1.0))),
        stock_to_leave_radial: Some(lit(length(0.2))),
        stock_to_leave_axial: Some(lit(length(0.1))),
        tolerance: Some(lit(length(0.01))),
        cut_mode: Some(constant(CutMode::Climb)),
        stepdown_rule: Some(constant(StepRule::Even)),
        stepover_rule: Some(constant(StepRule::ConstantEngagement)),
        multiple_depths: Some(lit(true)),
        finishing_stepdown: Some(finishing_steps()),
        finishing_stepover: Some(finishing_steps()),
        compensation: Some(constant(Compensation::Left)),
        repeat_final_pass: Some(lit(false)),
        closed_pass_overlap: Some(lit(length(0.5))),
        corners: Some(constant(CornerMode::Rounded)),
        roughing_passes: Some(lit(true)),
        finishing_passes: Some(lit(true)),
        pass_transition: Some(constant(PassTransition::Arc)),
        reduce_feed: Some(lit(false)),
        pass_angle: Some(lit(angle(0.785))),
        roughing_pass_count: Some(lit(3)),
        tabs: Some(tabs()),
        chamfer: Some(chamfer_cut()),
        jet_cut: Some(constant(JetCut::Through)),
        rest: Some(constant(RestSource::EarlierOperations)),
        helix_rise: Some(constant(HelixRise::Pitch(lit(length(1.5))))),
        thread_hand: Some(constant(Hand::Right)),
        thread_depth: Some(RadialDimension::Radius(lit(length(0.9)))),
        thread_infeed_angle: Some(lit(angle(0.5))),
        angular_stepover: Some(lit(angle(0.1))),
        axial_offset: Some(lit(length(0.3))),
        intermediate_stepdown: Some(lit(length(0.5))),
        stepdown_count: Some(lit(4)),
        bottom_up: Some(lit(true)),
        turning_side: Some(constant(TurningSide::Outside)),
    }
}

pub(super) fn operations() -> Vec<Operation> {
    vec![
        Operation {
            id: operation_id(1),
            name: Some("Adaptive1".to_owned()),
            comment: Some("roughing".to_owned()),
            kind: OperationKind::Machining {
                class: OperationClass::Milling(MillingClass::BottomAndSide),
                phase: Some(Phase::Rough),
            },
            native_kind: native_name("adaptive"),
            tooling: Some(Tooling::Preset(preset_id(1))),
            features: vec![feature_id(1)],
            groups: vec![group_id(1)],
            geometry: process_geometries(),
            heights: BTreeMap::from([
                (HeightRole::Clearance, heights().remove(0)),
                (HeightRole::Bottom, heights().remove(1)),
            ]),
            radii: BTreeMap::from([(
                RadiusRole::OuterClearance,
                constant(radial_limits().remove(2)),
            )]),
            axial: BTreeMap::from([(AxialRole::Front, axial_limits().remove(0))]),
            technology: full_technology(),
            tool_frame: Some(wcses().remove(0)),
            tool_axis: Some(tool_axis_controls().remove(0)),
            leads: Some(Leads {
                entry: Some(leads().remove(0)),
                exit: None,
            }),
            plunge: Some(plunge_strategies().remove(0)),
            linking: Some(linkings().remove(0)),
            cycle: Some(cycle_settings().remove(0)),
            suppressed: Some(lit(false)),
            configuration_states: BTreeMap::from([
                (id("configuration", 1), true),
                (id("configuration", 2), false),
            ]),
            optional: Some(dims(true).remove(1)),
            native_ref: Some("operation-1".to_owned()),
        },
        Operation {
            id: operation_id(2),
            name: None,
            comment: None,
            kind: OperationKind::Modifier {
                modifier: ModifierKind::Dogbone,
                inputs: vec![operation_id(1)],
            },
            native_kind: native_name("dressup-dogbone"),
            tooling: None,
            features: Vec::new(),
            groups: Vec::new(),
            geometry: Vec::new(),
            heights: BTreeMap::new(),
            radii: BTreeMap::new(),
            axial: BTreeMap::new(),
            technology: empty_technology(),
            tool_frame: None,
            tool_axis: None,
            leads: None,
            plunge: None,
            linking: None,
            cycle: None,
            suppressed: None,
            configuration_states: BTreeMap::new(),
            optional: None,
            native_ref: None,
        },
    ]
}

pub(super) fn machining_features() -> Vec<MachiningFeature> {
    vec![
        MachiningFeature {
            id: feature_id(1),
            name: Some("Pocket1".to_owned()),
            class: MachiningFeatureClass::ClosedPocket {},
            native_kind: native_name("pocket"),
            geometry: process_geometries(),
            native_ref: Some("feature-1".to_owned()),
        },
        MachiningFeature {
            id: feature_id(2),
            name: None,
            class: MachiningFeatureClass::Compound {
                members: vec![feature_id(1), feature_id(3)],
            },
            native_kind: native_name("compound"),
            geometry: Vec::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn nc_numbers() -> Vec<NcNumbers> {
    vec![
        NcNumbers {
            number: Some(lit(1)),
            length_offset: Some(lit(1)),
            diameter_offset: Some(dims(1).remove(1)),
            turret: Some(lit(2)),
        },
        NcNumbers {
            number: None,
            length_offset: None,
            diameter_offset: None,
            turret: None,
        },
    ]
}

pub(super) fn cutters() -> Vec<Cutter> {
    vec![
        Cutter {
            diameter: Some(lit(length(6.0))),
            corner_radius: Some(lit(length(0.5))),
            flute_length: Some(lit(length(20.0))),
            overall_length: Some(lit(length(60.0))),
            shank_diameter: Some(lit(length(6.0))),
            tip_diameter: Some(lit(length(0.0))),
            thread_pitch: Some(lit(length(1.0))),
            tip_angle: Some(lit(angle(2.36))),
            taper_angle: Some(lit(angle(0.0))),
            flutes: Some(lit(3)),
            hand: Some(Hand::Right),
        },
        Cutter {
            diameter: None,
            corner_radius: None,
            flute_length: None,
            overall_length: None,
            shank_diameter: None,
            tip_diameter: None,
            thread_pitch: None,
            tip_angle: None,
            taper_angle: None,
            flutes: None,
            hand: None,
        },
    ]
}

pub(super) fn turning_inserts() -> Vec<TurningInsert> {
    vec![
        TurningInsert {
            shape: Some('C'),
            relief: Some('N'),
            tolerance: Some('M'),
            cross_section: Some('G'),
            inscribed_diameter: Some(lit(length(12.7))),
            thickness: Some(lit(length(4.76))),
            corner_radius: Some(lit(length(0.8))),
            width: Some(lit(length(3.0))),
            tip_angle: Some(lit(angle(1.396))),
            thread_pitch: Some(lit(length(1.5))),
            relief_angle: Some(lit(angle(0.0))),
        },
        TurningInsert {
            shape: None,
            relief: None,
            tolerance: None,
            cross_section: None,
            inscribed_diameter: None,
            thickness: None,
            corner_radius: None,
            width: None,
            tip_angle: None,
            thread_pitch: None,
            relief_angle: None,
        },
    ]
}

pub(super) fn holder_profile() -> HolderProfile {
    HolderProfile {
        sections: vec![HolderSection {
            upper_diameter: length(40.0),
            lower_diameter: length(30.0),
            length: length(25.0),
        }],
    }
}

pub(super) fn tools() -> Vec<Tool> {
    let [full_nc, empty_nc] = <[NcNumbers; 2]>::try_from(nc_numbers()).unwrap();
    let [full_cutter, empty_cutter] = <[Cutter; 2]>::try_from(cutters()).unwrap();
    vec![
        Tool {
            id: tool_id(1),
            name: Some("6mm flat".to_owned()),
            class: ToolClass::FlatEndmill,
            native_kind: native_name("flat end mill"),
            library: Some(LibraryRef::Guid(uuid())),
            unit: Some(UnitSymbol::In),
            nc: full_nc,
            cutter: full_cutter,
            insert: Some(turning_inserts().remove(0)),
            holder: Some(holder_profile()),
            material: Some(text("carbide")),
            chip_load: Some(lit(length(0.05))),
            catalog: Some(ToolCatalog {
                vendor: Some(text("Vendor")),
                product_id: Some(text("V-6")),
                link: Some(text("catalog page")),
            }),
            native_ref: Some("tool-1".to_owned()),
        },
        Tool {
            id: tool_id(2),
            name: None,
            class: ToolClass::Unclassified,
            native_kind: native_name("custom"),
            library: None,
            unit: None,
            nc: empty_nc,
            cutter: empty_cutter,
            insert: None,
            holder: None,
            material: None,
            chip_load: None,
            catalog: None,
            native_ref: None,
        },
    ]
}

pub(super) fn presets() -> Vec<Preset> {
    let [full_nc, empty_nc] = <[NcNumbers; 2]>::try_from(nc_numbers()).unwrap();
    vec![
        Preset {
            id: preset_id(1),
            tool: tool_id(1),
            name: Some("Aluminium".to_owned()),
            nc: full_nc,
            technology: full_technology(),
            native_ref: Some("preset-1".to_owned()),
        },
        Preset {
            id: preset_id(2),
            tool: tool_id(1),
            name: None,
            nc: empty_nc,
            technology: empty_technology(),
            native_ref: None,
        },
    ]
}

pub(super) fn machines() -> Vec<Machine> {
    vec![
        Machine {
            id: machine_id(1),
            name: Some("Mill".to_owned()),
            job: Some(JobKind::Milling),
            vendor: Some(text("Vendor")),
            model: Some(text("VF-2")),
            control: Some(text("NGC")),
            linear_axes: Some(BTreeSet::from([FrameAxis::X, FrameAxis::Y, FrameAxis::Z])),
            rotary_axes: Some(RotaryAxes::Present {
                count: Some(count(2)),
            }),
            pockets: vec![ToolPocket {
                tool: tool_id(1),
                nc: nc_numbers().remove(0),
            }],
            native_ref: Some("machine-1".to_owned()),
        },
        Machine {
            id: machine_id(2),
            name: None,
            job: None,
            vendor: None,
            model: None,
            control: None,
            linear_axes: None,
            rotary_axes: None,
            pockets: Vec::new(),
            native_ref: None,
        },
    ]
}

pub(super) fn process_groups() -> Vec<ProcessGroup> {
    vec![
        ProcessGroup {
            id: group_id(1),
            kind: GroupKind::Geometry {
                geometry: process_geometries(),
            },
            name: Some("WORKPIECE".to_owned()),
            parent: Some(group_id(2)),
            native_ref: Some("group-1".to_owned()),
        },
        ProcessGroup {
            id: group_id(2),
            kind: GroupKind::Method,
            name: None,
            parent: None,
            native_ref: None,
        },
    ]
}

pub(super) fn motion_values() -> Vec<MotionValue> {
    vec![
        MotionValue::Integer(-1),
        MotionValue::Real(real(0.25)),
        MotionValue::Text("contour".to_owned()),
        MotionValue::Point(point(1.0, 2.0, 3.0)),
    ]
}

pub(super) fn probe_targets() -> Vec<ProbeTarget> {
    vec![
        ProbeTarget::Face {
            axis: FrameAxis::Z,
            reversed: true,
        },
        ProbeTarget::Wall {
            axis: FrameAxis::X,
            width: length(20.0),
        },
        ProbeTarget::Channel {
            axis: FrameAxis::Y,
            width: length(10.0),
            island: false,
        },
        ProbeTarget::RectangularPocket {
            widths: [length(30.0), length(20.0)],
            island: true,
        },
        ProbeTarget::RectangularBoss {
            widths: [length(30.0), length(20.0)],
        },
        ProbeTarget::CircularPocket {
            diameter: length(25.0),
            island: false,
        },
        ProbeTarget::CircularBoss {
            diameter: length(25.0),
        },
        ProbeTarget::Corner {
            inner: true,
            reversed: [false, true],
        },
        ProbeTarget::FaceAngle {
            axis: PlaneAxis::X,
            reversed: false,
            spacing: length(20.0),
            nominal: Some(angle(1.2)),
        },
        ProbeTarget::FaceAngle {
            axis: PlaneAxis::Y,
            reversed: true,
            spacing: length(20.0),
            nominal: None,
        },
        ProbeTarget::PartialCircularPocket {
            diameter: length(40.0),
            island: true,
            angles: [angle(0.0), angle(2.0), angle(4.0)],
        },
        ProbeTarget::PartialCircularBoss {
            diameter: length(40.0),
            angles: [angle(0.0), angle(2.0), angle(4.0)],
        },
        ProbeTarget::Native(text("f3d:probing-xy-pcd-hole")),
    ]
}

pub(super) fn cycle_kinds() -> Vec<CycleKind> {
    vec![
        CycleKind::Drill,
        CycleKind::Deep,
        CycleKind::ChipBreak,
        CycleKind::Bore,
        CycleKind::Ream,
        CycleKind::Tap,
        CycleKind::Groove,
        CycleKind::Probe(probe_targets().remove(0)),
        CycleKind::Native(text("f3d:gun-drilling")),
    ]
}

pub(super) fn cycles() -> Vec<Cycle> {
    vec![
        Cycle {
            kind: CycleKind::ChipBreak,
            at: point(10.0, 20.0, -10.0),
            clearance: Some(length(15.0)),
            retract: Some(length(5.0)),
            stock: Some(length(0.0)),
            depth: Some(length(10.0)),
            feeds: BTreeMap::from([
                (FeedRole::Plunge, feed(5.0)),
                (FeedRole::Retract, feed(20.0)),
            ]),
            dwell: Some(duration(0.5)),
            increment: Some(length(2.0)),
            increment_reduction: Some(length(0.2)),
            minimum_increment: Some(length(0.5)),
            full_retract_after: Some(length(6.0)),
            chip_break_retract: Some(length(0.1)),
            return_to: Some(CycleReturn::Retract),
            probe_clearance: Some(length(5.0)),
            probe_overtravel: Some(length(2.0)),
            parameters: BTreeMap::from([(text("accumulatedDepth"), MotionValue::Real(real(6.0)))]),
            expansion: Some(vec![Motion::Rapid {
                to: point(10.0, 20.0, 5.0),
                axis: None,
                feed: None,
            }]),
        },
        Cycle {
            kind: CycleKind::Drill,
            at: point(-15.0, -10.0, 12.0),
            clearance: None,
            retract: Some(length(22.0)),
            stock: None,
            depth: None,
            feeds: BTreeMap::new(),
            dwell: None,
            increment: None,
            increment_reduction: None,
            minimum_increment: None,
            full_retract_after: None,
            chip_break_retract: None,
            return_to: Some(CycleReturn::Initial),
            probe_clearance: None,
            probe_overtravel: None,
            parameters: BTreeMap::new(),
            expansion: None,
        },
    ]
}

pub(super) fn motions() -> Vec<Motion> {
    vec![
        Motion::Rapid {
            to: point(-15.0, -10.0, 24.0),
            axis: None,
            feed: Some(feed(50.0)),
        },
        Motion::Linear {
            to: point(0.0, 0.0, -1.0),
            axis: Some(UnitVector3::Z_AXIS),
            feed: Some(feed(16.6)),
        },
        Motion::Linear {
            to: point(1.0, 0.0, -1.0),
            axis: None,
            feed: None,
        },
        Motion::Arc {
            to: point(2.0, 1.0, -1.0),
            center: point(1.0, 1.0, 0.0),
            normal: UnitVector3::Z_AXIS,
            clockwise: true,
            feed: None,
        },
        Motion::Class(MotionClass {
            class: MovementClass::Link,
            native: Some(text("f3d:5")),
        }),
        Motion::Cycle(Box::new(cycles().remove(1))),
        Motion::ToolChange { number: Some(3) },
        Motion::ToolChange { number: None },
        Motion::Spindle {
            direction: SpindleDirection::CounterClockwise,
            speed: Some(speed(500.0)),
        },
        Motion::Spindle {
            direction: SpindleDirection::Off,
            speed: None,
        },
        Motion::Coolant(Coolant::Flood),
        Motion::Dwell(duration(1.0)),
        Motion::Comment("Pocket1".to_owned()),
        Motion::Attribute {
            key: text("level"),
            value: MotionValue::Integer(2),
        },
        Motion::Temperature {
            heater: Heater::Extruder { index: Some(0) },
            celsius: real(210.0),
            wait: true,
        },
        Motion::Fan {
            speed: Fraction::new(0.5).unwrap(),
        },
        Motion::Pierce,
        Motion::Native(text("G0 Z24")),
    ]
}
