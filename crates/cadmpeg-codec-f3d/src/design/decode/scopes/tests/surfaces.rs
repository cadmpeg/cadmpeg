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
fn ruled_surface_operation_reads_mode_parameters_and_ordered_edge_groups() {
    let mut bytes = vec![0; 366];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    let reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    bytes[27] = 1;
    reference(&mut bytes, 28, 12);
    reference(&mut bytes, 39, 11);
    bytes[54..58].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 58, 13);
    bytes[73..77].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 77, 99);
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    reference(&mut bytes, 96, 15);
    bytes[107..111].copy_from_slice(&36u32.to_le_bytes());
    for (ordinal, byte) in b"00000000-0000-0000-0000-000000000000".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    bytes[186..190].copy_from_slice(&6u32.to_le_bytes());

    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .expect("exact SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Normal);
    assert_eq!(operation.method_offset, 20);
    assert_eq!(operation.corner, DesignRuledSurfaceCorner::Rounded);
    assert_eq!(operation.corner_offset, 50);
    assert!(operation.alternate_face);
    assert_eq!(operation.alternate_face_offset, 27);
    assert_eq!(operation.angle_owner_record_index, 12);
    assert_eq!(operation.distance_owner_record_index, 11);
    assert_eq!(operation.edge_group_record_indices, [13, 15]);
    assert_eq!(operation.auxiliary_record_indices, [99]);
    assert_eq!(operation.direction_entity_id, None);

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, byte) in b"01234567-89ab-cdef-0123-456789abcdef".iter().enumerate() {
        bytes[111 + ordinal * 2] = *byte;
    }
    let operation = exact_ruled_surface_operation(&bytes, 0, 366, 186, &[11, 12, 13, 14, 15, 16])
        .expect("directed SurfaceRuled operation");
    assert_eq!(operation.method, DesignRuledSurfaceMethod::Direction);
    assert_eq!(
        operation.direction_entity_id.as_deref(),
        Some("01234567-89ab-cdef-0123-456789abcdef")
    );
}

#[test]
fn surface_stitch_tolerance_uses_its_fixed_scope_owned_frame() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"308", 300);
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&[1, 1, 0, 0, 0]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    bytes.extend_from_slice(&0.01f64.to_le_bytes());
    bytes.resize(104, 0);
    header(&mut bytes, *b"258", 300);
    bytes.extend_from_slice(&[0; 20]);
    header(&mut bytes, *b"331", 301);
    bytes.extend_from_slice(&[0; 20]);
    header(&mut bytes, *b"258", 301);

    assert_eq!(
        exact_surface_stitch_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            12,
            &[100, 200, 300, 301]
        ),
        Some(DesignSurfaceStitchOperation {
            gap_tolerance: 0.01,
            gap_tolerance_offset: 40,
            tolerance_record_index: 300,
            settings_record_index: 301,
        })
    );
}

