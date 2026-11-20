// SPDX-License-Identifier: Apache-2.0
//! A process document that breaks no part 10 rule, and helpers that build
//! and check variations of it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use cadmpeg_core::text::NonBlankString;

use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::evaluation::{EvaluationStatus, StateBase, StateDelta};
use crate::design::names::{CodecId, NativeName, NativeNameValue};
use crate::design::parameter::ParameterValue;
use crate::design::selection::{Anchor, OccurrencePath, PathStep, Picks, Selection, StepRef};
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::process::feature::{MachiningFeature, MachiningFeatureClass};
use crate::process::groups::{
    GroupKind, ProcessDefinition, ProcessGroup, ProcessOwner, ProcessParameter,
};
use crate::process::limits::{Height, HeightFrom, HeightRole};
use crate::process::operation::{
    DrillingClass, GeometryRole, GeometrySource, MillingClass, ModifierKind, Operation,
    OperationClass, OperationKind, ProcessGeometry, Tooling,
};
use crate::process::settings::{CycleSettings, HoleDepth};
use crate::process::setup::{Program, ProgramOutput, ProgramStep, Setup, Step, WorkpieceSetup};
use crate::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};
use crate::process::stock::Wcs;
use crate::process::technology::Technology;
use crate::process::tool::{Cutter, Machine, NcNumbers, Preset, Tool, ToolClass, ToolPocket};
use crate::process::toolpath::{Motion, Toolpath, ToolpathContent};
use crate::process::{Choice, ProcessDimension, WorkpieceSelection};
use crate::products::Occurrence;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::scalar::FiniteReal;
use crate::transform::Transform;
use crate::validate::design::fixture as design;

pub(super) fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:process:{kind}#{key}")).unwrap()
}

pub(super) fn text(value: &str) -> NonBlankString {
    NonBlankString::new(value).unwrap()
}

pub(super) fn native(value: &str) -> NativeName {
    NativeName {
        codec: CodecId::from(text("f3d")),
        value: NativeNameValue::Text(text(value)),
    }
}

pub(super) fn constant<T>(value: T) -> Choice<T> {
    Choice {
        value: Some(value),
        parameters: BTreeSet::new(),
    }
}

pub(super) fn lit<T>(value: T) -> ProcessDimension<T> {
    Dimension::Literal(value)
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

/// A selection of the setup's scope whose members the decoder did not read.
pub(super) fn unread<O: crate::design::selection::MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Current,
        picks: Picks::Unread,
        native: None,
    }
}

pub(super) fn workpiece_selection(workpiece: Option<u32>) -> WorkpieceSelection {
    WorkpieceSelection {
        workpiece,
        selection: unread(),
    }
}

/// A machining operation with no settings.
pub(super) fn machining(key: &str, class: OperationClass) -> Operation {
    Operation {
        id: id("operation", key),
        name: None,
        comment: None,
        kind: OperationKind::Machining { class, phase: None },
        native_kind: native(key),
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
    }
}

