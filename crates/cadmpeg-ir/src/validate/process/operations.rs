// SPDX-License-Identifier: Apache-2.0
//! Operation settings (§7.2, §7.3, §7.5): the settings a class admits,
//! radial limits and the geometry slots they name, leads, plunge, cycles,
//! tool axis control, WCS rules, and the literal ranges of the technology.

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;

use super::records::ProcessRecords;
use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::names::NameKind;
use crate::design::parameter::ParameterValue;
use crate::design::selection::{Ordered, Picks, PrincipalEntity, Selection};
use crate::geometry::InlineCurve;
use crate::process::cycle::{Cycle, CycleKind, ProbeTarget};
use crate::process::limits::{HeightFrom, HeightRole, RadialLimit, RadiusFrom, RadiusRole};
use crate::process::operation::{
    DrillingClass, GeometryRole, GeometrySource, MillingClass, Operation, OperationClass,
    OperationKind, ToolAxisControl, Tooling, TurningClass,
};
use crate::process::settings::{LineTurn, PlungeKind};
use crate::process::stock::{Wcs, WcsOrientation, WcsOrigin};
use crate::process::technology::{StepRule, TabPlacement, Technology};
use crate::process::tool::ToolClass;
use crate::process::toolpath::{Motion, ToolpathContent};
use crate::process::Choice;
use crate::process::ProcessDimension;
use crate::report::check::Finding;
use crate::scalar::Length;
use crate::validate::design::ops::picks::{self, Class};

pub(super) fn check_operations(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let process = &records.ir.process;
    for setup in &process.setups {
        check_wcs(setup.id.as_str(), &setup.wcs, findings);
    }
    for operation in &process.operations {
        check_operation(records, operation, findings);
    }
    for preset in &process.presets {
        let tool = records
            .tools
            .get(preset.tool.as_str())
            .map(|tool| &tool.class);
        check_technology(preset.id.as_str(), &preset.technology, tool, findings);
    }
    for toolpath in &process.toolpaths {
        if let ToolpathContent::Motions { motions, .. } = &toolpath.content {
            check_motions(toolpath.id.as_str(), motions, findings);
        }
    }
}

/// A setting of the operation that only some classes admit.
#[derive(Clone, Copy)]
enum ClassField {
    Radii,
    Axial,
    PullOut,
    PassClearance,
    TurningSide,
    ThreadInfeedAngle,
    ThreadHand,
    ThreadDepth,
    HelixRise,
}

impl ClassField {
    const fn name(self) -> &'static str {
        match self {
            Self::Radii => "radii",
            Self::Axial => "axial",
            Self::PullOut => "linking.pull_out",
            Self::PassClearance => "linking.pass_clearance",
            Self::TurningSide => "technology.turning_side",
            Self::ThreadInfeedAngle => "technology.thread_infeed_angle",
            Self::ThreadHand => "technology.thread_hand",
            Self::ThreadDepth => "technology.thread_depth",
            Self::HelixRise => "technology.helix_rise",
        }
    }

    fn allows(self, class: Option<&OperationClass>) -> bool {
        use OperationClass as C;
        match self {
            Self::Radii | Self::Axial | Self::PullOut | Self::PassClearance | Self::TurningSide => {
                matches!(class, Some(C::Turning(_)))
            }
            Self::ThreadInfeedAngle => matches!(class, Some(C::Turning(TurningClass::Threading))),
            Self::ThreadHand | Self::ThreadDepth => matches!(
                class,
                Some(
                    C::ThreadMilling
                        | C::Turning(TurningClass::Threading)
                        | C::Drilling(DrillingClass::Tapping | DrillingClass::ThreadDrilling)
                )
            ),
            Self::HelixRise => matches!(
                class,
                Some(C::ThreadMilling | C::Milling(MillingClass::Side))
            ),
        }
    }
}

