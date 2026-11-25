// SPDX-License-Identifier: Apache-2.0
//! A failure of the records of a timeline item (cadir-v11.md §3.4): the
//! writer drops the item whose records it cannot write and each record that
//! needs it, and keeps the design scope, its construction planes and its
//! other sketches.

use cadmpeg_ir::codec::write::MissingStateForm;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::options::Boolean;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;

use super::super::sketch::{feature, feature_id, frame, point2, unread, Source};
use super::super::{fail_records, tails, write_bytes_as};
use super::history::{
    decoded_rollback, with_a_rolled_back_extrude, with_a_second_extrude, with_fillet,
};
use super::{blind, extrude, kinds, loss, sketch_and_extrude, EVALUATION_MISSING, EXTRUDE};
use crate::loss::F3dLossCode;

/// [`with_a_rolled_back_extrude`] with a `Sketch` on a construction plane
/// after the first extrude, rolled back with the second extrude. Returns the
/// design and the second extrude.
fn with_a_rolled_back_plane() -> (CadIr, FeatureId) {
    let (mut ir, second) = with_a_rolled_back_extrude();
    let mut top = Source::new(
        "top",
        unread(),
        frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    top.entity(
        "line",
        PlanarCurve::Line {
            start: point2(1.0, 1.0),
            end: point2(3.0, 2.0),
        },
        &[],
    );
    let top = top.add_to(&mut ir);
    let Some(Container::Ordered {
        members, rollback, ..
    }) = ir.design.containers.first_mut()
    else {
        panic!("the cube's timeline");
    };
    members.insert(2, Member::Feature(top.clone()));
    *rollback = Some(Rollback::Before(Member::Feature(top)));
    (ir, second)
}

/// The families of the decoded item tails, in record order.
fn families(decoded: &CadIr) -> Vec<String> {
    tails(decoded).into_iter().map(|tail| tail.0).collect()
}

/// The text of each failure the test seam makes.
const TEST_FAILURE: &str = "a test failure of the records of";

/// Why the writer writes no item whose records `id` fail.
fn records_fail(id: &str) -> String {
    format!("the writer cannot write its records (not implemented yet: {TEST_FAILURE} {id})")
}

/// The records of the item of `feature` written with its state, and the
/// records of its result body: a failure in either names the item.
fn item_records(feature: &FeatureId) -> [String; 2] {
    [feature.to_string(), format!("body of {feature}")]
}

/// The records of the rolled-back item of `feature`, and the records of its
/// body.
fn rolled_records(feature: &FeatureId) -> [String; 2] {
    [
        format!("rolled {feature}"),
        format!("rolled body of {feature}"),
    ]
}

#[test]
fn a_rolled_back_item_whose_records_fail_is_dropped_alone() {
    let (ir, second) = with_a_rolled_back_plane();
    for failing in rolled_records(&second) {
        let _failing = fail_records([failing.as_str()]);
        let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude", "sketch"],
            "{failing}: {:#?}",
            report.losses
        );
        assert_eq!(
            families(&decoded),
            ["Sketch", "Extrude", "WorkPlane", "Sketch"]
        );
        assert_eq!(decoded.geometry.bodies.len(), 1);
        let missing = loss(&report, EVALUATION_MISSING).join("\n");
        assert!(
            missing.contains(&format!(
                "{second} (it follows the timeline marker, and the writer writes no rolled-back item for it: {})",
                records_fail(&failing)
            )),
            "{missing}"
        );
        assert!(!missing.contains(&format!("{EXTRUDE} (")), "{missing}");
    }
}

/// A rolled-back item that the stated rollback stands before is dropped
/// alone; the timeline marker stands before the next written item, and the
/// timeline value charges the stated rollback.
#[test]
fn a_dropped_item_at_the_stated_rollback_moves_the_marker_to_the_next_item() {
    let mut ir = sketch_and_extrude(blind(), true);
    let second = feature(
        "second",
        FeatureOperation::Extrude(Box::new(extrude(blind()))),
    );
    let second_id = second.id.clone();
    ir.design.features.push(second);
    let Some(Container::Ordered {
        members, rollback, ..
    }) = ir.design.containers.first_mut()
    else {
        panic!("the cube's timeline");
    };
    members.insert(2, Member::Feature(second_id.clone()));
    *rollback = Some(Rollback::Before(Member::Feature(second_id.clone())));
    ir.design.scopes[0].current[0].at = Position::After(Member::Feature(
        FeatureId::mint(EXTRUDE).expect("identity grammar"),
    ));
    let rolled = format!("rolled {second_id}");
    let _failing = fail_records([rolled.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch"],
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.bodies.len(), 1);
    let [Container::Ordered {
        members, rollback, ..
    }] = decoded.design.containers.as_slice()
    else {
        panic!("one decoded timeline: {:#?}", decoded.design.containers);
    };
    assert_eq!(rollback, &Some(Rollback::Before(members[2].clone())));
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{second_id} (it follows the timeline marker, and the writer writes no rolled-back item for it: {})",
            records_fail(&rolled)
        )),
        "{missing}"
    );
    let omitted = loss(
        &report,
        &F3dLossCode::WriterDesignRecordOmitted.kind().to_string(),
    )
    .join("\n");
    assert!(
        omitted.contains(&format!("rollback (before {second_id})")),
        "{omitted}"
    );
}

