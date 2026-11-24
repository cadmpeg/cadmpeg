// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::design::configuration::ParameterOverride;
use crate::design::construction::Construction;
use crate::design::dimension::{Dimension, ParamRef};
use crate::design::ops::sketch::BoundedCurve;
use crate::design::parameter::ParameterValue;
use crate::design::refs::ConfigurationRef;
use crate::design::scope::OccurrenceDesign;
use crate::design::selection::{Members, Picks};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::features::ParameterId;
use crate::geometry::analytic::{CircleCurve, LineCurve};
use crate::geometry::InlineCurve;
use crate::ids::OccurrenceId;
use crate::math::{Point3, Vector3};
use crate::process::cycle::{Cycle, CycleKind, ProbeTarget};
use crate::process::limits::{
    AxialFrom, AxialLimit, AxialRole, Height, HeightFrom, HeightRole, RadialLimit, RadiusFrom,
    RadiusRole,
};
use crate::process::operation::{
    GeometryRole, GeometrySource, OperationClass, ProcessGeometry, ProfileCurve, ToolAxisControl,
    TurningClass,
};
use crate::process::settings::{
    Lead, LeadAngleFrom, Leads, LineTurn, Linking, PassClearance, PlungeKind, PlungeStrategy,
    ProbeAt,
};
use crate::process::stock::WcsOrigin;
use crate::process::technology::{JetCut, StepRule, TabPlacement, TabShape, Tabs};
use crate::process::toolpath::{Motion, ToolpathContent};
use crate::process::{Choice, ProcessDimension};
use crate::scalar::{Angle, FiniteReal, Length};
use crate::topology::IncreasingParameterInterval;
use crate::transform::Transform;
use crate::validate::design::fixture as design;
use crate::validate::process::fixture::process_findings;
use crate::validate::process::fixture::{
    assert_error, assert_no_process_finding, constant, document, id, lit, place_pin, text,
    width_through, workpiece_selection,
};

fn length(value: f64) -> Length {
    Length::new(value).unwrap()
}

fn turning(ir: &mut crate::document::CadIr) {
    ir.process.operations[2].kind = crate::process::operation::OperationKind::Machining {
        class: OperationClass::Turning(TurningClass::Contouring),
        phase: None,
    };
}

#[test]
fn turning_settings_are_stated_only_on_turning_operations() {
    let mut ir = document();
    ir.process.operations[2].axial = BTreeMap::from([(
        AxialRole::Front,
        AxialLimit {
            base: constant(AxialFrom::ModelFront),
            offset: None,
            point: None,
        },
    )]);
    assert_error(
        &ir,
        "operation states `axial`, which its class does not admit",
    );
    turning(&mut ir);
    assert_no_process_finding(&ir);

    let mut ir = document();
    ir.process.operations[2].technology.thread_hand =
        Some(constant(crate::design::options::Hand::Right));
    assert_error(
        &ir,
        "`technology.thread_hand`, which its class does not admit",
    );
}

fn radius(base: RadiusFrom) -> crate::process::Choice<RadialLimit> {
    constant(RadialLimit::From { base, offset: None })
}

fn reference(slot: &str, source: GeometrySource) -> ProcessGeometry {
    ProcessGeometry {
        role: GeometryRole::Reference,
        source,
        slot: Some(text(slot)),
        combine: None,
    }
}