/// The cached number a process dimension states: its literal, or the cached
/// number of the parameter it binds, negated for `Negated`. A reference
/// `through` an occurrence path reads the cache of the occurrence the path
/// reaches (§5.13).
fn number(records: &ProcessRecords<'_>, dimension: &ProcessDimension<Length>) -> Option<f64> {
    let cached = |reference: &ParamRef| {
        let value = match reference {
            ParamRef::Process(id) => records.parameters.get(id.as_str())?.value.as_ref(),
            ParamRef::Design(ParameterRef {
                target: RefTarget::Parameter(id),
                through: None,
            }) => records.design.parameters.get(id.as_str())?.value.as_ref(),
            ParamRef::Design(ParameterRef {
                target: RefTarget::Parameter(id),
                through: Some(path),
            }) => records.ir.through_cache(id, path).ok()?,
            ParamRef::Design(_) => None,
        };
        match value {
            Some(ParameterValue::Number(x)) => Some(x.get()),
            _ => None,
        }
    };
    match dimension {
        Dimension::Literal(value) => Some(value.get()),
        Dimension::Parameter(reference) => cached(reference),
        Dimension::Negated(reference) => cached(reference).map(|x| -x),
    }
}

fn base(limit: &Choice<RadialLimit>) -> Option<&RadiusFrom> {
    match &limit.value {
        Some(RadialLimit::From { base, .. }) => Some(base),
        _ => None,
    }
}

/// What a limit or height that names a slot needs the slot's geometry to
/// hold.
#[derive(Clone, Copy, PartialEq)]
enum SlotSource {
    Selection,
    Points,
}

/// A slot names a `Reference` geometry of the operation whose source is a
/// `Selection`, or `Points` for `Positions`, holding at least one entry.
fn check_slot(operation: &Operation, slot: &str, wanted: SlotSource, findings: &mut Vec<Finding>) {
    let id = operation.id.as_str();
    let Some(geometry) = operation
        .geometry
        .iter()
        .find(|geometry| geometry.slot.as_ref().map(NonBlankString::as_str) == Some(slot))
    else {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.slot-names-no-geometry-operation",
                "slot `{slot}` names no geometry of the operation"
            ),
        );
        return;
    };
    if geometry.role != GeometryRole::Reference {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.slot-names-geometry-role-not-reference",
                "slot `{slot}` names geometry whose role is not `reference`"
            ),
        );
    }
    let (source, empty) = match &geometry.source {
        GeometrySource::Selection(selections) => {
            (Some(SlotSource::Selection), selections.is_empty())
        }
        GeometrySource::Points(points) => (Some(SlotSource::Points), points.is_empty()),
        _ => (None, false),
    };
    let wanted_name = match wanted {
        SlotSource::Selection => "a selection",
        SlotSource::Points => "points",
    };
    if source != Some(wanted) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.slot-names-geometry-holds-no",
                "slot `{slot}` names geometry that holds no {wanted_name}"
            ),
        );
    } else if empty {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.slot-names-geometry-holds-none",
                "slot `{slot}` names geometry that holds none of its {wanted_name}"
            ),
        );
    }
}

