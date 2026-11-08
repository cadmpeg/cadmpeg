// SPDX-License-Identifier: Apache-2.0

use super::decode;
use crate::test_support::test_b2::planar_two_edge_sheet;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Sense;
use cadmpeg_test_support::catia::joined_planar_sheet;

const FORWARD_SHEET_ROSTER: u8 = 0x33;
const CLOSED_LOOP_TAIL: u8 = 0x01;

#[test]
fn native_owned_sheet_decodes_joined_faces() {
    let decoded = decode(joined_planar_sheet(FORWARD_SHEET_ROSTER, CLOSED_LOOP_TAIL));
    assert_joined_faces(decoded.ir());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code.local_code() == "topology.owned-patch-subset" }));
    let inference = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "topology.owned-patch-sense-inferred")
        .expect("roster-bit face sense is reported as inferred");
    assert!(inference.message.contains("2 non-rolling-ball sheet faces"));
    let exactness = decoded.source_fidelity().annotations.exactness();
    for face in &decoded.ir().model.faces {
        assert_eq!(
            exactness[face.id.as_str()].fields()["sense"],
            cadmpeg_ir::Exactness::Inferred
        );
    }
}

#[test]
fn native_owned_sheet_rejects_an_invalid_roster_or_loop() {
    let invalid_roster = decode(joined_planar_sheet(0x31, CLOSED_LOOP_TAIL));
    assert!(invalid_roster.ir().model.faces.is_empty());
    let invalid_loop = decode(joined_planar_sheet(FORWARD_SHEET_ROSTER, 0x05));
    assert_eq!(invalid_loop.ir().model.faces.len(), 1);
    assert!(invalid_loop
        .ir()
        .model
        .coedges
        .iter()
        .all(|side| side.radial_next == side.id));
}

#[test]
fn planar_two_edge_sheet_reports_inferred_binding_and_face_sense() {
    let decoded = decode(planar_two_edge_sheet(0x1b));
    let [face] = decoded.ir().model.faces.as_slice() else {
        panic!("one recovered planar face");
    };
    assert_eq!(decoded.ir().model.edges.len(), 2);
    assert_eq!(face.sense, Sense::Reversed);
    let notes = decoded.source_fidelity().annotations.exactness();
    assert_eq!(
        notes[face.id.as_str()].fields()["sense"],
        cadmpeg_ir::Exactness::Inferred
    );
    let inference = decoded
        .report()
        .losses
        .iter()
        .find(|loss| loss.code.local_code() == "topology.two-edge-binding-inferred")
        .expect("two-edge binding loss");
    assert!(inference
        .message
        .contains("pcurve binding from endpoint proximity"));
    assert!(inference
        .message
        .contains("face sense from the class-61 singleton occurrence bit"));
}

#[test]
fn planar_two_edge_sheet_requires_matching_owner_complement() {
    let decoded = decode(planar_two_edge_sheet(0x1f));
    assert!(decoded.ir().model.surfaces.is_empty());
    assert!(decoded.ir().model.curves.is_empty());
    assert!(decoded.ir().model.edges.is_empty());
    assert!(decoded.ir().model.faces.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| { loss.code.local_code() != "topology.two-edge-binding-inferred" }));
}

fn assert_joined_faces(ir: &CadIr) {
    assert_eq!(ir.model.faces.len(), 2);
    assert_eq!(ir.model.edges.len(), 7);
    assert!(ir
        .model
        .faces
        .iter()
        .all(|face| face.sense == Sense::Forward));
    let shared: Vec<_> = ir
        .model
        .coedges
        .iter()
        .filter(|side| side.radial_next != side.id)
        .collect();
    let [first, second] = shared.as_slice() else {
        panic!("one edge shared by two coedges");
    };
    assert_eq!(first.edge, second.edge);
    assert_eq!(first.radial_next, second.id);
    assert_eq!(second.radial_next, first.id);
    assert_ne!(first.sense, second.sense);
}
