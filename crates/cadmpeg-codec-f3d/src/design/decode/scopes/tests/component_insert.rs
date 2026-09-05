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
fn component_insert_scope_joins_its_relation_carrier_role_and_transform() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, -2.1],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let role = "b2231f72-46dc-40fa-b8e8-10cd208d7df8";
    let mut bytes = Vec::new();
    header(&mut bytes, b"256", 10);
    let role_at = bytes.len();
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    bytes.extend_from_slice(&[0, 0]);
    let carrier_transform_at = bytes.len();
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let relation_at = bytes.len();
    header(&mut bytes, b"325", 20);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"259", 20);
    let scope_at = bytes.len();
    bytes.resize(scope_at + 399, 0);
    bytes[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
    bytes[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = scope_at + 50 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut bytes, b"259", 30);
    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#30".into(),
        byte_offset: scope_at as u64,
        class_tag: "451".into(),
        record_index: 30,
        frame_length: 399,
        kind: "Component Insert".into(),
        kind_offset: 0,
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
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![20],
        reference_member_offsets: vec![scope_at as u64 + 38],
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
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        derived_instance_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_frame: None,
        work_axis_construction: None,
        joint_origin_frame: None,
        work_point_construction: None,
        unclosed_construction_operand_groups: Vec::new(),
        hole_construction: None,
        extrude_profile: None,
        sweep_profile: None,
        base_flange_profile: None,
        sketch_entity: None,
        paired_class_tag: "259".into(),
        paired_byte_offset: (scope_at + 399) as u64,
    };

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("component insert construction");

    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, (role_at + 4) as u64);
    assert_eq!(construction.transform, transform);
    assert_eq!(construction.transform_offset, Some((scope_at + 50) as u64));
    assert_eq!(
        construction.carrier_transform_offset,
        Some(carrier_transform_at as u64)
    );

    for (frame_length, paired_class_tag, transform_at, relation_at, expanded_prologue) in [
        (381_usize, "261", 49_usize, 38_usize, true),
        (395, "258", 46, 34, false),
    ] {
        let mut legacy = bytes[..scope_at].to_vec();
        legacy.resize(scope_at + frame_length, 0);
        legacy[scope_at..scope_at + 4].copy_from_slice(&3_u32.to_le_bytes());
        legacy[scope_at + 4..scope_at + 7].copy_from_slice(b"451");
        legacy[scope_at + 7..scope_at + 11].copy_from_slice(&30_u32.to_le_bytes());
        if expanded_prologue {
            legacy[scope_at + 20] = 1;
            legacy[scope_at + 37] = 1;
            legacy[scope_at + 48] = 1;
        } else {
            legacy[scope_at + 33] = 1;
        }
        legacy[scope_at + relation_at..scope_at + relation_at + 4]
            .copy_from_slice(&20_u32.to_le_bytes());
        if !expanded_prologue {
            legacy[scope_at + transform_at - 2] = 1;
        }
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = scope_at + transform_at + ordinal * 8;
            legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        header(
            &mut legacy,
            paired_class_tag
                .as_bytes()
                .try_into()
                .expect("three-byte tag"),
            30,
        );
        let legacy_scope = DesignParameterScope {
            frame_length: frame_length as u64,
            paired_class_tag: paired_class_tag.into(),
            paired_byte_offset: (scope_at + frame_length) as u64,
            ..scope.clone()
        };
        let construction = exact_component_insert_construction(
            &legacy,
            &IndexedRecordOffsets::build(&legacy),
            &legacy_scope,
        )
        .unwrap_or_else(|| panic!("{frame_length}-byte component insert construction"));
        assert_eq!(
            construction.transform_offset,
            Some((scope_at + transform_at) as u64)
        );
        assert_eq!(construction.transform, transform);
    }

    let mut expanded = Vec::new();
    header(&mut expanded, b"312", 10);
    let expanded_carrier_transform_at = expanded.len();
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    let expanded_role_at = expanded.len();
    expanded.extend_from_slice(&36_u32.to_le_bytes());
    expanded.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
    expanded.extend_from_slice(&[0, 1, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let expanded_relation_at = expanded.len();
    header(&mut expanded, b"338", 20);
    expanded.resize(expanded_relation_at + 58, 0);
    expanded[expanded_relation_at + 21] = 1;
    expanded[expanded_relation_at + 22..expanded_relation_at + 26]
        .copy_from_slice(&10_u32.to_le_bytes());
    expanded[expanded_relation_at + 32..expanded_relation_at + 35].copy_from_slice(&[1, 0, 0]);
    expanded[expanded_relation_at + 35] = 1;
    expanded[expanded_relation_at + 36..expanded_relation_at + 40]
        .copy_from_slice(&99_u32.to_le_bytes());
    expanded[expanded_relation_at + 47] = 1;
    expanded[expanded_relation_at + 48..expanded_relation_at + 52]
        .copy_from_slice(&30_u32.to_le_bytes());
    let expanded_scope_at = expanded.len();
    header(&mut expanded, b"335", 30);
    expanded.resize(expanded_scope_at + 404, 0);
    expanded[expanded_scope_at + 20] = 1;
    let occurrence_identity = 0x0102_0304_0506_0708_u64;
    expanded[expanded_scope_at + 29..expanded_scope_at + 37]
        .copy_from_slice(&occurrence_identity.to_le_bytes());
    expanded[expanded_scope_at + 41] = 1;
    expanded[expanded_scope_at + 42..expanded_scope_at + 46].copy_from_slice(&20_u32.to_le_bytes());
    expanded[expanded_scope_at + 52..expanded_scope_at + 54].copy_from_slice(&[1, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = expanded_scope_at + 54 + ordinal * 8;
        expanded[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut expanded, b"260", 30);
    let expanded_scope = DesignParameterScope {
        byte_offset: expanded_scope_at as u64,
        class_tag: "335".into(),
        frame_length: 404,
        reference_member_offsets: vec![(expanded_scope_at + 42) as u64],
        paired_class_tag: "260".into(),
        paired_byte_offset: (expanded_scope_at + 404) as u64,
        ..scope.clone()
    };
    let construction = exact_component_insert_construction(
        &expanded,
        &IndexedRecordOffsets::build(&expanded),
        &expanded_scope,
    )
    .expect("404-byte component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(occurrence_identity));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        (expanded_role_at + 4) as u64
    );
    assert_eq!(construction.transform, transform);
    assert_eq!(
        construction.transform_offset,
        Some((expanded_scope_at + 54) as u64)
    );
    assert_eq!(
        construction.carrier_transform_offset,
        Some(expanded_carrier_transform_at as u64)
    );

    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let mut legacy = Vec::new();
    header(&mut legacy, b"288", 10);
    legacy.resize(30, 0);
    push_utf16(&mut legacy, "95cc7c78-04aa-4ffc-a36d-a512f02e0dda");
    let legacy_role_at = legacy.len();
    push_utf16(&mut legacy, role);
    legacy.extend_from_slice(&[1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    push_utf16(&mut legacy, "96e2c767-721c-4c81-bbbc-8cc143d323fb");
    legacy.push(0);
    let asset_identity = "864a8a41-7ed8-4c94-8871-ee9e87ab7648_urn:asset";
    push_utf16(&mut legacy, asset_identity);
    legacy.push(0);
    let legacy_carrier_transform_at = legacy.len();
    for value in transform.into_iter().flatten() {
        legacy.extend_from_slice(&value.to_le_bytes());
    }
    legacy.extend_from_slice(&[0; 4]);
    push_utf16(&mut legacy, asset_identity);
    legacy.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let legacy_relation_at = legacy.len();
    header(&mut legacy, b"325", 20);
    legacy.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        legacy.push(1);
        legacy.extend_from_slice(&reference.to_le_bytes());
        legacy.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    let legacy_scope_at = legacy.len();
    header(&mut legacy, b"346", 30);
    legacy.resize(legacy_scope_at + 381, 0);
    legacy[legacy_scope_at + 20] = 1;
    legacy[legacy_scope_at + 37] = 1;
    legacy[legacy_scope_at + 38..legacy_scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    legacy[legacy_scope_at + 48] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = legacy_scope_at + 49 + ordinal * 8;
        legacy[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    header(&mut legacy, b"261", 30);
    let legacy_scope = DesignParameterScope {
        byte_offset: legacy_scope_at as u64,
        frame_length: 381,
        paired_class_tag: "261".into(),
        paired_byte_offset: (legacy_scope_at + 381) as u64,
        ..scope
    };
    let construction = exact_component_insert_construction(
        &legacy,
        &IndexedRecordOffsets::build(&legacy),
        &legacy_scope,
    )
    .expect("class-288 legacy component insert construction");
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        (legacy_role_at + 4) as u64
    );
    assert_eq!(
        construction.carrier_transform_offset,
        Some(legacy_carrier_transform_at as u64)
    );
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(legacy_relation_at + 57, legacy_scope_at);
}

#[test]
fn compact_component_insert_identity_form_joins_grouped_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"382", 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_utf16(&mut bytes, metadata_guid_a);
    push_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"399", 20);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&10_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&99_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&30_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"263", 20);

    let scope_at = bytes.len();
    header(&mut bytes, b"296", 30);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"263", 30);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        "Component Insert",
        30,
    );
    scope.byte_offset = scope_at as u64;
    scope.class_tag = "296".into();
    scope.frame_length = 261;
    scope.reference_members = vec![20];
    scope.reference_member_offsets = vec![(scope_at + 38) as u64];
    scope.paired_class_tag = "263".into();
    scope.paired_byte_offset = (scope_at + 261) as u64;

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("compact identity component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(construction.transform, identity_matrix());
    assert_eq!(construction.transform_offset, None);
    assert_eq!(construction.carrier_transform_offset, None);
}

#[test]
fn class_410_component_insert_identity_form_joins_class_380_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"380", 166);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_utf16(&mut bytes, metadata_guid_a);
    push_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"310", 167);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [166_u32, 168, 169].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"261", 167);

    let scope_at = bytes.len();
    header(&mut bytes, b"410", 169);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&167_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"261", 169);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#169",
        "Component Insert",
        169,
    );
    scope.byte_offset = scope_at as u64;
    scope.class_tag = "410".into();
    scope.frame_length = 261;
    scope.reference_members = vec![167];
    scope.reference_member_offsets = vec![(scope_at + 38) as u64];
    scope.paired_class_tag = "261".into();
    scope.paired_byte_offset = (scope_at + 261) as u64;

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("class-410 component insert construction");
    assert_eq!(construction.relation_record_index, 167);
    assert_eq!(construction.carrier_record_index, 166);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(construction.transform, identity_matrix());
    assert_eq!(construction.transform_offset, None);
    assert_eq!(construction.carrier_transform_offset, None);

    bytes[4..7].copy_from_slice(b"382");
    assert!(exact_component_insert_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn class_434_component_insert_identity_form_joins_variable_role_class_341_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000_urn:test";
    let mut bytes = Vec::new();
    header(&mut bytes, b"341", 166);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_utf16(&mut bytes, metadata_guid_a);
    push_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);

    let relation_at = bytes.len();
    header(&mut bytes, b"348", 167);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [166_u32, 168, 169].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"266", 167);

    let scope_at = bytes.len();
    header(&mut bytes, b"434", 169);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&167_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"266", 169);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#169",
        "Component Insert",
        169,
    );
    scope.byte_offset = scope_at as u64;
    scope.class_tag = "434".into();
    scope.frame_length = 261;
    scope.reference_members = vec![167];
    scope.reference_member_offsets = vec![(scope_at + 38) as u64];
    scope.paired_class_tag = "266".into();
    scope.paired_byte_offset = (scope_at + 261) as u64;

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("class-434 component insert construction");
    assert_eq!(construction.relation_record_index, 167);
    assert_eq!(construction.carrier_record_index, 166);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(construction.transform, identity_matrix());
    assert_eq!(construction.transform_offset, None);
    assert_eq!(construction.carrier_transform_offset, None);
}