fn check_operation(
    records: &ProcessRecords<'_>,
    operation: &Operation,
    findings: &mut Vec<Finding>,
) {
    let id = operation.id.as_str();
    let class = match &operation.kind {
        OperationKind::Machining { class, .. } => Some(class),
        OperationKind::Modifier { .. } => None,
    };
    let turning = matches!(class, Some(OperationClass::Turning(_)));
    let linking = operation.linking.as_ref();
    let technology = &operation.technology;
    let stated = [
        (ClassField::Radii, !operation.radii.is_empty()),
        (ClassField::Axial, !operation.axial.is_empty()),
        (
            ClassField::PullOut,
            linking.is_some_and(|l| l.pull_out.is_some()),
        ),
        (
            ClassField::PassClearance,
            linking.is_some_and(|l| l.pass_clearance.is_some()),
        ),
        (ClassField::TurningSide, technology.turning_side.is_some()),
        (
            ClassField::ThreadInfeedAngle,
            technology.thread_infeed_angle.is_some(),
        ),
        (ClassField::ThreadHand, technology.thread_hand.is_some()),
        (ClassField::ThreadDepth, technology.thread_depth.is_some()),
        (ClassField::HelixRise, technology.helix_rise.is_some()),
    ];
    for (field, _) in stated
        .into_iter()
        .filter(|&(field, stated)| stated && !field.allows(class))
    {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.operation-states-class-does-not-admit",
                "operation states `{}`, which its class does not admit",
                field.name()
            ),
        );
    }
    check_radii(operation, findings);
    check_profile_ranges(operation, findings);
    for height in operation.heights.values() {
        match height.from.as_ref().and_then(|from| from.value.as_ref()) {
            Some(HeightFrom::Selection(slot)) => {
                check_slot(operation, slot.as_str(), SlotSource::Selection, findings);
            }
            Some(HeightFrom::Geometry) => {
                let machined = operation
                    .geometry
                    .iter()
                    .any(|geometry| geometry.role == GeometryRole::Machine);
                if !machined {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.height-measured-operation-machine-geometry-does", 
                            "a height is measured from the operation's `machine` geometry, which it does not hold"
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    let mut slots = BTreeSet::new();
    for slot in operation
        .geometry
        .iter()
        .filter_map(|geometry| geometry.slot.as_ref())
    {
        if !slots.insert(slot.as_str()) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.slot-names-two-geometries-operation",
                    "slot `{}` names two geometries of the operation",
                    slot.as_str()
                ),
            );
        }
    }
    if let Some(linking) = linking {
        if matches!(&linking.lift, Some(Dimension::Literal(lift)) if lift.get() < 0.0) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.literal-linking-lift-negative-lift-raises",
                    "a literal `linking.lift` is negative; a lift raises the tool"
                ),
            );
        }
        if let Some(clearance) = &linking.pass_clearance {
            if clearance.radial.is_none() && clearance.axial.is_none() {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.pass-clearance-states-neither-radial-nor",
                        "`pass_clearance` states neither `radial` nor `axial`"
                    ),
                );
            }
        }
    }
    if let Some(plunge) = &operation.plunge {
        let helix = matches!(
            plunge.kind,
            Some(Choice {
                value: Some(PlungeKind::Helix),
                ..
            })
        );
        if plunge.helix_taper.is_some() && !helix {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.plunge-helix-taper-stated-plunge-kind",
                    "`plunge.helix_taper` is stated on a plunge whose kind is not `helix`"
                ),
            );
        }
    }
    check_leads(records, operation, turning, findings);
    check_cycle(operation, class, findings);
    check_tool_axis(operation, findings);
    if let Some(frame) = &operation.tool_frame {
        check_wcs(id, frame, findings);
    }
    let tool = match &operation.tooling {
        Some(Tooling::Tool(tool)) => records.tools.get(tool.as_str()),
        Some(Tooling::Preset(preset)) => records
            .presets
            .get(preset.as_str())
            .and_then(|preset| records.tools.get(preset.tool.as_str())),
        None => None,
    };
    check_technology(id, technology, tool.map(|tool| &tool.class), findings);
}

/// A profile curve whose carrier is a line, a parabola, or a hyperbola, or a
/// placement of one, states its `range` (part 10).
fn check_profile_ranges(operation: &Operation, findings: &mut Vec<Finding>) {
    let curves = operation
        .geometry
        .iter()
        .filter_map(|geometry| match &geometry.source {
            GeometrySource::Profile { curves, .. } => Some(curves),
            _ => None,
        })
        .flatten()
        .filter_map(|curve| curve.geometry.as_ref());
    for curve in curves.filter(|curve| curve.range.is_none()) {
        let mut carrier = &curve.carrier;
        while let InlineCurve::Transformed { basis, .. } = carrier {
            carrier = basis;
        }
        if matches!(
            carrier,
            InlineCurve::Line(_) | InlineCurve::Parabola(_) | InlineCurve::Hyperbola(_)
        ) {
            super::error(
                findings,
                operation.id.as_str(),
                crate::rule!(
                    "process.profile-curve-unbounded-carrier-states-no",
                    "a profile curve on an unbounded carrier states no `range`"
                ),
            );
        }
    }
}