#[test]
fn radial_limits_name_stated_roles_and_reference_slots() {
    let mut ir = document();
    turning(&mut ir);
    ir.process.operations[2].radii = BTreeMap::from([(
        RadiusRole::Outer,
        radius(RadiusFrom::Limit(RadiusRole::Outer)),
    )]);
    assert_error(&ir, "which is its own or not stated");

    let mut ir = document();
    turning(&mut ir);
    ir.process.operations[2].radii = BTreeMap::from([
        (
            RadiusRole::OuterClearance,
            radius(RadiusFrom::Limit(RadiusRole::OuterRetract)),
        ),
        (
            RadiusRole::OuterRetract,
            radius(RadiusFrom::Limit(RadiusRole::OuterClearance)),
        ),
    ]);
    assert_error(&ir, "reaches itself through `limit` bases");

    let mut ir = document();
    turning(&mut ir);
    ir.process.operations[2].radii = BTreeMap::from([(
        RadiusRole::Outer,
        radius(RadiusFrom::SelectionMax(text("sel"))),
    )]);
    assert_error(&ir, "slot `sel` names no geometry of the operation");
    ir.process.operations[2]
        .geometry
        .push(reference("sel", GeometrySource::Selection(Vec::new())));
    assert_error(
        &ir,
        "slot `sel` names geometry that holds none of its a selection",
    );
    ir.process.operations[2].geometry[1].source =
        GeometrySource::Selection(vec![workpiece_selection(Some(0))]);
    assert_no_process_finding(&ir);
    ir.process.operations[2].geometry[1].role = GeometryRole::Boundary;
    assert_error(&ir, "whose role is not `reference`");
    let mut duplicate = ir.process.operations[2].geometry[1].clone();
    duplicate.role = GeometryRole::Reference;
    ir.process.operations[2].geometry.push(duplicate);
    assert_error(&ir, "slot `sel` names two geometries of the operation");

    let mut ir = document();
    turning(&mut ir);
    let positions = || {
        radius(RadiusFrom::Positions {
            slot: text("grooves"),
            side: constant(AxialRole::Front),
        })
    };
    ir.process.operations[2].geometry.push(reference(
        "grooves",
        GeometrySource::Selection(vec![workpiece_selection(Some(0))]),
    ));
    ir.process.operations[2].radii = BTreeMap::from([
        (RadiusRole::Inner, positions()),
        (RadiusRole::OuterClearance, positions()),
        (
            RadiusRole::OuterRetract,
            radius(RadiusFrom::Limit(RadiusRole::Inner)),
        ),
    ]);
    assert_error(&ir, "it is the base of at most one");
    assert_error(&ir, "which only `inner` or `outer` is");
    assert_error(&ir, "whose base is `positions`");
    assert_error(&ir, "slot `grooves` names geometry that holds no points");
}

#[test]
fn heights_measured_from_geometry_need_it() {
    let mut ir = document();
    ir.process.operations[2].geometry[0].role = GeometryRole::Check;
    assert_error(&ir, "which it does not hold");

    let mut ir = document();
    ir.process.operations[2].heights.insert(
        HeightRole::Top,
        Height {
            from: Some(constant(HeightFrom::Selection(text("top")))),
            offset: None,
        },
    );
    assert_error(&ir, "slot `top` names no geometry of the operation");
}

fn lead(turn: LineTurn, arc: Option<f64>) -> Lead {
    Lead {
        arc_radius: arc.map(|arc| lit(length(arc))),
        ..lead_on(turn, lit(length(0.0)))
    }
}

fn lead_on(turn: LineTurn, arc: ProcessDimension<Length>) -> Lead {
    Lead {
        enabled: None,
        arc_radius: Some(arc),
        arc_sweep: None,
        line_length: None,
        vertical_radius: None,
        line_turn: Some(constant(turn)),
        from_axis: None,
        helical: None,
    }
}

fn angle_turn(degrees: f64, from: Option<LeadAngleFrom>) -> LineTurn {
    LineTurn::Angle {
        angle: lit(Angle::new(degrees.to_radians()).unwrap()),
        from: from.map(constant),
        along: None,
    }
}

