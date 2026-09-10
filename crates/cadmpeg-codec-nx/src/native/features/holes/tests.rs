// SPDX-License-Identifier: Apache-2.0

use super::*;

use crate::native::features::test_support::check_lane_wire;

#[test]
fn symbolic_thread_text_frame_derives_marker() {
    let json = r#"{"id":"frame","symbolic_thread":"thread","ordinal":0,"marker":3,"value":"CUT","source_offset":10}"#;
    let frame: crate::native::features::holes::FeatureSymbolicThreadTextFrame =
        serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&frame).unwrap(), json);
    for marker in [0, 4, 255] {
        let invalid = json.replace("\"marker\":3", &format!("\"marker\":{marker}"));
        let error = serde_json::from_str::<
            crate::native::features::holes::FeatureSymbolicThreadTextFrame,
        >(&invalid)
        .unwrap_err();
        assert!(error.to_string().contains("marker"));
    }
}

#[test]
fn nx_symbolic_thread_retains_all_complete_type_three_text_frames() {
    let label = "SYMBOLIC_THREAD";
    let payload = b"\x03\x0bM Profile\0\x03\x0aM3_x_0.5\0\x03\x05CUT\0";
    let record = crate::om::operation_record::OperationPayload::new(payload, 500, label).unwrap();

    let frames = crate::native::features::holes::symbolic_thread_text_frames(record)
        .expect("two text frames");
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].marker, crate::om::OperationTextMarker::Text);
    assert_eq!(frames[0].offset, 500);
    assert_eq!(frames[0].value.as_str(), "M Profile");
    assert_eq!(frames[1].offset, 512);
    assert_eq!(frames[1].value.as_str(), "M3_x_0.5");
    assert_eq!(frames[2].offset, 523);
    assert_eq!(frames[2].value.as_str(), "CUT");
}

#[test]
fn nx_symbolic_thread_requires_two_complete_type_three_text_frames() {
    let label = "SYMBOLIC_THREAD";
    let payload = b"\x03\x0bM Profile\0\x03\x0aM3_x_0.5";
    let record = crate::om::operation_record::OperationPayload::new(payload, 500, label).unwrap();

    assert!(crate::native::features::holes::symbolic_thread_text_frames(record).is_none());
}

