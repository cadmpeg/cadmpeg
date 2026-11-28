// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::unknown::part::record_undecoded_kind;

use super::state_records;
use crate::brep::graph::{Brep, DroppedRecord};

fn body() -> Body {
    Body {
        id: BodyId::mint("sldprt:brep:body#42").unwrap(),
        kind: BodyKind::Sheet,
        transform: None,
        name: None,
        color: None,
        visible: None,
        unstated: None,
        native_ref: None,
    }
}

fn withheld(record: &str) -> DroppedRecord {
    DroppedRecord::GivenUp {
        record: record.to_owned(),
        at: None,
        error: "the record does not read".to_owned(),
        dropping: Vec::new(),
    }
}

#[test]
fn a_replayed_state_keeps_readable_bodies_and_reports_withheld_records() {
    let body = body();
    let state = state_records(Brep {
        bodies: vec![body.clone()],
        dropped: vec![withheld("sldprt:brep:body#43")],
        ..Brep::default()
    })
    .unwrap()
    .unwrap();
    assert_eq!(state.bodies.as_slice(), std::slice::from_ref(&body.id));
    assert_eq!(state.geometry.bodies, [body]);
    assert_eq!(state.losses.len(), 1);
    assert_eq!(state.losses[0].code, record_undecoded_kind());
    assert!(state.losses[0].message.contains("sldprt:brep:body#43"));
}

#[test]
fn a_replayed_partition_whose_body_was_withheld_does_not_state_an_empty_model() {
    let state = state_records(Brep {
        dropped: vec![withheld("sldprt:brep:body#43")],
        ..Brep::default()
    })
    .unwrap();
    assert!(matches!(state, Err(reason) if reason == "the state has no readable body"));
}