#[test]
fn lead_angles() {
    let mut ir = document();
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead(angle_turn(45.0, None), Some(0.0))),
        exit: None,
    });
    assert_no_process_finding(&ir);
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead(angle_turn(120.0, None), None)),
        exit: Some(lead(angle_turn(45.0, None), Some(2.0))),
    });
    assert_error(&ir, "a literal lead angle lies outside 0° to 90°");
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");

    let mut ir = document();
    ir.process.parameters[0].value = Some(ParameterValue::Number(FiniteReal::new(0.0).unwrap()));
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead_on(
            angle_turn(45.0, None),
            Dimension::Parameter(ParamRef::Process(id("parameter", "depth"))),
        )),
        exit: None,
    });
    assert_no_process_finding(&ir);
    ir.process.parameters[0].value = None;
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead_on(
            angle_turn(45.0, None),
            Dimension::Parameter(ParamRef::Process(id("parameter", "missing"))),
        )),
        exit: None,
    });
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");

    let mut ir = document();
    place_pin(&mut ir);
    if let Some(width) = ir.design.parameters.last_mut() {
        width.value = Some(ParameterValue::Number(FiniteReal::new(2.0).unwrap()));
    }
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead_on(
            angle_turn(45.0, None),
            Dimension::Parameter(width_through("pin")),
        )),
        exit: None,
    });
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");

    let mut ir = document();
    ir.process.operations[2].leads = Some(Leads {
        entry: Some(lead(
            angle_turn(45.0, Some(LeadAngleFrom::SpindleAxis)),
            None,
        )),
        exit: None,
    });
    assert_error(
        &ir,
        "measured from the spindle axis on an operation that is not turning",
    );
    turning(&mut ir);
    assert_no_process_finding(&ir);
}

/// A radius `through` occurrence `pin` reads the pin's own override of
/// `width`, else `width`'s cache in the configuration the pin uses; a lead
/// whose radius reads zero there has no arc.
#[test]
fn a_through_arc_radius_reads_the_occurrences_cache() {
    let number = |x: f64| Some(ParameterValue::Number(FiniteReal::new(x).unwrap()));
    let angled = |ir: &mut CadIr, width: f64| {
        place_pin(ir);
        if let Some(parameter) = ir.design.parameters.last_mut() {
            parameter.value = number(width);
        }
        ir.process.operations[2].leads = Some(Leads {
            entry: Some(lead_on(
                angle_turn(45.0, None),
                Dimension::Parameter(width_through("pin")),
            )),
            exit: None,
        });
    };
    let pin = || design::id::<OccurrenceId>("occurrence", "pin");
    let width = || design::id::<ParameterId>("parameter", "width");
    let override_width = |ir: &mut CadIr, value: f64| {
        let state = OccurrenceDesign {
            configuration: None,
            arrangements: Vec::new(),
            grounded: None,
            flexible: None,
            parameters: BTreeMap::from([(
                width(),
                ParameterOverride {
                    expression: None,
                    value: number(value),
                },
            )]),
        };
        ir.design.scopes[0].occurrences.insert(pin(), state);
    };

    let mut ir = document();
    angled(&mut ir, 0.0);
    assert_no_process_finding(&ir);

    let mut ir = document();
    angled(&mut ir, 2.0);
    override_width(&mut ir, 0.0);
    assert_no_process_finding(&ir);
    override_width(&mut ir, 2.0);
    if let Some(parameter) = ir.design.parameters.last_mut() {
        parameter.value = number(0.0);
    }
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");

    let mut ir = document();
    angled(&mut ir, 2.0);
    let mut small = design::configuration("small", None);
    small.overrides.parameters.insert(
        width(),
        ParameterOverride {
            expression: None,
            value: number(0.0),
        },
    );
    ir.design.configurations.push(small);
    override_width(&mut ir, 0.0);
    if let Some(state) = ir.design.scopes[0].occurrences.get_mut(&pin()) {
        state.parameters.clear();
        state.configuration = Some(ConfigurationRef::Local(design::id(
            "configuration",
            "small",
        )));
    }
    assert_no_process_finding(&ir);
    if let Some(state) = ir.design.scopes[0].occurrences.get_mut(&pin()) {
        state.configuration = Some(ConfigurationRef::Named(text("large")));
    }
    assert_error(&ir, "a lead with an arc states a `LineTurn::Angle`");
}

