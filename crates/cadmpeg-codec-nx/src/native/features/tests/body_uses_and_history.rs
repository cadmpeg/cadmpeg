// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn segment_body_lineage_statuses_cover_every_bound_image() {
    use super::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
    };
    use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};
    let labels = [
        FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "EXTRUDE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 0,
        },
        FeatureOperationLabel {
            id: "operation#1".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 1,
            value: "UNITE".to_string(),
            object_indices: [None; 4],
            raw_object_indices: std::array::from_fn(|_| vec![0xff]),
            source_offset: 1,
        },
    ];
    let references = [FeatureBodyReference {
        id: "reference#0".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 10,
        raw_body_object_index: vec![10],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 10,
        raw_target_object_index: vec![10],
        target_source_offset: 1,
        tool_object_indices: vec![21],
        raw_tool_object_indices: vec![vec![21]],
        tool_source_offsets: vec![1],
        source_offset: 1,
    }];
    let binding =
        |id: &str, stream_ordinal: u32, stream_kind: &str, body, alias| SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("stream#{stream_ordinal}"),
            stream_ordinal,
            stream_kind: stream_kind.to_string(),
            body_object_index: body,
            body_alias_object_index: alias,
            stream_role: 19,
            source_offset: u64::from(stream_ordinal),
        };
    let statuses = segment_body_lineage_statuses(
        &labels,
        &references,
        &[],
        &[],
        &booleans,
        &[],
        &[
            binding("binding#0", 0, "partition", 10, 11),
            binding("binding#1", 1, "plain", 20, 21),
        ],
        &[],
    )
    .expect("required invariant");
    assert_eq!(statuses.len(), 2);
    assert!(statuses[0].terminal);
    assert!(!statuses[1].terminal);
}

#[test]
fn unique_feature_body_references_require_one_field_per_operation() {
    let reference =
        |id: &str, operation_label: &str, body_object_index| super::FeatureBodyReference {
            id: id.to_string(),
            operation_label: operation_label.to_string(),
            body_object_index,
            raw_body_object_index: vec![body_object_index as u8],
            source_offset: 0,
        };
    let references = [
        reference("reference#0", "operation#0", 10),
        reference("reference#1", "operation#0", 11),
        reference("reference#2", "operation#1", 12),
    ];
    let unique = super::unique_feature_body_references(&references);
    assert!(!unique.contains_key("operation#0"));
    assert_eq!(unique["operation#1"].id, "reference#2");
}

#[test]
fn feature_body_segment_uses_require_one_alias_pair() {
    use super::{feature_body_segment_uses, FeatureBodyReference};
    use crate::native::segments::SegmentBodyBinding;
    let reference = FeatureBodyReference {
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body_object_index: 11,
        raw_body_object_index: vec![11],
        source_offset: 90,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#3".into(),
        stream_ordinal: 3,
        stream_kind: "plain".into(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 40,
    };
    let uses = feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        std::slice::from_ref(&binding),
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].feature_body_reference, reference.id);
    assert_eq!(uses[0].segment_body_binding, binding.id);
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        &[binding.clone(), binding.clone()]
    )
    .is_empty());
    let duplicate_reference = FeatureBodyReference {
        id: "nx:feature-history:body-reference#1".into(),
        operation_label: reference.operation_label.clone(),
        body_object_index: 12,
        raw_body_object_index: vec![12],
        source_offset: 91,
    };
    assert!(feature_body_segment_uses(
        &[reference, duplicate_reference],
        &[],
        &[],
        &[],
        std::slice::from_ref(&binding),
    )
    .is_empty());
}