#[test]
fn base_feature_scope_decodes_parallel_result_body_runs() {
    let mut bytes = vec![0u8; 375];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [
        (101u64, [0, 0, 1, 0, 0, 0]),
        (202, [0; 6]),
        (301, [0; 6]),
        (302, [0, 0, 2, 0, 0, 0]),
    ] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor] = 1;
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for reference in [301u32, 302] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
        cursor += 11;
    }
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&401u64.to_le_bytes());
    cursor += 15;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    cursor += 4;
    for result in [501u32, 502] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&result.to_le_bytes());
        cursor += 11;
    }
    assert!(cursor <= 171);

    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
        byte_offset: 0,
        class_tag: "306".into(),
        record_index: 1,
        frame_length: 375,
        kind: "Base Feature".into(),
        kind_offset: 273,
        extrude_prologue: None,
        coil_operation: None,
        coil_operation_offset: None,
        coil_extent: None,
        coil_extent_offset: None,
        coil_section: None,
        coil_section_offset: None,
        coil_section_placement: None,
        coil_section_placement_offset: None,
        coil_clockwise: None,
        coil_clockwise_offset: None,
        coil_placement: None,
        coil_transform: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: Some(2),
        history_state_id_offset: 0,
        previous_history_state_id: Some(2),
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![301],
        reference_member_offsets: vec![0],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange_operation: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_extrude_parameters: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_transform: None,
        work_plane_transform_offset: None,
        work_plane_reference: None,
        work_plane_reference_offset: None,
        work_plane_construction: None,
        work_axis_construction: None,
        joint_origin_transform: None,
        joint_origin_transform_offset: None,
        joint_origin_reference: None,
        joint_origin_reference_offset: None,
        work_point_construction: None,
        unclosed_construction_operand_groups: Vec::new(),
        hole_construction: None,
        extrude_profile: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 375,
    };
    let construction = exact_base_feature_construction(&bytes, &scope)
        .expect("generated Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        body_reference_records,
        metadata_record,
        result_records,
        body_entity_fields,
        ..
    } = &construction
    else {
        panic!("parallel Base Feature frame selected the wrong form");
    };
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(body_reference_records, &[301, 302]);
    assert_eq!(*metadata_record, 401);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(body_entity_fields[0], [0, 0, 1, 0, 0, 0]);
    let serialized = serde_json::to_value(&construction).expect("serialize legacy form");
    assert!(serialized.get("form").is_none());
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize legacy form"),
        construction
    );

    let mut expanded_bytes = Vec::new();
    expanded_bytes.extend_from_slice(&bytes[..84]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&[0; 6]);
    expanded_bytes.extend_from_slice(&2u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&bytes[99..131]);
    expanded_bytes.extend_from_slice(&bytes[131..133]);
    expanded_bytes.extend_from_slice(&bytes[137..]);
    expanded_bytes.resize(366, 0);
    let mut expanded_scope = scope.clone();
    expanded_scope.class_tag = "384".into();
    expanded_scope.paired_class_tag = "264".into();
    expanded_scope.frame_length = 366;
    expanded_scope.kind_offset = 265;
    expanded_scope.paired_byte_offset = 366;
    let expanded = exact_base_feature_construction(&expanded_bytes, &expanded_scope)
        .expect("expanded Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        result_records,
        metadata_field,
        ..
    } = &expanded
    else {
        panic!("expanded Base Feature frame selected the wrong form");
    };
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(metadata_field, &[0, 0]);

    let mut legacy_compact_bytes = expanded_bytes.clone();
    legacy_compact_bytes[90] = 1;
    legacy_compact_bytes[96..100].copy_from_slice(&101u32.to_le_bytes());
    legacy_compact_bytes[107..111].copy_from_slice(&202u32.to_le_bytes());
    let mut legacy_compact_scope = expanded_scope.clone();
    legacy_compact_scope.class_tag = "420".into();
    legacy_compact_scope.paired_class_tag = "258".into();
    let legacy_compact =
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope)
            .expect("legacy compact Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        body_reference_records,
        result_records,
        metadata_field,
        ..
    } = &legacy_compact
    else {
        panic!("legacy compact Base Feature frame selected the wrong form");
    };
    assert_eq!(body_entity_suffixes, &[101, 202]);
    assert_eq!(body_reference_records, &[301, 302]);
    assert_eq!(result_records, &[501, 502]);
    assert_eq!(metadata_field, &[0, 0]);

    legacy_compact_bytes[96..100].copy_from_slice(&301u32.to_le_bytes());
    assert!(
        exact_base_feature_construction(&legacy_compact_bytes, &legacy_compact_scope).is_none()
    );

    let mut snapshot_bytes = vec![0u8; 485];
    snapshot_bytes[19] = 1;
    snapshot_bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [1u8, 2, 3, 4, 5, 6]), (202, [6u8, 5, 4, 3, 2, 1])] {
        snapshot_bytes[cursor] = 1;
        snapshot_bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        snapshot_bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    snapshot_bytes[cursor + 4..cursor + 8].copy_from_slice(&1u32.to_le_bytes());
    cursor += 8;
    let related_guids = [
        "11111111-2222-3333-4444-555555555555",
        "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    ];
    for guid in related_guids {
        let encoded = crate::bytes::lp_utf16_bytes(guid);
        snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
        cursor += encoded.len();
    }
    snapshot_bytes[cursor..cursor + 7].copy_from_slice(&[0, 0, 1, 1, 0, 0, 0]);
    cursor += 7;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&101u64.to_le_bytes());
    cursor += 8;
    cursor += 3;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&301u64.to_le_bytes());
    cursor += 8;
    cursor += 6;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 8].copy_from_slice(&401u64.to_le_bytes());
    cursor += 8;
    cursor += 6;
    cursor += 4;
    let third_guid = "00000000-0000-0000-0000-000000000000";
    let encoded = crate::bytes::lp_utf16_bytes(third_guid);
    snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    cursor += 3;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    snapshot_bytes[cursor] = 1;
    cursor += 1;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&301u32.to_le_bytes());
    cursor += 4;
    cursor += 6;
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&7u32.to_le_bytes());
    cursor += 4;
    let encoded = crate::bytes::lp_utf16_bytes("Base Feature");
    snapshot_bytes[cursor..cursor + encoded.len()].copy_from_slice(&encoded);
    cursor += encoded.len();
    snapshot_bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    assert_eq!(cursor, 401);

    let mut snapshot_scope = scope;
    snapshot_scope.class_tag = "314".into();
    snapshot_scope.frame_length = 485;
    snapshot_scope.kind_offset = 373;
    snapshot_scope.feature_ordinal_offset = 397;
    snapshot_scope.history_state_id = Some(7);
    snapshot_scope.history_state_id_offset = 365;
    snapshot_scope.previous_history_state_id = None;
    snapshot_scope.previous_history_state_id_offset = 0;
    snapshot_scope.reference_count_offset = 350;
    snapshot_scope.reference_members = vec![301];
    snapshot_scope.reference_member_offsets = vec![355];
    snapshot_scope.paired_class_tag = "259".into();
    snapshot_scope.paired_byte_offset = 485;
    let construction = exact_base_feature_construction(&snapshot_bytes, &snapshot_scope)
        .expect("body-snapshot Base Feature frame is canonical");
    let serialized = serde_json::to_value(&construction).expect("serialize snapshot form");
    assert!(serialized.get("form").is_none());
    assert_eq!(
        serde_json::from_value::<DesignBaseFeatureConstruction>(serialized)
            .expect("deserialize snapshot form"),
        construction
    );
    let DesignBaseFeatureConstruction::BodySnapshot {
        body_entity_suffixes,
        body_entity_fields,
        related_guids: decoded_guids,
        related_guid_offsets,
        linkage_record,
        linkage_record_offset,
        auxiliary_record,
        auxiliary_record_offset,
        ..
    } = construction
    else {
        panic!("body-snapshot Base Feature frame selected the wrong form");
    };
    assert_eq!(body_entity_suffixes, [101, 202]);
    assert_eq!(body_entity_fields[0], [1, 2, 3, 4, 5, 6]);
    assert_eq!(
        decoded_guids,
        [
            related_guids[0].to_owned(),
            related_guids[1].to_owned(),
            third_guid.to_owned()
        ]
    );
    assert_eq!(related_guid_offsets, [66, 142, 275]);
    assert_eq!(linkage_record, 301);
    assert_eq!(linkage_record_offset, 234);
    assert_eq!(auxiliary_record, 401);
    assert_eq!(auxiliary_record_offset, 253);

    let mut packed_snapshot_bytes = vec![0u8; 485];
    packed_snapshot_bytes[..54].copy_from_slice(&snapshot_bytes[..54]);
    packed_snapshot_bytes[54..58].copy_from_slice(&snapshot_bytes[54..58]);
    packed_snapshot_bytes[58] = 0;
    packed_snapshot_bytes[59..63].copy_from_slice(&snapshot_bytes[58..62]);
    packed_snapshot_bytes[63..215].copy_from_slice(&snapshot_bytes[62..214]);
    packed_snapshot_bytes[214..].copy_from_slice(&snapshot_bytes[214..]);
    let packed = exact_base_feature_construction(&packed_snapshot_bytes, &snapshot_scope)
        .expect("packed body-snapshot Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::BodySnapshot {
        related_guid_offsets,
        ..
    } = packed
    else {
        panic!("packed body-snapshot Base Feature frame selected the wrong form");
    };
    assert_eq!(related_guid_offsets, [67, 143, 275]);

    let mut invalid_scope = snapshot_scope;
    invalid_scope.reference_members = vec![302];
    assert!(exact_base_feature_construction(&snapshot_bytes, &invalid_scope).is_none());
}