#[test]
fn plunge_and_linking() {
    let mut ir = document();
    ir.process.operations[2].plunge = Some(PlungeStrategy {
        kind: Some(constant(PlungeKind::Ramp)),
        angle: None,
        helix_diameter: None,
        helix_taper: Some(lit(Angle::new(0.1).unwrap())),
        start_above: None,
    });
    assert_error(&ir, "on a plunge whose kind is not `helix`");

    let mut ir = document();
    ir.process.operations[2].linking = Some(Linking {
        stay_down: None,
        stay_down_distance: None,
        lift: Some(lit(length(-1.0))),
        retract: None,
        pull_out: None,
        pass_clearance: None,
    });
    assert_error(&ir, "a literal `linking.lift` is negative");

    let mut ir = document();
    turning(&mut ir);
    ir.process.operations[2].linking = Some(Linking {
        stay_down: None,
        stay_down_distance: None,
        lift: None,
        retract: None,
        pull_out: None,
        pass_clearance: Some(PassClearance {
            radial: None,
            axial: None,
        }),
    });
    assert_error(&ir, "`pass_clearance` states neither `radial` nor `axial`");
}

#[test]
fn cycles_belong_to_drilling_and_probing() {
    let mut ir = document();
    ir.process.operations[2].cycle = ir.process.operations[1].cycle.clone();
    assert_error(&ir, "which only a drilling or probing operation states");

    let mut ir = document();
    ir.process.operations[1].kind = crate::process::operation::OperationKind::Machining {
        class: OperationClass::Probing,
        phase: None,
    };
    assert_error(
        &ir,
        "a probing operation's `cycle` states `probe_at` and no other field",
    );
    if let Some(cycle) = &mut ir.process.operations[1].cycle {
        cycle.depth = None;
        cycle.probe_at = Some(constant(ProbeAt::Picked));
    }
    assert_no_process_finding(&ir);

    let mut ir = document();
    if let Some(cycle) = &mut ir.process.operations[1].cycle {
        cycle.probe_at = Some(constant(ProbeAt::Nominal));
    }
    assert_error(
        &ir,
        "`cycle.probe_at` is stated on an operation that is not probing",
    );

    let mut ir = document();
    if let Some(depth) = ir.process.operations[1]
        .cycle
        .as_mut()
        .and_then(|cycle| cycle.depth.as_mut())
    {
        depth.tip_through = None;
    }
    assert_error(&ir, "`cycle.depth` states no field");

    let mut ir = document();
    if let Some(depth) = ir.process.operations[1]
        .cycle
        .as_mut()
        .and_then(|cycle| cycle.depth.as_mut())
    {
        depth.break_through = Some(lit(length(-0.5)));
    }
    assert_error(&ir, "a literal `cycle.depth.break_through` is negative");

    let mut ir = document();
    ir.process.operations[1].heights.clear();
    assert_error(&ir, "the operation states no `bottom` height");
}

#[test]
fn a_tool_axis_control_picks_its_count() {
    let mut ir = document();
    let mut line = workpiece_selection(Some(0));
    line.selection.picks = Picks::Named(Members::default());
    ir.process.operations[2].tool_axis = Some(ToolAxisControl::ThroughLine { line: line.clone() });
    assert_error(&ir, "`through_line.line` picks 0 entities; it picks one");
    ir.process.operations[2].tool_axis = Some(ToolAxisControl::AlongRuling { surfaces: line });
    assert_error(&ir, "`along_ruling.surfaces` picks nothing");
}

#[test]
fn a_frame_origin_needs_a_frame_orientation() {
    let mut ir = document();
    ir.process.setups[0].wcs.origin = Some(constant(WcsOrigin::FrameOrigin));
    assert_error(
        &ir,
        "WCS origin `frame_origin` is stated without a `frame` orientation",
    );
    ir.process.setups[0].wcs.orientation = Some(Choice {
        value: None,
        parameters: BTreeSet::from([id("parameter", "depth")]),
    });
    assert_error(
        &ir,
        "WCS origin `frame_origin` is stated without a `frame` orientation",
    );
}

