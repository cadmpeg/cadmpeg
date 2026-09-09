// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::CreoLossCode;
use crate::CreoCodec;

#[test]
fn headset_hook_holder_reports_zero_solution_carrier_samples() {
    let bytes = include_bytes!("../../tests/fixtures/headset-hook-holder.prt");
    let result = CreoCodec
        .decode(
            &mut Cursor::new(bytes.as_slice()),
            &DecodeOptions::default(),
        )
        .expect("headset hook holder decode");
    let coverage = result.report().coverage();
    assert!(coverage
        .get("brep_vertex_carrier_zero_candidate_count")
        .is_some_and(|count| *count > 0));
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.code == CreoLossCode::BrepTransferIncomplete.kind())
        .expect("B-rep rejection loss");
    let (_, samples) = loss
        .message
        .split_once("rejection samples: ")
        .expect("carrier rejection samples");
    assert!(samples.contains("[faces:"));
    assert!(samples.contains(";carriers:"));
    assert!(samples.contains(";unique:0]"));
}