/// The failure of a later item that is written with its state ends the
/// history-true run at it: the item before it keeps the state the writer
/// writes for it when nothing fails, and its body. The item follows the
/// timeline marker as a rolled-back item, whose entry states the failure
/// once.
#[test]
fn a_later_run_item_whose_records_fail_ends_the_run_after_the_state_before_it() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    for failing in item_records(&second) {
        let _failing = fail_records([failing.as_str()]);
        let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
        assert_eq!(
            kinds(&decoded),
            ["sketch", "extrude", "sketch", "extrude"],
            "{failing}: {:#?}",
            report.losses
        );
        let (rollback, members) = decoded_rollback(&decoded);
        assert_eq!(rollback, Some(Rollback::Before(members[3].clone())));
        assert_eq!(decoded.geometry.bodies.len(), 1, "{failing}");
        assert_eq!(decoded.geometry.faces.len(), 6, "{failing}");
        let stated = tails(&decoded)
            .into_iter()
            .filter(|tail| tail.0 == "Extrude")
            .map(|tail| (tail.2, tail.3, tail.4, tail.5))
            .collect::<Vec<_>>();
        assert_eq!(
            stated,
            [(false, false, Some(3), Some(2)), (true, true, None, None)],
            "{failing}"
        );
        let missing = loss(&report, EVALUATION_MISSING).join("\n");
        assert!(!missing.contains(&format!("{EXTRUDE} (")), "{missing}");
        let reason = records_fail(&failing);
        assert!(
            missing.contains(&format!(
                "{second} (the writer writes it without its state: {reason})"
            )),
            "{missing}"
        );
        assert_eq!(missing.matches(TEST_FAILURE).count(), 1, "{missing}");
    }
}

/// Where the rolled-back records of the item fail as well, the writer
/// writes no item for it, and its entry states each failure once.
#[test]
fn a_run_item_whose_records_fail_in_both_forms_states_each_failure_once() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    let rolled = format!("rolled {second}");
    let _failing = fail_records([second.as_str(), rolled.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch"],
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.bodies.len(), 1);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{second} (it follows the timeline marker, and the writer writes no rolled-back item for it: {}; the writer writes it without its state: {}",
            records_fail(&rolled),
            records_fail(second.as_str())
        )),
        "{missing}"
    );
    assert_eq!(missing.matches(TEST_FAILURE).count(), 2, "{missing}");
}

#[test]
fn a_run_item_whose_records_fail_leaves_the_later_items_rolled_back() {
    let (ir, second) = with_a_rolled_back_plane();
    let _failing = fail_records([EXTRUDE]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert_eq!(
        families(&decoded),
        ["Sketch", "Extrude", "WorkPlane", "Sketch", "Extrude"]
    );
    assert!(decoded.geometry.bodies.is_empty());
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the writer writes it without its state: {})",
            records_fail(EXTRUDE)
        )),
        "{missing}"
    );
    assert_eq!(missing.matches(TEST_FAILURE).count(), 1, "{missing}");
    assert!(!missing.contains(&format!("{second} (")), "{missing}");
}

#[test]
fn a_design_with_no_body_and_no_item_keeps_its_sketch() {
    let mut ir = sketch_and_extrude(blind(), false);
    with_fillet(&mut ir);
    let rolled = format!("rolled {EXTRUDE}");
    let _failing = fail_records([rolled.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(kinds(&decoded), ["sketch"], "{:#?}", report.losses);
    assert_eq!(families(&decoded), ["Sketch"]);
    assert!(decoded.geometry.bodies.is_empty());
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (it follows the timeline marker, and the writer writes no rolled-back item for it: {}",
            records_fail(&rolled)
        )),
        "{missing}"
    );
}

#[test]
fn a_sketch_whose_records_fail_is_dropped_with_the_item_that_needs_it() {
    let ir = sketch_and_extrude(blind(), true);
    let profile = feature_id("profile");
    let _failing = fail_records([profile.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::GeometryTrue);
    assert_eq!(kinds(&decoded), ["sketch", "base"], "{:#?}", report.losses);
    assert_eq!(decoded.model.sketches.len(), 1);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{profile} (its sketch is dropped because {})",
            records_fail(profile.as_str())
        )),
        "{missing}"
    );
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (the Base Feature holds it: its sketch is not written"
        )),
        "{missing}"
    );
    assert!(!missing.contains(feature_id("later").as_str()), "{missing}");
}

/// In the history-true form, a dropped sketch ends the run at the item
/// that needs it: the item before it keeps its state and its body. The
/// entry of the item states the reason once.
#[test]
fn a_dropped_sketch_ends_the_run_at_the_item_that_needs_it() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    let sketch = feature_id("second");
    let _failing = fail_records([sketch.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.bodies.len(), 1);
    assert_eq!(decoded.geometry.faces.len(), 6);
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(!missing.contains(&format!("{EXTRUDE} (")), "{missing}");
    for entry in [
        format!(
            "{sketch} (its sketch is dropped because {})",
            records_fail(sketch.as_str())
        ),
        format!(
            "{second} (it follows the timeline marker, and the writer writes no rolled-back item for it: its sketch is not written"
        ),
    ] {
        assert!(missing.contains(&entry), "{entry}\n{missing}");
    }
    assert_eq!(
        missing.matches("its sketch is not written").count(),
        1,
        "{missing}"
    );
}