#[test]
fn base_feature_scope_decodes_class_452_compact_result_body_run() {
    let mut bytes = vec![0u8; 314];
    bytes[19] = 1;
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let mut cursor = 24;
    for (value, field) in [(101u64, [0; 6]), (201, [0; 6])] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
        bytes[cursor + 9..cursor + 15].copy_from_slice(&field);
        cursor += 15;
    }
    bytes[cursor] = 1;
    bytes[cursor + 6] = 1;
    bytes[cursor + 7..cursor + 11].copy_from_slice(&1u32.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 5].copy_from_slice(&101u32.to_le_bytes());
    cursor += 11;
    bytes[cursor] = 0;
    cursor += 1;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
    bytes[cursor + 9..cursor + 11].copy_from_slice(&[0, 0]);
    cursor += 11;
    bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
    cursor += 4;
    bytes[cursor] = 1;
    bytes[cursor + 1..cursor + 5].copy_from_slice(&401u32.to_le_bytes());
    assert_eq!(cursor + 11, 103);

    let mut scope =
        DesignParameterScope::empty("f3d:scope#base-feature-compact", "Base Feature", 70);
    scope.class_tag = "452".into();
    scope.frame_length = 314;
    scope.kind_offset = 213;
    scope.reference_members = vec![301];
    scope.paired_class_tag = "266".into();
    scope.paired_byte_offset = 314;
    let construction = exact_base_feature_construction(&bytes, &scope)
        .expect("class-452 compact Base Feature frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        body_reference_records,
        metadata_record,
        result_records,
        ..
    } = construction
    else {
        panic!("class-452 compact Base Feature frame selected the wrong form");
    };
    assert_eq!(body_entity_suffixes, [101]);
    assert_eq!(body_reference_records, [201]);
    assert_eq!(metadata_record, 301);
    assert_eq!(result_records, [401]);

    let mut nonzero_prefix = bytes;
    nonzero_prefix[11] = 1;
    assert!(exact_base_feature_construction(&nonzero_prefix, &scope).is_none());
}

