// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn named_solid_primitives_bind_ordered_parameter_owners() {
    fn owner(
        scope_record_index: u32,
        record_index: u32,
        local_ordinal: u32,
        value: f64,
    ) -> DesignParameterOwner {
        DesignParameterOwner {
            id: format!("f3d:Design/BulkStream.dat:owner#{record_index}"),
            byte_offset: u64::from(record_index),
            frame_length: 104,
            class_tag: "272".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value: value,
            evaluated_value_offset: u64::from(record_index) + 100,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    }

    let mut bytes = vec![0; 100];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[25] = 1;
    let mut box_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#12",
        crate::records::DesignFeatureKind::BoxPrimitive,
        12,
    );
    box_scope.frame_length = bytes.len() as u64;
    box_scope.reference_members = vec![20, 21, 22, 23, 24];
    let box_owners = vec![
        owner(12, 20, 0, 3.0),
        owner(12, 21, 1, 4.0),
        owner(12, 22, 2, 2.0),
        owner(12, 23, 3, 0.5),
        owner(12, 24, 4, -0.25),
    ];
    let records = IndexedRecordOffsets::build(&bytes);
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &box_scope, &box_owners),
        Some(DesignSolidPrimitive::Box(crate::records::DesignBoxPrimitive {
            length: 3.0,
            width: 4.0,
            height: 2.0,
            offset_x: 0.5,
            offset_y: -0.25,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 20,
            ..
        }))
    ));

    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cylinder_scope = box_scope;
    cylinder_scope.payload = crate::records::DesignFeatureKind::CylinderPrimitive.into();
    cylinder_scope.record_index = 13;
    cylinder_scope.reference_members = vec![30, 31];
    let cylinder_owners = vec![owner(13, 30, 0, 0.7), owner(13, 31, 1, 3.0)];
    assert!(matches!(
        exact_solid_primitive(&bytes, &records, &cylinder_scope, &cylinder_owners,),
        Some(DesignSolidPrimitive::Cylinder(crate::records::DesignCylinderPrimitive {
            height: 0.7,
            diameter: 3.0,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 20,
            ..
        }))
    ));
}