#[test]
fn feature_body_segment_uses_bridge_unique_offset_store_aliases() {
    use super::{feature_body_segment_uses, FeatureBodyDataBlockUse, FeatureBodyReference};
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        id: "reference#0".into(),
        operation_label: "operation#0".into(),
        body_object_index: 11,
        raw_body_object_index: vec![11],
        source_offset: 90,
    };
    let data_block_use = FeatureBodyDataBlockUse {
        id: "data-block-use#0".into(),
        feature_body_reference: reference.id.clone(),
        data_block: "block#11".into(),
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: 0,
        object_index: 3,
        raw_object_index: vec![3],
        data_block: "block#3".into(),
        source_offset: 80,
    };
    let blocks = [
        DataBlock {
            id: "block#3".into(),
            section_ordinal: 2,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 1,
            sha256: String::new(),
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#11".into(),
            section_ordinal: 2,
            block_ordinal: 11,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 1,
            sha256: String::new(),
            source_entry: String::new(),
            source_offset: 0,
        },
    ];
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: "partition".into(),
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 40,
    };
    let uses = feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].segment_body_binding, "binding#0");

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[data_block_use.clone(), data_block_use.clone()],
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
    )
    .is_empty());

    let second_input = FeatureInputBlock {
        id: "input#1".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: 1,
        object_index: 4,
        raw_object_index: vec![4],
        data_block: "block#4".into(),
        source_offset: 81,
    };
    let second_block = DataBlock {
        id: "block#4".into(),
        section_ordinal: 3,
        block_ordinal: 4,
        role: DataBlockRole::Column,
        section_offset: 0,
        byte_len: 1,
        sha256: String::new(),
        source_entry: String::new(),
        source_offset: 0,
    };
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        &[input.clone(), second_input],
        &[blocks[0].clone(), blocks[1].clone(), second_block],
        std::slice::from_ref(&binding),
    )
    .is_empty());

    let mut duplicate_alias = binding.clone();
    duplicate_alias.id = "binding#1".into();
    duplicate_alias.body_object_index = 20;
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        &[binding.clone(), duplicate_alias],
    )
    .is_empty());

    let mut primary_collision = binding.clone();
    primary_collision.id = "binding#2".into();
    primary_collision.body_object_index = 11;
    primary_collision.body_alias_object_index = 12;
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        &[binding, primary_collision],
    )
    .is_empty());
}

#[test]
fn feature_body_segment_uses_reject_primary_index_offset_collision() {
    use super::{feature_body_segment_uses, FeatureBodyDataBlockUse, FeatureBodyReference};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        id: "reference#0".into(),
        operation_label: "operation#0".into(),
        body_object_index: 11,
        raw_body_object_index: vec![11],
        source_offset: 90,
    };
    let data_block_use = FeatureBodyDataBlockUse {
        id: "data-block-use#0".into(),
        feature_body_reference: reference.id.clone(),
        data_block: "block#11".into(),
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: "partition".into(),
        body_object_index: 11,
        body_alias_object_index: 12,
        stream_role: 19,
        source_offset: 40,
    };
    assert!(
        feature_body_segment_uses(&[reference], &[data_block_use], &[], &[], &[binding],)
            .is_empty()
    );
}

#[test]
fn feature_body_segment_uses_exclude_missing_offset_store_ordinals() {
    use super::{feature_body_segment_uses, FeatureBodyReference, FeatureInputBlock};
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        id: "reference#99".into(),
        operation_label: "operation#0".into(),
        body_object_index: 99,
        raw_body_object_index: vec![99],
        source_offset: 90,
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: 0,
        object_index: 3,
        raw_object_index: vec![3],
        data_block: "block#3".into(),
        source_offset: 80,
    };
    let block = DataBlock {
        id: "block#3".into(),
        section_ordinal: 2,
        block_ordinal: 3,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: "00".into(),
        source_entry: "part".into(),
        source_offset: 20,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: "plain".into(),
        body_object_index: 99,
        body_alias_object_index: 100,
        stream_role: 19,
        source_offset: 40,
    };

    assert!(
        feature_body_segment_uses(&[reference], &[], &[input], &[block], &[binding]).is_empty()
    );
}

#[test]
fn feature_body_segment_uses_exclude_ambiguous_offset_store_namespaces() {
    use super::{feature_body_segment_uses, FeatureBodyReference, FeatureInputBlock};
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        id: "reference#99".into(),
        operation_label: "operation#0".into(),
        body_object_index: 99,
        raw_body_object_index: vec![99],
        source_offset: 90,
    };
    let input = |slot: u8, object_index: u32, data_block: &str| FeatureInputBlock {
        id: format!("input#{slot}"),
        operation_label: reference.operation_label.clone(),
        input_slot: slot,
        object_index,
        raw_object_index: vec![object_index as u8],
        data_block: data_block.into(),
        source_offset: 80 + u64::from(slot),
    };
    let block = |id: &str, section_ordinal: u32, block_ordinal: u32| DataBlock {
        id: id.into(),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: "00".into(),
        source_entry: "part".into(),
        source_offset: 20,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: "plain".into(),
        body_object_index: 99,
        body_alias_object_index: 100,
        stream_role: 19,
        source_offset: 40,
    };

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[input(0, 3, "block#3"), input(1, 4, "block#4"),],
        &[block("block#3", 2, 3), block("block#4", 3, 4)],
        &[binding],
    )
    .is_empty());
}

