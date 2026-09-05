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
fn sketch_profile_frame_resolves_its_decimal_entity_suffix() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"308");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "e72ed0d8-58b4-4b8e-800d-5eaeea9c0c4b");
    lp_utf16(&mut bytes, "172");
    let tail_at = bytes.len();
    bytes.extend_from_slice(&[0; 94]);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "308".into(),
        record_index: 100,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#172".into(),
        byte_offset: 1000,
        entity_suffix: 172,
        entity_id: "0_172".into(),
        class_tag: "269".into(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: Some(200),
        record_reference_offset: Some(1010),
        declared_reference_count: Some(0),
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };

    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        4,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("sketch-profile operand");
    assert_eq!(profile.scope_reference_ordinal, 4);
    assert_eq!(profile.entity_suffix, 172);
    assert_eq!(profile.entity_id, "0_172");
    assert_eq!(profile.paired_byte_offset, paired_at as u64);

    bytes.truncate(paired_at - 94);
    bytes[4..7].copy_from_slice(b"319");
    let mut compact_tail = vec![0; 93];
    compact_tail[0] = 1;
    compact_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    compact_tail[12] = 1;
    compact_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    compact_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[53..57].copy_from_slice(&99u32.to_le_bytes());
    compact_tail[57] = 1;
    compact_tail[58..62].copy_from_slice(&102u32.to_le_bytes());
    compact_tail[70] = 1;
    compact_tail[71..75].copy_from_slice(&101u32.to_le_bytes());
    compact_tail[82] = 1;
    compact_tail[83..87].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_tail);
    let compact_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let compact_header = DesignRecordHeader {
        class_tag: "319".into(),
        ..header
    };
    let compact = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("compact sketch-profile operand");
    assert_eq!(compact.scope_reference_ordinal, 2);
    assert_eq!(compact.paired_byte_offset, compact_paired_at as u64);

    bytes.truncate(tail_at);
    let mut omitted_ordinal_tail = vec![0; 89];
    omitted_ordinal_tail[0] = 1;
    omitted_ordinal_tail[8..12].copy_from_slice(&1u32.to_le_bytes());
    omitted_ordinal_tail[12] = 1;
    omitted_ordinal_tail[13..17].copy_from_slice(&500u32.to_le_bytes());
    omitted_ordinal_tail[41..45].copy_from_slice(&99u32.to_le_bytes());
    omitted_ordinal_tail[53] = 1;
    omitted_ordinal_tail[54..58].copy_from_slice(&102u32.to_le_bytes());
    omitted_ordinal_tail[66] = 1;
    omitted_ordinal_tail[67..71].copy_from_slice(&101u32.to_le_bytes());
    omitted_ordinal_tail[78] = 1;
    omitted_ordinal_tail[79..83].copy_from_slice(&777u32.to_le_bytes());
    bytes.extend_from_slice(&omitted_ordinal_tail);
    let omitted_paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"258");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    let omitted = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        2,
        &compact_header,
        std::slice::from_ref(&entity),
    )
    .expect("omitted-ordinal sketch-profile operand");
    assert_eq!(omitted.paired_byte_offset, omitted_paired_at as u64);
}

#[test]
fn generated_base_flange_profile_frame_resolves() {
    let (bytes, _) = crate::test_support::generated_design_base_flange_bulkstream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let profile_offset = records
        .offsets(1501)
        .first()
        .copied()
        .expect("generated BaseFlange profile");
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#1501".into(),
        byte_offset: profile_offset as u64,
        class_tag: "377".into(),
        record_index: 1501,
    };
    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:entity#800".into(),
        byte_offset: 0,
        entity_suffix: 800,
        entity_id: "Sketch_800".into(),
        class_tag: "365".into(),
        optional_slot_present: false,
        module: Some(DESIGN_MODULE_SKETCH.to_owned()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    let profile = parse_sketch_profile(
        &bytes,
        "f3d:Design/BulkStream.dat",
        1,
        &header,
        std::slice::from_ref(&entity),
    )
    .expect("generated BaseFlange profile operand");
    assert_eq!(profile.entity_id, "Sketch_800");
    assert_eq!(profile.entity_suffix, 800);
}

