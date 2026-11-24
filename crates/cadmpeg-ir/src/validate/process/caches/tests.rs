// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::construction::FrameAxis;
use crate::design::evaluation::EvaluationStatus;
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::math::Point3;
use crate::process::cycle::{Cycle, CycleKind, ProbeTarget};
use crate::process::toolpath::{Motion, ToolpathContent};
use crate::report::Severity;
use crate::scalar::{Length, PositiveLength};
use crate::transform::Transform;
use crate::validate::process::fixture::{document, process_findings};

/// The tolerance in force in these tests, exact in binary64.
const LINEAR: f64 = 1.0 / 1024.0;

fn fixture() -> CadIr {
    let mut ir = document();
    ir.tolerances.linear = PositiveLength::new(LINEAR).unwrap();
    ir
}

fn translation(x: f64) -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap()
}

fn content(ir: &mut CadIr) -> (&mut Transform, &mut Vec<Motion>) {
    match &mut ir.process.toolpaths[0].content {
        ToolpathContent::Motions { frame, motions } => (frame, motions),
        ToolpathContent::Native(_) => unreachable!("the fixture's toolpath holds motions"),
    }
}

fn warnings(ir: &CadIr, fragment: &str) -> usize {
    process_findings(ir)
        .iter()
        .filter(|finding| {
            finding.severity == Severity::Warning && finding.message.contains(fragment)
        })
        .count()
}

fn drill(kind: CycleKind, stock: f64, bottom: f64, depth: f64) -> Motion {
    let length = |value: f64| Some(Length::new(value).unwrap());
    Motion::Cycle(Box::new(Cycle {
        kind,
        at: FinitePoint3::new(Point3::new(1.0, 2.0, bottom)).unwrap(),
        clearance: None,
        retract: None,
        stock: length(stock),
        depth: length(depth),
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
    }))
}

const FRAME: &str = "is not its operation's resolved frame";
const DEPTH: &str = "minus the z of its point";

#[test]
fn a_toolpath_frame_is_its_operation_resolved_frame() {
    let mut ir = fixture();
    ir.process.setups[0].wcs.placement = Some(translation(1.0));
    assert_eq!(warnings(&ir, FRAME), 1);

    *content(&mut ir).0 = translation(1.0 + LINEAR);
    assert_eq!(warnings(&ir, FRAME), 0);
    *content(&mut ir).0 = translation(1.0 + 2.0 * LINEAR);
    assert_eq!(warnings(&ir, FRAME), 1);

    ir.process.toolpaths[0].status = EvaluationStatus::Stale;
    assert_eq!(warnings(&ir, FRAME), 0);

    let mut ir = fixture();
    ir.process.setups[0].wcs.placement = Some(translation(1.0));
    let operation = ir
        .process
        .operations
        .iter_mut()
        .find(|operation| operation.id == ir.process.toolpaths[0].operation)
        .unwrap();
    let mut tool_frame = ir.process.setups[0].wcs.clone();
    tool_frame.placement = Some(Transform::identity());
    operation.tool_frame = Some(tool_frame);
    assert_eq!(warnings(&ir, FRAME), 0);
}

#[test]
fn a_cycle_depth_is_its_stock_level_minus_the_z_of_its_point() {
    let mut ir = fixture();
    content(&mut ir)
        .1
        .push(drill(CycleKind::Drill, 0.5, -0.25, 0.75));
    assert_eq!(warnings(&ir, DEPTH), 0);
    content(&mut ir).1[0] = drill(CycleKind::Drill, 0.5, -0.25, 0.75 + LINEAR);
    assert_eq!(warnings(&ir, DEPTH), 0);
    content(&mut ir).1[0] = drill(CycleKind::Drill, 0.5, -0.25, 0.75 + 2.0 * LINEAR);
    assert_eq!(warnings(&ir, DEPTH), 1);

    let probe = CycleKind::Probe(ProbeTarget::Face {
        axis: FrameAxis::Z,
        reversed: true,
    });
    content(&mut ir).1[0] = drill(probe, 0.5, -0.25, 2.0);
    assert_eq!(warnings(&ir, DEPTH), 0);
}
