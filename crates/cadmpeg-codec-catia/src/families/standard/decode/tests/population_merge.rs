// SPDX-License-Identifier: Apache-2.0
//! Give-up charges a later standard population carries into the merged report.

use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::unknown::part::{dropped_with, needs, record_undecoded_kind, Cause, GivenUpRecord};

use crate::families::standard::decode::merge_population_give_ups;

fn id(text: &str) -> Identity {
    Identity::new(text).expect("a typed id")
}

#[test]
fn a_population_merge_keeps_and_rescopes_its_give_ups_and_dropped_records() {
    let charge = GivenUpRecord::new(
        id("catia:standard:face#3"),
        Cause::Undecoded,
        "catia/topology.boundary-graph-not-emitted: the curve support table is absent",
    )
    .loss();
    let dropped = dropped_with(id("catia:standard:loop#7"), id("catia:standard:face#3"));
    let summary = crate::loss::CatiaLossCode::GeometryCarrierSummary.note("population summary");
    let mut merged = Vec::new();
    merge_population_give_ups(&mut merged, vec![charge, summary, dropped], "population-1")
        .expect("rescope the population give-ups");

    let [charge, dropped] = merged.as_slice() else {
        panic!("the charge and the dropped-with note: {merged:?}");
    };
    assert_eq!(charge.code, record_undecoded_kind());
    assert!(
        charge
            .message
            .starts_with("record catia:standard:population-1/face#3 given up: "),
        "{}",
        charge.message
    );
    let (record, needed) = needs(dropped).expect("a dropped-with note");
    assert_eq!(record.as_str(), "catia:standard:population-1/loop#7");
    assert_eq!(needed.as_str(), "catia:standard:population-1/face#3");
    assert!(
        dropped
            .message
            .contains("catia:standard:population-1/loop#7"),
        "{}",
        dropped.message
    );
}