#[test]
fn class_426_component_insert_joins_legacy_relation_and_class_369_carrier() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let push_utf16 = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    };
    let push_ascii = |bytes: &mut Vec<u8>, value: &str| {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    };
    let component_guid = "11111111-2222-3333-4444-555555555555";
    let type_guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let metadata_guid_a = "66666666-7777-8888-9999-aaaaaaaaaaaa";
    let metadata_guid_b = "bbbbbbbb-cccc-dddd-eeee-ffffffffffff";
    let role = "cccccccc-dddd-eeee-ffff-000000000000";
    let mut bytes = Vec::new();
    header(&mut bytes, b"369", 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&17_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&[0; 4]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 3, 0, 0, 0, 0, 1, 0, 0, 0]);
    push_utf16(&mut bytes, metadata_guid_a);
    push_utf16(&mut bytes, metadata_guid_b);
    bytes.extend_from_slice(&[0, 1, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, component_guid);
    bytes.push(0);
    push_ascii(&mut bytes, type_guid);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 0, 0, 0, 0]);
    push_utf16(&mut bytes, role);
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(bytes.len(), 695);

    let relation_at = bytes.len();
    header(&mut bytes, b"345", 20);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&10_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&21_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&30_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, b"258", 20);
    bytes.extend_from_slice(&[0; 19]);

    let child_at = bytes.len();
    header(&mut bytes, b"393", 21);
    bytes.extend_from_slice(&[0; 20]);
    bytes.push(1);
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&42_u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(bytes.len(), child_at + 58);

    let scope_at = bytes.len();
    header(&mut bytes, b"426", 30);
    bytes.resize(scope_at + 261, 0);
    bytes[scope_at + 20] = 1;
    bytes[scope_at + 25..scope_at + 33].copy_from_slice(&17_u64.to_le_bytes());
    bytes[scope_at + 37] = 1;
    bytes[scope_at + 38..scope_at + 42].copy_from_slice(&20_u32.to_le_bytes());
    bytes[scope_at + 48..scope_at + 50].copy_from_slice(&[1, 1]);
    bytes[scope_at + 50..scope_at + 54].copy_from_slice(&36_u32.to_le_bytes());
    bytes[scope_at + 54..scope_at + 126].copy_from_slice(
        &"00000000-0000-0000-0000-000000000000"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    header(&mut bytes, b"258", 30);

    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#30",
        "Component Insert",
        30,
    );
    scope.byte_offset = scope_at as u64;
    scope.class_tag = "426".into();
    scope.frame_length = 261;
    scope.reference_members = vec![20];
    scope.reference_member_offsets = vec![(scope_at + 38) as u64];
    scope.paired_class_tag = "258".into();
    scope.paired_byte_offset = (scope_at + 261) as u64;

    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("class-426 component insert construction");
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(construction.neutron_role_offset, 159);
    assert_eq!(construction.transform, identity_matrix());
    assert_eq!(construction.transform_offset, None);
    assert_eq!(construction.carrier_transform_offset, None);

    let external_role = "cccccccc-dddd-eeee-ffff-000000000000_urn:adsk.test:asset";
    let mut external_bytes = bytes[..155].to_vec();
    external_bytes.extend_from_slice(&crate::bytes::lp_utf16_bytes(external_role));
    external_bytes.extend_from_slice(&[0, 4, 0, 0, 0, 0, 1, 0, 0, 0]);
    external_bytes.extend_from_slice(&bytes[241..525]);
    external_bytes.extend_from_slice(&crate::bytes::lp_utf16_bytes(external_role));
    external_bytes.extend_from_slice(&bytes[601..607]);
    external_bytes.extend_from_slice(&crate::bytes::lp_utf16_bytes(external_role));
    external_bytes.extend_from_slice(&bytes[683..695]);
    let carrier_shift = external_bytes.len() - 695;
    external_bytes.extend_from_slice(&bytes[695..]);
    let external_scope_at = scope_at + carrier_shift;
    let mut external_scope = scope.clone();
    external_scope.byte_offset = external_scope_at as u64;
    external_scope.reference_member_offsets = vec![(external_scope_at + 38) as u64];
    external_scope.paired_byte_offset = (external_scope_at + 261) as u64;
    let external_construction = exact_component_insert_construction(
        &external_bytes,
        &IndexedRecordOffsets::build(&external_bytes),
        &external_scope,
    )
    .expect("class-426 external-role component insert construction");
    assert_eq!(external_construction.neutron_role, external_role);
    assert_eq!(external_construction.neutron_role_offset, 159);

    bytes[4..7].copy_from_slice(b"380");
    assert!(exact_component_insert_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope
    )
    .is_none());
}