#[test]
fn extrude_operand_identity_walks_shared_wrapper_grammar_to_a_fixed_leaf() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
        scope_record_index: 12,
        scope_reference_ordinal: 0,
        record_index: 100,
        byte_offset: 1000,
        class_tag: "332".into(),
        members: vec![200],
        lost_edge_references: Vec::new(),
        member_offsets: vec![1026],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 1021,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![300],
            trailing_record_offsets: vec![1043],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 180,
            opaque_index_offset: 1071,
            opaque_scalar: 0.125,
            opaque_scalar_offset: 1075,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Bodies),
        role_offset: 1053,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1124,
    };
    let wrapper_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: "326".into(),
        record_index: 300,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"326", 300);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    header(&mut bytes, *b"326", 305);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    header(&mut bytes, *b"324", 400);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    header(&mut bytes, *b"301", 900);

    let identity = parse_construction_operand_identity(&bytes, &group, &wrapper_header)
        .expect("identity chain");
    assert_eq!(identity.wrappers.iter().map(|wrapper| wrapper.record_index).collect::<Vec<_>>(), [300, 305]);
    assert_eq!(identity.wrappers.iter().map(|wrapper| wrapper.byte_offset).collect::<Vec<_>>(), [0, 24]);
    assert_eq!(identity.following_record_index, 400);
    assert_eq!(identity.following_byte_offset, 48);
    let persistent = identity
        .persistent_identity
        .as_ref()
        .expect("fixed persistent identity leaf");
    assert_eq!(persistent.local_id, 586);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset, 238);

    let mut expanded_bytes = bytes[..233].to_vec();
    expanded_bytes.extend_from_slice(&[0; 4]);
    expanded_bytes.push(1);
    expanded_bytes.extend_from_slice(&900u32.to_le_bytes());
    expanded_bytes.extend_from_slice(&[0; 6]);
    header(&mut expanded_bytes, *b"301", 900);
    let expanded = parse_construction_operand_identity(&expanded_bytes, &group, &wrapper_header)
        .expect("identity chain with expanded tail reference");
    let persistent = expanded
        .persistent_identity
        .expect("expanded persistent identity leaf");
    assert_eq!(persistent.tail_slot_offset, 233);
    assert_eq!(persistent.next_record_index, 900);
    assert_eq!(persistent.next_byte_offset, 248);

    let mut bound_group = group;
    let mut terminating_identity = identity;
    terminating_identity.id =
        "f3d:Design/BulkStream.dat:design-construction-operand-identity#200".into();
    terminating_identity.wrappers[0].byte_offset = 200;
    bind_lost_edge_groups(
        std::slice::from_mut(&mut bound_group),
        std::slice::from_ref(&terminating_identity),
        &[LostEdgeReference {
            id: "f3d:Design/BulkStream.dat:lost-edge-reference#152".into(),
            record_byte_offset: 152,
            class_tag_offset: 156,
            class_tag: "419".into(),
            record_index: 299,
            record_index_offset: 159,
            byte_offset: 181,
            next_byte_offset: 200,
            next_class_tag: "326".into(),
            next_record_index: 300,
        }],
    )
    .expect("lost-edge run terminates at the group identity");
    assert_eq!(
        bound_group.lost_edge_references,
        ["f3d:Design/BulkStream.dat:lost-edge-reference#152"]
    );
}