#[test]
fn technology_literals_and_rules() {
    let mut ir = document();
    let technology = &mut ir.process.presets[0].technology;
    technology.roughing_pass_count = Some(lit(0));
    technology.closed_pass_overlap = Some(lit(length(-1.0)));
    technology.tabs = Some(Tabs {
        height: lit(length(0.0)),
        width: lit(length(2.0)),
        shape: constant(TabShape::Rectangular),
        placement: TabPlacement::AtPoints(Vec::new()),
    });
    technology.stepdown_rule = Some(constant(StepRule::ConstantEngagement));
    technology.jet_cut = Some(constant(JetCut::Through));
    assert_error(&ir, "a literal `roughing_pass_count` is 0");
    assert_error(&ir, "a literal `closed_pass_overlap` is negative");
    assert_error(&ir, "a literal `tabs.height` is not positive");
    assert_error(&ir, "`at_points` tabs hold no point");
    assert_error(&ir, "which only `stepover_rule` states");
    assert_error(
        &ir,
        "`jet_cut` is stated where the technology's tool is no jet cutter",
    );
    ir.process.presets[0].technology = crate::validate::process::fixture::empty_technology();
    ir.process.presets[0].technology.stepover_rule = Some(constant(StepRule::ConstantEngagement));
    assert_no_process_finding(&ir);
}

fn cycle(kind: CycleKind) -> Cycle {
    Cycle {
        kind,
        at: FinitePoint3::new(Point3 {
            x: 0.0,
            y: 0.0,
            z: -5.0,
        })
        .unwrap(),
        clearance: None,
        retract: None,
        stock: None,
        depth: None,
        feeds: BTreeMap::new(),
        dwell: None,
        increment: None,
        increment_reduction: None,
        minimum_increment: None,
        full_retract_after: None,
        chip_break_retract: None,
        return_to: None,
        probe_clearance: None,
        probe_overtravel: None,
        parameters: BTreeMap::new(),
        expansion: None,
    }
}

#[test]
fn motion_cycles() {
    let mut ir = document();
    let mut probing = cycle(CycleKind::Drill);
    probing.probe_overtravel = Some(length(2.0));
    let narrow = cycle(CycleKind::Probe(ProbeTarget::CircularBoss {
        diameter: length(0.0),
    }));
    ir.process.toolpaths[0].content = ToolpathContent::Motions {
        frame: Transform::default(),
        motions: vec![
            Motion::Cycle(Box::new(probing)),
            Motion::Cycle(Box::new(narrow)),
        ],
    };
    assert_error(&ir, "states a probe clearance or overtravel");
    assert_error(
        &ir,
        "a probe target's width, diameter, or spacing is not positive",
    );
}

fn profile(carrier: InlineCurve, range: Option<[f64; 2]>) -> ProcessGeometry {
    ProcessGeometry {
        role: GeometryRole::Machine,
        source: GeometrySource::Profile {
            plane: Construction {
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: None,
                resolved_in: BTreeMap::new(),
            },
            curves: vec![ProfileCurve {
                geometry: Some(BoundedCurve {
                    carrier,
                    range: range.and_then(IncreasingParameterInterval::new),
                }),
                source: None,
            }],
        },
        slot: None,
        combine: None,
    }
}

fn line() -> InlineCurve {
    InlineCurve::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    )
}

const UNBOUNDED: &str = "a profile curve on an unbounded carrier states no `range`";

fn ranged(carrier: InlineCurve, range: Option<[f64; 2]>) -> bool {
    let mut ir = document();
    ir.process.operations[2]
        .geometry
        .push(profile(carrier, range));
    process_findings(&ir)
        .iter()
        .all(|finding| !finding.message.contains(UNBOUNDED))
}

/// Part 10: a profile curve on a line, a parabola, or a hyperbola states its
/// range; a circle's `None` is one full turn.
#[test]
fn a_profile_curve_on_an_unbounded_carrier_states_its_range() {
    assert!(!ranged(line(), None));
    assert!(ranged(line(), Some([0.0, 3.0])));
    let circle = InlineCurve::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    );
    assert!(ranged(circle, None));
    let placed = InlineCurve::Transformed {
        basis: Box::new(line()),
        transform: Transform::default(),
    };
    assert!(!ranged(placed, None));
}
