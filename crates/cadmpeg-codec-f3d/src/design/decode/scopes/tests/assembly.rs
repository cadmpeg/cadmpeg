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
fn assembly_operand_paths_follow_ordered_locator_envelopes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        "Assemble",
        scope_record_index,
    );
    scope.class_tag = "273".into();
    scope.frame_length = 637;
    scope.reference_members = vec![50, 51, 52, 53];
    scope.paired_class_tag = "259".into();
    scope.paired_byte_offset = 637;
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "457".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    };
    let rectangular_owners = [
        owner(50, 0, 3.0, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 10.0, 503),
        owner(53, 3, 0.0, 504),
    ];
    let mut assembly_bytes = assembly_operand_frame_fixture(scope_record_index);
    assembly_bytes[362..366].copy_from_slice(&2_u32.to_le_bytes());
    for (at, target) in [(366, 64_u32), (377, 67_u32)] {
        assembly_bytes[at] = 1;
        assembly_bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    }

    let write_path_reference = |bytes: &mut [u8], at: usize, target: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&target.to_le_bytes());
    };
    let push_path_locator = |bytes: &mut Vec<u8>, locator_index: u32, wrapper_index: u32| {
        let mut locator = vec![0; 190];
        locator[0..4].copy_from_slice(&3_u32.to_le_bytes());
        locator[4..7].copy_from_slice(b"304");
        locator[7..11].copy_from_slice(&locator_index.to_le_bytes());
        write_path_reference(&mut locator, 21, locator_index + 100);
        for ordinal in 0..16 {
            let value = if ordinal % 5 == 0 { 1.0_f64 } else { 0.0 };
            locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
        }
        write_path_reference(&mut locator, 162, scope_record_index);
        write_path_reference(&mut locator, 173, wrapper_index);
        locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
        bytes.extend_from_slice(&locator);
    };
    let push_path_wrapper = |bytes: &mut Vec<u8>, wrapper_index: u32, path_record_index: u32| {
        let mut wrapper = vec![0; 37];
        wrapper[0..4].copy_from_slice(&3_u32.to_le_bytes());
        wrapper[4..7].copy_from_slice(b"382");
        wrapper[7..11].copy_from_slice(&wrapper_index.to_le_bytes());
        wrapper[21] = 1;
        wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
        write_path_reference(&mut wrapper, 26, path_record_index);
        bytes.extend_from_slice(&wrapper);
    };
    let push_path = |bytes: &mut Vec<u8>, record_index: u32, guids: &[&str]| {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"329");
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&(guids.len() as u32).to_le_bytes());
        for guid in guids {
            let encoded = guid.encode_utf16().collect::<Vec<_>>();
            bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
            bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
        }
    };
    let push_identity_path =
        |bytes: &mut Vec<u8>, record_index: u32, path: &[&str], identities: &[&str; 4]| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(b"390");
            bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
            for guid in path.iter().chain(&identities[..2]) {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u64.to_le_bytes());
            for guid in &identities[2..] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
        };
    let identities = [
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    let mut identity_path_bytes = assembly_bytes.clone();
    push_path_locator(&mut identity_path_bytes, 64, 66);
    let first_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
        &identities,
    );
    push_path_wrapper(&mut identity_path_bytes, 66, 65);
    push_path_locator(&mut identity_path_bytes, 67, 69);
    let second_identity_path_at = identity_path_bytes.len();
    push_identity_path(
        &mut identity_path_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
        &identities,
    );
    push_path_wrapper(&mut identity_path_bytes, 69, 68);
    identity_path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    identity_path_bytes.extend_from_slice(b"396");
    identity_path_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &IndexedRecordOffsets::build(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("identity-qualified assembly occurrence paths");
    assert_eq!(identity_paths[0].class_tag, "390");
    assert_eq!(identity_paths[0].occurrence_guids.len(), 2);
    assert_eq!(identity_paths[0].identity_guids, identities);
    for path_at in [first_identity_path_at, second_identity_path_at] {
        identity_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"386");
    }
    let compact_identity_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &IndexedRecordOffsets::build(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("compact identity-qualified assembly occurrence paths");
    assert!(compact_identity_paths
        .iter()
        .all(|path| path.class_tag == "386"));
    for path_at in [first_identity_path_at, second_identity_path_at] {
        identity_path_bytes[path_at + 4..path_at + 7].copy_from_slice(b"329");
    }
    let extended_class_329_paths = exact_assembly_alignment(
        &identity_path_bytes,
        &IndexedRecordOffsets::build(&identity_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("identity-qualified class-329 assembly occurrence paths");
    assert!(extended_class_329_paths.iter().all(|path| {
        path.class_tag == "329"
            && !path.occurrence_guids.is_empty()
            && path.identity_guids == identities
    }));
    let first_identity_length_at = usize::try_from(
        extended_class_329_paths[0].identity_guid_offsets[0]
            .checked_sub(4)
            .expect("identity length precedes text"),
    )
    .expect("identity length offset fits usize");
    let mut malformed_class_329_identity = identity_path_bytes.clone();
    malformed_class_329_identity[first_identity_length_at..first_identity_length_at + 4]
        .copy_from_slice(&35_u32.to_le_bytes());
    assert!(exact_assembly_alignment(
        &malformed_class_329_identity,
        &IndexedRecordOffsets::build(&malformed_class_329_identity),
        &scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_paths.is_none()));

    let first_locator_at = assembly_bytes.len();
    push_path_locator(&mut assembly_bytes, 64, 66);
    push_path(
        &mut assembly_bytes,
        65,
        &[
            "11111111-1111-1111-1111-111111111111",
            "22222222-2222-2222-2222-222222222222",
        ],
    );
    push_path_wrapper(&mut assembly_bytes, 66, 65);
    push_path_locator(&mut assembly_bytes, 67, 69);
    push_path(
        &mut assembly_bytes,
        68,
        &["33333333-3333-3333-3333-333333333333"],
    );
    push_path_wrapper(&mut assembly_bytes, 69, 68);
    assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    assembly_bytes.extend_from_slice(b"396");
    assembly_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let paths = exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("exact assembly occurrence paths");
    assert_eq!(paths[0].link.locator_record_index, 64);
    assert_eq!(paths[0].link.wrapper_record_index, 66);
    assert_eq!(paths[0].link.locator_reference_offset, 367);
    assert_eq!(paths[0].link.locator_scope_reference_offset, 811);
    assert_eq!(paths[0].link.wrapper_reference_offset, 822);
    assert_eq!(paths[0].link.path_reference_offset, 1_042);
    assert_eq!(
        paths
            .each_ref()
            .map(|path| { (path.record_index, path.occurrence_guids.clone()) }),
        [
            (
                65,
                vec![
                    "11111111-1111-1111-1111-111111111111".into(),
                    "22222222-2222-2222-2222-222222222222".into(),
                ],
            ),
            (68, vec!["33333333-3333-3333-3333-333333333333".into()],),
        ]
    );
    let mut reversed_path_assignment = assembly_bytes.clone();
    reversed_path_assignment[367..371].copy_from_slice(&67_u32.to_le_bytes());
    reversed_path_assignment[378..382].copy_from_slice(&64_u32.to_le_bytes());
    let reversed_paths = exact_assembly_alignment(
        &reversed_path_assignment,
        &IndexedRecordOffsets::build(&reversed_path_assignment),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("scope locator order assigns paths to operand frames");
    assert_eq!(reversed_paths.map(|path| path.record_index), [68, 65]);

    let mut duplicate_path_assignment = assembly_bytes.clone();
    duplicate_path_assignment[378..382].copy_from_slice(&64_u32.to_le_bytes());
    assert!(exact_assembly_alignment(
        &duplicate_path_assignment,
        &IndexedRecordOffsets::build(&duplicate_path_assignment),
        &scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_paths.is_none()));

    for locator_zero_at in [first_locator_at + 32, first_locator_at + 161] {
        let mut invalid_locator = assembly_bytes.clone();
        invalid_locator[locator_zero_at] = 1;
        assert!(exact_assembly_alignment(
            &invalid_locator,
            &IndexedRecordOffsets::build(&invalid_locator),
            &scope,
            &rectangular_owners,
        )
        .is_some_and(|alignment| alignment.operand_paths.is_none()));
    }
    let first_wrapper_at =
        usize::try_from(paths[0].link.wrapper_byte_offset).expect("wrapper offset fits usize");
    for (relative_offset, value) in [(21, 0), (22, 2), (26, 0)] {
        let mut invalid_wrapper = assembly_bytes.clone();
        invalid_wrapper[first_wrapper_at + relative_offset] = value;
        assert!(exact_assembly_alignment(
            &invalid_wrapper,
            &IndexedRecordOffsets::build(&invalid_wrapper),
            &scope,
            &rectangular_owners,
        )
        .is_some_and(|alignment| alignment.operand_paths.is_none()));
    }
    let first_wrapper_end = first_wrapper_at + 37;
    let mut extended_wrapper = assembly_bytes.clone();
    extended_wrapper.insert(first_wrapper_end, 0);
    assert!(exact_assembly_alignment(
        &extended_wrapper,
        &IndexedRecordOffsets::build(&extended_wrapper),
        &scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_paths.is_none()));

    let push_class_294_path =
        |bytes: &mut Vec<u8>, record_index: u32, occurrence: &str, identities: &[&str; 4]| {
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(b"294");
            bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
            bytes.extend_from_slice(&[1, 0, 0, 0]);
            for guid in [occurrence, identities[0], identities[1]] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u64.to_le_bytes());
            for guid in &identities[2..] {
                let encoded = guid.encode_utf16().collect::<Vec<_>>();
                bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
                bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
            }
            bytes.extend_from_slice(&2_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
        };
    let class_294_identities = [
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    let mut class_294_path_bytes = assembly_bytes[..648].to_vec();
    push_path_locator(&mut class_294_path_bytes, 64, 66);
    push_class_294_path(
        &mut class_294_path_bytes,
        65,
        "11111111-1111-1111-1111-111111111111",
        &class_294_identities,
    );
    push_path_wrapper(&mut class_294_path_bytes, 66, 65);
    push_path_locator(&mut class_294_path_bytes, 67, 69);
    push_class_294_path(
        &mut class_294_path_bytes,
        68,
        "22222222-2222-2222-2222-222222222222",
        &class_294_identities,
    );
    push_path_wrapper(&mut class_294_path_bytes, 69, 68);
    class_294_path_bytes.extend_from_slice(&3_u32.to_le_bytes());
    class_294_path_bytes.extend_from_slice(b"396");
    class_294_path_bytes.extend_from_slice(&70_u32.to_le_bytes());
    let class_294_paths = exact_assembly_alignment(
        &class_294_path_bytes,
        &IndexedRecordOffsets::build(&class_294_path_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_paths)
    .expect("class-294 identity-qualified assembly occurrence paths");
    assert!(class_294_paths.iter().all(|path| {
        path.class_tag == "294"
            && path.occurrence_guids.len() == 1
            && path
                .identity_guids
                .iter()
                .map(String::as_str)
                .eq(class_294_identities.iter().copied())
    }));

    assembly_bytes[25] = 0;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .and_then(|alignment| alignment.operand_frames)
    .is_some());
    assembly_bytes[25] = 2;
    assert!(exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners
    )
    .is_some_and(|alignment| alignment.operand_frames.is_none()));

    scope.reference_members.push(99);
    assert_eq!(
        exact_assembly_alignment(
            &assembly_bytes,
            &IndexedRecordOffsets::build(&assembly_bytes),
            &scope,
            &rectangular_owners
        ),
        None
    );
}

#[test]
fn as_built_alignment_uses_locator_frames_and_parameter_owner_lanes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        "As-built",
        scope_record_index,
    );
    scope.class_tag = "439".into();
    scope.frame_length = 399;
    scope.reference_members = vec![50, 51, 52, 53];
    scope.paired_class_tag = "262".into();
    scope.paired_byte_offset = 399;

    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: 0,
            frame_length: 103,
            class_tag: "321".into(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        }
    };
    let owners = [
        owner(50, 0, 0.25, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 2.0, 503),
        owner(53, 3, 3.0, 504),
    ];

    let mut bytes = vec![0_u8; 399];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"439");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[47..51].copy_from_slice(&2_u32.to_le_bytes());
    for (at, locator_record_index) in [(51, 64_u32), (62, 67_u32)] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&locator_record_index.to_le_bytes());
    }
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let first_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        64,
        70,
        "11111111-1111-1111-1111-111111111111",
        [1.0, 2.0, 3.0],
    );
    let second_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        67,
        80,
        "22222222-2222-2222-2222-222222222222",
        [4.0, 5.0, 6.0],
    );
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&90_u32.to_le_bytes());

    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
    )
    .expect("exact As-built alignment");
    assert_eq!(alignment.angle, 0.25);
    assert_eq!(alignment.offset, [1.0, 2.0, 3.0]);
    assert_eq!(alignment.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(alignment.value_offsets, [501, 502, 503, 504]);
    let frames = alignment.operand_frames.expect("locator transforms");
    assert_eq!(
        frames
            .each_ref()
            .map(|frame| (frame.reference_record_index, frame.transform[0][3])),
        [(70, 1.0), (80, 4.0)]
    );
    assert_eq!(
        frames.each_ref().map(|frame| frame.transform_offset),
        [
            u64::try_from(first_locator_at + 33).expect("fixture offset fits u64"),
            u64::try_from(second_locator_at + 33).expect("fixture offset fits u64"),
        ]
    );
    let paths = alignment.operand_paths.expect("locator occurrence paths");
    assert_eq!(
        paths.each_ref().map(|path| (
            path.link.locator_record_index,
            path.occurrence_guids[0].as_str()
        )),
        [
            (64, "11111111-1111-1111-1111-111111111111"),
            (67, "22222222-2222-2222-2222-222222222222"),
        ]
    );

    let mut invalid_transform = bytes.clone();
    invalid_transform[first_locator_at + 33..first_locator_at + 41]
        .copy_from_slice(&2.0_f64.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &invalid_transform,
        &IndexedRecordOffsets::build(&invalid_transform),
        &scope,
        &owners,
    )
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames, None);
    assert_eq!(incomplete.operand_paths, None);

    let mut duplicate_reference = bytes;
    duplicate_reference[63..67].copy_from_slice(&64_u32.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &duplicate_reference,
        &IndexedRecordOffsets::build(&duplicate_reference),
        &scope,
        &owners,
    )
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames, None);
    assert_eq!(incomplete.operand_paths, None);
}