#[test]
fn feature_body_data_block_uses_inherit_the_operation_input_store() {
    use super::{feature_body_data_block_uses, FeatureBodyReference, FeatureInputBlock};
    use crate::native::om::{DataBlock, DataBlockRole};

    let reference = FeatureBodyReference {
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body_object_index: 72,
        raw_body_object_index: vec![72],
        source_offset: 90,
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: "operation#0".into(),
        input_slot: 0,
        object_index: 3,
        raw_object_index: vec![3],
        data_block: "nx:om-data-blocks-2:block#3".into(),
        source_offset: 80,
    };
    let block = |id: &str, section_ordinal, block_ordinal| DataBlock {
        id: id.into(),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: "00".into(),
        source_entry: "part".into(),
        source_offset: 20,
    };
    let blocks = [
        block("nx:om-data-blocks-2:block#3", 2, 3),
        block("nx:om-data-blocks-1:block#72", 1, 72),
        block("nx:om-data-blocks-2:block#72", 2, 72),
    ];
    let uses = feature_body_data_block_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&input),
        &blocks,
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].data_block, blocks[2].id);
    let duplicate_reference = FeatureBodyReference {
        id: "nx:feature-history:body-reference#1".into(),
        operation_label: "operation#0".into(),
        body_object_index: 73,
        raw_body_object_index: vec![73],
        source_offset: 91,
    };
    assert!(
        feature_body_data_block_uses(&[reference, duplicate_reference], &[input], &blocks,)
            .is_empty()
    );
}

#[test]
fn feature_body_lineage_closes_overlapping_alias_pairs_transitively() {
    use super::{
        FeatureBodyReference, FeatureBooleanKind, FeatureBooleanOperation, FeatureOperationLabel,
    };
    use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: 1 - u64::from(ordinal),
    };
    let labels = [label(1, "UNITE"), label(0, "EXTRUDE")];
    let references = [FeatureBodyReference {
        id: "reference#30".to_string(),
        operation_label: "operation#0".to_string(),
        body_object_index: 30,
        raw_body_object_index: vec![30],
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target_object_index: 99,
        raw_target_object_index: vec![99],
        target_source_offset: 1,
        tool_object_indices: vec![10],
        raw_tool_object_indices: vec![vec![10]],
        tool_source_offsets: vec![1],
        source_offset: 1,
    }];
    let binding = |id: &str, stream_ordinal, body, alias| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("stream#{stream_ordinal}"),
        stream_ordinal,
        stream_kind: "partition".to_string(),
        body_object_index: body,
        body_alias_object_index: alias,
        stream_role: 19,
        source_offset: u64::from(stream_ordinal),
    };
    let bindings = [
        binding("binding#0", 0, 10, 20),
        binding("binding#1", 1, 30, 20),
        binding("binding#2", 2, 40, 20),
    ];

    let statuses = segment_body_lineage_statuses(
        &labels,
        &references,
        &[],
        &[],
        &booleans,
        &[],
        &bindings,
        &[],
    )
    .expect("required invariant");
    assert_eq!(statuses.len(), 3);
    assert!(statuses.iter().all(|status| !status.terminal));
}

#[test]
fn nx_simple_hole_construction_groups_require_shared_four_block_identity() {
    use super::{
        feature_simple_hole_construction_groups, FeatureOperationLabel,
        FeatureSimpleHoleRepeatedScalarLane, FeatureSimpleHoleRepeatedScalarLaneBlockReferences,
    };
    let label = |id: &str, ordinal: u32| FeatureOperationLabel {
        id: id.into(),
        section_link: "section#1".into(),
        ordinal,
        value: "SIMPLE HOLE".into(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let lane = |operation: &str| FeatureSimpleHoleRepeatedScalarLane {
        id: format!("lane-{operation}"),
        operation_label: operation.into(),
        values: vec![25.4],
        raw_values: vec![[0x30; 8]],
        first_witness_offsets: vec![1],
        second_witness_offsets: vec![2],
    };
    let reference =
        |operation: &str, last: &str| FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
            id: format!("reference-{operation}"),
            operation_label: operation.into(),
            first_data_blocks: ["block-1".into(), "block-2".into()],
            second_data_blocks: ["block-3".into(), last.into()],
            first_reference_prefix: None,
            second_reference_prefix: None,
            first_reference_offsets: [3, 4],
            second_reference_offsets: [5, 6],
        };
    let lanes = [
        lane("operation#1-2"),
        lane("operation#1-3"),
        lane("operation#1-4"),
    ];
    let references = [
        reference("operation#1-4", "block-5"),
        reference("operation#1-3", "block-4"),
        reference("operation#1-2", "block-4"),
    ];
    // The native label arena is newest-first. The group must reverse that
    // source order, rather than infer history from operation-label text.
    let labels = [
        label("operation#1-2", 0),
        label("operation#1-3", 1),
        label("operation#1-4", 2),
    ];
    let groups = feature_simple_hole_construction_groups(&labels, &lanes, &references);
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].operation_labels,
        ["operation#1-3", "operation#1-2"]
    );
    assert_eq!(
        groups[0].scalar_lanes,
        ["lane-operation#1-3", "lane-operation#1-2"]
    );
    assert_eq!(
        groups[0].block_references,
        ["reference-operation#1-3", "reference-operation#1-2"]
    );

    let duplicate_references = [
        reference("operation#1-2", "block-4"),
        reference("operation#1-2", "block-4"),
    ];
    assert!(
        feature_simple_hole_construction_groups(&labels, &lanes, &duplicate_references).is_empty()
    );

    let duplicate_lanes = [
        lane("operation#1-2"),
        lane("operation#1-2"),
        lane("operation#1-3"),
        lane("operation#1-4"),
    ];
    let shared_references = [
        reference("operation#1-2", "block-4"),
        reference("operation#1-3", "block-4"),
        reference("operation#1-4", "block-4"),
    ];
    assert!(
        feature_simple_hole_construction_groups(&labels, &duplicate_lanes, &shared_references)
            .is_empty()
    );

    let unknown_lanes = [lane("operation#1-8"), lane("operation#1-9")];
    let unknown_references = [
        reference("operation#1-8", "block-4"),
        reference("operation#1-9", "block-4"),
    ];
    assert!(
        feature_simple_hole_construction_groups(&labels, &unknown_lanes, &unknown_references)
            .is_empty()
    );
}