#[test]
fn nested_entity_selection_member_retains_compact_and_expanded_identities() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "269".into(),
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![926],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![200],
            trailing_record_offsets: vec![943],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 971,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 975,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        role_offset: 953,

        paired_class_tag: "265".into(),
        paired_byte_offset: 1024,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "333".into(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"333", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&103u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[1, 0, 0]);
    header(&mut bytes, *b"265", 100);
    header(&mut bytes, *b"301", 101);
    header(&mut bytes, *b"446", 102);
    let identity_at = bytes.len();
    header(&mut bytes, *b"429", 103);
    bytes.extend_from_slice(&[0; 18]);
    bytes.extend_from_slice(&1331u64.to_le_bytes());
    bytes.extend_from_slice(&183u64.to_le_bytes());
    let next_at = bytes.len();
    header(&mut bytes, *b"311", 104);

    let operand = parse_entity_selection_operand(&bytes, &group, 0, &record)
        .expect("nested entity-selection frame");
    assert_eq!(operand.primary_identity, 1331);
    assert_eq!(operand.secondary_identity.map(|identity| identity.value), Some(183));
    assert_eq!(operand.identity_record_offset, identity_at as u64);
    assert_eq!(operand.next_byte_offset, next_at as u64);

    let mut compact = bytes[..identity_at].to_vec();
    header(&mut compact, *b"429", 103);
    compact.extend_from_slice(&[0; 10]);
    compact.extend_from_slice(&1331u64.to_le_bytes());
    let compact_next_at = compact.len();
    header(&mut compact, *b"311", 109);
    let compact_operand = parse_entity_selection_operand(&compact, &group, 0, &record)
        .expect("compact nested entity-selection frame");
    assert_eq!(compact_operand.primary_identity, 1331);
    assert_eq!(compact_operand.secondary_identity.map(|identity| identity.value), None);
    assert_eq!(compact_operand.identity_record_offset, identity_at as u64);
    assert_eq!(compact_operand.next_record_index, 109);
    assert_eq!(compact_operand.next_byte_offset, compact_next_at as u64);

    let mut curve_identity = bytes[..identity_at].to_vec();
    header(&mut curve_identity, *b"429", 103);
    curve_identity.extend_from_slice(&[0; 10]);
    curve_identity.extend_from_slice(&77u64.to_le_bytes());
    curve_identity.extend_from_slice(&1331u64.to_le_bytes());
    curve_identity.extend_from_slice(&183u64.to_le_bytes());
    let curve_next_at = curve_identity.len();
    header(&mut curve_identity, *b"311", 104);
    let curve_operand = parse_entity_selection_operand(&curve_identity, &group, 0, &record)
        .expect("expanded Sketch-curve entity-selection frame");
    assert_eq!(curve_operand.primary_identity, 1331);
    assert_eq!(curve_operand.secondary_identity.map(|identity| identity.value), Some(183));
    assert_eq!(curve_operand.curve_secondary_identity.map(|identity| identity.value), Some(77));
    assert_eq!(
        curve_operand.curve_secondary_identity.map(|identity| identity.offset),
        Some(identity_at as u64 + 21)
    );
    assert_eq!(curve_operand.next_byte_offset, curve_next_at as u64);

    let mut class_338_curve_identity = bytes[..identity_at].to_vec();
    class_338_curve_identity[4..7].copy_from_slice(b"338");
    header(&mut class_338_curve_identity, *b"361", 103);
    class_338_curve_identity.extend_from_slice(&[0; 9]);
    class_338_curve_identity.push(1);
    class_338_curve_identity.extend_from_slice(&[0; 12]);
    class_338_curve_identity.extend_from_slice(&949u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&249u32.to_le_bytes());
    class_338_curve_identity.extend_from_slice(&0u32.to_le_bytes());
    let class_338_next_at = class_338_curve_identity.len();
    header(&mut class_338_curve_identity, *b"268", 104);
    let class_338_record = DesignRecordHeader {
        class_tag: "338".into(),
        ..record
    };
    let class_338_operand =
        parse_entity_selection_operand(&class_338_curve_identity, &group, 0, &class_338_record)
            .expect("class-338 Sketch-curve entity-selection frame");
    assert_eq!(class_338_operand.primary_identity, 949);
    assert_eq!(class_338_operand.secondary_identity.map(|identity| identity.value), Some(249));
    assert_eq!(class_338_operand.curve_secondary_identity.map(|identity| identity.value), None);
    assert_eq!(
        class_338_operand.primary_identity_offset,
        identity_at as u64 + 33
    );
    assert_eq!(
        class_338_operand.secondary_identity.map(|identity| identity.offset),
        Some(identity_at as u64 + 41)
    );
    assert_eq!(class_338_operand.next_byte_offset, class_338_next_at as u64);

    let mut invalid_class_338 = class_338_curve_identity.clone();
    invalid_class_338[identity_at + 20] = 0;
    assert!(
        parse_entity_selection_operand(&invalid_class_338, &group, 0, &class_338_record).is_none()
    );
}