#[test]
fn nx_simple_hole_template_requires_exact_ordered_tokens() {
    use crate::native::features::holes::SimpleHoleEndTreatment;
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::SimpleHoleFamily;
    use crate::native::features::holes::SimpleHoleForm;
    use crate::native::features::FeatureOperationLabel;
    use crate::native::features::FeaturePayloadString;

    let label = FeatureOperationLabel {
        id: "operation#3".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 3,
        value: "SIMPLE HOLE".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 100,
    };
    let record = FeatureOperationRecord {
        id: "record#3".to_string(),
        operation_label: label.id.clone(),
        ordinal: 3,
        sha256: crate::native::hex::Sha256Hex::digest(b"a"),
        payload_sha256: crate::native::hex::Sha256Hex::digest(b"b"),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(90, 120, 40)
            .unwrap(),
    };
    let string = FeaturePayloadString {
        id: "payload-string#3-0".to_string(),
        operation_record: record.id.clone(),
        ordinal: 0,
        value: crate::payload_text::PayloadText::new(
            "Hole_GeneralHole_Simple_Through_StartChamfer_EndChamfer".to_string(),
        )
        .unwrap(),
        source_offset: 130,
    };
    let templates = crate::native::features::holes::feature_simple_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    );
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].payload_string, string.id);
    assert_eq!(templates[0].family, SimpleHoleFamily::GeneralHole);
    assert_eq!(templates[0].form, SimpleHoleForm::Simple);
    assert_eq!(templates[0].extent, SimpleHoleExtent::Through);
    assert_eq!(
        templates[0].start_treatment,
        SimpleHoleEndTreatment::Chamfer
    );
    assert_eq!(templates[0].end_treatment, SimpleHoleEndTreatment::Chamfer);

    assert_eq!(
        crate::native::features::holes::parse_simple_hole_template(
            "Hole_GeneralHole_Counterbored_Through"
        ),
        Some((
            SimpleHoleForm::Counterbored,
            SimpleHoleExtent::Through,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        crate::native::features::holes::parse_simple_hole_template("Hole_GeneralHole_Simple_Blind"),
        Some((
            SimpleHoleForm::Simple,
            SimpleHoleExtent::Blind,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        crate::native::features::holes::parse_simple_hole_template(
            "Hole_GeneralHole_Countersunk_Through"
        ),
        Some((
            SimpleHoleForm::Countersunk,
            SimpleHoleExtent::Through,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert_eq!(
        crate::native::features::holes::parse_simple_hole_template(
            "Hole_GeneralHole_Countersunk_Blind"
        ),
        Some((
            SimpleHoleForm::Countersunk,
            SimpleHoleExtent::Blind,
            SimpleHoleEndTreatment::None,
            SimpleHoleEndTreatment::None,
        ))
    );
    assert!(crate::native::features::holes::parse_simple_hole_template(
        "Hole_GeneralHole_Simple_Through"
    )
    .is_none());
    assert!(crate::native::features::holes::parse_simple_hole_template(
        "Hole_GeneralHole_Counterbored_Blind"
    )
    .is_none());

    let mut counterbored_label = label.clone();
    counterbored_label.id = "operation#4".to_string();
    counterbored_label.value = "CBORE_HOLE".to_string();
    let mut counterbored_record = record.clone();
    counterbored_record.id = "record#4".to_string();
    counterbored_record.operation_label = counterbored_label.id.clone();
    let counterbored_string = FeaturePayloadString {
        id: "payload-string#4-0".to_string(),
        operation_record: counterbored_record.id.clone(),
        ordinal: 0,
        value: crate::payload_text::PayloadText::new(
            "Hole_GeneralHole_Counterbored_Through".to_string(),
        )
        .unwrap(),
        source_offset: 130,
    };
    let counterbored_templates = crate::native::features::holes::feature_simple_hole_templates(
        &[counterbored_label],
        &[counterbored_record],
        &[counterbored_string],
    );
    let [counterbored_template] = counterbored_templates.as_slice() else {
        panic!("counterbored hole template was not admitted");
    };
    assert_eq!(counterbored_template.form, SimpleHoleForm::Counterbored);
    assert_eq!(counterbored_template.extent, SimpleHoleExtent::Through);

    let mut countersunk_label = label.clone();
    countersunk_label.id = "operation#5".to_string();
    countersunk_label.value = "CSUNK_HOLE".to_string();
    let mut countersunk_record = record.clone();
    countersunk_record.id = "record#5".to_string();
    countersunk_record.operation_label = countersunk_label.id.clone();
    let countersunk_string = FeaturePayloadString {
        id: "payload-string#5-0".to_string(),
        operation_record: countersunk_record.id.clone(),
        ordinal: 0,
        value: crate::payload_text::PayloadText::new(
            "Hole_GeneralHole_Countersunk_Blind".to_string(),
        )
        .unwrap(),
        source_offset: 130,
    };
    let countersunk_templates = crate::native::features::holes::feature_simple_hole_templates(
        &[countersunk_label],
        &[countersunk_record],
        &[countersunk_string],
    );
    let [countersunk_template] = countersunk_templates.as_slice() else {
        panic!("countersunk hole template was not admitted");
    };
    assert_eq!(countersunk_template.form, SimpleHoleForm::Countersunk);
    assert_eq!(countersunk_template.extent, SimpleHoleExtent::Blind);

    let mut duplicate = string.clone();
    duplicate.id = "payload-string#3-1".to_string();
    duplicate.ordinal = 1;
    duplicate.source_offset += 64;
    assert!(
        crate::native::features::holes::feature_simple_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &[string.clone(), duplicate],
        )
        .is_empty()
    );

    let unknown = FeaturePayloadString {
        id: "payload-string#3-1".to_string(),
        operation_record: record.id.clone(),
        ordinal: 1,
        value: crate::payload_text::PayloadText::new("Hole_Unknown".to_string()).unwrap(),
        source_offset: 194,
    };
    assert!(
        crate::native::features::holes::feature_simple_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &[string.clone(), unknown],
        )
        .is_empty()
    );

    let mut malformed = string;
    malformed.value = crate::payload_text::PayloadText::new(
        "Hole_GeneralHole_Simple_Through_EndChamfer_StartChamfer".to_string(),
    )
    .unwrap();
    assert!(
        crate::native::features::holes::feature_simple_hole_templates(
            &[label],
            &[record],
            &[malformed]
        )
        .is_empty()
    );
}

#[test]
fn nx_threaded_hole_template_requires_simple_hole_and_exact_tokens() {
    use crate::native::features::holes::SimpleHoleExtent;
    use crate::native::features::holes::ThreadedHoleFamily;
    use crate::native::features::FeatureOperationLabel;
    use crate::native::features::FeaturePayloadString;

    let label = FeatureOperationLabel {
        id: "operation#threaded".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 7,
        value: "SIMPLE HOLE".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 100,
    };
    let record = FeatureOperationRecord {
        id: "record#threaded".to_string(),
        operation_label: label.id.clone(),
        ordinal: 7,
        sha256: crate::native::hex::Sha256Hex::digest(b"a"),
        payload_sha256: crate::native::hex::Sha256Hex::digest(b"b"),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(90, 120, 40)
            .unwrap(),
    };
    let string = FeaturePayloadString {
        id: "payload-string#threaded-0".to_string(),
        operation_record: record.id.clone(),
        ordinal: 0,
        value: crate::payload_text::PayloadText::new(
            "Hole_ThreadedHole_M Profile_Blind".to_string(),
        )
        .unwrap(),
        source_offset: 130,
    };
    let templates = crate::native::features::holes::feature_threaded_hole_templates(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&string),
    );
    let [template] = templates.as_slice() else {
        panic!("threaded-hole template was not admitted");
    };
    assert_eq!(template.payload_string, string.id);
    assert_eq!(template.family, ThreadedHoleFamily::MProfile);
    assert_eq!(template.extent, SimpleHoleExtent::Blind);
    assert_eq!(template.source_offset, string.source_offset);
    assert_eq!(
        crate::native::features::holes::parse_threaded_hole_template("Hole_ThreadedHole_UNC_Blind"),
        Some((ThreadedHoleFamily::Unc, SimpleHoleExtent::Blind,))
    );
    assert!(
        crate::native::features::holes::parse_threaded_hole_template("Hole_ThreadedHole_UNF_Blind")
            .is_none()
    );
    assert!(
        crate::native::features::holes::parse_threaded_hole_template(
            "Hole_ThreadedHole_M Profile_Through"
        )
        .is_none()
    );

    let mut non_simple_label = label.clone();
    non_simple_label.value = "CBORE_HOLE".to_string();
    assert!(
        crate::native::features::holes::feature_threaded_hole_templates(
            &[non_simple_label],
            std::slice::from_ref(&record),
            std::slice::from_ref(&string),
        )
        .is_empty()
    );

    let mut duplicate = string.clone();
    duplicate.id = "payload-string#threaded-1".to_string();
    duplicate.ordinal = 1;
    duplicate.source_offset += 64;
    assert!(
        crate::native::features::holes::feature_threaded_hole_templates(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            &[string.clone(), duplicate],
        )
        .is_empty()
    );

    let mut unknown = string;
    unknown.value = crate::payload_text::PayloadText::new(
        "Hole_ThreadedHole_M Profile_Blind_Extra".to_string(),
    )
    .unwrap();
    assert!(
        crate::native::features::holes::feature_threaded_hole_templates(
            &[label],
            &[record],
            &[unknown]
        )
        .is_empty()
    );
}

#[test]
fn repeated_scalar_lane_preserves_parallel_wire_and_requires_complete_tokens() {
    check_lane_wire::<FeatureSimpleHoleRepeatedScalarLane>(
        r#"{"id":"lane","operation_label":"operation","values":[2.5,4.0],"raw_values":[[48,4,0,0,0,0,0,0],[48,16,0,0,0,0,0,0]],"first_witness_offsets":[10,18],"second_witness_offsets":[40,48]}"#,
        &[
            "values",
            "raw_values",
            "first_witness_offsets",
            "second_witness_offsets",
        ],
    );
}

#[test]
fn hole_group_preserves_parallel_wire_and_requires_complete_members() {
    check_lane_wire::<FeatureSimpleHoleConstructionGroup>(
        r#"{"id":"group","first_data_blocks":["a","b"],"second_data_blocks":["c","d"],"operation_labels":["first","second"],"scalar_lanes":["scalar-a","scalar-b"],"block_references":["refs-a","refs-b"]}"#,
        &["operation_labels", "scalar_lanes", "block_references"],
    );
}

#[test]
fn hole_group_rejects_short_and_duplicate_members_at_deserialization() {
    for labels in [vec![], vec!["first"], vec!["first", "first"]] {
        let count = labels.len();
        let wire = serde_json::json!({
            "id": "group",
            "first_data_blocks": ["a", "b"],
            "second_data_blocks": ["c", "d"],
            "operation_labels": labels,
            "scalar_lanes": (0..count).map(|index| format!("scalar-{index}")).collect::<Vec<_>>(),
            "block_references": (0..count).map(|index| format!("refs-{index}")).collect::<Vec<_>>(),
        });
        let error = serde_json::from_value::<FeatureSimpleHoleConstructionGroup>(wire).unwrap_err();
        assert!(error.to_string().contains("operation_labels"));
    }
}

#[test]
fn repeated_scalar_lane_rejects_empty_and_inconsistent_atoms() {
    let empty = r#"{"id":"lane","operation_label":"operation","values":[],"raw_values":[],"first_witness_offsets":[],"second_witness_offsets":[]}"#;
    assert!(
        serde_json::from_str::<FeatureSimpleHoleRepeatedScalarLane>(empty)
            .unwrap_err()
            .to_string()
            .contains("values")
    );
    let invalid = r#"{"id":"lane","operation_label":"operation","values":[4.0],"raw_values":[[48,4,0,0,0,0,0,0]],"first_witness_offsets":[10],"second_witness_offsets":[40]}"#;
    assert!(
        serde_json::from_str::<FeatureSimpleHoleRepeatedScalarLane>(invalid)
            .unwrap_err()
            .to_string()
            .contains("raw_values")
    );
}

#[test]
fn nx_simple_hole_construction_groups_require_shared_four_block_identity() {
    use crate::native::features::holes::feature_simple_hole_construction_groups;
    use crate::native::features::holes::FeatureSimpleHoleRepeatedScalarLane;
    use crate::native::features::holes::{
        FeatureSimpleHoleRepeatedScalarLaneBlockReferences, SimpleHoleBlockReference,
        SimpleHoleReferencePair,
    };
    use crate::native::features::FeatureOperationLabel;
    let label = |id: &str, ordinal: u32| FeatureOperationLabel {
        id: id.into(),
        section_link: "section#1".into(),
        ordinal,
        value: "SIMPLE HOLE".into(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    };
    let lane = |operation: &str| FeatureSimpleHoleRepeatedScalarLane {
        id: format!("lane-{operation}"),
        operation_label: operation.into(),
        values: crate::om::nonempty::NonEmpty::new([crate::om::scalar::RepeatedScalar {
            scalar: {
                let mut raw = 25.4_f64.to_be_bytes();
                raw[0] -= 0x10;
                crate::om::scalar::ShiftedBinary64::read(&raw).unwrap()
            },
            witness_offsets: [1, 2],
        }])
        .unwrap(),
    };
    let reference =
        |operation: &str, last: &str| FeatureSimpleHoleRepeatedScalarLaneBlockReferences {
            id: format!("reference-{operation}"),
            operation_label: operation.into(),
            first: SimpleHoleReferencePair {
                references: [
                    SimpleHoleBlockReference {
                        data_block: "block-1".into(),
                        source_offset: 3,
                    },
                    SimpleHoleBlockReference {
                        data_block: "block-2".into(),
                        source_offset: 4,
                    },
                ],
                wrapped: false,
            },
            second: SimpleHoleReferencePair {
                references: [
                    SimpleHoleBlockReference {
                        data_block: "block-3".into(),
                        source_offset: 5,
                    },
                    SimpleHoleBlockReference {
                        data_block: last.into(),
                        source_offset: 6,
                    },
                ],
                wrapped: false,
            },
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
        groups[0]
            .members
            .iter()
            .map(|member| member.operation_label.as_str())
            .collect::<Vec<_>>(),
        ["operation#1-3", "operation#1-2"]
    );
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.scalar_lane.as_str())
            .collect::<Vec<_>>(),
        ["lane-operation#1-3", "lane-operation#1-2"]
    );
    assert_eq!(
        groups[0]
            .members
            .iter()
            .map(|member| member.block_reference.as_str())
            .collect::<Vec<_>>(),
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
    use crate::native::features::holes::feature_hole_package_construction_group_uses;
    use crate::native::features::holes::FeatureHolePackageConstructionGroupLane;
    use crate::native::features::holes::FeatureSimpleHoleConstructionGroup;
    let blocks = [
        "block-1".to_string(),
        "block-2".to_string(),
        "block-3".to_string(),
        "block-4".to_string(),
    ];
    let lane = FeatureHolePackageConstructionGroupLane {
        id: "package-lane".into(),
        operation_label: "package-operation".into(),
        selector: std::num::NonZeroU8::new(0x46).unwrap(),
        branch: std::num::NonZeroU8::new(0x11).unwrap(),
        references: std::array::from_fn(|index| {
            crate::native::features::reference::ConstructionReference {
                token: crate::om::reference_index::ReferenceIndexToken::from_wire(
                    index as u32 + 1,
                    &[0xf0, index as u8 + 1],
                )
                .unwrap(),
                data_block: blocks[index].clone(),
                source_offset: [132, 134, 141, 143][index],
            }
        }),
        payload_offset: 20,
        source_offset: 120,
    };
    let group = FeatureSimpleHoleConstructionGroup {
        id: "simple-hole-group".into(),
        first_data_blocks: [blocks[0].clone(), blocks[1].clone()],
        second_data_blocks: [blocks[2].clone(), blocks[3].clone()],
        members: crate::native::features::holes::SimpleHoleConstructionMembers::new(vec![
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "simple-hole-1".into(),
                scalar_lane: "scalar-1".into(),
                block_reference: "references-1".into(),
            },
            crate::native::features::holes::FeatureSimpleHoleConstructionMember {
                operation_label: "simple-hole-2".into(),
                scalar_lane: "scalar-2".into(),
                block_reference: "references-2".into(),
            },
        ])
        .unwrap(),
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
fn simple_hole_reference_pairs_preserve_wire_and_reject_wrong_witness_wrappers() {
    use crate::om::simple_hole_references::{FIRST_PREFIX, SECOND_PREFIX};
    let json = r#"{"id":"references","operation_label":"operation","first_data_blocks":["a","b"],"second_data_blocks":["c","d"],"first_reference_offsets":[10,13],"second_reference_offsets":[30,32]}"#;
    let base: serde_json::Value = serde_json::from_str(json).unwrap();
    let record: FeatureSimpleHoleRepeatedScalarLaneBlockReferences =
        serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), json);
    for first in [false, true] {
        for second in [false, true] {
            let mut wire = base.clone();
            if first {
                wire["first_reference_prefix"] = serde_json::json!(FIRST_PREFIX);
            }
            if second {
                wire["second_reference_prefix"] = serde_json::json!(SECOND_PREFIX);
            }
            let record: FeatureSimpleHoleRepeatedScalarLaneBlockReferences =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(record).unwrap(), wire);
        }
    }
    for (field, prefix) in [
        ("first_reference_prefix", SECOND_PREFIX),
        ("second_reference_prefix", FIRST_PREFIX),
    ] {
        let mut wire = base.clone();
        wire[field] = serde_json::json!(prefix);
        let error =
            serde_json::from_value::<FeatureSimpleHoleRepeatedScalarLaneBlockReferences>(wire)
                .unwrap_err();
        assert!(error.to_string().contains(field));
    }
}