#[test]
fn shifted_cylinder_primitives_bind_exact_generation_frames() {
    fn indexed_header(bytes: &mut [u8], class_tag: &[u8; 3], record_index: u32) {
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(class_tag);
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
    }

    fn guid(bytes: &mut [u8], count_offset: usize) {
        bytes[count_offset..count_offset + 4].copy_from_slice(&36u32.to_le_bytes());
        let value = "00000000-0000-0000-0000-000000000000";
        for (ordinal, code_unit) in value.encode_utf16().enumerate() {
            let at = count_offset + 4 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&code_unit.to_le_bytes());
        }
    }

    fn owner(
        scope_record_index: u32,
        record_index: u32,
        local_ordinal: u32,
        value: f64,
        stream: &str,
    ) -> DesignParameterOwner {
        DesignParameterOwner {
            id: format!("f3d:{stream}:owner#{record_index}"),
            byte_offset: u64::from(record_index),
            frame_length: 103,
            class_tag: "294".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value: value,
            evaluated_value_offset: u64::from(record_index) + 40,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    }

    fn scope(
        class_tag: &str,
        paired_class_tag: &str,
        record_index: u32,
        frame_length: usize,
        reference_members: Vec<u32>,
    ) -> DesignParameterScope {
        let stream = "Design/BulkStream.dat";
        let id = format!("f3d:{stream}:scope#{record_index}");
        let mut scope = DesignParameterScope::empty(
            &id,
            crate::records::DesignFeatureKind::CylinderPrimitive,
            record_index,
        );
        scope.byte_offset = 0;
        scope.class_tag = class_tag.into();
        scope.paired_class_tag = paired_class_tag.into();
        scope.paired_byte_offset = frame_length as u64;
        scope.frame_length = frame_length as u64;
        scope.reference_members = reference_members;
        let (reference_count, history_state, kind, feature_ordinal, previous) =
            if frame_length == 352 {
                (174, 233, 241, 275, 306)
            } else {
                (302, 383, 391, 425, 456)
            };
        scope.reference_count_offset = reference_count;
        scope.history_state_id_offset = history_state;
        scope.kind_offset = kind;
        scope.feature_ordinal_offset = feature_ordinal;
        scope.previous_history_state_id_offset = Some(previous);
        scope
    }

    fn common_prefix(bytes: &mut [u8], operation: u32, references: &[u32]) {
        bytes[21] = 1;
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        let mut reversed = references.iter().rev().copied();
        let first = reversed.next().unwrap();
        bytes[26] = 1;
        bytes[27] = 1;
        bytes[28..32].copy_from_slice(&first.to_le_bytes());
        for (offset, record_index) in [38, 49, 60].into_iter().zip(reversed.take(3)) {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
        }
    }

    let mut compact = vec![0; 352];
    indexed_header(&mut compact, b"297", 12);
    let compact_references = [100, 101, 102, 103, 104];
    common_prefix(&mut compact, 4, &compact_references);
    compact[71] = 1;
    compact[72..76].copy_from_slice(&1u32.to_le_bytes());
    compact[76] = 1;
    compact[77..81].copy_from_slice(&99u32.to_le_bytes());
    guid(&mut compact, 95);
    let compact_scope = scope("297", "258", 12, 352, compact_references.into());
    let compact_owners = vec![
        owner(12, 103, 0, 0.7, "Design/BulkStream.dat"),
        owner(12, 104, 1, 3.0, "Design/BulkStream.dat"),
    ];
    assert!(matches!(
        exact_solid_primitive(
            &compact,
            &IndexedRecordOffsets::build(&compact),
            &compact_scope,
            &compact_owners,
        ),
        Some(DesignSolidPrimitive::Cylinder(crate::records::DesignCylinderPrimitive {
            height: 0.7,
            diameter: 3.0,
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 22,
            transform: None,
            ..
        }))
    ));

    for (class_tag, paired_class_tag) in [("297", "258"), ("375", "258"), ("414", "272")] {
        let mut expanded = vec![0; 502];
        indexed_header(&mut expanded, class_tag.as_bytes().try_into().unwrap(), 12);
        let expanded_references = [100, 101, 102, 103, 104, 105, 106];
        common_prefix(&mut expanded, 1, &expanded_references);
        let transform: [[f64; 4]; 4] = [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = 72 + ordinal * 8;
            expanded[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        expanded[208] = 1;
        expanded[209..213].copy_from_slice(&0x0100_0000u32.to_le_bytes());
        expanded[213..217].copy_from_slice(&100u32.to_le_bytes());
        guid(&mut expanded, 223);
        let expanded_scope = scope(
            class_tag,
            paired_class_tag,
            12,
            502,
            expanded_references.into(),
        );
        let expanded_owners = vec![
            owner(12, 105, 0, 0.7, "Design/BulkStream.dat"),
            owner(12, 106, 1, 3.0, "Design/BulkStream.dat"),
        ];
        assert!(matches!(
            exact_solid_primitive(
                &expanded,
                &IndexedRecordOffsets::build(&expanded),
                &expanded_scope,
                &expanded_owners,
            ),
            Some(DesignSolidPrimitive::Cylinder(crate::records::DesignCylinderPrimitive {
                height: 0.7,
                diameter: 3.0,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 22,
                transform: Some(crate::records::Located { offset: 72, .. }),
                ..
            }))
        ));

        let mut translated = expanded;
        translated[72 + 3 * 8..72 + 4 * 8].copy_from_slice(&1.0f64.to_le_bytes());
        assert!(exact_solid_primitive(
            &translated,
            &IndexedRecordOffsets::build(&translated),
            &expanded_scope,
            &expanded_owners,
        )
        .is_none());
    }
}

#[test]
fn combine_scope_projects_ordered_target_tools_and_retention() {
    fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"283", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"259", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"283", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"259", record_index);
    }
    fn selection_record(bytes: &mut Vec<u8>, record_index: u32, suffix: u8) {
        indexed_header(bytes, b"389", record_index);
        lp_utf16(
            bytes,
            &format!("00000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        lp_utf16(
            bytes,
            &format!("10000000-0000-0000-0000-0000000000{suffix:02x}"),
        );
        indexed_header(bytes, b"306", record_index);
    }

    let scope_record_index = 90u32;
    let references = [91u32, 92, 93, 94, 95, 96];
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, b"382", scope_record_index);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&[0; 7]);
    bytes.resize(64, 0);
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&17u32.to_le_bytes());
    lp_utf16(&mut bytes, "Combine");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    tail[31..35].copy_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    indexed_header(&mut bytes, b"259", scope_record_index);
    for (ordinal, pair) in references.chunks_exact(2).enumerate() {
        if ordinal == 2 {
            target_record(&mut bytes, pair[0], pair[1]);
        } else {
            operation_record(&mut bytes, pair[0], pair[1]);
        }
        selection_record(&mut bytes, pair[1], u8::try_from(pair[1]).unwrap());
    }

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: scope_record_index,
        class_tag: "382".into(),
        byte_offset: 0,
    };
    let mut scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("Combine scope");
    let operation = exact_combine_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
        .expect("Combine construction");
    assert_eq!(
        operation,
        DesignCombineOperation {
            form: DesignCombineForm::Standard,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 20,
            keep_tools: true,
            keep_tools_offset: 25,
            target: DesignCombineBodySelection {
                record_index: 96,
                external_identity: None,
            },
            tools: vec![
                DesignCombineBodySelection {
                    record_index: 92,
                    external_identity: None,
                },
                DesignCombineBodySelection {
                    record_index: 94,
                    external_identity: None,
                },
            ],
        }
    );
    if let crate::records::DesignScopePayload::Combine(slot) = &mut scope.payload {
        *slot = Some(operation);
    }
    assert_eq!(
        project_combine(&scope, "Design1/BulkStream.dat"),
        Some(cadmpeg_ir::features::FeatureDefinition::Combine {
            target: cadmpeg_ir::features::BodySelection::Native(
                "Design1/BulkStream.dat:design-record#96".into(),
            ),
            tools: cadmpeg_ir::features::BodySelection::NativeSet(vec![
                "Design1/BulkStream.dat:design-record#92".into(),
                "Design1/BulkStream.dat:design-record#94".into(),
            ]),
            op: cadmpeg_ir::features::BooleanKind::Join,
            keep_tools: true,
        })
    );

    let mut compact_bytes = bytes.clone();
    compact_bytes[4..7].copy_from_slice(b"387");
    compact_bytes[11..21].fill(0);
    compact_bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[25] = 0;
    compact_bytes[26..29].fill(0);
    compact_bytes[29..31].copy_from_slice(&[1, 0]);
    compact_bytes[31..35].copy_from_slice(&1u32.to_le_bytes());
    compact_bytes[35] = 1;
    compact_bytes[36..44].copy_from_slice(&200u64.to_le_bytes());
    compact_bytes[44..46].fill(0);
    let mut compact_scope = scope.clone();
    compact_scope.class_tag = "387".into();
    compact_scope.paired_class_tag = "258".into();
    compact_scope.frame_length = 328;
    let compact = exact_combine_operation(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &compact_scope,
    )
    .expect("compact Combine construction");
    assert_eq!(compact.operation, DesignExtrudeOperation::Join);
    assert_eq!(compact.operation_offset, 21);
    assert!(!compact.keep_tools);
    assert_eq!(compact.form, DesignCombineForm::Compact);
    assert_eq!(compact.target.record_index, 96);
    assert_eq!(
        compact
            .tools
            .iter()
            .map(|tool| tool.record_index)
            .collect::<Vec<_>>(),
        [92, 94]
    );

    let mut malformed_compact_tail = compact_bytes;
    malformed_compact_tail[45] = 1;
    assert!(exact_combine_operation(
        &malformed_compact_tail,
        &IndexedRecordOffsets::build(&malformed_compact_tail),
        &compact_scope,
    )
    .is_none());
}

