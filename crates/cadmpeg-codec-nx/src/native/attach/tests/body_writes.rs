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
        endpoint_tag: 0x10,
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

fn body_identity_use(
    id: &str,
    write: &str,
    binding: &str,
) -> crate::native::features::FeatureOperationBodyIdentitySegmentUse {
    crate::native::features::FeatureOperationBodyIdentitySegmentUse {
        id: id.into(),
        operation_body_write: write.into(),
        body_identity: 17,
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
        (
            "binding-a",
            vec![BodyId::mint("body-a").expect("identity grammar")],
        ),
        (
            "binding-b",
            vec![
                BodyId::mint("body-b1").expect("identity grammar"),
                BodyId::mint("body-b2").expect("identity grammar"),
            ],
        ),
    ]);

    let outputs = super::operation_body_image_outputs_by_write(&uses, &bodies);

    assert_eq!(
        outputs.get("write-a"),
        Some(&BodyId::mint("body-a").expect("identity grammar"))
    );
    assert!(!outputs.contains_key("write-b"));
}

#[test]
fn complete_body_image_outputs_reject_partial_and_duplicate_results() {
    let write_a = native_body_write("write-a");
    let write_b = native_body_write("write-b");
    let writes = [&write_a, &write_b];
    let complete = BTreeMap::from([
        ("write-a", BodyId::mint("body-a").expect("identity grammar")),
        ("write-b", BodyId::mint("body-b").expect("identity grammar")),
    ]);
    assert_eq!(
        super::complete_operation_body_image_outputs(&writes, &complete),
        [
            BodyId::mint("body-a").expect("identity grammar"),
            BodyId::mint("body-b").expect("identity grammar")
        ]
    );

    let partial = BTreeMap::from([("write-a", BodyId::mint("body-a").expect("identity grammar"))]);
    assert!(super::complete_operation_body_image_outputs(&writes, &partial).is_empty());

    let duplicate = BTreeMap::from([
        ("write-a", BodyId::mint("body").expect("identity grammar")),
        ("write-b", BodyId::mint("body").expect("identity grammar")),
    ]);
    assert!(super::complete_operation_body_image_outputs(&writes, &duplicate).is_empty());
}

fn native_boolean(
    target_object_index: u32,
    tool_object_indices: Vec<u32>,
) -> crate::native::features::FeatureBooleanOperation {
    crate::native::features::FeatureBooleanOperation {
        id: "boolean".into(),
        operation_label: "operation".into(),
        kind: crate::native::features::FeatureBooleanKind::Subtract,
        target_object_index,
        raw_target_object_index: vec![1],
        target_source_offset: 0,
        raw_tool_object_indices: vec![vec![2]; tool_object_indices.len()],
        tool_source_offsets: vec![0; tool_object_indices.len()],
        tool_object_indices,
        source_offset: 0,
    }
}

#[test]
fn boolean_body_write_requires_one_target_image_and_excludes_tools() {
    let mut write = native_body_write("write");
    write.body_image_object_index = 40;
    let boolean = native_boolean(40, vec![41, 42]);

    assert!(super::body_writes_match_boolean_target(&[&write], None));
    assert!(super::body_writes_match_boolean_target(&[], Some(&boolean)));
    assert!(super::body_writes_match_boolean_target(
        &[&write],
        Some(&boolean)
    ));

    let wrong_target = native_boolean(43, vec![41, 42]);
    assert!(!super::body_writes_match_boolean_target(
        &[&write],
        Some(&wrong_target)
    ));
    let target_is_tool = native_boolean(40, vec![40, 41]);
    assert!(!super::body_writes_match_boolean_target(
        &[&write],
        Some(&target_is_tool)
    ));
    assert!(!super::body_writes_match_boolean_target(
        &[&write, &write],
        Some(&boolean)
    ));
}

#[test]
fn duplicate_body_image_uses_do_not_assign_an_output() {
    let uses = [
        body_image_use("use-a", "write", "binding-a"),
        body_image_use("use-b", "write", "binding-b"),
    ];
    let bodies = BTreeMap::from([(
        "binding-a",
        vec![BodyId::mint("body-a").expect("identity grammar")],
    )]);

    assert!(super::operation_body_image_outputs_by_write(&uses, &bodies).is_empty());
}

#[test]
fn body_identity_outputs_require_one_body_per_unique_plain_binding() {
    let uses = [
        body_identity_use("use-a", "write-a", "binding-a"),
        body_identity_use("use-b", "write-b", "binding-b"),
    ];
    let bodies = BTreeMap::from([
        (
            "binding-a",
            vec![BodyId::mint("body-a").expect("identity grammar")],
        ),
        (
            "binding-b",
            vec![
                BodyId::mint("body-b1").expect("identity grammar"),
                BodyId::mint("body-b2").expect("identity grammar"),
            ],
        ),
    ]);

    let outputs = super::operation_body_identity_outputs_by_write(&uses, &bodies);

    assert_eq!(
        outputs.get("write-a"),
        Some(&BodyId::mint("body-a").expect("identity grammar"))
    );
    assert!(!outputs.contains_key("write-b"));
}