#[test]
fn nx_hole_package_group_uses_require_one_exact_lane_and_group() {
    use super::{
        feature_hole_package_construction_group_uses, FeatureHolePackageConstructionGroupLane,
        FeatureSimpleHoleConstructionGroup,
    };
    let blocks = [
        "block-1".to_string(),
        "block-2".to_string(),
        "block-3".to_string(),
        "block-4".to_string(),
    ];
    let lane = FeatureHolePackageConstructionGroupLane {
        id: "package-lane".into(),
        operation_label: "package-operation".into(),
        selector: 0x46,
        branch: 0x11,
        object_indices: [1, 2, 3, 4],
        raw_object_indices: std::array::from_fn(|index| vec![0xf0, index as u8 + 1]),
        data_blocks: blocks.clone(),
        payload_offset: 20,
        source_offset: 120,
        reference_source_offsets: [132, 134, 141, 143],
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "simple-hole-group".into(),
        first_data_blocks: [blocks[0].clone(), blocks[1].clone()],
        second_data_blocks: [blocks[2].clone(), blocks[3].clone()],
        operation_labels: vec!["simple-hole-1".into(), "simple-hole-2".into()],
        scalar_lanes: vec!["scalar-1".into(), "scalar-2".into()],
        block_references: vec!["references-1".into(), "references-2".into()],
    };

    let uses = feature_hole_package_construction_group_uses(
        std::slice::from_ref(&lane),
        std::slice::from_ref(&group),
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].operation_label, lane.operation_label);
    assert_eq!(uses[0].construction_group_lane, lane.id);
    assert_eq!(uses[0].simple_hole_construction_group, group.id);

    assert!(feature_hole_package_construction_group_uses(
        &[lane.clone(), lane.clone()],
        std::slice::from_ref(&group),
    )
    .is_empty());
    assert!(feature_hole_package_construction_group_uses(
        std::slice::from_ref(&lane),
        &[group.clone(), group],
    )
    .is_empty());
}

