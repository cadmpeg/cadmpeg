// SPDX-License-Identifier: Apache-2.0
//! A failure of the records of a timeline item (cadir-v11.md §3.4): the
//! writer drops the item whose records it cannot write and each record that
//! needs it, and keeps the design scope, its construction planes and its
//! other sketches.

use cadmpeg_ir::codec::write::MissingStateForm;
use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;

use super::super::sketch::{feature_id, frame, point2, unread, Source};
use super::super::{fail_records, tails, write_bytes_as};
use super::history::{with_a_rolled_back_extrude, with_fillet};
use super::{blind, kinds, loss, sketch_and_extrude, EVALUATION_MISSING, EXTRUDE};

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

/// Why the writer writes no item for `feature`, whose records fail.
fn records_fail(feature: &str) -> String {
    format!(
        "the writer cannot write its records (not implemented yet: a test failure of the records of {feature})"
    )
}

#[test]
fn a_rolled_back_item_whose_records_fail_is_dropped_alone() {
    let (ir, second) = with_a_rolled_back_plane();
    fail_records([second.as_str()]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude", "sketch"],
        "{:#?}",
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
            records_fail(second.as_str())
        )),
        "{missing}"
    );
    assert!(!missing.contains(&format!("{EXTRUDE} (")), "{missing}");
}

#[test]
fn a_run_item_whose_records_fail_leaves_the_later_items_rolled_back() {
    let (ir, second) = with_a_rolled_back_plane();
    fail_records([EXTRUDE]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert_eq!(
        families(&decoded),
        ["Sketch", "WorkPlane", "Sketch", "Extrude"]
    );
    assert!(decoded.geometry.bodies.is_empty());
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    for note in [
        format!(
            "it follows the timeline marker, and the writer writes no rolled-back item for it: {}",
            records_fail(EXTRUDE)
        ),
        format!(
            "the writer writes it without its state: {}",
            records_fail(EXTRUDE)
        ),
    ] {
        assert!(missing.contains(&note), "{note}\n{missing}");
    }
    assert!(!missing.contains(&format!("{second} (")), "{missing}");
}

#[test]
fn a_design_with_no_body_and_no_item_keeps_its_sketch() {
    let mut ir = sketch_and_extrude(blind(), false);
    with_fillet(&mut ir);
    fail_records([EXTRUDE]);
    let (report, decoded, _) = write_bytes_as(&ir, MissingStateForm::HistoryTrue);
    assert_eq!(kinds(&decoded), ["sketch"], "{:#?}", report.losses);
    assert_eq!(families(&decoded), ["Sketch"]);
    assert!(decoded.geometry.bodies.is_empty());
    let missing = loss(&report, EVALUATION_MISSING).join("\n");
    assert!(
        missing.contains(&format!(
            "{EXTRUDE} (it follows the timeline marker, and the writer writes no rolled-back item for it: {}",
            records_fail(EXTRUDE)
        )),
        "{missing}"
    );
}

#[test]
fn a_sketch_whose_records_fail_is_dropped_with_the_item_that_needs_it() {
    let ir = sketch_and_extrude(blind(), true);
    let profile = feature_id("profile");
    fail_records([profile.as_str()]);
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
    assert!(!missing.contains(feature_id("later").as_str()), "{missing}");
}