#[test]
fn base_feature_scope_decodes_class_409_262_result_body_variants() {
    fn frame(body_count: usize) -> (Vec<u8>, DesignParameterScope) {
        let frame_length = 262 + 52 * body_count;
        let mut bytes = vec![0u8; frame_length];
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&(2 * body_count as u32).to_le_bytes());
        let mut cursor = 24;
        for ordinal in 0..body_count {
            let value = 101 + ordinal as u64;
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        for ordinal in 0..body_count {
            let value = 201 + ordinal as u64;
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        bytes[cursor] = 1;
        bytes[cursor + 7..cursor + 11].copy_from_slice(&(body_count as u32).to_le_bytes());
        cursor += 11;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&(201 + ordinal as u32).to_le_bytes());
            cursor += 11;
        }
        bytes[cursor] = 0;
        cursor += 1;
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
        cursor += 11;
        bytes[cursor..cursor + 4].copy_from_slice(&(body_count as u32).to_le_bytes());
        cursor += 4;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&(401 + ordinal as u32).to_le_bytes());
            cursor += 11;
        }
        let mut scope =
            DesignParameterScope::empty("f3d:scope#base-feature-409-262", "Base Feature", 70);
        scope.class_tag = "409".into();
        scope.paired_class_tag = "262".into();
        scope.frame_length = frame_length as u64;
        scope.kind_offset = (frame_length + 102) as u64;
        scope.paired_byte_offset = frame_length as u64;
        scope.reference_members = vec![301];
        assert!(cursor <= frame_length);
        (bytes, scope)
    }

    for body_count in [1, 3, 4] {
        let (bytes, scope) = frame(body_count);
        let construction = exact_base_feature_construction(&bytes, &scope)
            .expect("class-409/class-262 result-body frame is canonical");
        let DesignBaseFeatureConstruction::ResultBodies {
            body_entity_suffixes,
            body_reference_records,
            metadata_record,
            result_records,
            ..
        } = construction
        else {
            panic!("class-409/class-262 frame selected the wrong form");
        };
        assert_eq!(body_entity_suffixes.len(), body_count);
        assert_eq!(body_reference_records.len(), body_count);
        assert_eq!(metadata_record, 301);
        assert_eq!(result_records.len(), body_count);
    }

    let prefix = 17;
    let mut zero_body = vec![0u8; prefix + 258];
    zero_body[prefix + 20] = 1;
    zero_body[prefix + 32] = 1;
    zero_body[prefix + 33..prefix + 41].copy_from_slice(&701u64.to_le_bytes());
    let mut zero_scope =
        DesignParameterScope::empty("f3d:scope#base-feature-409-262-zero", "Base Feature", 71);
    zero_scope.class_tag = "409".into();
    zero_scope.paired_class_tag = "262".into();
    zero_scope.byte_offset = prefix as u64;
    zero_scope.frame_length = 258;
    zero_scope.kind_offset = (prefix + 157) as u64;
    zero_scope.paired_byte_offset = (prefix + 258) as u64;
    zero_scope.reference_members = vec![701];
    let construction = exact_base_feature_construction(&zero_body, &zero_scope)
        .expect("class-409/class-262 zero-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        ..
    } = construction
    else {
        panic!("class-409/class-262 zero-body frame selected the wrong form");
    };
    assert!(body_entity_suffixes.is_empty());
    assert_eq!(metadata_record, 701);
    assert_eq!(metadata_record_offset, (prefix + 33) as u64);
    assert_eq!(metadata_field, [0; 6]);

    let mut nonzero_padding = zero_body.clone();
    nonzero_padding[prefix + 47] = 1;
    assert!(exact_base_feature_construction(&nonzero_padding, &zero_scope).is_none());

    let mut mismatched_pair = zero_scope;
    mismatched_pair.paired_byte_offset -= 1;
    assert!(exact_base_feature_construction(&zero_body, &mismatched_pair).is_none());
}