/// Radial limits: a `Limit` names another stated role and the `Limit`
/// references form no cycle; `Positions` occurs on `Inner` or `Outer`, on at
/// most one role, and no `Limit` names its role; selection and position
/// bases name their slots.
fn check_radii(operation: &Operation, findings: &mut Vec<Finding>) {
    let id = operation.id.as_str();
    let positions: Vec<&RadiusRole> = operation
        .radii
        .iter()
        .filter(|(_, limit)| matches!(base(limit), Some(RadiusFrom::Positions { .. })))
        .map(|(role, _)| role)
        .collect();
    if positions.len() > 1 {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.positions-base-radial-roles-base-most",
                "`positions` is the base of {} radial roles; it is the base of at most one",
                positions.len()
            ),
        );
    }
    for (role, limit) in &operation.radii {
        match base(limit) {
            Some(RadiusFrom::Limit(target)) => {
                if target == role || !operation.radii.contains_key(target) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.radial-limit-names-role-own-not", "radial limit {role:?} names role {target:?}, which is its own or not stated"),
                    );
                }
                if positions.contains(&target) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.radial-limit-names-role-base-positions", "radial limit {role:?} names role {target:?}, whose base is `positions`"),
                    );
                }
            }
            Some(RadiusFrom::Positions { slot, .. }) => {
                if !matches!(role, RadiusRole::Inner | RadiusRole::Outer) {
                    super::error(
                        findings,
                        id,
                        crate::rule!("process.radial-limit-based-positions-only-inner", "radial limit {role:?} is based on `positions`, which only `inner` or `outer` is"),
                    );
                }
                check_slot(operation, slot.as_str(), SlotSource::Points, findings);
            }
            Some(RadiusFrom::SelectionMin(slot) | RadiusFrom::SelectionMax(slot)) => {
                check_slot(operation, slot.as_str(), SlotSource::Selection, findings);
            }
            _ => {}
        }
        let mut at = role;
        for _ in 0..operation.radii.len() {
            match operation.radii.get(at).and_then(base) {
                Some(RadiusFrom::Limit(next)) if next == role => {
                    super::error(
                        findings,
                        id,
                        crate::rule!(
                            "process.radial-limit-reaches-itself-through-limit",
                            "radial limit {role:?} reaches itself through `limit` bases"
                        ),
                    );
                    break;
                }
                Some(RadiusFrom::Limit(next)) => at = next,
                _ => break,
            }
        }
    }
}

/// A literal `LineTurn::Angle` lies from 0° to 90°, on a lead with no arc
/// (its `arc_radius` not stated, or known to be zero), and `SpindleAxis`
/// occurs only on a turning operation.
fn check_leads(
    records: &ProcessRecords<'_>,
    operation: &Operation,
    turning: bool,
    findings: &mut Vec<Finding>,
) {
    let id = operation.id.as_str();
    let leads = operation
        .leads
        .iter()
        .flat_map(|leads| leads.entry.iter().chain(leads.exit.iter()));
    for lead in leads {
        let Some(Choice {
            value: Some(LineTurn::Angle { angle, from, .. }),
            ..
        }) = &lead.line_turn
        else {
            continue;
        };
        if let Dimension::Literal(angle) = angle {
            if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&angle.get()) {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.literal-lead-angle-lies-outside-0",
                        "a literal lead angle lies outside 0° to 90°"
                    ),
                );
            }
        }
        let arc = lead
            .arc_radius
            .as_ref()
            .is_some_and(|radius| number(records, radius) != Some(0.0));
        if arc {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.lead-arc-states-lineturn-angle",
                    "a lead with an arc states a `LineTurn::Angle`"
                ),
            );
        }
        let spindle = matches!(
            from,
            Some(Choice {
                value: Some(crate::process::settings::LeadAngleFrom::SpindleAxis),
                ..
            })
        );
        if spindle && !turning {
            super::error(
                findings,
                id,
                crate::rule!("process.lead-angle-measured-spindle-axis-operation", "a lead angle is measured from the spindle axis on an operation that is not turning"),
            );
        }
    }
}