#[test]
fn axial_assembly_selectors_bind_component_insert_occurrences_exactly() {
    let first_transform = identity_matrix();
    let mut second_transform = identity_matrix();
    second_transform[2][3] = 4.25;
    let first_role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let second_role = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
    let mut bytes = Vec::new();
    let first_members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        first_role,
        false,
    );
    let second_members = append_axial_test_component_operand(
        &mut bytes,
        80,
        [100, 120],
        second_transform,
        8_001,
        second_role,
        true,
    );
    let mut assembly =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:assembly#500", "Assemble", 500);
    assembly.frame_length = 772;
    assembly.reference_members = first_members
        .into_iter()
        .chain(second_members)
        .chain([90, 91])
        .collect();
    assembly.assembly_alignment = Some(axial_test_alignment([first_transform, second_transform]));
    let mut scopes = vec![
        assembly,
        axial_test_component_scope(200, first_role),
        axial_test_component_scope(300, second_role),
    ];
    let unresolved_scopes = scopes.clone();

    bind_axial_assembly_operand_targets(&bytes, &IndexedRecordOffsets::build(&bytes), &mut scopes);
    let targets = scopes[0]
        .assembly_alignment
        .as_ref()
        .and_then(|alignment| alignment.axial_operand_targets.as_ref())
        .expect("two exact pathless assembly targets");
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        construction_byte_offset,
        construction_transform_offset,
        axis_record_index_offsets,
        construction_paired_byte_offset,
        selectors,
        ..
    } = &targets[0]
    else {
        panic!("first operand must select a component insertion");
    };
    assert_eq!(*component_insert_scope_record_index, 200);
    assert_eq!(
        *construction_transform_offset,
        construction_byte_offset + 48
    );
    assert_eq!(axis_record_index_offsets[0], construction_byte_offset + 193);
    assert_eq!(axis_record_index_offsets[1], construction_byte_offset + 209);
    assert_eq!(
        *construction_paired_byte_offset,
        construction_byte_offset + 380
    );
    assert_eq!(selectors[0].axis_paired_class_tag, "261");
    assert_eq!(selectors[0].selector_paired_class_tag, "261");
    assert_eq!(selectors[0].occurrence_reference, 10_001);
    assert_eq!(selectors[1].occurrence_reference, 10_002);
    assert_eq!(selectors[0].external_object_reference, 7_001);
    assert!(selectors[0].external_version_urn.is_none());
    let DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
        component_insert_scope_record_index,
        selectors: versioned_selectors,
        ..
    } = &targets[1]
    else {
        panic!("second operand must select a component insertion");
    };
    assert_eq!(*component_insert_scope_record_index, 300);
    assert!(versioned_selectors[0].external_property_key.is_some());
    assert_eq!(
        versioned_selectors[0].external_version_urn.as_deref(),
        Some("urn:test:version:2")
    );

    let mut mismatched = bytes.clone();
    let mismatch_at =
        usize::try_from(selectors[1].external_object_reference_offset).expect("test offset");
    mismatched[mismatch_at..mismatch_at + 8].copy_from_slice(&7_002_u64.to_le_bytes());
    let mut mismatched_scopes = unresolved_scopes;
    bind_axial_assembly_operand_targets(
        &mismatched,
        &IndexedRecordOffsets::build(&mismatched),
        &mut mismatched_scopes,
    );
    assert!(mismatched_scopes[0]
        .assembly_alignment
        .as_ref()
        .is_some_and(|alignment| alignment.axial_operand_targets.is_none()));
}

