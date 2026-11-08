// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use super::decode;
use crate::test_support::test_sheet::joined_planar_sheet;
use cadmpeg_codec_step::StepCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::Sense;

const FORWARD_SHEET_ROSTER: u8 = 0x33;
const CLOSED_LOOP_TAIL: u8 = 0x01;

#[test]
fn native_owned_sheet_preserves_joined_faces_through_step() {
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

    let step = StepCodec::default();
    let mut exported = Vec::new();
    step.plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut exported))
        .expect("write decoded sheet as STEP");
    let readback = step
        .decode(&mut Cursor::new(exported), &DecodeOptions::default())
        .expect("read the exported STEP sheet");
    assert_joined_faces(readback.ir());
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