#[test]
fn combine_extended_reference_scope_retains_external_tool_identity() {
    fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    fn local_reference(bytes: &mut Vec<u8>, target: u64) {
        bytes.push(1);
        bytes.extend_from_slice(&target.to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
    }
    fn operation_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"304", record_index);
        bytes.extend_from_slice(&[0; 9]);
        bytes.push(1);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&24u32.to_le_bytes());
        bytes.extend_from_slice(b"DcFeatureOperationIdFlag");
        bytes.extend_from_slice(&23u32.to_le_bytes());
        bytes.extend_from_slice(b"IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&7u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"261", record_index);
    }
    fn target_record(bytes: &mut Vec<u8>, record_index: u32, selection_record_index: u32) {
        indexed_header(bytes, b"304", record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&selection_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        indexed_header(bytes, b"261", record_index);
    }
    fn external_selection_record(bytes: &mut Vec<u8>, scope: u32, record_index: u32) {
        const ASSET: &str = "11111111-1111-4111-8111-111111111111";
        const CONTEXT: &str = "22222222-2222-4222-8222-222222222222";
        indexed_header(bytes, b"312", record_index);
        bytes.extend_from_slice(&[0; 14]);
        local_reference(bytes, u64::from(record_index + 3));
        bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        lp_utf16(bytes, CONTEXT);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        local_reference(bytes, 5_001);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&6_001u64.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(bytes, ASSET);
        bytes.push(0);
        lp_utf16(bytes, "component-body-link");
        bytes.push(1);
        lp_utf16(bytes, "33333333-3333-4333-8333-333333333333");
        lp_utf16(bytes, "urn:example:version:4");
        bytes.extend_from_slice(&9u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&11u64.to_le_bytes());
        bytes.extend_from_slice(&48u32.to_le_bytes());
        bytes.extend_from_slice(&12u64.to_le_bytes());
        local_reference(bytes, u64::from(record_index + 2));
        bytes.extend_from_slice(&[0; 2]);
        local_reference(bytes, u64::from(record_index + 1));
        bytes.push(0);
        local_reference(bytes, u64::from(scope));
        indexed_header(bytes, b"261", record_index);
    }
    fn simple_selection_record(bytes: &mut Vec<u8>, record_index: u32) {
        indexed_header(bytes, b"312", record_index);
        lp_utf16(bytes, "44444444-4444-4444-8444-444444444444");
        lp_utf16(bytes, "55555555-5555-4555-8555-555555555555");
        indexed_header(bytes, b"261", record_index);
    }

    let scope_record_index = 90u32;
    let mut bytes = vec![0; 363];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"329");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[29] = 1;
    bytes[30] = 1;
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes[35] = 1;
    bytes[36..44].copy_from_slice(&700u64.to_le_bytes());
    bytes[44..46].fill(0);
    indexed_header(&mut bytes, b"261", scope_record_index);
    operation_record(&mut bytes, 91, 92);
    external_selection_record(&mut bytes, scope_record_index, 92);
    target_record(&mut bytes, 93, 94);
    simple_selection_record(&mut bytes, 94);

    let mut scope = DesignParameterScope::empty(
        "scope",
        crate::records::DesignFeatureKind::Combine,
        scope_record_index,
    );
    scope.byte_offset = 0;
    scope.class_tag = "329".into();
    scope.paired_class_tag = "261".into();
    scope.frame_length = 363;
    scope.reference_members = vec![91, 92, 93, 94];
    let records = IndexedRecordOffsets::build(&bytes);
    let operation = exact_combine_operation(&bytes, &records, &scope)
        .expect("extended-reference Combine construction");
    assert_eq!(operation.form, DesignCombineForm::ExtendedReference);
    assert_eq!(operation.operation, DesignExtrudeOperation::Cut);
    assert_eq!(operation.operation_offset, 31);
    assert!(operation.keep_tools);
    assert_eq!(operation.keep_tools_offset, 30);
    assert_eq!(operation.target.record_index, 94);
    let [tool] = operation.tools.as_slice() else {
        panic!("one external tool");
    };
    let identity = tool
        .external_identity
        .as_ref()
        .expect("cross-document body identity");
    assert_eq!(identity.occurrence_reference, 5_001);
    assert_eq!(identity.external_body_reference, 6_001);
    assert_eq!(identity.external_segment, 7);
    assert_eq!(identity.external_link_name, "component-body-link");
    assert_eq!(
        identity.external_version.as_ref().map(|version| version.property_key.value.as_str()),
        Some("33333333-3333-4333-8333-333333333333")
    );
    assert_eq!(
        identity.external_version.as_ref().map(|version| version.version_urn.value.as_str()),
        Some("urn:example:version:4")
    );
    assert_eq!(identity.tail_values, [11, 12]);
    assert_eq!(
        identity.tail_value_offsets[1],
        identity.tail_value_offsets[0] + 12
    );

    let mut malformed_scope = scope.clone();
    malformed_scope.frame_length = 367;
    assert!(exact_combine_operation(&bytes, &records, &malformed_scope).is_none());
    let mut malformed_reference = bytes.clone();
    malformed_reference[35] = 0;
    assert!(exact_combine_operation(
        &malformed_reference,
        &IndexedRecordOffsets::build(&malformed_reference),
        &scope,
    )
    .is_none());

    let mut malformed_external_asset = bytes;
    malformed_external_asset[usize::try_from(identity.external_asset_id_offset).unwrap()] = b'g';
    let operation = exact_combine_operation(
        &malformed_external_asset,
        &IndexedRecordOffsets::build(&malformed_external_asset),
        &scope,
    )
    .expect("operation remains exact when only the optional external identity is malformed");
    assert!(operation.tools[0].external_identity.is_none());
}
