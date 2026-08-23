// SPDX-License-Identifier: Apache-2.0
//! Feature-output lineage from operation body-write frames.

use super::*;
use crate::test_support::{composed_feature_history_payload, prt_with_named_payloads};
use crate::NxCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use std::io::Cursor;

fn body_write(group: u8, image: u8) -> Vec<u8> {
    vec![
        0x01, 0x02, 0x11, group, 0x97, 0x75, 0x01, 0x02, 0x10, image, 0xff,
    ]
}

#[test]
fn repeated_body_identity_builds_output_lineage() {
    let payload = composed_feature_history_payload(
        &[
            (&[0xff; 4], "BLOCK", body_write(0x31, 0x41)),
            (&[0xff; 4], "EXTRUDE", body_write(0x32, 0x42)),
        ],
        &[],
    );
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("body-write fixture");
    let features = &result.ir().model.features;
    let [block, extrude] = features.as_slice() else {
        panic!("two modeling operations");
    };
    assert_eq!(
        block.dependencies.as_slice(),
        std::slice::from_ref(&extrude.id)
    );
    assert_eq!(block.source_properties["body_write.0.body_identity"], "17");
    assert_eq!(
        extrude.source_properties["body_write.0.body_identity"],
        "17"
    );

    let results = &result.ir().model.feature_result_topologies;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].bodies, results[1].bodies);
    assert_ne!(results[0].native_ref, results[1].native_ref);
}