/// `cycle` occurs only on a drilling or probing operation; on a probing one
/// it states `probe_at` and nothing else, and only there `probe_at`; a
/// `depth` states a field, a literal `break_through` is not negative, and an
/// operation whose cycle states `depth` states a `Bottom` height.
fn check_cycle(operation: &Operation, class: Option<&OperationClass>, findings: &mut Vec<Finding>) {
    let id = operation.id.as_str();
    let Some(cycle) = &operation.cycle else {
        return;
    };
    let probing = matches!(class, Some(OperationClass::Probing));
    if !probing && !matches!(class, Some(OperationClass::Drilling(_))) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.operation-states-cycle-only-drilling-probing",
                "operation states a `cycle`, which only a drilling or probing operation states"
            ),
        );
    }
    if probing {
        let other = cycle.kind.is_some()
            || cycle.increment.is_some()
            || cycle.increment_reduction.is_some()
            || cycle.minimum_increment.is_some()
            || cycle.full_retract_after.is_some()
            || cycle.chip_break_retract.is_some()
            || cycle.dwell.is_some()
            || cycle.depth.is_some();
        if cycle.probe_at.is_none() || other {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.probing-operation-cycle-states-probe-no",
                    "a probing operation's `cycle` states `probe_at` and no other field"
                ),
            );
        }
    } else if cycle.probe_at.is_some() {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.cycle-probe-stated-operation-not-probing",
                "`cycle.probe_at` is stated on an operation that is not probing"
            ),
        );
    }
    if let Some(Choice {
        value: Some(CycleKind::Probe(target)),
        ..
    }) = &cycle.kind
    {
        check_probe_target(id, target, findings);
    }
    if let Some(depth) = &cycle.depth {
        if depth.tip_through.is_none() && depth.break_through.is_none() {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.cycle-depth-states-no-field",
                    "`cycle.depth` states no field"
                ),
            );
        }
        if matches!(&depth.break_through, Some(Dimension::Literal(b)) if b.get() < 0.0) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.literal-cycle-depth-break-through-negative",
                    "a literal `cycle.depth.break_through` is negative"
                ),
            );
        }
        if !operation.heights.contains_key(&HeightRole::Bottom) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.operation-cycle-states-depth-operation-states",
                    "operation's cycle states `depth`, but the operation states no `bottom` height"
                ),
            );
        }
    }
}

/// The number of picks a named selection lists; `None` for a rule or an
/// unread selection.
fn named_picks(selection: &Selection<Ordered>) -> Option<usize> {
    match &selection.picks {
        Picks::Named(members) => Some(members.as_slice().len()),
        Picks::Unread | Picks::Rule { .. } => None,
    }
}