#[test]
fn extrude_selection_group_and_members_have_exact_counted_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#12".into(),
        byte_offset: 1000,
        class_tag: "301".into(),
        record_index: 12,
        frame_length: 200,
        kind_offset: 1100,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: None,
        history_state_id_offset: 0,
        previous_history_state_id: None,
        previous_history_state_id_offset: None,
        reference_count_offset: 1080,
        reference_members: vec![100],
        reference_member_offsets: vec![1085],
        payload: crate::records::DesignFeatureKind::Extrude.into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "331".into(),
        record_index: 100,
    };
    let mut group_bytes = Vec::new();
    header(&mut group_bytes, *b"331", 100);
    group_bytes.extend_from_slice(&[0; 10]);
    group_bytes.push(1);
    group_bytes.extend_from_slice(&12u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    group_bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [200u32, 201] {
        group_bytes.push(1);
        group_bytes.extend_from_slice(&member.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
    }
    group_bytes.extend_from_slice(&180u32.to_le_bytes());
    group_bytes.extend_from_slice(&0.25f64.to_le_bytes());
    group_bytes.extend_from_slice(&180u32.to_le_bytes());
    group_bytes.push(1);
    group_bytes.extend_from_slice(&102u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    group_bytes.extend_from_slice(&[1, 1, 0, 1]);
    group_bytes.extend_from_slice(&101u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 7]);
    group_bytes.push(1);
    group_bytes.extend_from_slice(&12u32.to_le_bytes());
    group_bytes.extend_from_slice(&[0; 6]);
    let paired_at = group_bytes.len();
    header(&mut group_bytes, *b"259", 100);

    let mut group = parse_extrude_selection_group(&group_bytes, &scope, 0, &record)
        .expect("counted Extrude selection group");
    assert_eq!(group.members.iter().map(|member| member.value).collect::<Vec<_>>(), [200, 201]);
    assert_eq!(group.opaque_index, 180);
    assert_eq!(group.opaque_scalar, 0.25);
    assert!(group.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    let member_record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#200".into(),
        byte_offset: 0,
        class_tag: "290".into(),
        record_index: 200,
    };
    let mut member_bytes = Vec::new();
    header(&mut member_bytes, *b"290", 200);
    member_bytes.extend_from_slice(&[0; 10]);
    member_bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut member_bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut member_bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    member_bytes.extend_from_slice(&2u32.to_le_bytes());
    member_bytes.extend_from_slice(&[0; 5]);
    header(&mut member_bytes, *b"290", 201);

    let mut member = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
        .expect("fixed Extrude selection member");
    assert_eq!(member.local_id, 586);
    assert_eq!(member.next_byte_offset, 190);
    assert_eq!(member.next_record_index, 201);
    assert!(!member.tail_slot_present);
    assert_eq!(member.tail_slot_offset, 185);

    member_bytes[185] = 1;
    let member_with_slot = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
        .expect("Extrude selection member with present tail slot");
    assert!(member_with_slot.tail_slot_present);
    assert_eq!(member_with_slot.tail_slot_offset, 185);

    let terminal_member =
        parse_extrude_selection_member(&member_bytes[..190], &group, 0, &member_record)
            .expect("terminal fixed Extrude selection member");
    assert_eq!(terminal_member.next_byte_offset, 190);
    assert_eq!(terminal_member.next_record_index, 0);

    let mut edge_identity_bytes = Vec::new();
    header(&mut edge_identity_bytes, *b"278", 5887);
    edge_identity_bytes.extend_from_slice(&[0; 12]);
    edge_identity_bytes.push(1);
    edge_identity_bytes.extend_from_slice(&5890u32.to_le_bytes());
    edge_identity_bytes.extend_from_slice(&[0; 6]);
    edge_identity_bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(
        &mut edge_identity_bytes,
        "ad3001bb-a0fc-44c2-9b7a-c8b8fb70bfc0",
    );
    lp_utf16(
        &mut edge_identity_bytes,
        "1d8b67fc-c638-4af3-b13d-776dce4f472d",
    );
    let edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("fixed edge-treatment selection identity");
    assert_eq!(edge_identity.local_id, 5890);
    assert!(!edge_identity.compact_layout);
    assert_eq!(edge_identity.local_id_offset, 24);
    assert_eq!(edge_identity.asset_id_offset, 42);
    assert_eq!(edge_identity.context_id_offset, 118);

    edge_identity_bytes.remove(22);
    let compact_edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("compact fixed edge-treatment selection identity");
    assert!(compact_edge_identity.compact_layout);
    assert_eq!(compact_edge_identity.local_id, 5890);
    assert_eq!(compact_edge_identity.local_id_offset, 23);
    assert_eq!(compact_edge_identity.asset_id_offset, 41);
    assert_eq!(compact_edge_identity.context_id_offset, 117);

    edge_identity_bytes.remove(21);
    let shortest_edge_identity =
        crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
            .expect("short compact edge-treatment selection identity");
    assert!(shortest_edge_identity.compact_layout);
    assert_eq!(shortest_edge_identity.local_id, 5890);
    assert_eq!(shortest_edge_identity.local_id_offset, 22);
    assert_eq!(shortest_edge_identity.asset_id_offset, 40);
    assert_eq!(shortest_edge_identity.context_id_offset, 116);

    group.id = "f3d:Design/BulkStream.dat:selection-group#100".into();
    member.id = "f3d:Design/BulkStream.dat:selection-member#200".into();
    let identity = DesignConstructionOperandIdentity {
        id: "f3d:Design/BulkStream.dat:operand-identity#50".into(),
        group_record_index: 50,
        wrappers: vec![crate::records::DesignIdentityWrapper { record_index: 150, byte_offset: 50, class_tag: "289".into() }],
        following_record_index: 200,
        following_byte_offset: 0,
        following_class_tag: "290".into(),
        tracking_path: None,
        persistent_identity: Some(DesignConstructionPersistentIdentity {
            local_id: 586,
            local_id_offset: 21,
            asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
            asset_id_offset: 33,
            context_id: "0b2382d1-caaf-4eb9-b40d-a6322a7ed829".into(),
            context_id_offset: 113,
            tail_slot_present: false,
            tail_slot_offset: 185,
            next_record_index: 201,
            next_byte_offset: 190,
        }),
    };
    bind_extrude_selection_identities(
        std::slice::from_mut(&mut member),
        std::slice::from_ref(&identity),
    );
    assert_eq!(member.operand_identity_ids, [identity.id]);
    let mut owning_scope = scope;
    if let crate::records::DesignScopePayload::Extrude(slot)
    | crate::records::DesignScopePayload::Extrusion(slot)
    | crate::records::DesignScopePayload::Extrusao(slot) = &mut owning_scope.payload
    {
        slot.get_or_insert_with(Default::default).extrude_profile =
            Some(DesignSketchProfileOperand {
                scope_reference_ordinal: 1,
                record_index: 300,
                byte_offset: 3000,
                class_tag: "308".into(),
                asset_id: "df9087bd-02a6-4a3f-a132-7e69990f323c".into(),
                asset_id_offset: 3040,
                entity_id: "0_172".into(),
                entity_suffix: 172,
                entity_reference_offset: 3120,
                region_selection: None,
                paired_class_tag: "259".into(),
                paired_byte_offset: 3200,
            });
    }
    let curve = SketchCurveIdentity {
        id: "f3d:Design/BulkStream.dat:sketch-curve#400".into(),
        record_index: 400,
        owner_reference: Some(172),
        class_tag: "270".into(),
        byte_offset: 4000,
        geometry_offset: 100,
        entity_genesis: None,
        primary_id: 586,
        secondary_id: 0,
        geometry: None,
    };
    bind_extrude_selection_geometry(
        std::slice::from_mut(&mut member),
        std::slice::from_ref(&group),
        std::slice::from_ref(&owning_scope),
        &[],
        &[curve],
    );
    assert!(matches!(
        member.resolved_geometry,
        Some(SketchRelationOperand::Curve {
            record_index: 400,
            primary_id: 586,
            secondary_id: 0,
        })
    ));

    let remaining_members = group.members.split_off(1);
    let sketch_id = SketchId("f3d:model:sketch#172".into());
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
        profiles: vec![vec![SketchEntityUse {
            entity: neutral_sketch_curve_id(&sketch_id, 586, 0),
            reversed: false,
        }]],
        native_ref: None,
    };
    let arrangement_budget = WorkBudget::new(MAX_ARRANGEMENT_WALK_WORK);
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &std::collections::HashMap::new(),
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
                arrangement_budget: &arrangement_budget,
            }
            .scoped(&[]),
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0]
    ));
    let mut point_member = member.clone();
    point_member.id = "f3d:Design/BulkStream.dat:selection-member#201".into();
    point_member.record_index = 201;
    point_member.group_member_ordinal = 1;
    point_member.local_id = 587;
    point_member.resolved_geometry = Some(SketchRelationOperand::Point {
        record_index: 401,
        persistent_id: Some(587),
    });
    group.members.extend(remaining_members);
    let mut sketch = sketch;
    let second_profile_id = SketchEntityId("second-profile".into());
    sketch.profiles.push(vec![SketchEntityUse {
        entity: second_profile_id.clone(),
        reversed: false,
    }]);
    let point_entity = SketchEntity::new(
        neutral_sketch_point_id(&sketch_id, 587),
        sketch_id.clone(),
        SketchGeometry::Point {
            position: Point2::new(0.5, 1.0),
        },
    );
    let line_entity = SketchEntity::new(
        neutral_sketch_curve_id(&sketch_id, 586, 0),
        sketch_id.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
        },
    );
    let second_profile_entity = SketchEntity::new(
        second_profile_id,
        sketch_id.clone(),
        SketchGeometry::Line {
            start: Point2::new(0.0, 1.0),
            end: Point2::new(1.0, 1.0),
        },
    );
    let profile_entities = [line_entity, second_profile_entity, point_entity];
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            &[member.clone(), point_member],
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &profile_entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &std::collections::HashMap::new(),
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
                arrangement_budget: &arrangement_budget,
            }
            .scoped(&[]),
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0, 1]
    ));
    member.resolved_geometry = None;
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &std::collections::HashMap::new(),
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
                arrangement_budget: &arrangement_budget,
            }
            .scoped(&[]),
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchSelection {
            sketch: ref actual_sketch,
            selections: ref actual_selections,
        } if actual_sketch == &sketch_id && actual_selections == &[group.id.clone()]
    ));
    let mut single_profile_sketch = sketch.clone();
    single_profile_sketch.profiles.truncate(1);
    assert!(matches!(
        resolved_extrude_profile_selection(
            &sketch_id,
            &group,
            std::slice::from_ref(&member),
            &single_profile_sketch,
            crate::design::profile_select::ExtrudeProfileResolution {
                entities: &[],
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &std::collections::HashMap::new(),
                linear_tolerance: 1.0e-6,
                angular_tolerance: 1.0e-9,
                arrangement_budget: &arrangement_budget,
            }
            .scoped(&[]),
            None,
            None,
        ),
        cadmpeg_ir::features::ProfileRef::SketchProfiles {
            sketch: ref actual_sketch,
            ref profiles,
        } if actual_sketch == &sketch_id && profiles == &[0]
    ));
}