#[test]
fn nx_block_payload_points_require_exactly_two_named_scalars() {
    use super::{
        feature_block_payload_point_groups, feature_block_payload_points, FeatureBlockPayloadName,
        FeatureBlockPayloadNamedRecord, FeatureBlockPayloadScalar,
    };

    let operation_label = "operation".to_string();
    let construction_payload = "payload".to_string();
    let name = FeatureBlockPayloadName {
        id: "name".to_string(),
        operation_label: operation_label.clone(),
        construction_payload: construction_payload.clone(),
        ordinal: 0,
        type_code: Some(131),
        raw_type_code: Some(vec![0x80, 0x83]),
        type_code_payload_offset: Some(11),
        type_code_source_offset: Some(101),
        payload_leading: false,
        value: "Point7".to_string(),
        payload_offset: 10,
        source_offset: 100,
    };
    let scalar = |id: &str, ordinal: u32, value: f64| {
        let mut raw_value = value.to_be_bytes();
        raw_value[0] -= 0x10;
        FeatureBlockPayloadScalar {
            id: id.to_string(),
            operation_label: operation_label.clone(),
            construction_payload: construction_payload.clone(),
            ordinal,
            field_code: 100,
            value,
            raw_value,
            payload_offset: 20 + u64::from(ordinal) * 13,
            source_offset: 110 + u64::from(ordinal) * 13,
        }
    };
    let scalars = [scalar("first", 0, 1.25), scalar("second", 1, -2.5)];
    let record = FeatureBlockPayloadNamedRecord {
        id: "record".to_string(),
        operation_label,
        construction_payload,
        name_field: name.id.clone(),
        scalar_fields: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
        payload_start_offset: 10,
        payload_end_offset: 50,
    };

    let points = feature_block_payload_points(
        std::slice::from_ref(&record),
        std::slice::from_ref(&name),
        &scalars,
    );
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point7");
    assert_eq!(points[0].coordinates, [1.25, -2.5]);

    let mut duplicate = points[0].clone();
    duplicate.id = "point-2".to_string();
    let groups = feature_block_payload_point_groups(&[points[0].clone(), duplicate]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].points.len(), 2);
    assert_eq!(groups[0].coordinates, [1.25, -2.5]);

    let mut conflicting = points[0].clone();
    conflicting.id = "conflicting".to_string();
    conflicting.coordinates[1] = f64::from_bits((-2.5_f64).to_bits() + 1);
    assert!(feature_block_payload_point_groups(&[points[0].clone(), conflicting]).is_empty());

    let mut incomplete = record.clone();
    incomplete.scalar_fields.pop();
    assert!(
        feature_block_payload_points(&[incomplete], std::slice::from_ref(&name), &scalars,)
            .is_empty()
    );
    let mut malformed = name;
    malformed.value = "Point0".to_string();
    assert!(feature_block_payload_points(&[record], &[malformed], &scalars).is_empty());
}

#[test]
fn operation_common_frame_types_the_parasolid_modification_field() {
    let mut state = [0; 8];
    assert_eq!(operation_modifies_parasolid_data(state), Some(false));
    state[4] = 1;
    assert_eq!(operation_modifies_parasolid_data(state), Some(true));
    state[4] = 2;
    assert_eq!(operation_modifies_parasolid_data(state), None);
}

#[test]
fn operation_common_frame_retains_the_split_tracking_data_field() {
    assert_eq!(
        operation_split_tracking_data([1, 2, 3, 0, 1, 0x56, 0xa9, 7]),
        [0x56, 0xa9]
    );
}

#[test]
fn operation_history_reverses_source_order_within_each_section() {
    let label = |section: &str, ordinal, value: &str| super::FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let labels = [
        label("first", 0, "newest-first"),
        label("first", 1, "oldest-first"),
        label("second", 0, "newest-second"),
        label("second", 1, "oldest-second"),
    ];

    let values = super::feature_operation_chronological_labels(&labels)
        .into_iter()
        .map(|label| label.value.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-first",
            "newest-first",
            "oldest-second",
            "newest-second"
        ]
    );
}

#[test]
fn operation_history_groups_interleaved_sections_before_reversing() {
    let label = |section: &str, ordinal, value: &str| super::FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset: u64::from(ordinal),
    };
    let labels = [
        label("first", 0, "newest-first"),
        label("second", 0, "newest-second"),
        label("first", 1, "oldest-first"),
        label("second", 1, "oldest-second"),
    ];

    let values = super::feature_operation_chronological_labels(&labels)
        .into_iter()
        .map(|label| label.value.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-first",
            "newest-first",
            "oldest-second",
            "newest-second"
        ]
    );
}

#[test]
fn operation_history_uses_serialized_offsets_for_section_and_member_order() {
    let label = |section: &str, ordinal, value: &str, source_offset| super::FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        object_indices: [None; 4],
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        source_offset,
    };
    let labels = [
        label("first", 1, "oldest-first", 210),
        label("second", 1, "oldest-second", 110),
        label("first", 0, "newest-first", 200),
        label("second", 0, "newest-second", 100),
    ];

    let values = super::feature_operation_chronological_labels(&labels)
        .into_iter()
        .map(|label| label.value.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-second",
            "newest-second",
            "oldest-first",
            "newest-first"
        ]
    );
}

#[test]
fn operation_common_frame_types_the_legacy_inactive_modules_field() {
    let mut state = [0; 8];
    assert_eq!(operation_legacy_inactive_modules(state), Some(false));
    state[3] = 1;
    assert_eq!(operation_legacy_inactive_modules(state), Some(true));
    state[3] = 2;
    assert_eq!(operation_legacy_inactive_modules(state), None);
}