/// `InPlane.plane` picks one plane, `ThroughLine.line` one line, and
/// `NormalToSurface.surfaces` and `AlongRuling.surfaces` at least one face.
/// A pick whose target states no kind is not refused, and a body or a
/// feature's result is taken to hold faces.
fn check_tool_axis(operation: &Operation, findings: &mut Vec<Finding>) {
    let id = operation.id.as_str();
    let admits: fn(Class) -> bool;
    let (selection, one, what, wanted) = match &operation.tool_axis {
        Some(ToolAxisControl::InPlane { plane }) => {
            admits = picks::is_plane;
            (plane, true, "`in_plane.plane`", "a plane")
        }
        Some(ToolAxisControl::ThroughLine { line }) => {
            admits = is_line;
            (line, true, "`through_line.line`", "a line")
        }
        Some(ToolAxisControl::NormalToSurface { surfaces }) => {
            admits = holds_faces;
            (surfaces, false, "`normal_to_surface.surfaces`", "a face")
        }
        Some(ToolAxisControl::AlongRuling { surfaces }) => {
            admits = holds_faces;
            (surfaces, false, "`along_ruling.surfaces`", "a face")
        }
        Some(ToolAxisControl::Native(_)) | None => return,
    };
    if let Some(class) = picks::refused(&selection.selection, admits) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.picks-picks",
                "{what} picks {class}; it picks {wanted}"
            ),
        );
    }
    let Some(count) = named_picks(&selection.selection) else {
        return;
    };
    if one && count != 1 {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.picks-entities-picks-one",
                "{what} picks {count} entities; it picks one"
            ),
        );
    } else if !one && count == 0 {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.picks-nothing-picks-least-one-face",
                "{what} picks nothing; it picks at least one face"
            ),
        );
    }
}

/// A line: an edge or a curve, a datum, a principal axis, a sketch's axis,
/// or a sketch entity.
const fn is_line(class: Class) -> bool {
    matches!(
        class,
        Class::Name(NameKind::Edge | NameKind::Curve)
            | Class::Datum
            | Class::SketchFrame
            | Class::SketchEntity
            | Class::SpatialSketchEntity
            | Class::Principal(
                PrincipalEntity::XAxis | PrincipalEntity::YAxis | PrincipalEntity::ZAxis
            )
    )
}

/// What holds a face: a face or a surface, a datum or principal plane, a
/// sketch region, a body, or a feature's result.
const fn holds_faces(class: Class) -> bool {
    picks::is_plane(class)
        || matches!(
            class,
            Class::Name(NameKind::Body) | Class::FeatureResult | Class::SketchRegion
        )
}

/// `WcsOrigin::FrameOrigin` occurs only with `WcsOrientation::Frame`.
fn check_wcs(holder: &str, wcs: &Wcs, findings: &mut Vec<Finding>) {
    let frame_origin = matches!(
        wcs.origin,
        Some(Choice {
            value: Some(WcsOrigin::FrameOrigin),
            ..
        })
    );
    if !frame_origin {
        return;
    }
    let frame = matches!(
        wcs.orientation,
        Some(Choice {
            value: Some(WcsOrientation::Frame(_)),
            ..
        })
    );
    if !frame {
        super::error(
            findings,
            holder,
            crate::rule!(
                "process.wcs-origin-frame-origin-stated-without",
                "WCS origin `frame_origin` is stated without a `frame` orientation"
            ),
        );
    }
}

