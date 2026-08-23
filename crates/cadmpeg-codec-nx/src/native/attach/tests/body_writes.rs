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

fn native_body_write(id: &str) -> crate::native::features::FeatureOperationBodyWrite {
    crate::native::features::FeatureOperationBodyWrite {
        id: id.into(),
        operation_label: "operation".into(),
        operation_record: "record".into(),
        ordinal: 0,
        body_identity: 17,
        group_node: 1,
        raw_group_node: vec![1],
        group_node_source_offset: 0,
        body_image_object_index: 2,
        body_image_data_block: Some("block".into()),
        raw_body_image_object_index: vec![2],
        body_image_object_index_source_offset: 0,
        byte_len: 1,
        source_offset: 0,
    }
}

fn body_image_use(
    id: &str,
    write: &str,
    binding: &str,
) -> crate::native::features::FeatureOperationBodyImageSegmentUse {
    crate::native::features::FeatureOperationBodyImageSegmentUse {
        id: id.into(),
        operation_body_write: write.into(),
        body_image_data_block: "block".into(),
        segment_body_binding: binding.into(),
    }
}

#[test]
fn body_image_outputs_require_one_body_per_binding() {
    let uses = [
        body_image_use("use-a", "write-a", "binding-a"),
        body_image_use("use-b", "write-b", "binding-b"),
    ];
    let bodies = BTreeMap::from([
        ("binding-a", vec![BodyId("body-a".into())]),
        (
            "binding-b",
            vec![BodyId("body-b1".into()), BodyId("body-b2".into())],
        ),
    ]);

    let outputs = super::operation_body_image_outputs_by_write(&uses, &bodies);

    assert_eq!(outputs.get("write-a"), Some(&BodyId("body-a".into())));
    assert!(!outputs.contains_key("write-b"));
}

#[test]
fn complete_body_image_outputs_reject_partial_and_duplicate_results() {
    let write_a = native_body_write("write-a");
    let write_b = native_body_write("write-b");
    let writes = [&write_a, &write_b];
    let complete = BTreeMap::from([
        ("write-a", BodyId("body-a".into())),
        ("write-b", BodyId("body-b".into())),
    ]);
    assert_eq!(
        super::complete_operation_body_image_outputs(&writes, &complete),
        [BodyId("body-a".into()), BodyId("body-b".into())]
    );

    let partial = BTreeMap::from([("write-a", BodyId("body-a".into()))]);
    assert!(super::complete_operation_body_image_outputs(&writes, &partial).is_empty());

    let duplicate = BTreeMap::from([
        ("write-a", BodyId("body".into())),
        ("write-b", BodyId("body".into())),
    ]);
    assert!(super::complete_operation_body_image_outputs(&writes, &duplicate).is_empty());
}

#[test]
fn duplicate_body_image_uses_do_not_assign_an_output() {
    let uses = [
        body_image_use("use-a", "write", "binding-a"),
        body_image_use("use-b", "write", "binding-b"),
    ];
    let bodies = BTreeMap::from([("binding-a", vec![BodyId("body-a".into())])]);

    assert!(super::operation_body_image_outputs_by_write(&uses, &bodies).is_empty());
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
