// SPDX-License-Identifier: Apache-2.0
//! CC0 Fusion API saves of suppressed and rolled-back timeline items.

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::design::container::{Container, Rollback};
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::{Codec, DecodeOptions};

use crate::F3dCodec;

/// The suppression state of each named feature, the name of the timeline
/// item the rollback marker precedes, and the decode loss codes.
fn timeline_state(bytes: &[u8]) -> (BTreeMap<String, bool>, Option<String>, Vec<String>) {
    let result = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the corpus file");
    let codes = result
        .report()
        .losses
        .iter()
        .map(|loss| loss.code.local_code().to_owned())
        .collect();
    let ir = result.ir();
    let suppressed = ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.name.clone()?, feature.suppressed?)))
        .collect();
    let [Container::Ordered { rollback, .. }] = ir.design.containers.as_slice() else {
        panic!("one ordered timeline")
    };
    let marker = match rollback {
        None | Some(Rollback::End) => None,
        Some(Rollback::Before(Member::Feature(id))) => Some(
            ir.design
                .features
                .iter()
                .find(|feature| &feature.id == id)
                .and_then(|feature| feature.name.clone())
                .expect("named marker feature"),
        ),
        Some(Rollback::Before(member)) => panic!("marker before {member:?}"),
    };
    (suppressed, marker, codes)
}

fn expect(suppressed: &BTreeMap<String, bool>, expected: &[(&str, bool)]) {
    for (name, state) in expected {
        assert_eq!(suppressed.get(*name), Some(state), "{name}: {suppressed:?}");
    }
}

/// The rolled-back Combine and Fillet store the no-result flag and no
/// suppression state byte: f3d.md §3.1 states them active with no current
/// result.
#[test]
fn rolled_back_features_after_an_edit_are_active() {
    let (suppressed, marker, codes) = timeline_state(include_bytes!(
        "../../../../corpus/f3d/timeline_rolled_after_edit.f3d"
    ));
    expect(&suppressed, &[("Combine 1", false), ("Fillet 1", false)]);
    assert_eq!(marker.as_deref(), Some("Combine 1"));
    assert!(
        !codes
            .iter()
            .any(|code| code == "feature.no-result-active-assumed"),
        "{codes:?}"
    );
}

#[test]
fn a_suppressed_feature_after_the_marker_stays_suppressed() {
    let (suppressed, marker, _) = timeline_state(include_bytes!(
        "../../../../corpus/f3d/timeline_suppressed_rolled.f3d"
    ));
    expect(&suppressed, &[("Combine 1", false), ("Fillet 1", true)]);
    assert_eq!(marker.as_deref(), Some("Combine 1"));
}

#[test]
fn either_suppression_state_byte_marks_a_feature_suppressed() {
    let (suppressed, marker, _) = timeline_state(include_bytes!(
        "../../../../corpus/f3d/timeline_combine_suppressed.f3d"
    ));
    expect(
        &suppressed,
        &[("Combine 1", true), ("Fillet 1", false), ("Fillet 2", true)],
    );
    assert_eq!(marker.as_deref(), Some("Fillet 2"));
}

#[test]
fn a_suppressed_timeline_group_suppresses_its_items() {
    let (suppressed, marker, _) = timeline_state(include_bytes!(
        "../../../../corpus/f3d/timeline_group_suppressed.f3d"
    ));
    expect(
        &suppressed,
        &[
            ("Extrude 1", false),
            ("Sketch 3", true),
            ("Extrude 3", true),
        ],
    );
    assert_eq!(marker, None);
}

/// The occurrence item before the first rolled-back item stores no tail
/// state, so the marker stands before that rolled-back item and a loss says
/// that it can stand before the occurrence item.
#[test]
fn a_marker_after_an_occurrence_item_charges_the_marker_bound_loss() {
    let (suppressed, marker, codes) = timeline_state(include_bytes!(
        "../../../../corpus/f3d/timeline_marker_after_occurrence.f3d"
    ));
    expect(&suppressed, &[("Sketch 2", false), ("Extrude 2", false)]);
    assert_eq!(marker.as_deref(), Some("Sketch 2"));
    assert!(
        codes
            .iter()
            .any(|code| code == "history.rollback-bound-unread"),
        "{codes:?}"
    );
}