pub(super) fn tool(key: &str, class: ToolClass) -> Tool {
    Tool {
        id: id("tool", key),
        name: None,
        class,
        native_kind: native(key),
        library: None,
        unit: None,
        nc: no_numbers(),
        cutter: Cutter {
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
        insert: None,
        holder: None,
        material: None,
        chip_load: None,
        catalog: None,
        native_ref: None,
    }
}

fn no_numbers() -> NcNumbers {
    NcNumbers {
        number: None,
        length_offset: None,
        diameter_offset: None,
        turret: None,
    }
}

pub(super) fn parameter(key: &str, owner: ProcessOwner) -> ProcessParameter {
    ProcessParameter {
        id: id("parameter", key),
        owner,
        name: text(key),
        kind: Some(ValueKind::Quantity(Quantity::LENGTH)),
        definition: ProcessDefinition::Computed,
        value: Some(ParameterValue::Number(FiniteReal::new(2.0).unwrap())),
        native_ref: None,
    }
}

pub(super) fn state(key: &str, of: ProcessStateOf, base: ProcessBase) -> ProcessState {
    ProcessState {
        id: id("state", key),
        of,
        configuration: None,
        status: EvaluationStatus::Current,
        content: Some(ProcessContent {
            base,
            delta: StateDelta::default(),
        }),
        feature: None,
    }
}

/// The design fixture with one setup that machines its part: a milling
/// operation, a drilling operation in a folder, and a modifier of the
/// drilling operation; a tool, its preset, and a machine holding it; a
/// machining feature, a method group, a program that outputs the setup, a
/// parameter, a toolpath, and the setup's stock and the state after the
/// milling operation.
pub(super) fn document() -> CadIr {
    let mut ir = design::document();
    let process = &mut ir.process;
    process.setups.push(Setup {
        id: id("setup", "1"),
        scope: design::id("definition", "part"),
        name: None,
        job: None,
        axes: None,
        steps: vec![
            Step::Operation(id("operation", "mill")),
            Step::Folder {
                name: Some("holes".to_owned()),
                steps: vec![Step::Operation(id("operation", "dogbone"))],
            },
        ],
        workpieces: vec![WorkpieceSetup {
            workpiece: unread(),
            placement: None,
        }],
        fixtures: None,
        stock: None,
        wcs: Wcs {
            orientation: None,
            origin: None,
            placement: None,
        },
        work_offsets: Vec::new(),
        machine: Some(id("machine", "1")),
        native_ref: None,
    });
    let mut mill = machining("mill", OperationClass::Milling(MillingClass::Side));
    mill.tooling = Some(Tooling::Preset(id("preset", "1")));
    mill.features = vec![id("feature", "hole")];
    mill.groups = vec![id("group", "method")];
    mill.geometry = vec![ProcessGeometry {
        role: GeometryRole::Machine,
        source: GeometrySource::Selection(vec![workpiece_selection(Some(0))]),
        slot: None,
        combine: None,
    }];
    mill.heights = BTreeMap::from([(
        HeightRole::Bottom,
        Height {
            from: Some(constant(HeightFrom::Geometry)),
            offset: Some(Dimension::Parameter(
                crate::design::dimension::ParamRef::Process(id("parameter", "depth")),
            )),
        },
    )]);
    let mut drill = machining("drill", OperationClass::Drilling(DrillingClass::Drilling));
    drill.tooling = Some(Tooling::Tool(id("tool", "1")));
    drill.heights = BTreeMap::from([(
        HeightRole::Bottom,
        Height {
            from: Some(constant(HeightFrom::Wcs)),
            offset: None,
        },
    )]);
    drill.cycle = Some(CycleSettings {
        kind: None,
        increment: None,
        increment_reduction: None,
        minimum_increment: None,
        full_retract_after: None,
        chip_break_retract: None,
        dwell: None,
        depth: Some(HoleDepth {
            tip_through: Some(lit(true)),
            break_through: None,
        }),
        probe_at: None,
    });
    let mut dogbone = machining("dogbone", OperationClass::Unclassified);
    dogbone.kind = OperationKind::Modifier {
        modifier: ModifierKind::Dogbone,
        inputs: vec![id("operation", "drill")],
    };
    process.operations = vec![dogbone, drill, mill];
    process.features.push(MachiningFeature {
        id: id("feature", "hole"),
        name: None,
        class: MachiningFeatureClass::RoundHole {},
        native_kind: native("hole"),
        geometry: Vec::new(),
        native_ref: None,
    });
    process.groups.push(ProcessGroup {
        id: id("group", "method"),
        kind: GroupKind::Method,
        name: None,
        parent: None,
        native_ref: None,
    });
    process.tools.push(tool("1", ToolClass::FlatEndmill));
    process.presets.push(Preset {
        id: id("preset", "1"),
        tool: id("tool", "1"),
        name: None,
        nc: no_numbers(),
        technology: empty_technology(),
        native_ref: None,
    });
    process.machines.push(Machine {
        id: id("machine", "1"),
        name: None,
        job: None,
        vendor: None,
        model: None,
        control: None,
        linear_axes: None,
        rotary_axes: None,
        pockets: vec![ToolPocket {
            tool: id("tool", "1"),
            nc: no_numbers(),
        }],
        native_ref: None,
    });
    process.programs.push(Program {
        id: id("program", "1"),
        name: None,
        steps: vec![ProgramStep::Setup(id("setup", "1"))],
        post: None,
        machine: Some(id("machine", "1")),
        output: ProgramOutput {
            filename: None,
            folder: None,
            extension: None,
            unit: None,
        },
        native_ref: None,
    });
    process.parameters.push(parameter(
        "depth",
        ProcessOwner::Operation(id("operation", "mill")),
    ));
    process.toolpaths.push(Toolpath {
        id: id("toolpath", "mill"),
        operation: id("operation", "mill"),
        feature: None,
        configuration: None,
        status: EvaluationStatus::Current,
        content: ToolpathContent::Motions {
            frame: Transform::default(),
            motions: Vec::<Motion>::new(),
        },
    });
    process.states = vec![
        state(
            "after-mill",
            ProcessStateOf::After(id("operation", "mill")),
            ProcessBase::State(id("state", "stock")),
        ),
        state(
            "stock",
            ProcessStateOf::Stock(id("setup", "1")),
            ProcessBase::Design(StateBase::Current(design::after("fillet"))),
        ),
    ];
    ir
}

/// Places a `pin` definition in the fixture's part through occurrence `pin`,
/// with an occurrence `nested` of the pin inside the pin, and adds a design
/// length parameter `width`.
pub(super) fn place_pin(ir: &mut CadIr) {
    let mut pin = ir.model.product_definitions[0].clone();
    pin.id = design::id("definition", "pin");
    pin.bodies.clear();
    pin.occurrences = vec![design::id("occurrence", "nested")];
    ir.model.product_definitions[0].occurrences = vec![design::id("occurrence", "pin")];
    ir.model.product_definitions.push(pin);
    let occurrence = |key: &str| Occurrence {
        id: design::id("occurrence", key),
        prototype: design::id("definition", "pin"),
        transform: None,
        suppressed: None,
        visible: None,
        representation: None,
        overrides: Vec::new(),
        name: None,
        native_ref: None,
    };
    ir.model.occurrences.push(occurrence("nested"));
    ir.model.occurrences.push(occurrence("pin"));
    ir.design.parameters.push(design::parameter(
        "width",
        ValueKind::Quantity(Quantity::LENGTH),
    ));
}

/// A reference to `place_pin`'s `width` through occurrence `occurrence`.
pub(super) fn width_through(occurrence: &str) -> ParamRef {
    ParamRef::Design(ParameterRef {
        target: RefTarget::Parameter(design::id("parameter", "width")),
        through: Some(OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(design::id("occurrence", occurrence)),
            configuration: None,
        }])),
    })
}

pub(super) fn process_findings(ir: &CadIr) -> Vec<Finding> {
    crate::validate::design::fixture::family_findings(ir, Check::Process, super::check_process)
}

#[track_caller]
pub(super) fn assert_no_process_finding(ir: &CadIr) {
    let findings = process_findings(ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[track_caller]
pub(super) fn assert_finding(ir: &CadIr, severity: Severity, fragment: &str) {
    let findings = process_findings(ir);
    assert!(
        findings
            .iter()
            .any(|finding| finding.severity == severity && finding.message.contains(fragment)),
        "no process {severity:?} containing {fragment:?} in {findings:#?}"
    );
}

#[track_caller]
pub(super) fn assert_error(ir: &CadIr, fragment: &str) {
    assert_finding(ir, Severity::Error, fragment);
}
