// SPDX-License-Identifier: Apache-2.0
//! A failure of the records of a timeline item (cadir-v11.md §3.4): the
//! writer drops the item whose records it cannot write and each record that
//! needs it, and keeps the design scope, its construction planes and its
//! other sketches.

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::MissingStateForm;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::options::Boolean;
use cadmpeg_ir::design::refs::{Member, Position};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::report::Severity;

use super::super::super::{Culprit, RecordFailure};
use super::super::sketch::{feature, feature_id, frame, point2, unread, Source};
use super::super::{fail_records, tails, write_bytes_as};
use super::history::{
    decoded_rollback, with_a_rolled_back_extrude, with_a_second_extrude, with_fillet,
};
use super::{blind, extrude, kinds, loss, sketch_and_extrude, EVALUATION_MISSING, EXTRUDE};
use crate::loss::F3dLossCode;
use crate::writer::generate::reduce::{Failure, Reduction};
use crate::writer::generate::writes_alone;

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

/// The reason the writer gives for the bodies after a held state.
fn held_reason(before: &FeatureId) -> String {
    format!("the history-true design writes the state of the items before {before} at its timeline marker")
}

/// The ids of the topology and geometry records of the second extrude's
/// prism in [`with_a_second_extrude`], in id order.
fn second_records(ir: &CadIr) -> Vec<String> {
    let serde_json::Value::Object(arenas) =
        json::to_value(&ir.geometry).expect("the geometry serializes")
    else {
        panic!("the geometry's arenas");
    };
    arenas
        .values()
        .filter_map(serde_json::Value::as_array)
        .flatten()
        .filter_map(|record| record["id"].as_str())
        .filter(|id| id.contains("#second-"))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The writer charges the result body of the item that ends the run, and
/// each other record only that body uses, by id: each record is named once
/// (cadir-v11.md §3.2).
#[test]
fn a_held_state_charges_the_body_after_it_and_its_records() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    let _failing = fail_records([second.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    let reason = held_reason(&second);
    let charged = |code: F3dLossCode| {
        report
            .losses
            .iter()
            .filter(|loss| loss.code == code.kind() && loss.message.contains(&reason))
            .map(|loss| (loss.severity, loss.message.clone()))
            .collect::<Vec<_>>()
    };
    let body = ir.design.scopes[0].current[0]
        .bodies
        .last()
        .expect("the prism's body");
    assert_eq!(
        charged(F3dLossCode::WriterBodyOmitted),
        [(
            Severity::Error,
            format!("body {body} was not written, with the topology and geometry only it uses: {reason}")
        )]
    );
    let body_id = body.to_string();
    let records = second_records(&ir);
    assert!(records.contains(&body_id), "{records:?}");
    let records = records
        .into_iter()
        .filter(|record| *record != body_id)
        .collect::<Vec<_>>();
    assert_eq!(
        charged(F3dLossCode::WriterGeometryOmitted),
        [(
            Severity::Warning,
            format!(
                "{} topology and geometry record(s) of omitted bodies, or used by no written body, were not written ({reason}): {}",
                records.len(),
                records.join(", ")
            )
        )]
    );
}

/// A held state takes the status of the current state it comes from: the
/// writer charges a stale current state with or without the failure
/// (cadir-v11.md §3.4).
#[test]
fn a_held_state_keeps_the_stale_status_of_the_current_state() {
    let (mut ir, second) = with_a_second_extrude(Boolean::NewBody);
    ir.design.scopes[0].current[0].status = EvaluationStatus::Stale;
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(decoded.geometry.bodies.len(), 2);
    let stale = loss(&report, "export/evaluation.stale").join("\n");
    assert!(stale.contains(" (stale)"), "{stale}");
    let _failing = fail_records([second.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(decoded.geometry.bodies.len(), 1);
    let stale = loss(&report, "export/evaluation.stale").join("\n");
    assert!(
        stale.contains(&format!("current state after {EXTRUDE} (stale)")),
        "{stale}"
    );
}

/// Where a later failure drops the geometry items after a held state, the
/// document holds every current body again, as it does with no held state
/// (cadir-v11.md §3.4, Geometry-true).
#[test]
fn dropped_items_after_a_held_state_write_every_current_body() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    let error = || CodecError::NotImplemented("a test failure of the design".into());
    let mut reduction = Reduction::new(&ir, MissingStateForm::HistoryTrue);
    reduction.prepare().expect("the document prepares");
    reduction
        .degrade(
            Failure::records(RecordFailure {
                error: error(),
                culprit: Some(Culprit::Item {
                    feature: second.clone(),
                    rolled: false,
                }),
            }),
            writes_alone,
        )
        .expect("the run ends at the second extrude");
    assert_eq!(reduction.ir().geometry.bodies.len(), 1);
    while !reduction.fallback().drops_items() {
        reduction
            .degrade(Failure::neutral_design(error()), writes_alone)
            .expect("the design scope falls back");
    }
    assert!(!reduction.drops_neutral_design());
    assert_eq!(reduction.ir().geometry.bodies.len(), 2);
    let reason = held_reason(&second);
    let (losses, _) = reduction.finish();
    assert!(
        !losses.iter().any(|loss| loss.message.contains(&reason)),
        "{losses:#?}"
    );
}

/// Where the plan of the design fails after a held state, the writer drops
/// the design scope, and the file holds every current body with no loss of
/// the held state (cadir-v11.md §3.4, Geometry-true).
#[test]
fn a_dropped_design_after_a_held_state_writes_every_current_body() {
    let (ir, second) = with_a_second_extrude(Boolean::NewBody);
    let _failing = fail_records([second.as_str()]);
    super::super::super::FAIL_ENDED_PLANS.with(|fail| fail.set(true));
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    super::super::super::FAIL_ENDED_PLANS.with(|fail| fail.set(false));
    assert_eq!(decoded.geometry.bodies.len(), 2, "{:#?}", report.losses);
    assert_eq!(decoded.geometry.faces.len(), 12, "{:#?}", report.losses);
    let reason = held_reason(&second);
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.code == F3dLossCode::WriterBodyOmitted.kind()
                || loss.message.contains(&reason)),
        "{:#?}",
        report.losses
    );
    assert!(
        report.losses.iter().any(|loss| loss
            .message
            .contains("(not implemented yet: a test failure of the design plan)")
            && loss.message.contains(second.as_str())),
        "{:#?}",
        report.losses
    );
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