/// The literal ranges of a technology; `ConstantEngagement` only in
/// `stepover_rule`; `jet_cut` only with a jet cutter.
fn check_technology(
    holder: &str,
    technology: &Technology,
    tool: Option<&ToolClass>,
    findings: &mut Vec<Finding>,
) {
    let negative =
        |d: &ProcessDimension<Length>| matches!(d, Dimension::Literal(v) if v.get() < 0.0);
    let not_positive =
        |d: &ProcessDimension<Length>| matches!(d, Dimension::Literal(v) if v.get() <= 0.0);
    let zero = |d: &ProcessDimension<u32>| matches!(d, Dimension::Literal(0));
    let tabs = technology.tabs.as_ref();
    let placement = tabs.map(|tabs| &tabs.placement);
    let ranges = [
        (
            technology
                .closed_pass_overlap
                .as_ref()
                .is_some_and(negative),
            "a literal `closed_pass_overlap` is negative",
        ),
        (
            technology.roughing_pass_count.as_ref().is_some_and(zero),
            "a literal `roughing_pass_count` is 0",
        ),
        (
            tabs.is_some_and(|tabs| not_positive(&tabs.height)),
            "a literal `tabs.height` is not positive",
        ),
        (
            tabs.is_some_and(|tabs| not_positive(&tabs.width)),
            "a literal `tabs.width` is not positive",
        ),
        (
            matches!(placement, Some(TabPlacement::PerContour(n)) if zero(n)),
            "a literal `per_contour` tab count is 0",
        ),
        (
            matches!(placement, Some(TabPlacement::Spacing(s)) if not_positive(s)),
            "a literal tab `spacing` is not positive",
        ),
        (
            matches!(placement, Some(TabPlacement::AtPoints(points)) if points.is_empty()),
            "`at_points` tabs hold no point",
        ),
        (
            technology
                .chamfer
                .as_ref()
                .is_some_and(|chamfer| negative(&chamfer.width)),
            "a literal `chamfer.width` is negative",
        ),
    ];
    for (_, message) in ranges.into_iter().filter(|(refused, _)| *refused) {
        super::error(
            findings,
            holder,
            crate::rule!("process.technology-value-ranges", "{message}"),
        );
    }
    if matches!(
        technology.stepdown_rule,
        Some(Choice {
            value: Some(StepRule::ConstantEngagement),
            ..
        })
    ) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "process.stepdown-rule-constant-engagement-only-stepover",
                "`stepdown_rule` is `constant_engagement`, which only `stepover_rule` states"
            ),
        );
    }
    if technology.jet_cut.is_some() && !matches!(tool, Some(ToolClass::JetCutter)) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "process.jet-cut-stated-where-technology-tool",
                "`jet_cut` is stated where the technology's tool is no jet cutter"
            ),
        );
    }
}

/// Probe values occur only in a probe cycle, and a probe target's widths,
/// diameters, and spacing are positive.
fn check_motions(holder: &str, motions: &[Motion], findings: &mut Vec<Finding>) {
    for motion in motions {
        if let Motion::Cycle(cycle) = motion {
            check_motion_cycle(holder, cycle, findings);
        }
    }
}

fn check_motion_cycle(holder: &str, cycle: &Cycle, findings: &mut Vec<Finding>) {
    match &cycle.kind {
        CycleKind::Probe(target) => check_probe_target(holder, target, findings),
        _ => {
            if cycle.probe_clearance.is_some() || cycle.probe_overtravel.is_some() {
                super::error(
                    findings,
                    holder,
                    crate::rule!(
                        "process.cycle-not-probe-cycle-states-probe",
                        "a cycle that is not a probe cycle states a probe clearance or overtravel"
                    ),
                );
            }
        }
    }
}

fn check_probe_target(holder: &str, target: &ProbeTarget, findings: &mut Vec<Finding>) {
    let lengths: Vec<Length> = match target {
        ProbeTarget::Wall { width, .. } | ProbeTarget::Channel { width, .. } => vec![*width],
        ProbeTarget::RectangularPocket { widths, .. } | ProbeTarget::RectangularBoss { widths } => {
            widths.to_vec()
        }
        ProbeTarget::CircularPocket { diameter, .. }
        | ProbeTarget::CircularBoss { diameter }
        | ProbeTarget::PartialCircularPocket { diameter, .. }
        | ProbeTarget::PartialCircularBoss { diameter, .. } => vec![*diameter],
        ProbeTarget::FaceAngle { spacing, .. } => vec![*spacing],
        ProbeTarget::Face { .. } | ProbeTarget::Corner { .. } | ProbeTarget::Native(_) => {
            Vec::new()
        }
    };
    if lengths.iter().any(|length| length.get() <= 0.0) {
        super::error(
            findings,
            holder,
            crate::rule!(
                "process.probe-target-width-diameter-spacing-not",
                "a probe target's width, diameter, or spacing is not positive"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