#[test]
fn base_feature_scope_decodes_class_444_263_result_body_variants() {
    fn frame(body_count: usize) -> (Vec<u8>, DesignParameterScope) {
        let frame_length = 262 + 52 * body_count;
        let mut bytes = vec![0u8; frame_length];
        bytes[19] = 1;
        bytes[20..24].copy_from_slice(&(2 * body_count as u32).to_le_bytes());
        let mut cursor = 24;
        for ordinal in 0..body_count {
            let value = 101 + ordinal as u64;
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        for ordinal in 0..body_count {
            let value = 201 + ordinal as u64;
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 9].copy_from_slice(&value.to_le_bytes());
            cursor += 15;
        }
        bytes[cursor] = 1;
        bytes[cursor + 7..cursor + 11].copy_from_slice(&(body_count as u32).to_le_bytes());
        cursor += 11;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&(201 + ordinal as u32).to_le_bytes());
            cursor += 11;
        }
        bytes[cursor] = 0;
        cursor += 1;
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 9].copy_from_slice(&301u64.to_le_bytes());
        cursor += 11;
        bytes[cursor..cursor + 4].copy_from_slice(&(body_count as u32).to_le_bytes());
        cursor += 4;
        for ordinal in 0..body_count {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&(401 + ordinal as u32).to_le_bytes());
            cursor += 11;
        }
        let mut scope =
            DesignParameterScope::empty("f3d:scope#base-feature-444-263", "Base Feature", 72);
        scope.class_tag = "444".into();
        scope.paired_class_tag = "263".into();
        scope.frame_length = frame_length as u64;
        scope.kind_offset = (frame_length + 102) as u64;
        scope.paired_byte_offset = frame_length as u64;
        scope.reference_members = vec![301];
        assert!(cursor <= frame_length);
        (bytes, scope)
    }

    for body_count in [1, 3, 4] {
        let (bytes, scope) = frame(body_count);
        let construction = exact_base_feature_construction(&bytes, &scope)
            .expect("class-444/class-263 result-body frame is canonical");
        let DesignBaseFeatureConstruction::ResultBodies {
            body_entity_suffixes,
            body_reference_records,
            metadata_record,
            result_records,
            ..
        } = construction
        else {
            panic!("class-444/class-263 frame selected the wrong form");
        };
        assert_eq!(body_entity_suffixes.len(), body_count);
        assert_eq!(body_reference_records.len(), body_count);
        assert_eq!(metadata_record, 301);
        assert_eq!(result_records.len(), body_count);
    }

    let (mut bytes, scope) = frame(1);
    bytes[24 + 30 + 6] = 1;
    assert!(exact_base_feature_construction(&bytes, &scope).is_none());

    let prefix = 17;
    let mut zero_body = vec![0u8; prefix + 258];
    zero_body[prefix + 20] = 1;
    zero_body[prefix + 32] = 1;
    zero_body[prefix + 33..prefix + 41].copy_from_slice(&701u64.to_le_bytes());
    zero_body[prefix + 55..prefix + 59].copy_from_slice(&36u32.to_le_bytes());
    let guid = "00000000-0000-0000-0000-000000000000";
    let guid_utf16 = guid.encode_utf16().collect::<Vec<_>>();
    for (ordinal, code_unit) in guid_utf16.into_iter().enumerate() {
        zero_body[prefix + 59 + ordinal * 2..prefix + 61 + ordinal * 2]
            .copy_from_slice(&code_unit.to_le_bytes());
    }
    zero_body[prefix + 134..prefix + 138].copy_from_slice(&1u32.to_le_bytes());
    zero_body[prefix + 138] = 1;
    zero_body[prefix + 139..prefix + 143].copy_from_slice(&701u32.to_le_bytes());
    zero_body[prefix + 149..prefix + 153].copy_from_slice(&17u32.to_le_bytes());
    zero_body[prefix + 153..prefix + 157].copy_from_slice(&12u32.to_le_bytes());
    for (ordinal, code_unit) in "Base Feature".encode_utf16().enumerate() {
        zero_body[prefix + 157 + ordinal * 2..prefix + 159 + ordinal * 2]
            .copy_from_slice(&code_unit.to_le_bytes());
    }
    zero_body[prefix + 181..prefix + 185].copy_from_slice(&1u32.to_le_bytes());
    zero_body[prefix + 212..prefix + 216].copy_from_slice(&2u32.to_le_bytes());
    let mut zero_scope =
        DesignParameterScope::empty("f3d:scope#base-feature-444-263-zero", "Base Feature", 73);
    zero_scope.class_tag = "444".into();
    zero_scope.paired_class_tag = "263".into();
    zero_scope.byte_offset = prefix as u64;
    zero_scope.frame_length = 258;
    zero_scope.kind_offset = (prefix + 157) as u64;
    zero_scope.paired_byte_offset = (prefix + 258) as u64;
    zero_scope.reference_count_offset = (prefix + 134) as u64;
    zero_scope.reference_members = vec![701];
    zero_scope.reference_member_offsets = vec![(prefix + 139) as u64];
    let construction = exact_base_feature_construction(&zero_body, &zero_scope)
        .expect("class-444/class-263 zero-body frame is canonical");
    let DesignBaseFeatureConstruction::ResultBodies {
        body_entity_suffixes,
        metadata_record,
        metadata_record_offset,
        metadata_field,
        ..
    } = construction
    else {
        panic!("class-444/class-263 zero-body frame selected the wrong form");
    };
    assert!(body_entity_suffixes.is_empty());
    assert_eq!(metadata_record, 701);
    assert_eq!(metadata_record_offset, (prefix + 33) as u64);
    assert_eq!(metadata_field, [0; 14]);

    let mut nonzero_tail = zero_body.clone();
    nonzero_tail[prefix + 41] = 1;
    assert!(exact_base_feature_construction(&nonzero_tail, &zero_scope).is_none());

    let mut mismatched_reference = zero_body.clone();
    mismatched_reference[prefix + 139] = 1;
    assert!(exact_base_feature_construction(&mismatched_reference, &zero_scope).is_none());

    let mut mismatched_pair = zero_scope;
    mismatched_pair.paired_byte_offset -= 1;
    assert!(exact_base_feature_construction(&zero_body, &mismatched_pair).is_none());
}