#[test]
fn axial_assembly_selector_binds_a_document_root_joint_origin() {
    let first_transform = identity_matrix();
    let mut second_transform = identity_matrix();
    second_transform[1][3] = 2.5;
    let role = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
    let mut bytes = Vec::new();
    let members = append_axial_test_component_operand(
        &mut bytes,
        70,
        [10, 30],
        first_transform,
        7_001,
        role,
        false,
    );
    let mut assembly =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:assembly#500", "Assemble", 500);
    assembly.frame_length = 705;
    assembly.reference_members = members.into_iter().chain([90, 91]).collect();
    assembly.assembly_alignment = Some(axial_test_alignment([first_transform, second_transform]));
    let mut origin = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:joint-origin#80",
        "JointOrigin",
        80,
    );
    origin.joint_origin_transform = Some(second_transform);
    let mut scopes = vec![assembly, axial_test_component_scope(200, role), origin];

    bind_axial_assembly_operand_targets(&bytes, &IndexedRecordOffsets::build(&bytes), &mut scopes);
    let targets = scopes[0]
        .assembly_alignment
        .as_ref()
        .and_then(|alignment| alignment.axial_operand_targets.as_ref())
        .expect("component and root assembly targets");
    assert!(matches!(
        &targets[0],
        DesignAssemblyAxialOperandTarget::ComponentInsertOccurrence {
            component_insert_scope_record_index: 200,
            ..
        }
    ));
    assert_eq!(
        targets[1],
        DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin {
            scope_record_index: 80
        }
    );
}

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
        copy_paste_component_operation: None,
        mirror_construction: None,
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
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
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
    assert_eq!(construction.transform_offset, (scope_at + 50) as u64);
    assert_eq!(
        construction.carrier_transform_offset,
        carrier_transform_at as u64
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
            (scope_at + transform_at) as u64
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
        (expanded_scope_at + 54) as u64
    );
    assert_eq!(
        construction.carrier_transform_offset,
        expanded_carrier_transform_at as u64
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
        legacy_carrier_transform_at as u64
    );
    assert_eq!(construction.relation_record_index, 20);
    assert_eq!(legacy_relation_at + 57, legacy_scope_at);
}