#[test]
fn conflicting_body_output_witnesses_remain_unresolved() {
    let mut outputs =
        BTreeMap::from([("write", BodyId::mint("body-a").expect("identity grammar"))]);
    let mut conflicts = BTreeSet::new();

    super::merge_operation_body_outputs(
        &mut outputs,
        &mut conflicts,
        [("write", BodyId::mint("body-b").expect("identity grammar"))],
    );
    super::merge_operation_body_outputs(
        &mut outputs,
        &mut conflicts,
        [("write", BodyId::mint("body-a").expect("identity grammar"))],
    );

    assert!(!outputs.contains_key("write"));
    assert!(conflicts.contains("write"));
}

#[test]
fn group_partition_witness_projects_every_write_of_the_bound_body_identity() {
    let write_a = native_body_write("write-a");
    let mut write_b = native_body_write("write-b");
    write_b.group_node = 2;
    let use_ = crate::native::features::FeatureBodyWriteGroupPartitionUse {
        id: "partition-use".into(),
        body_write: "unlabeled-write".into(),
        body_identity: 17,
        group_node: 3,
        partition_stream_ordinal: 4,
        parasolid_group_records: vec!["group".into()],
        parasolid_group_members: Vec::new(),
    };
    let body = cadmpeg_ir::topology::Body {
        id: BodyId::mint("nx:s4:body#8").expect("identity grammar"),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };

    let writes = [write_a, write_b];
    let outputs = super::operation_body_group_partition_outputs_by_write(
        &writes,
        &[use_],
        std::slice::from_ref(&body),
    );

    assert_eq!(outputs.get("write-a"), Some(&body.id));
    assert_eq!(outputs.get("write-b"), Some(&body.id));
}

fn group_member(
    id: &str,
    family: &str,
    current_member_xmt: Option<u32>,
) -> crate::native::parasolid::ParasolidGroupMember {
    crate::native::parasolid::ParasolidGroupMember {
        id: id.into(),
        partition_stream_ordinal: 4,
        group_xmt: 10,
        group_node_id: 7,
        ordinal: 0,
        list_record_xmt: 20,
        member_xmt: 30,
        member_family: family.into(),
        member_node_id: Some(50),
        current_member_xmt,
    }
}

fn group_use(member_ids: &[&str]) -> crate::native::features::FeatureOperationBodyPartitionUse {
    crate::native::features::FeatureOperationBodyPartitionUse {
        id: "partition-use".into(),
        operation_body_write: "write".into(),
        body_image_segment_use: "image-use".into(),
        segment_body_binding: "binding".into(),
        partition_stream_ordinal: 4,
        group_node: 7,
        parasolid_group_records: Vec::new(),
        parasolid_group_members: member_ids.iter().map(|id| (*id).into()).collect(),
    }
}

fn direct_group_use(
    member_ids: &[&str],
) -> crate::native::features::FeatureBodyWriteGroupPartitionUse {
    crate::native::features::FeatureBodyWriteGroupPartitionUse {
        id: "direct-partition-use".into(),
        body_write: "write".into(),
        body_identity: 17,
        group_node: 7,
        partition_stream_ordinal: 4,
        parasolid_group_records: Vec::new(),
        parasolid_group_members: member_ids.iter().map(|id| (*id).into()).collect(),
    }
}

#[test]
fn result_topology_uses_only_unique_current_group_members() {
    let use_ = group_use(&["face", "edge", "vertex", "historical", "shell"]);
    let members = [
        group_member("face", "FACE", Some(40)),
        group_member("edge", "EDGE", Some(41)),
        group_member("vertex", "VERTEX", Some(42)),
        group_member("historical", "FACE", None),
        group_member("shell", "SHELL", Some(43)),
    ];
    let result = super::feature_result_group_members(
        use_.partition_stream_ordinal,
        &use_.parasolid_group_members,
        &members,
    );

    assert_eq!(result.faces, ["nx:s4:face#40"]);
    assert_eq!(result.edges, ["nx:s4:edge#41"]);
    assert_eq!(result.vertices, ["nx:s4:vertex#42"]);

    let duplicate_members = [
        group_member("face", "FACE", Some(40)),
        group_member("face", "FACE", Some(40)),
    ];
    assert!(
        super::feature_result_group_members(4, &["face".into()], &duplicate_members)
            .faces
            .is_empty()
    );
}

#[test]
fn result_topology_accepts_either_partition_witness_and_rejects_disagreement() {
    let members = [
        group_member("face", "FACE", Some(40)),
        group_member("edge", "EDGE", Some(41)),
    ];
    let image = group_use(&["face"]);
    let direct = direct_group_use(&["face"]);

    let from_image = super::operation_body_write_result_group_members(
        "write",
        std::slice::from_ref(&image),
        &[],
        &members,
    );
    let from_direct = super::operation_body_write_result_group_members(
        "write",
        &[],
        std::slice::from_ref(&direct),
        &members,
    );
    assert_eq!(from_image.faces, ["nx:s4:face#40"]);
    assert_eq!(from_direct.faces, from_image.faces);

    let conflict = direct_group_use(&["edge"]);
    let rejected =
        super::operation_body_write_result_group_members("write", &[image], &[conflict], &members);
    assert!(rejected.faces.is_empty());
    assert!(rejected.edges.is_empty());
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
    assert_eq!(block.source_properties["body_write.0.endpoint_tag"], "16");
    assert_eq!(
        extrude.source_properties["body_write.0.body_identity"],
        "17"
    );

    let results = &result.ir().model.feature_result_topologies;
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].bodies, results[1].bodies);
    assert_ne!(results[0].native_ref, results[1].native_ref);
}