#[test]
fn class_283_component_insert_admits_compact_and_transformed_scopes() {
    let header = |bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    };
    let role = "b2231f72-46dc-40fa-b8e8-10cd208d7df8_urn:adsk.test:asset";
    let null_guid = "00000000-0000-0000-0000-000000000000";
    let component_guid = "11111111-2222-3333-4444-555555555555";

    let make_fixture = |frame_length: usize, transform: [[f64; 4]; 4]| {
        let mut bytes = Vec::new();
        let carrier_at = bytes.len();
        header(&mut bytes, b"334", 10);
        bytes.resize(
            carrier_at + crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE,
            0,
        );
        bytes[carrier_at + crate::layout::component_insert_carrier_334_prefix::COMPONENT_IDENTITY
            ..carrier_at
                + crate::layout::component_insert_carrier_334_prefix::COMPONENT_IDENTITY
                + 76]
            .copy_from_slice(&crate::bytes::lp_utf16_bytes(component_guid));
        let role_at = carrier_at + crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE;
        bytes.extend(role.encode_utf16().flat_map(u16::to_le_bytes));
        bytes.extend_from_slice(&[0, 0x21, 0, 0, 0, 0, 1, 0, 0, 0]);
        bytes.extend_from_slice(&crate::bytes::lp_utf16_bytes(component_guid));
        assert_eq!(
            role_at
                + role.encode_utf16().count() * 2
                + 10
                + crate::bytes::lp_utf16_bytes(component_guid).len(),
            bytes.len()
        );

        let relation_at = bytes.len();
        header(&mut bytes, b"365", 20);
        bytes.extend_from_slice(&[0; 10]);
        for (reference, zero_count) in [(10_u32, 8), (99, 7), (30, 6)] {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend(std::iter::repeat_n(0, zero_count));
        }
        assert_eq!(bytes.len(), relation_at + 57);
        header(&mut bytes, b"262", 20);

        let scope_at = bytes.len();
        header(&mut bytes, b"283", 30);
        bytes.resize(scope_at + frame_length, 0);
        bytes[scope_at + 21..scope_at + 29].copy_from_slice(&17_u64.to_le_bytes());
        bytes[scope_at + 33] = 1;
        bytes[scope_at + 34..scope_at + 38].copy_from_slice(&20_u32.to_le_bytes());
        if frame_length == 257 {
            bytes[scope_at + 44..scope_at + 46].copy_from_slice(&[1, 1]);
            bytes[scope_at + 46..scope_at + 122]
                .copy_from_slice(&crate::bytes::lp_utf16_bytes(null_guid));
            bytes[scope_at + 125..scope_at + 129].copy_from_slice(&1_u32.to_le_bytes());
            bytes[scope_at + 129] = 1;
            bytes[scope_at + 130..scope_at + 134].copy_from_slice(&20_u32.to_le_bytes());
            bytes[scope_at + 140..scope_at + 144].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes[scope_at + 211..scope_at + 215].copy_from_slice(&u32::MAX.to_le_bytes());
        } else {
            bytes[scope_at + 44..scope_at + 46].copy_from_slice(&[1, 0]);
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = scope_at + 46 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
            bytes[scope_at + 174..scope_at + 250]
                .copy_from_slice(&crate::bytes::lp_utf16_bytes(null_guid));
            bytes[scope_at + 253..scope_at + 257].copy_from_slice(&1_u32.to_le_bytes());
            bytes[scope_at + 257] = 1;
            bytes[scope_at + 258..scope_at + 262].copy_from_slice(&20_u32.to_le_bytes());
            bytes[scope_at + 268..scope_at + 272].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes[scope_at + 339..scope_at + 343].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        header(&mut bytes, b"262", 30);

        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#30",
            "Component Insert",
            30,
        );
        scope.byte_offset = scope_at as u64;
        scope.class_tag = "283".into();
        scope.frame_length = frame_length as u64;
        scope.reference_members = vec![20];
        scope.reference_member_offsets = vec![(scope_at + 34) as u64];
        scope.paired_class_tag = "262".into();
        scope.paired_byte_offset = (scope_at + frame_length) as u64;
        (bytes, scope, scope_at)
    };

    let identity = identity_matrix();
    let (bytes, scope, _) = make_fixture(257, identity);
    let records = IndexedRecordOffsets::build(&bytes);
    let construction = exact_component_insert_construction(&bytes, &records, &scope)
        .expect("class-283 compact component insert construction");
    assert_eq!(construction.carrier_record_index, 10);
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE as u64
    );
    assert_eq!(construction.transform, identity);
    assert_eq!(construction.transform_offset, None);
    assert_eq!(construction.carrier_transform_offset, None);

    let transformed = [
        [1.0, 0.0, 0.0, -2.1],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let (bytes, scope, scope_at) = make_fixture(385, transformed);
    let construction =
        exact_component_insert_construction(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("class-283 transformed component insert construction");
    assert_eq!(construction.occurrence_identity, Some(17));
    assert_eq!(construction.neutron_role, role);
    assert_eq!(
        construction.neutron_role_offset,
        crate::layout::component_insert_carrier_334_prefix::NEUTRON_ROLE as u64
    );
    assert_eq!(construction.transform, transformed);
    assert_eq!(construction.transform_offset, Some((scope_at + 46) as u64));
    assert_eq!(construction.carrier_transform_offset, None);
}

#[test]
fn class_414_component_insert_admits_shifted_identity_and_matrix_prologues() {
    let relation_record_index = 20_u32;
    let occurrence_identity = 17_u64;
    let null_guid = crate::bytes::lp_utf16_bytes("00000000-0000-0000-0000-000000000000");

    let mut identity = vec![0_u8; 267];
    identity[21..29].copy_from_slice(&occurrence_identity.to_le_bytes());
    identity[33] = 1;
    identity[34..38].copy_from_slice(&relation_record_index.to_le_bytes());
    identity[44..46].copy_from_slice(&[1, 1]);
    identity[46..122].copy_from_slice(&null_guid);
    assert_eq!(
        super::super::exact_component_insert_identity_scope_shifted(
            &identity,
            0,
            relation_record_index,
        ),
        Some(occurrence_identity)
    );

    let transform: [[f64; 4]; 4] = [
        [1.0, 0.0, 0.0, 4.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut matrix = vec![0_u8; 389];
    matrix[20] = 1;
    matrix[25..33].copy_from_slice(&occurrence_identity.to_le_bytes());
    matrix[37] = 1;
    matrix[38..42].copy_from_slice(&relation_record_index.to_le_bytes());
    matrix[48..50].copy_from_slice(&[1, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        matrix[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    matrix[178..254].copy_from_slice(&null_guid);
    assert_eq!(
        super::super::exact_component_insert_scope_414_264_389(&matrix, 0, relation_record_index,),
        Some((transform, Some(50), occurrence_identity))
    );
}