pub(super) fn assembly_operand_frame_fixture(scope_record_index: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 648];
    bytes[0..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"273");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[20] = 1;
    bytes[25] = 1;
    for (reference_at, transform_at, reference, translation) in [
        (28, 40, 70_u32, [1.0_f64, 2.0, 3.0]),
        (168, 180, 80_u32, [4.0, 5.0, 6.0]),
    ] {
        bytes[reference_at] = 1;
        bytes[reference_at + 1..reference_at + 5].copy_from_slice(&reference.to_le_bytes());
        for (ordinal, value) in [
            1.0,
            0.0,
            0.0,
            translation[0],
            0.0,
            1.0,
            0.0,
            translation[1],
            0.0,
            0.0,
            1.0,
            translation[2],
            0.0,
            0.0,
            0.0,
            1.0,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[transform_at + ordinal * 8..transform_at + ordinal * 8 + 8]
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[637..641].copy_from_slice(&3_u32.to_le_bytes());
    bytes[641..644].copy_from_slice(b"259");
    bytes[644..648].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes
}

fn append_as_built_path_envelope(
    bytes: &mut Vec<u8>,
    scope_record_index: u32,
    locator_record_index: u32,
    operand_record_index: u32,
    occurrence_guid: &str,
    translation: [f64; 3],
) {
    let write_reference = |bytes: &mut [u8], at: usize, record_index: u32| {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    };
    let mut locator = vec![0_u8; 190];
    locator[..4].copy_from_slice(&3_u32.to_le_bytes());
    locator[4..7].copy_from_slice(b"309");
    locator[7..11].copy_from_slice(&locator_record_index.to_le_bytes());
    write_reference(&mut locator, 21, operand_record_index);
    for (ordinal, value) in [
        1.0,
        0.0,
        0.0,
        translation[0],
        0.0,
        1.0,
        0.0,
        translation[1],
        0.0,
        0.0,
        1.0,
        translation[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
    .into_iter()
    .enumerate()
    {
        locator[33 + ordinal * 8..41 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
    }
    write_reference(&mut locator, 162, scope_record_index);
    write_reference(&mut locator, 173, locator_record_index + 2);
    locator[184..188].copy_from_slice(&2_u32.to_le_bytes());
    bytes.extend(locator);

    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"294");
    bytes.extend_from_slice(&u64::from(locator_record_index + 1).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    let identities = [
        occurrence_guid,
        "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        "cccccccc-cccc-cccc-cccc-cccccccccccc",
        "dddddddd-dddd-dddd-dddd-dddddddddddd",
    ];
    for guid in &identities[..3] {
        let encoded = guid.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    }
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    for guid in &identities[3..] {
        let encoded = guid.encode_utf16().collect::<Vec<_>>();
        bytes.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
        bytes.extend(encoded.into_iter().flat_map(u16::to_le_bytes));
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);

    let wrapper_record_index = locator_record_index + 2;
    let path_record_index = locator_record_index + 1;
    let mut wrapper = vec![0_u8; 37];
    wrapper[..4].copy_from_slice(&3_u32.to_le_bytes());
    wrapper[4..7].copy_from_slice(b"271");
    wrapper[7..11].copy_from_slice(&wrapper_record_index.to_le_bytes());
    wrapper[21] = 1;
    wrapper[22..26].copy_from_slice(&1_u32.to_le_bytes());
    write_reference(&mut wrapper, 26, path_record_index);
    bytes.extend(wrapper);
}

fn append_axial_test_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

fn append_axial_test_utf16(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
    bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
}

fn append_axial_test_reference(bytes: &mut Vec<u8>, target: u64) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
}

fn write_axial_test_reference(bytes: &mut [u8], at: usize, target: u64) {
    bytes[at] = 1;
    bytes[at + 1..at + 9].copy_from_slice(&target.to_le_bytes());
    bytes[at + 9..at + 11].fill(0);
}

fn append_axial_test_selector(
    bytes: &mut Vec<u8>,
    axis_record_index: u32,
    occurrence_reference: u64,
    external_object_reference: u64,
    role: &str,
    versioned: bool,
) -> u32 {
    const ASSET: &str = "11111111-1111-1111-1111-111111111111";
    const CONTEXT: &str = "22222222-2222-2222-2222-222222222222";
    const PROPERTY: &str = "33333333-3333-3333-3333-333333333333";

    append_axial_test_header(bytes, b"316", axis_record_index);
    append_axial_test_header(bytes, b"261", axis_record_index);
    let selector_record_index = axis_record_index + 3;
    append_axial_test_header(bytes, b"277", selector_record_index);
    bytes.extend_from_slice(&[0; 11]);
    append_axial_test_reference(bytes, u64::from(selector_record_index + 3));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    append_axial_test_utf16(bytes, ASSET);
    append_axial_test_utf16(bytes, CONTEXT);
    for value in [2_u32, 0, 1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    append_axial_test_reference(bytes, occurrence_reference);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&external_object_reference.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    append_axial_test_utf16(bytes, ASSET);
    bytes.push(0);
    append_axial_test_utf16(bytes, "component-link");
    bytes.push(u8::from(versioned));
    if versioned {
        append_axial_test_utf16(bytes, PROPERTY);
        append_axial_test_utf16(bytes, "urn:test:version:2");
    }
    append_axial_test_header(bytes, b"261", selector_record_index);
    append_axial_test_header(bytes, b"298", selector_record_index + 5);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    append_axial_test_utf16(bytes, role);
    selector_record_index
}

fn append_axial_test_component_operand(
    bytes: &mut Vec<u8>,
    construction_record_index: u32,
    axis_record_indices: [u32; 2],
    transform: [[f64; 4]; 4],
    external_object_reference: u64,
    role: &str,
    versioned: bool,
) -> Vec<u32> {
    let first_selector = append_axial_test_selector(
        bytes,
        axis_record_indices[0],
        10_001,
        external_object_reference,
        role,
        versioned,
    );
    let second_selector = append_axial_test_selector(
        bytes,
        axis_record_indices[1],
        10_002,
        external_object_reference,
        role,
        versioned,
    );
    let construction_at = bytes.len();
    append_axial_test_header(bytes, b"305", construction_record_index);
    bytes.resize(construction_at + 380, 0);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = construction_at + 48 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    write_axial_test_reference(
        bytes,
        construction_at + 192,
        u64::from(axis_record_indices[0]),
    );
    write_axial_test_reference(
        bytes,
        construction_at + 208,
        u64::from(axis_record_indices[1]),
    );
    append_axial_test_header(bytes, b"261", construction_record_index);
    vec![
        axis_record_indices[0],
        first_selector,
        axis_record_indices[1],
        second_selector,
        construction_record_index,
    ]
}

fn axial_test_alignment(transforms: [[[f64; 4]; 4]; 2]) -> DesignAssemblyAlignment {
    DesignAssemblyAlignment {
        angle: 0.0,
        offset: [0.0; 3],
        owner_record_indices: vec![90, 91],
        value_offsets: vec![1, 2],
        operand_frames: Some([
            DesignAssemblyOperandFrame {
                reference_record_index: 70,
                reference_offset: 1,
                transform: transforms[0],
                transform_offset: 2,
            },
            DesignAssemblyOperandFrame {
                reference_record_index: 80,
                reference_offset: 3,
                transform: transforms[1],
                transform_offset: 4,
            },
        ]),
        operand_paths: None,
        axial_operand_targets: None,
        joint_origin_scope_record_index: None,
    }
}

fn axial_test_component_scope(record_index: u32, role: &str) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:component-insert#{record_index}"),
        "Component Insert",
        record_index,
    );
    scope.component_insert_construction = Some(DesignComponentInsertConstruction {
        relation_record_index: record_index + 1,
        carrier_record_index: record_index + 2,
        occurrence_identity: None,
        neutron_role: role.into(),
        neutron_role_offset: 0,
        transform: identity_matrix(),
        transform_offset: 0,
        carrier_transform_offset: 0,
    });
    scope
}
