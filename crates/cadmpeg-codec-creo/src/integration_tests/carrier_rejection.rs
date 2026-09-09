// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::loss::CreoLossCode;
use crate::CreoCodec;

#[test]
fn parallel_carriers_report_zero_solution_samples() {
    let mut payload = b"srf_array\0\xf8\x04".to_vec();
    for (surface, height) in [(1, 0.0), (2, 1.0), (3, 2.0), (4, 4.0)] {
        crate::test_support::push_generated_plane_row(
            &mut payload,
            surface,
            false,
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, height],
        );
    }
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\x06topol_ref_data\0");
    for (curve, faces, next) in [
        (10, [1, 2], [12, 13]),
        (11, [1, 3], [10, 15]),
        (12, [1, 4], [11, 14]),
        (13, [2, 3], [14, 11]),
        (14, [2, 4], [10, 15]),
        (15, [3, 4], [13, 12]),
    ] {
        crate::test_support::push_generated_topology_row(&mut payload, curve, faces, next);
    }
    let bytes = crate::test_support::build_prt("synthetic", &[("VisibGeom", payload)]);
    let result = CreoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic carrier decode");
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
