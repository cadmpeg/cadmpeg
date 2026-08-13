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
fn parameter_scope_parses_named_variable_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "draft-name");
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x4e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x4d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x4c);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("named variable-tail scope");
    assert_eq!(scope.kind, "Draft");
    assert_eq!(scope.feature_ordinal, 1);
    assert_eq!(scope.history_state_id, Some(7));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
    assert_eq!(scope.reference_members, [55]);
    assert_eq!(scope.frame_length, paired_at as u64);

    let mut owner_scope = scope.clone();
    owner_scope.reference_members = vec![327, 330, 55, 56, 57, 58];
    let owners = vec![
        DesignParameterOwner {
            id: "f3d:test:owner#327".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "272".into(),
            record_index: 327,
            scope_record_index: 12,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 111,
            parameter_record_index: 326,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 328,
        },
        DesignParameterOwner {
            id: "f3d:test:owner#330".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "272".into(),
            record_index: 330,
            scope_record_index: 12,
            local_ordinal: 1,
            evaluated_value: 0.0,
            evaluated_value_offset: 222,
            parameter_record_index: 329,
            owned_ordinal: 4,
            variant: Some(0),
            companion_record_index: 331,
        },
    ];
    let operation = exact_draft_operation_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &owner_scope,
        &owners,
    )
    .expect("owner-lane Draft operation");
    assert_eq!(operation.angle, 0.0);
    assert_eq!(operation.angle_record_index, 327);
    assert_eq!(operation.opposite_angle_record_index, 330);
    assert_eq!(operation.angle_offset, 111);
    assert_eq!(operation.opposite_angle_offset, 222);
}

#[test]
fn parameter_scope_parses_named_tail_with_empty_label() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "CylinderPrimitive");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x0f);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x0e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x0d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("empty-label named scope");
    assert_eq!(scope.kind, "CylinderPrimitive");
    assert_eq!(scope.frame_length, paired_at as u64);
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
}

#[test]
fn parameter_scope_uses_same_index_pair_and_fixed_kind_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    let reference_count_at = bytes.len();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    let reference_at = bytes.len();
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Sketch");
    let feature_ordinal_at = bytes.len();
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let mut scope =
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap();
    assert_eq!(scope.kind, "Sketch");
    assert_eq!(scope.feature_ordinal, 1);
    assert_eq!(scope.feature_ordinal_offset, feature_ordinal_at as u64);
    assert_eq!(scope.history_state_id, Some(7));
    assert_eq!(scope.previous_history_state_id, Some(2));
    assert_eq!(scope.reference_count_offset, reference_count_at as u64);
    assert_eq!(scope.reference_members, [55]);
    assert_eq!(scope.reference_member_offsets, [reference_at as u64]);
    assert_eq!(scope.frame_length, paired_at as u64);
    assert_eq!(scope.paired_class_tag, "261");
    assert_eq!(scope.paired_byte_offset, paired_at as u64);
    let discovered = crate::design::decode::scopes::parameter_scope_candidate_headers(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
    )
    .into_iter()
    .filter_map(|header| {
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
    })
    .collect::<Vec<_>>();
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].record_index, 12);

    let mut compact_tail = bytes.clone();
    compact_tail.remove(paired_at - 1);
    let compact = parse_parameter_scope(
        &compact_tail,
        &IndexedRecordOffsets::build(&compact_tail),
        &header,
    )
    .expect("scope with compact fixed tail");
    assert_eq!(compact.kind, "Sketch");
    assert_eq!(compact.frame_length, paired_at as u64 - 1);
    assert_eq!(compact.previous_history_state_id, Some(2));
    assert!(
        !crate::design::decode::scopes::parameter_scope_tail_length_is_valid("CopyPasteBodies", 78,)
    );

    for tail_length in [72, 76] {
        let mut legacy = bytes[..feature_ordinal_at].to_vec();
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[30..34].copy_from_slice(&2u32.to_le_bytes());
        legacy.extend_from_slice(&tail);
        legacy.extend_from_slice(&3u32.to_le_bytes());
        legacy.extend_from_slice(b"261");
        legacy.extend_from_slice(&12u32.to_le_bytes());
        let decoded =
            parse_parameter_scope(&legacy, &IndexedRecordOffsets::build(&legacy), &header)
                .expect("scope with legacy fixed tail");
        assert_eq!(decoded.kind, "Sketch");
        assert_eq!(decoded.previous_history_state_id, Some(2));
        assert_eq!(
            decoded.previous_history_state_id_offset,
            (feature_ordinal_at + 30) as u64
        );
    }

    let mut extended_tail = bytes[..feature_ordinal_at].to_vec();
    let mut tail = [0; 87];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[41..45].copy_from_slice(&3u32.to_le_bytes());
    extended_tail.extend_from_slice(&tail);
    extended_tail.extend_from_slice(&3u32.to_le_bytes());
    extended_tail.extend_from_slice(b"261");
    extended_tail.extend_from_slice(&12u32.to_le_bytes());
    let extended = parse_parameter_scope(
        &extended_tail,
        &IndexedRecordOffsets::build(&extended_tail),
        &header,
    )
    .expect("scope with extended fixed tail");
    assert_eq!(extended.previous_history_state_id, Some(3));
    assert_eq!(
        extended.previous_history_state_id_offset,
        (feature_ordinal_at + 41) as u64
    );

    for tail_length in [82, 104] {
        let mut variant = bytes[..feature_ordinal_at].to_vec();
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        variant.extend_from_slice(&tail);
        variant.extend_from_slice(&3u32.to_le_bytes());
        variant.extend_from_slice(b"261");
        variant.extend_from_slice(&12u32.to_le_bytes());
        let decoded =
            parse_parameter_scope(&variant, &IndexedRecordOffsets::build(&variant), &header)
                .expect("scope with extended no-history fixed tail");
        assert_eq!(decoded.kind, "Sketch");
        assert_eq!(decoded.previous_history_state_id, None);
        assert_eq!(decoded.previous_history_state_id_offset, 0);
    }

    let mut copy_scope = Vec::new();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"316");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 10]);
    copy_scope.extend_from_slice(&1u32.to_le_bytes());
    copy_scope.push(1);
    copy_scope.extend_from_slice(&55u32.to_le_bytes());
    copy_scope.extend_from_slice(&[0; 6]);
    copy_scope.extend_from_slice(&u32::MAX.to_le_bytes());
    lp_utf16(&mut copy_scope, "CopyPasteBodies");
    let copy_feature_ordinal_at = copy_scope.len();
    let mut copy_tail = [0; 110];
    copy_tail[0..4].copy_from_slice(&2u32.to_le_bytes());
    copy_tail[53..57].copy_from_slice(&u32::MAX.to_le_bytes());
    copy_scope.extend_from_slice(&copy_tail);
    let copy_paired_at = copy_scope.len();
    copy_scope.extend_from_slice(&3u32.to_le_bytes());
    copy_scope.extend_from_slice(b"259");
    copy_scope.extend_from_slice(&12u32.to_le_bytes());
    let copy = parse_parameter_scope(
        &copy_scope,
        &IndexedRecordOffsets::build(&copy_scope),
        &header,
    )
    .expect("CopyPasteBodies scope with extended tail");
    assert_eq!(copy.kind, "CopyPasteBodies");
    assert_eq!(copy.feature_ordinal, 2);
    assert_eq!(copy.feature_ordinal_offset, copy_feature_ordinal_at as u64);
    assert_eq!(copy.history_state_id, None);
    assert_eq!(copy.previous_history_state_id, None);
    assert_eq!(
        copy.previous_history_state_id_offset,
        (copy_feature_ordinal_at + 53) as u64
    );
    assert_eq!(copy.frame_length, copy_paired_at as u64);

    let mut operation_bytes = vec![0; 80];
    operation_bytes[29] = 1;
    operation_bytes[30..34].copy_from_slice(&55u32.to_le_bytes());
    operation_bytes[34..40].fill(0);
    operation_bytes[40] = 1;
    operation_bytes[41..45].copy_from_slice(&44u32.to_le_bytes());
    operation_bytes[45..51].fill(0);
    let body_group_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"264");
    operation_bytes.extend_from_slice(&55u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 10]);
    operation_bytes.extend_from_slice(&1u32.to_le_bytes());
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&66u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 6]);
    let relation_at = operation_bytes.len();
    operation_bytes.extend_from_slice(&3u32.to_le_bytes());
    operation_bytes.extend_from_slice(b"314");
    operation_bytes.extend_from_slice(&44u32.to_le_bytes());
    operation_bytes.extend_from_slice(&[0; 8]);
    operation_bytes.push(1);
    operation_bytes.extend_from_slice(&2u32.to_le_bytes());
    for suffix in [1206, 1215] {
        operation_bytes.push(1);
        operation_bytes.extend_from_slice(&u32::to_le_bytes(suffix));
        operation_bytes.extend_from_slice(&[0; 10]);
    }
    let mut operation_scope = copy.clone();
    operation_scope.byte_offset = 0;
    operation_scope.paired_byte_offset = 60;
    operation_scope.reference_members = vec![55, 66];
    let operation = crate::design::decode::scopes::exact_copy_paste_bodies_operation(
        &operation_bytes,
        &IndexedRecordOffsets::build(&operation_bytes),
        &operation_scope,
    )
    .expect("single-body CopyPasteBodies relation");
    assert_eq!(operation.body_group_record_index, 55);
    assert_eq!(operation.body_group_byte_offset, body_group_at as u64);
    assert_eq!(operation.body_operand_record_indices, [66]);
    assert_eq!(operation.relation_record_index, 44);
    assert_eq!(operation.relation_byte_offset, relation_at as u64);
    assert_eq!(operation.source_body_entity_suffixes, [1206]);
    assert_eq!(operation.copied_body_entity_suffixes, [1215]);

    // A Sketch scope may also carry the generic ordered reference table
    // used by `EntityGenesis`-form streams; the table then has more than
    // one member and the entity join happens by unique suffix match.
    let mut generic_reference = vec![1];
    generic_reference.extend_from_slice(&56u32.to_le_bytes());
    generic_reference.extend_from_slice(&[0; 6]);
    let mut generic_references = bytes.clone();
    generic_references[reference_count_at..reference_count_at + 4]
        .copy_from_slice(&2u32.to_le_bytes());
    generic_references.splice(reference_at + 10..reference_at + 10, generic_reference);
    let generic_scope = parse_parameter_scope(
        &generic_references,
        &IndexedRecordOffsets::build(&generic_references),
        &header,
    )
    .expect("generic-table Sketch scope");
    assert_eq!(generic_scope.kind, "Sketch");
    assert_eq!(generic_scope.reference_members, [55, 56]);

    let work_plane_at = bytes.len();
    let mut work_plane = vec![0; 362];
    work_plane[0..4].copy_from_slice(&3u32.to_le_bytes());
    work_plane[4..7].copy_from_slice(b"293");
    work_plane[7..11].copy_from_slice(&55u32.to_le_bytes());
    work_plane[55] = 1;
    work_plane[57] = 1;
    work_plane[58..62].copy_from_slice(&99u32.to_le_bytes());
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        work_plane[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    work_plane.extend_from_slice(&3u32.to_le_bytes());
    work_plane.extend_from_slice(b"261");
    work_plane.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&work_plane);
    let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
        .expect("exact WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (work_plane_at + 76) as u64);
    assert_eq!(decoded.reference, Some((99, (work_plane_at + 58) as u64)));

    let extended_at = bytes.len();
    let mut extended = vec![0; 373];
    extended[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended[4..7].copy_from_slice(b"263");
    extended[7..11].copy_from_slice(&57u32.to_le_bytes());
    extended[55..58].copy_from_slice(&[1, 0, 1]);
    extended[58..62].copy_from_slice(&100u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 76 + ordinal * 8;
        extended[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended.extend_from_slice(&3u32.to_le_bytes());
    extended.extend_from_slice(b"261");
    extended.extend_from_slice(&57u32.to_le_bytes());
    bytes.extend_from_slice(&extended);
    let mut extended_scope = scope.clone();
    extended_scope.reference_members = vec![57];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &extended_scope,
    )
    .expect("extended referenced WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (extended_at + 76) as u64);
    assert_eq!(decoded.reference, Some((100, (extended_at + 58) as u64)));

    let direct_at = bytes.len();
    let mut direct = vec![0; 352];
    direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    direct[4..7].copy_from_slice(b"293");
    direct[7..11].copy_from_slice(&56u32.to_le_bytes());
    direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    direct.extend_from_slice(&3u32.to_le_bytes());
    direct.extend_from_slice(b"261");
    direct.extend_from_slice(&56u32.to_le_bytes());
    bytes.extend_from_slice(&direct);
    let mut direct_scope = scope.clone();
    direct_scope.reference_members = vec![56];
    let decoded =
        exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &direct_scope)
            .expect("direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let extended_direct_at = bytes.len();
    let mut extended_direct = vec![0; 363];
    extended_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    extended_direct[4..7].copy_from_slice(b"289");
    extended_direct[7..11].copy_from_slice(&61u32.to_le_bytes());
    extended_direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        extended_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    extended_direct.extend_from_slice(&3u32.to_le_bytes());
    extended_direct.extend_from_slice(b"258");
    extended_direct.extend_from_slice(&61u32.to_le_bytes());
    bytes.extend_from_slice(&extended_direct);
    let mut extended_direct_scope = scope.clone();
    extended_direct_scope.reference_members = vec![61];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &extended_direct_scope,
    )
    .expect("extended direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (extended_direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let large_direct_at = bytes.len();
    let mut large_direct = vec![0; 374];
    large_direct[0..4].copy_from_slice(&3u32.to_le_bytes());
    large_direct[4..7].copy_from_slice(b"267");
    large_direct[7..11].copy_from_slice(&62u32.to_le_bytes());
    large_direct[55] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 66 + ordinal * 8;
        large_direct[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    large_direct.extend_from_slice(&3u32.to_le_bytes());
    large_direct.extend_from_slice(b"258");
    large_direct.extend_from_slice(&62u32.to_le_bytes());
    bytes.extend_from_slice(&large_direct);
    let mut large_direct_scope = scope.clone();
    large_direct_scope.reference_members = vec![62];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &large_direct_scope,
    )
    .expect("large direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (large_direct_at + 66) as u64);
    assert_eq!(decoded.reference, None);

    let mut axis_bytes = vec![0; 232];
    axis_bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    axis_bytes[4..7].copy_from_slice(b"701");
    axis_bytes[7..11].copy_from_slice(&100u32.to_le_bytes());
    axis_bytes[21..25].copy_from_slice(&8u32.to_le_bytes());
    let axis_values = [1.0_f64, 2.0, 3.0, 0.0, -3.0, 4.0, 0.0, 0.0];
    for (ordinal, value) in axis_values.into_iter().enumerate() {
        let at = 25 + ordinal * 8;
        axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    axis_bytes[118..122].copy_from_slice(&2u32.to_le_bytes());
    for (ordinal, record_index) in [102_u32, 104].into_iter().enumerate() {
        let at = 122 + ordinal * 11;
        axis_bytes[at] = 1;
        axis_bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    axis_bytes.extend_from_slice(&3u32.to_le_bytes());
    axis_bytes.extend_from_slice(b"258");
    axis_bytes.extend_from_slice(&100u32.to_le_bytes());
    for (record_index, point) in [(102_u32, [1.0_f64, 2.0, 3.0]), (104, [1.0, -1.0, 7.0])] {
        let start = axis_bytes.len();
        axis_bytes.resize(start + 197, 0);
        axis_bytes[start..start + 4].copy_from_slice(&3u32.to_le_bytes());
        axis_bytes[start + 4..start + 7].copy_from_slice(b"702");
        axis_bytes[start + 7..start + 11].copy_from_slice(&record_index.to_le_bytes());
        for (ordinal, value) in point.into_iter().enumerate() {
            let at = start + 42 + ordinal * 8;
            axis_bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        axis_bytes.extend_from_slice(&3u32.to_le_bytes());
        axis_bytes.extend_from_slice(b"258");
        axis_bytes.extend_from_slice(&record_index.to_le_bytes());
    }
    let mut axis_scope = scope.clone();
    axis_scope.id = "f3d:native:parameter-scope#55".into();
    axis_scope.kind = "WorkAxis".into();
    axis_scope.reference_members = vec![100, 101, 102, 103, 104];
    let construction = exact_work_axis_construction(
        &axis_bytes,
        &IndexedRecordOffsets::build(&axis_bytes),
        &axis_scope,
    )
    .expect("exact two-point WorkAxis construction");
    assert_eq!(construction.origin, [1.0, 2.0, 3.0]);
    assert_eq!(construction.displacement, [0.0, -3.0, 4.0]);
    assert_eq!(construction.origin_offset, 25);
    assert_eq!(construction.displacement_offset, 49);
    assert_eq!(construction.point_record_indices, [102, 104]);
    axis_scope.work_axis_construction = Some(construction);
    let (axis_features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&axis_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        axis_features.as_slice(),
        [Feature {
            definition: FeatureDefinition::DatumAxis { origin, direction },
            ..
        }] if *origin == Point3::new(10.0, 20.0, 30.0)
            && *direction == Vector3::new(0.0, -0.6, 0.8)
    ));

    let compact_at = bytes.len();
    let mut compact = vec![0; 321];
    compact[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact[4..7].copy_from_slice(b"293");
    compact[7..11].copy_from_slice(&58u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact.extend_from_slice(&3u32.to_le_bytes());
    compact.extend_from_slice(b"261");
    compact.extend_from_slice(&58u32.to_le_bytes());
    bytes.extend_from_slice(&compact);
    let mut compact_scope = scope.clone();
    compact_scope.reference_members = vec![58];
    let decoded =
        exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &compact_scope)
            .expect("compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_431_at = bytes.len();
    let mut compact_431 = vec![0; 325];
    compact_431[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_431[4..7].copy_from_slice(b"431");
    compact_431[7..11].copy_from_slice(&67u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_431[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_431.extend_from_slice(&3u32.to_le_bytes());
    compact_431.extend_from_slice(b"257");
    compact_431.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_431);
    let mut compact_431_scope = scope.clone();
    compact_431_scope.reference_members = vec![67];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_431_scope,
    )
    .expect("class-431 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_431_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_364_at = bytes.len();
    let mut compact_364 = vec![0; 321];
    compact_364[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364[4..7].copy_from_slice(b"364");
    compact_364[7..11].copy_from_slice(&65u32.to_le_bytes());
    compact_364[46] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364.extend_from_slice(&3u32.to_le_bytes());
    compact_364.extend_from_slice(b"264");
    compact_364.extend_from_slice(&65u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364);
    let mut compact_364_scope = scope.clone();
    compact_364_scope.reference_members = vec![65];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_364_scope,
    )
    .expect("class-364 marked compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_364_at + 49) as u64);
    assert_eq!(decoded.reference, None);

    let compact_364_variant_at = bytes.len();
    let mut compact_364_variant = vec![0; 321];
    compact_364_variant[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_364_variant[4..7].copy_from_slice(b"364");
    compact_364_variant[7..11].copy_from_slice(&66u32.to_le_bytes());
    compact_364_variant[45..49].copy_from_slice(&[0xcc, 0xcd, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_364_variant[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_364_variant.extend_from_slice(&3u32.to_le_bytes());
    compact_364_variant.extend_from_slice(b"264");
    compact_364_variant.extend_from_slice(&66u32.to_le_bytes());
    bytes.extend_from_slice(&compact_364_variant);
    let mut compact_364_variant_scope = scope.clone();
    compact_364_variant_scope.reference_members = vec![66];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_364_variant_scope,
    )
    .expect("class-364 compact direct WorkPlane frame variant");
    assert_eq!(decoded.transform, transform);
    assert_eq!(
        decoded.transform_offset,
        (compact_364_variant_at + 49) as u64
    );
    assert_eq!(decoded.reference, None);

    let compact_450_at = bytes.len();
    let mut compact_450 = vec![0; 326];
    compact_450[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_450[4..7].copy_from_slice(b"450");
    compact_450[7..11].copy_from_slice(&59u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_450[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_450.extend_from_slice(&3u32.to_le_bytes());
    compact_450.extend_from_slice(b"259");
    compact_450.extend_from_slice(&59u32.to_le_bytes());
    bytes.extend_from_slice(&compact_450);
    let mut compact_450_scope = scope.clone();
    compact_450_scope.reference_members = vec![59];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_450_scope,
    )
    .expect("class-450 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_450_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let class_279_at = bytes.len();
    let mut class_279 = vec![0; 326];
    class_279[0..4].copy_from_slice(&3u32.to_le_bytes());
    class_279[4..7].copy_from_slice(b"279");
    class_279[7..11].copy_from_slice(&69u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        class_279[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    class_279.extend_from_slice(&3u32.to_le_bytes());
    class_279.extend_from_slice(b"266");
    class_279.extend_from_slice(&69u32.to_le_bytes());
    bytes.extend_from_slice(&class_279);
    let mut class_279_scope = scope.clone();
    class_279_scope.reference_members = vec![69];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &class_279_scope,
    )
    .expect("class-279 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (class_279_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let compact_409_short_at = bytes.len();
    let mut compact_409_short = compact_450.clone();
    compact_409_short[4..7].copy_from_slice(b"409");
    compact_409_short[7..11].copy_from_slice(&64u32.to_le_bytes());
    compact_409_short[330..333].copy_from_slice(b"258");
    compact_409_short[333..337].copy_from_slice(&64u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409_short);
    let mut compact_409_short_scope = scope.clone();
    compact_409_short_scope.reference_members = vec![64];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_409_short_scope,
    )
    .expect("short class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_409_short_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let compact_409_at = bytes.len();
    let mut compact_409 = vec![0; 337];
    compact_409[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_409[4..7].copy_from_slice(b"409");
    compact_409[7..11].copy_from_slice(&63u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 50 + ordinal * 8;
        compact_409[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_409.extend_from_slice(&3u32.to_le_bytes());
    compact_409.extend_from_slice(b"258");
    compact_409.extend_from_slice(&63u32.to_le_bytes());
    bytes.extend_from_slice(&compact_409);
    let mut compact_409_scope = scope.clone();
    compact_409_scope.reference_members = vec![63];
    let decoded = exact_work_plane_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_409_scope,
    )
    .expect("class-409 compact direct WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (compact_409_at + 50) as u64);
    assert_eq!(decoded.reference, None);

    let joint_origin_at = bytes.len();
    let mut joint_origin = vec![0; 336];
    joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    joint_origin[4..7].copy_from_slice(b"450");
    joint_origin[7..11].copy_from_slice(&60u32.to_le_bytes());
    joint_origin[45] = 1;
    joint_origin[46..50].copy_from_slice(&61u32.to_le_bytes());
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 60 + ordinal * 8;
        joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    joint_origin.extend_from_slice(&3u32.to_le_bytes());
    joint_origin.extend_from_slice(b"259");
    joint_origin.extend_from_slice(&60u32.to_le_bytes());
    bytes.extend_from_slice(&joint_origin);
    let mut joint_origin_scope = scope.clone();
    joint_origin_scope.kind = "JointOrigin".into();
    joint_origin_scope.reference_members = vec![60];
    let decoded = exact_joint_origin_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &joint_origin_scope,
    )
    .expect("exact JointOrigin frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, (joint_origin_at + 60) as u64);
    assert_eq!(decoded.reference, Some((61, (joint_origin_at + 46) as u64)));

    for frame_length in [300, 322, 344] {
        let mut construction_scope = joint_origin_scope.clone();
        construction_scope.frame_length = frame_length;
        assert!(
            exact_joint_origin_frame(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                &construction_scope,
            )
            .is_none(),
            "construction envelope {frame_length} must defer its solved frame to Assemble"
        );
    }

    let compact_joint_origin_at = bytes.len();
    let mut compact_joint_origin = vec![0; 385];
    compact_joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_joint_origin[4..7].copy_from_slice(b"364");
    compact_joint_origin[7..11].copy_from_slice(&67u32.to_le_bytes());
    compact_joint_origin[45..49].copy_from_slice(&[1, 1, 0, 0]);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        compact_joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_joint_origin.extend_from_slice(&3u32.to_le_bytes());
    compact_joint_origin.extend_from_slice(b"264");
    compact_joint_origin.extend_from_slice(&67u32.to_le_bytes());
    bytes.extend_from_slice(&compact_joint_origin);
    let mut compact_joint_origin_scope = scope.clone();
    compact_joint_origin_scope.kind = "JointOrigin".into();
    compact_joint_origin_scope.reference_members = vec![67];
    let decoded = exact_joint_origin_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_joint_origin_scope,
    )
    .expect("exact compact JointOrigin frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(
        decoded.transform_offset,
        (compact_joint_origin_at + 49) as u64
    );
    assert_eq!(decoded.reference, None);

    let move_at = bytes.len();
    let mut move_frame = vec![0; 254];
    move_frame[0..4].copy_from_slice(&3u32.to_le_bytes());
    move_frame[4..7].copy_from_slice(b"368");
    move_frame[7..11].copy_from_slice(&90u32.to_le_bytes());
    move_frame[43..47].copy_from_slice(&5u32.to_le_bytes());
    let mut move_transform = identity_matrix();
    move_transform[1][3] = 15.0;
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        move_frame[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    move_frame.extend_from_slice(&3u32.to_le_bytes());
    move_frame.extend_from_slice(b"265");
    move_frame.extend_from_slice(&90u32.to_le_bytes());
    bytes.extend_from_slice(&move_frame);
    let mut move_scope = scope.clone();
    move_scope.kind = "Move".into();
    move_scope.reference_members = vec![90];
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &move_scope,
    )
    .expect("class-368 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (move_at + 48) as u64);
    assert_eq!(decoded.form, 5);

    let compact_move_at = bytes.len();
    let mut compact_move = vec![0; 253];
    compact_move[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_move[4..7].copy_from_slice(b"296");
    compact_move[7..11].copy_from_slice(&91u32.to_le_bytes());
    compact_move[43..47].copy_from_slice(&1u32.to_le_bytes());
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        compact_move[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    compact_move.extend_from_slice(&3u32.to_le_bytes());
    compact_move.extend_from_slice(b"265");
    compact_move.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(&compact_move);
    let mut compact_move_scope = scope.clone();
    compact_move_scope.kind = "Move".into();
    compact_move_scope.reference_members = vec![91];
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_move_scope,
    )
    .expect("class-296 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (compact_move_at + 48) as u64);
    assert_eq!(decoded.transform_record_index, 91);
    assert_eq!(decoded.form, 1);
    assert_eq!(decoded.form_offset, (compact_move_at + 43) as u64);
    bytes[compact_move_at + 4..compact_move_at + 7].copy_from_slice(b"362");
    bytes[compact_move_at + 43..compact_move_at + 47].copy_from_slice(&5u32.to_le_bytes());
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_move_scope,
    )
    .expect("class-362 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.form, 5);

    let class_433_move_at = bytes.len();
    let mut class_433_move = vec![0; 253];
    class_433_move[0..4].copy_from_slice(&3u32.to_le_bytes());
    class_433_move[4..7].copy_from_slice(b"433");
    class_433_move[7..11].copy_from_slice(&92u32.to_le_bytes());
    class_433_move[43..47].copy_from_slice(&5u32.to_le_bytes());
    for (ordinal, value) in move_transform.into_iter().flatten().enumerate() {
        let at = 48 + ordinal * 8;
        class_433_move[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    class_433_move.extend_from_slice(&3u32.to_le_bytes());
    class_433_move.extend_from_slice(b"265");
    class_433_move.extend_from_slice(&92u32.to_le_bytes());
    bytes.extend_from_slice(&class_433_move);
    let mut class_433_move_scope = scope.clone();
    class_433_move_scope.kind = "Move".into();
    class_433_move_scope.reference_members = vec![92];
    let decoded = crate::design::decode::scopes::exact_move_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &class_433_move_scope,
    )
    .expect("class-433 Move frame");
    assert_eq!(decoded.transform, move_transform);
    assert_eq!(decoded.transform_offset, (class_433_move_at + 48) as u64);
    assert_eq!(decoded.transform_record_index, 92);
    assert_eq!(decoded.form, 5);

    let scale_at = bytes.len();
    let mut scale = vec![0; 317];
    scale[20..24].copy_from_slice(&1u32.to_le_bytes());
    scale[25..33].copy_from_slice(&1.5f64.to_le_bytes());
    for (offset, record_index) in [(33, 105u32), (44, 101), (68, 102)] {
        scale[offset] = 1;
        scale[offset + 1..offset + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    scale[55..59].copy_from_slice(&1u32.to_le_bytes());
    scale[60..64].copy_from_slice(&1u32.to_le_bytes());
    scale[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&scale);
    let mut scale_scope = scope.clone();
    scale_scope.byte_offset = scale_at as u64;
    scale_scope.kind = "Maßstab".into();
    scale_scope.frame_length = 317;
    scale_scope.reference_members = vec![101, 102, 103, 104, 105];
    assert_eq!(
        exact_scale_operation(&bytes, &scale_scope),
        Some(DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 101,
            uniform_factor: 1.5,
            uniform_factor_offset: (scale_at + 25) as u64,
        })
    );

    let sphere_at = bytes.len();
    let mut sphere = vec![0; 462];
    sphere[0..4].copy_from_slice(&3u32.to_le_bytes());
    sphere[4..7].copy_from_slice(b"302");
    sphere[7..11].copy_from_slice(&80u32.to_le_bytes());
    sphere[25..29].copy_from_slice(&4u32.to_le_bytes());
    sphere[29] = 1;
    sphere[30] = 1;
    sphere[41] = 1;
    sphere[42..46].copy_from_slice(&70u32.to_le_bytes());
    sphere[52] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 64 + ordinal * 8;
        sphere[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&sphere);
    let mut diameter = vec![0; 104];
    diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
    diameter[4..7].copy_from_slice(b"277");
    diameter[7..11].copy_from_slice(&70u32.to_le_bytes());
    diameter[40..48].copy_from_slice(&8.0f64.to_le_bytes());
    diameter.extend_from_slice(&3u32.to_le_bytes());
    diameter.extend_from_slice(b"261");
    diameter.extend_from_slice(&70u32.to_le_bytes());
    bytes.extend_from_slice(&diameter);
    let mut sphere_scope = scope.clone();
    sphere_scope.byte_offset = sphere_at as u64;
    sphere_scope.kind = "SpherePrimitive".into();
    sphere_scope.frame_length = 462;
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &sphere_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Sphere {
            diameter: 8.0,
            diameter_record_index: 70,
            operation: DesignExtrudeOperation::NewBody,
            ..
        })
    ));

    let torus_at = bytes.len();
    let mut torus = vec![0; 486];
    torus[0..4].copy_from_slice(&3u32.to_le_bytes());
    torus[4..7].copy_from_slice(b"305");
    torus[7..11].copy_from_slice(&81u32.to_le_bytes());
    torus[25..29].copy_from_slice(&4u32.to_le_bytes());
    torus[29] = 1;
    torus[30] = 1;
    torus[31..35].copy_from_slice(&71u32.to_le_bytes());
    torus[41] = 1;
    torus[52] = 1;
    torus[53..57].copy_from_slice(&72u32.to_le_bytes());
    torus[63] = 1;
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 75 + ordinal * 8;
        torus[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&torus);
    for (record_index, value) in [(71u32, 15.0f64), (72, 4.0)] {
        let mut diameter = vec![0; 104];
        diameter[0..4].copy_from_slice(&3u32.to_le_bytes());
        diameter[4..7].copy_from_slice(b"277");
        diameter[7..11].copy_from_slice(&record_index.to_le_bytes());
        diameter[40..48].copy_from_slice(&value.to_le_bytes());
        diameter.extend_from_slice(&3u32.to_le_bytes());
        diameter.extend_from_slice(b"261");
        diameter.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&diameter);
    }
    let mut torus_scope = scope.clone();
    torus_scope.byte_offset = torus_at as u64;
    torus_scope.kind = "TorusPrimitive".into();
    torus_scope.frame_length = 486;
    assert!(matches!(
        exact_solid_primitive(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &torus_scope,
            &[],
        ),
        Some(DesignSolidPrimitive::Torus {
            major_diameter: 15.0,
            minor_diameter: 4.0,
            operation: DesignExtrudeOperation::NewBody,
            ..
        })
    ));

    let offset_at = bytes.len();
    let mut offset = vec![0; 286];
    offset[25] = 1;
    offset[26..30].copy_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&offset);
    let mut distance = vec![0; 104];
    distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    distance[4..7].copy_from_slice(b"277");
    distance[7..11].copy_from_slice(&73u32.to_le_bytes());
    distance[40..48].copy_from_slice(&(-0.5f64).to_le_bytes());
    distance.extend_from_slice(&3u32.to_le_bytes());
    distance.extend_from_slice(b"261");
    distance.extend_from_slice(&73u32.to_le_bytes());
    bytes.extend_from_slice(&distance);
    let mut offset_scope = scope.clone();
    offset_scope.byte_offset = offset_at as u64;
    offset_scope.kind = "OffsetFaces".into();
    offset_scope.frame_length = 286;
    offset_scope.reference_members = vec![1, 2, 3, 73];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces {
            distance: -0.5,
            distance_record_index: 73,
            ..
        })
    ));

    let compact_offset_at = bytes.len();
    let mut compact_offset = vec![0; 275];
    compact_offset[25] = 1;
    compact_offset[26..30].copy_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_offset);
    let mut compact_distance = vec![0; 105];
    compact_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_distance[4..7].copy_from_slice(b"312");
    compact_distance[7..11].copy_from_slice(&1_777u32.to_le_bytes());
    compact_distance[24] = 1;
    compact_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_distance[40..48].copy_from_slice(&0.254f64.to_le_bytes());
    compact_distance.extend_from_slice(&3u32.to_le_bytes());
    compact_distance.extend_from_slice(b"259");
    compact_distance.extend_from_slice(&1_777u32.to_le_bytes());
    bytes.extend_from_slice(&compact_distance);
    offset_scope.byte_offset = compact_offset_at as u64;
    offset_scope.frame_length = 275;
    offset_scope.reference_members = vec![1, 2, 1_777];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope),
        Some(DesignDirectFaceOperation::OffsetFaces {
            distance: 0.254,
            distance_record_index: 1_777,
            ..
        })
    ));

    let thicken_at = bytes.len();
    let mut thicken = vec![0; 301];
    thicken[47] = 1;
    thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thicken);
    let mut thickness = vec![0; 104];
    thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    thickness[4..7].copy_from_slice(b"277");
    thickness[7..11].copy_from_slice(&74u32.to_le_bytes());
    thickness[40..48].copy_from_slice(&(-1.0f64).to_le_bytes());
    thickness.extend_from_slice(&3u32.to_le_bytes());
    thickness.extend_from_slice(b"261");
    thickness.extend_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&thickness);
    let mut thicken_scope = scope.clone();
    thicken_scope.byte_offset = thicken_at as u64;
    thicken_scope.kind = "Thicken".into();
    thicken_scope.frame_length = 301;
    thicken_scope.reference_members = vec![1, 2, 74];
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    thicken_scope.frame_length = 295;
    assert_eq!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        None
    );
    let compact_thicken_at = bytes.len();
    let mut compact_thicken = vec![0; 295];
    compact_thicken[45] = 1;
    compact_thicken[46] = 1;
    compact_thicken[47..51].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&compact_thicken);
    thicken_scope.byte_offset = compact_thicken_at as u64;
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    let shifted_thicken_at = bytes.len();
    let mut shifted_thicken = vec![0; 312];
    shifted_thicken[34] = 1;
    shifted_thicken[35..39].copy_from_slice(&200u32.to_le_bytes());
    shifted_thicken[46..48].copy_from_slice(&[1, 1]);
    shifted_thicken[48..52].copy_from_slice(&74u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_thicken);
    let shifted_thicken_scope = DesignParameterScope {
        byte_offset: shifted_thicken_at as u64,
        frame_length: 312,
        reference_members: vec![74, 200, 201, 202],
        ..thicken_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &shifted_thicken_scope,
        ),
        Some(DesignDirectFaceOperation::Thicken {
            signed_thickness: -1.0,
            thickness_record_index: 74,
            ..
        })
    ));
    thicken_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope);
    let thicken_group = DesignConstructionOperandGroup {
        id: "thicken-group".into(),
        scope_record_index: thicken_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 200,
        byte_offset: 0,
        class_tag: "264".into(),
        members: vec![201],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![202],
            trailing_record_offsets: vec![0],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0005_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,

        paired_class_tag: "264".into(),
        paired_byte_offset: 0,
    };
    assert!(matches!(
        crate::design::feature_project::project_thicken(&thicken_scope, &[], std::slice::from_ref(&thicken_group)),
        Some(cadmpeg_ir::features::FeatureDefinition::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(cadmpeg_ir::features::Length(10.0)),
            side: Some(cadmpeg_ir::features::ThickenSide::Reverse),
        }) if native == "thicken-group"
    ));
    let mut bounded_face_thicken_group = thicken_group.clone();
    bounded_face_thicken_group.role = 0x0000_0012_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_thicken(
            &thicken_scope,
            &[],
            std::slice::from_ref(&bounded_face_thicken_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Thicken {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            ..
        }) if native == "thicken-group"
    ));
    let shell_at = bytes.len();
    let mut shell = vec![0; 278];
    shell[25] = 1;
    shell[27] = 1;
    shell[28..32].copy_from_slice(&1_778u32.to_le_bytes());
    shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shell[55] = 1;
    shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shell);
    let mut shell_thickness = vec![0; 105];
    shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    shell_thickness[4..7].copy_from_slice(b"321");
    shell_thickness[7..11].copy_from_slice(&1_778u32.to_le_bytes());
    shell_thickness[24] = 1;
    shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    shell_thickness[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    shell_thickness.extend_from_slice(b"265");
    shell_thickness.extend_from_slice(&1_778u32.to_le_bytes());
    bytes.extend_from_slice(&shell_thickness);
    let mut shell_scope = scope.clone();
    shell_scope.byte_offset = shell_at as u64;
    shell_scope.kind = "Shell".into();
    shell_scope.frame_length = 278;
    shell_scope.reference_members = vec![200, 201, 1_778];
    shell_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &shell_scope);
    assert!(matches!(
        shell_scope.direct_face_operation,
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.5,
            thickness_record_index: 1_778,
            outward: true,
            ..
        })
    ));
    let mut shell_group = thicken_group.clone();
    shell_group.id = "shell-group".into();
    shell_group.scope_record_index = shell_scope.record_index;
    shell_group.role = 0x0000_0010_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_shell(&shell_scope, &[], std::slice::from_ref(&shell_group)),
        Some(cadmpeg_ir::features::FeatureDefinition::Shell {
            removed_faces: cadmpeg_ir::features::FaceSelection::Native(native),
            thickness: Some(cadmpeg_ir::features::Length(5.0)),
            outward: Some(true),
            ..
        }) if native == "shell-group"
    ));
    let compact_shell_at = bytes.len();
    let mut compact_shell = vec![0; 268];
    compact_shell[21] = 1;
    compact_shell[22] = 1;
    compact_shell[23..27].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell[42..46].copy_from_slice(&1u32.to_le_bytes());
    compact_shell[46] = 1;
    compact_shell[47..51].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell);
    let mut compact_shell_thickness = vec![0; 103];
    compact_shell_thickness[0..4].copy_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness[4..7].copy_from_slice(b"354");
    compact_shell_thickness[7..11].copy_from_slice(&9_000u32.to_le_bytes());
    compact_shell_thickness[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    compact_shell_thickness[24] = 1;
    compact_shell_thickness[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[40..48].copy_from_slice(&0.25f64.to_le_bytes());
    compact_shell_thickness[48] = 1;
    compact_shell_thickness[49..53].copy_from_slice(&9_001u32.to_le_bytes());
    compact_shell_thickness[59..63].copy_from_slice(&10u32.to_le_bytes());
    compact_shell_thickness[67] = 1;
    compact_shell_thickness[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness[80] = 1;
    compact_shell_thickness[81..85].copy_from_slice(&9_002u32.to_le_bytes());
    compact_shell_thickness[92] = 1;
    compact_shell_thickness[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    compact_shell_thickness.extend_from_slice(&3u32.to_le_bytes());
    compact_shell_thickness.extend_from_slice(b"258");
    compact_shell_thickness.extend_from_slice(&9_000u32.to_le_bytes());
    bytes.extend_from_slice(&compact_shell_thickness);
    let mut compact_shell_scope = DesignParameterScope {
        byte_offset: compact_shell_at as u64,
        frame_length: 268,
        reference_members: vec![200, 201, 9_000],
        ..shell_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &compact_shell_scope),
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.25,
            thickness_record_index: 9_000,
            outward: true,
            outward_offset,
            ..
        }) if outward_offset == (compact_shell_at + 21) as u64
    ));
    let shifted_shell_at = bytes.len();
    let mut shifted_shell = vec![0; 278];
    shifted_shell[20] = 1;
    shifted_shell[25] = 1;
    shifted_shell[27] = 1;
    shifted_shell[28..32].copy_from_slice(&9_000u32.to_le_bytes());
    shifted_shell[51..55].copy_from_slice(&1u32.to_le_bytes());
    shifted_shell[55] = 1;
    shifted_shell[56..60].copy_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&shifted_shell);
    let shifted_shell_scope = DesignParameterScope {
        byte_offset: shifted_shell_at as u64,
        frame_length: 278,
        reference_members: vec![9_000, 200, 201],
        ..shell_scope.clone()
    };
    assert!(matches!(
        exact_direct_face_operation(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &shifted_shell_scope,
        ),
        Some(DesignDirectFaceOperation::Shell {
            thickness: 0.25,
            thickness_record_index: 9_000,
            outward: false,
            outward_offset,
            ..
        }) if outward_offset == (shifted_shell_at + 21) as u64
    ));
    compact_shell_scope.direct_face_operation = exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_shell_scope,
    );
    shell_group.role = 0x0000_0004_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_shell(
            &compact_shell_scope,
            &[],
            std::slice::from_ref(&shell_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Shell {
            bodies: Some(cadmpeg_ir::features::BodySelection::Native(body)),
            removed_faces: cadmpeg_ir::features::FaceSelection::Faces(removed),
            thickness: Some(cadmpeg_ir::features::Length(2.5)),
            outward: Some(true),
            ..
        }) if body == "shell-group" && removed.is_empty()
    ));
    offset_scope.direct_face_operation =
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &offset_scope);
    let mut offset_group = thicken_group.clone();
    offset_group.id = "offset-group".into();
    offset_group.scope_record_index = offset_scope.record_index;
    offset_group.role = 0x0000_0010_0000_0000;
    assert!(matches!(
        crate::design::feature_project::project_offset_faces(
            &offset_scope,
            &[],
            &[],
            std::slice::from_ref(&offset_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::MoveFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(native),
            motion: cadmpeg_ir::features::FaceMotion::Offset {
                distance: cadmpeg_ir::features::Length(2.54)
            },
        }) if native == "offset-group"
    ));
    bytes[compact_thicken_at + 46] = 0;
    assert_eq!(
        exact_direct_face_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &thicken_scope),
        None
    );

    for (record_index, ordinal, value) in [(75u32, 0u8, -2.0f64), (76, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut extrude_scope = scope.clone();
    extrude_scope.kind = "Extrude".into();
    extrude_scope.extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: 28,
        direction_face_extend_values: [1, 2],
        side_extent_discriminators: [1, 0],
        side_extent_discriminator_offsets: [77, 90],
        first_side_target_ordinal: None,
        extent: DesignExtrudeExtent::OneSidedDistance,
        direction_face_extend_offsets: [32, 36],
        direction_reversed: false,
        direction_reversed_offset: 40,
        solid_operation: true,
        solid_operation_offset: 41,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 42,
    });
    extrude_scope.reference_members = vec![50, 75, 76, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: -2.0,
                    record_index: 75,
                    value_offset: (bytes.len() - 2 * 115 + 40) as u64,
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: 0.0,
                record_index: 76,
                value_offset: (bytes.len() - 115 + 40) as u64,
            }),
        })
    );
    extrude_scope.reference_members = vec![50, 75, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::FixedScalar(
                DesignFixedExtrudeScalar {
                    value: -2.0,
                    record_index: 75,
                    value_offset: (bytes.len() - 2 * 115 + 40) as u64,
                },
            )),
            taper_angle: None,
        })
    );
    extrude_scope.reference_members = vec![50, 75, 76, 51];
    extrude_scope.reference_members.push(75);
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        None
    );

    let extend_distance_at = bytes.len();
    let extend_distance_record_index = 400u32;
    let extend_boundary_record_index = 500u32;
    let extend_edge_record_indices = [503u32, 507u32];
    let mut extend_distance = vec![0; 104];
    extend_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_distance[4..7].copy_from_slice(b"299");
    extend_distance[7..11].copy_from_slice(&extend_distance_record_index.to_le_bytes());
    extend_distance[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    extend_distance[24] = 1;
    extend_distance[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[35] = 0;
    extend_distance[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    extend_distance[48] = 1;
    extend_distance[49..53].copy_from_slice(&(extend_distance_record_index - 1).to_le_bytes());
    extend_distance[59..63].copy_from_slice(&1016u32.to_le_bytes());
    extend_distance[67] = 1;
    extend_distance[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance[78..81].copy_from_slice(&[1, 0, 0]);
    extend_distance[81] = 1;
    extend_distance[82..86].copy_from_slice(&(extend_distance_record_index + 1).to_le_bytes());
    extend_distance[93] = 1;
    extend_distance[94..98].copy_from_slice(&scope.record_index.to_le_bytes());
    extend_distance.extend_from_slice(&3u32.to_le_bytes());
    extend_distance.extend_from_slice(b"258");
    extend_distance.extend_from_slice(&extend_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_distance);

    let extend_boundary_at = bytes.len();
    let extend_boundary_tail = 25 + extend_edge_record_indices.len() * 11;
    let mut extend_boundary = vec![0; 113 + extend_edge_record_indices.len() * 11];
    extend_boundary[0..4].copy_from_slice(&3u32.to_le_bytes());
    extend_boundary[4..7].copy_from_slice(b"290");
    extend_boundary[7..11].copy_from_slice(&extend_boundary_record_index.to_le_bytes());
    extend_boundary[21..25]
        .copy_from_slice(&(extend_edge_record_indices.len() as u32).to_le_bytes());
    for (ordinal, record_index) in extend_edge_record_indices.iter().enumerate() {
        let at = 25 + ordinal * 11;
        extend_boundary[at] = 1;
        extend_boundary[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
    }
    extend_boundary[extend_boundary_tail + 2..extend_boundary_tail + 6]
        .copy_from_slice(&1u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 6] = 1;
    extend_boundary[extend_boundary_tail + 7..extend_boundary_tail + 11]
        .copy_from_slice(&900u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 21..extend_boundary_tail + 25]
        .copy_from_slice(&8u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 35..extend_boundary_tail + 39]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 39..extend_boundary_tail + 47]
        .copy_from_slice(&1.0e-6f64.to_le_bytes());
    extend_boundary[extend_boundary_tail + 47..extend_boundary_tail + 51]
        .copy_from_slice(&210u32.to_le_bytes());
    extend_boundary[extend_boundary_tail + 51] = 1;
    extend_boundary[extend_boundary_tail + 52..extend_boundary_tail + 56]
        .copy_from_slice(&(extend_boundary_record_index + 2).to_le_bytes());
    extend_boundary[extend_boundary_tail + 62..extend_boundary_tail + 65]
        .copy_from_slice(&[1, 0, 0]);
    extend_boundary[extend_boundary_tail + 65] = 1;
    extend_boundary[extend_boundary_tail + 66..extend_boundary_tail + 70]
        .copy_from_slice(&(extend_boundary_record_index + 1).to_le_bytes());
    extend_boundary[extend_boundary_tail + 77] = 1;
    extend_boundary[extend_boundary_tail + 78..extend_boundary_tail + 82]
        .copy_from_slice(&scope.record_index.to_le_bytes());
    extend_boundary.extend_from_slice(&3u32.to_le_bytes());
    extend_boundary.extend_from_slice(b"258");
    extend_boundary.extend_from_slice(&extend_boundary_record_index.to_le_bytes());
    bytes.extend_from_slice(&extend_boundary);

    let mut extend_scope = scope.clone();
    extend_scope.id = "f3d:native:parameter-scope#12".into();
    extend_scope.kind = "SurfaceExtend".into();
    extend_scope.reference_members = vec![
        extend_distance_record_index,
        extend_boundary_record_index,
        extend_edge_record_indices[0],
        extend_edge_record_indices[1],
    ];
    let operation =
        exact_surface_extend_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope)
            .expect("exact SurfaceExtend construction");
    assert_eq!(
        operation,
        DesignSurfaceExtendOperation {
            distance: 0.04,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            method: DesignSurfaceExtendMethod::Tangent,
            method_offset: (extend_boundary_at + extend_boundary_tail + 2) as u64,
            boundary_record_index: extend_boundary_record_index,
            boundary_reference_record_index: 900,
            boundary_reference_offset: (extend_boundary_at + extend_boundary_tail + 6) as u64,
            edge_record_indices: extend_edge_record_indices.to_vec(),
            tolerance: 1.0e-6,
            tolerance_offset: (extend_boundary_at + extend_boundary_tail + 39) as u64,
        }
    );
    extend_scope.surface_extend_operation = Some(operation);
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&extend_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features.as_slice(),
        [Feature {
            definition: FeatureDefinition::ExtendSurface {
                faces: FaceSelection::Native(native),
                distance: Some(Length(distance)),
                method: cadmpeg_ir::features::SurfaceExtension::Linear,
            },
            ..
        }] if native.ends_with(":design-record#500") && *distance == 0.4
    ));

    bytes[extend_distance_at + 40..extend_distance_at + 48]
        .copy_from_slice(&(-0.4f64).to_le_bytes());
    bytes[extend_boundary_at + extend_boundary_tail + 21
        ..extend_boundary_at + extend_boundary_tail + 25]
        .copy_from_slice(&65u32.to_le_bytes());
    extend_scope.kind = "SurfaceOffset".into();
    extend_scope.surface_extend_operation = None;
    let operation =
        exact_surface_offset_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope)
            .expect("exact SurfaceOffset construction");
    assert_eq!(
        operation,
        DesignSurfaceOffsetOperation {
            distance: -0.4,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::BoundaryCarrier {
                boundary_mode: 1,
                boundary_mode_offset: (extend_boundary_at + extend_boundary_tail + 2) as u64,
                boundary_record_index: extend_boundary_record_index,
                boundary_reference_record_index: 900,
                boundary_reference_offset: (extend_boundary_at + extend_boundary_tail + 6) as u64,
                edge_record_indices: extend_edge_record_indices.to_vec(),
                tolerance: 1.0e-6,
                tolerance_offset: (extend_boundary_at + extend_boundary_tail + 39) as u64,
            },
        }
    );
    extend_scope.surface_offset_operation = Some(operation);
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&extend_scope),
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features.as_slice(),
        [Feature {
            definition: FeatureDefinition::OffsetSurface {
                faces: FaceSelection::Native(native),
                distance: Some(Length(distance)),
            },
            ..
        }] if native.ends_with(":design-record#500") && *distance == -4.0
    ));

    let grouped_record_index = 600u32;
    let grouped_member_record_index = 601u32;
    let mut grouped = Vec::new();
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"282");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&grouped_member_record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[0; 2]);
    grouped.extend_from_slice(&1u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&0x0000_0041_0000_0000u64.to_le_bytes());
    grouped.extend_from_slice(&[0; 10]);
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.extend_from_slice(&0.0001f64.to_le_bytes());
    grouped.extend_from_slice(&252u32.to_le_bytes());
    grouped.push(1);
    grouped.extend_from_slice(&(grouped_record_index + 2).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&[1, 1, 0, 1]);
    grouped.extend_from_slice(&(grouped_record_index + 1).to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.push(0);
    grouped.push(1);
    grouped.extend_from_slice(&extend_scope.record_index.to_le_bytes());
    grouped.extend_from_slice(&[0; 6]);
    grouped.extend_from_slice(&3u32.to_le_bytes());
    grouped.extend_from_slice(b"260");
    grouped.extend_from_slice(&grouped_record_index.to_le_bytes());
    bytes.extend_from_slice(&grouped);
    let mut grouped_scope = extend_scope.clone();
    grouped_scope.reference_members = vec![
        extend_distance_record_index,
        grouped_record_index,
        grouped_member_record_index,
    ];
    let grouped_operation = exact_surface_offset_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &grouped_scope,
    )
    .expect("exact grouped SurfaceOffset construction");
    assert_eq!(
        grouped_operation,
        DesignSurfaceOffsetOperation {
            distance: -0.4,
            distance_offset: (extend_distance_at + 40) as u64,
            distance_record_index: extend_distance_record_index,
            support: DesignSurfaceOffsetSupport::FaceGroups {
                group_record_indices: vec![grouped_record_index],
            },
        }
    );

    bytes[extend_boundary_at + 21..extend_boundary_at + 25]
        .copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        exact_surface_offset_operation(&bytes, &IndexedRecordOffsets::build(&bytes), &extend_scope,),
        None
    );

    let embedded_default_at = bytes.len();
    for (record_index, ordinal) in [(273u32, 0u8), (274, 1)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let embedded_distance_at = bytes.len();
    let embedded_distance_record_index = 275u32;
    let mut embedded_distance = vec![0; 100];
    embedded_distance[0..4].copy_from_slice(&3u32.to_le_bytes());
    embedded_distance[4..7].copy_from_slice(b"314");
    embedded_distance[7..11].copy_from_slice(&embedded_distance_record_index.to_le_bytes());
    embedded_distance[21] = 1;
    embedded_distance[22..26].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance[32..36].copy_from_slice(&1u32.to_le_bytes());
    embedded_distance[36] = 1;
    embedded_distance[37..41].copy_from_slice(&999u32.to_le_bytes());
    embedded_distance[47..51].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[51..59].copy_from_slice(&0.25f64.to_le_bytes());
    embedded_distance[59..63].copy_from_slice(&210u32.to_le_bytes());
    embedded_distance[63] = 1;
    embedded_distance[64..68].copy_from_slice(&(embedded_distance_record_index + 2).to_le_bytes());
    embedded_distance[74] = 1;
    embedded_distance[77] = 1;
    embedded_distance[78..82].copy_from_slice(&(embedded_distance_record_index + 1).to_le_bytes());
    embedded_distance[89] = 1;
    embedded_distance[90..94].copy_from_slice(&scope.record_index.to_le_bytes());
    embedded_distance.extend_from_slice(&3u32.to_le_bytes());
    embedded_distance.extend_from_slice(b"258");
    embedded_distance.extend_from_slice(&embedded_distance_record_index.to_le_bytes());
    bytes.extend_from_slice(&embedded_distance);
    extrude_scope.reference_members = vec![50, 273, 274, embedded_distance_record_index, 51];
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        Some(DesignFixedExtrudeParameters {
            along_distance: Some(DesignFixedExtrudeDistance::DistanceConstruction(
                DesignFixedExtrudeScalar {
                    value: 0.25,
                    record_index: embedded_distance_record_index,
                    value_offset: (embedded_distance_at + 51) as u64,
                },
            )),
            taper_angle: Some(DesignFixedExtrudeScalar {
                value: 0.0,
                record_index: 274,
                value_offset: (embedded_default_at + 115 + 40) as u64,
            }),
        })
    );
    extrude_scope.reference_members.insert(2, 273);
    assert_eq!(
        exact_fixed_extrude_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &extrude_scope
        ),
        None
    );

    let draft_start = bytes.len();
    for (record_index, ordinal, value) in [(175u32, 0u8, 0.4f64), (176, 1, 0.0)] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut draft_scope = scope.clone();
    draft_scope.kind = "Draft".into();
    draft_scope.frame_length = 361;
    draft_scope.reference_members = vec![175, 176, 181, 182, 186, 190, 193];
    let expected = Some(DesignDraftOperation {
        angle: 0.4,
        angle_record_index: 175,
        angle_offset: (draft_start + 40) as u64,
        opposite_angle_record_index: 176,
        opposite_angle_offset: (draft_start + 155) as u64,
    });
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // The ordered reference table is in record-index order, so the scalar lanes
    // hold no fixed position in it. Their local ordinals order them, and moving
    // them within the table must not change the recovered operation.
    draft_scope.reference_members = vec![181, 182, 186, 190, 193, 175, 176];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        expected
    );

    // A table that reaches only one of the two lanes has no complete operation.
    draft_scope.reference_members = vec![175, 181, 182, 186, 190, 193];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );

    draft_scope.reference_members = vec![175, 176, 181, 182, 186];
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );
    draft_scope.reference_members = vec![175, 176, 181, 182, 186, 190, 193];

    let fillet_start = bytes.len();
    for (record_index, ordinal, value) in [
        (77u32, 0u8, 1.0f64),
        (78, 1, 0.0),
        (79, 2, 0.65),
        (87, 3, 0.4),
        (88, 4, 0.2),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut fillet_scope = scope.clone();
    fillet_scope.kind = "Fillet".into();
    fillet_scope.reference_members = vec![77, 50, 78, 79, 87, 88];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: Some(crate::records::DesignFixedFilletTangencyWeight {
                    value: 1.0,
                    record_index: 77,
                    value_offset: (fillet_start + 40) as u64,
                }),
                radii: vec![0.0, 0.65, 0.4],
                radius_record_indexes: vec![78, 79, 87],
                radius_offsets: vec![
                    (fillet_start + 115 + 40) as u64,
                    (fillet_start + 230 + 40) as u64,
                    (fillet_start + 345 + 40) as u64,
                ],
                intermediate_parameters: vec![0.2],
                intermediate_parameter_record_indexes: vec![88],
                intermediate_parameter_offsets: vec![(fillet_start + 460 + 40) as u64],
            }],
        })
    );
    fillet_scope.reference_members = vec![50, 77];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: None,
                radii: vec![1.0],
                radius_record_indexes: vec![77],
                radius_offsets: vec![(fillet_start + 40) as u64],
                intermediate_parameters: Vec::new(),
                intermediate_parameter_record_indexes: Vec::new(),
                intermediate_parameter_offsets: Vec::new(),
            }],
        })
    );

    let dynamic_scalar_at = bytes.len();
    let mut dynamic_scalar = vec![0; 103];
    dynamic_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    dynamic_scalar[4..7].copy_from_slice(b"406");
    dynamic_scalar[7..11].copy_from_slice(&89u32.to_le_bytes());
    dynamic_scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    dynamic_scalar[24] = 1;
    dynamic_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[40..48].copy_from_slice(&0.5f64.to_le_bytes());
    dynamic_scalar[48] = 1;
    dynamic_scalar[49..53].copy_from_slice(&90u32.to_le_bytes());
    dynamic_scalar[67] = 1;
    dynamic_scalar[68..72].copy_from_slice(&scope.record_index.to_le_bytes());
    dynamic_scalar[80] = 1;
    dynamic_scalar[81..85].copy_from_slice(&91u32.to_le_bytes());
    dynamic_scalar[92] = 1;
    dynamic_scalar[93..97].copy_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&dynamic_scalar);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&89u32.to_le_bytes());
    fillet_scope.reference_members = vec![89];
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::DesignFixedFilletGroup {
                tangency_weight: None,
                radii: vec![0.5],
                radius_record_indexes: vec![89],
                radius_offsets: vec![(dynamic_scalar_at + 40) as u64],
                intermediate_parameters: Vec::new(),
                intermediate_parameter_record_indexes: Vec::new(),
                intermediate_parameter_offsets: Vec::new(),
            }],
        })
    );

    let second_group_at = bytes.len();
    for (record_index, ordinal, value) in [
        (92u32, 0u8, 1.0f64),
        (93, 1, 0.5),
        (94, 2, 0.75),
        (95, 3, 0.25),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"406");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"259");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    fillet_scope.reference_members = vec![92, 93, 94, 95];
    let fixed =
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope)
            .expect("two constant-radius Fillet scalar groups");
    assert_eq!(fixed.groups.len(), 2);
    assert_eq!(fixed.groups[0].radii, [0.5]);
    assert_eq!(fixed.groups[1].radii, [0.25]);
    assert_eq!(
        fixed.groups[1]
            .tangency_weight
            .as_ref()
            .map(|weight| (weight.value, weight.value_offset)),
        Some((0.75, (second_group_at + 2 * 115 + 40) as u64))
    );

    let chamfer_scalar_start = bytes.len();
    let mut chamfer_scalar = vec![0; 104];
    chamfer_scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
    chamfer_scalar[4..7].copy_from_slice(b"277");
    chamfer_scalar[7..11].copy_from_slice(&86u32.to_le_bytes());
    chamfer_scalar[24] = 1;
    chamfer_scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
    chamfer_scalar[35] = 0;
    chamfer_scalar[40..48].copy_from_slice(&0.04f64.to_le_bytes());
    chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    chamfer_scalar.extend_from_slice(b"261");
    chamfer_scalar.extend_from_slice(&86u32.to_le_bytes());
    bytes.extend_from_slice(&chamfer_scalar);
    let mut chamfer_scope = scope.clone();
    chamfer_scope.kind = "Chamfer".into();
    chamfer_scope.reference_members = vec![86];
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::EqualDistance {
            distance: crate::records::DesignFixedChamferDistance {
                value: 0.04,
                record_index: 86,
                value_offset: (chamfer_scalar_start + 40) as u64,
            },
        })
    );
    let second_chamfer_scalar_start = bytes.len();
    let mut second_chamfer_scalar = chamfer_scalar[..104].to_vec();
    second_chamfer_scalar[7..11].copy_from_slice(&96u32.to_le_bytes());
    second_chamfer_scalar[35] = 1;
    second_chamfer_scalar[40..48].copy_from_slice(&0.08f64.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(&3u32.to_le_bytes());
    second_chamfer_scalar.extend_from_slice(b"261");
    second_chamfer_scalar.extend_from_slice(&96u32.to_le_bytes());
    bytes.extend_from_slice(&second_chamfer_scalar);
    chamfer_scope.reference_members = vec![86, 96];
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::TwoDistances {
            first: crate::records::DesignFixedChamferDistance {
                value: 0.04,
                record_index: 86,
                value_offset: (chamfer_scalar_start + 40) as u64,
            },
            second: crate::records::DesignFixedChamferDistance {
                value: 0.08,
                record_index: 96,
                value_offset: (second_chamfer_scalar_start + 40) as u64,
            },
        })
    );
    chamfer_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    let indexed_owner = DesignParameterOwner {
        id: "f3d:Design/BulkStream.dat:parameter-owner#97".into(),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "292".into(),
        record_index: 97,
        scope_record_index: chamfer_scope.record_index,
        local_ordinal: 0,
        evaluated_value: 0.04,
        evaluated_value_offset: 0,
        parameter_record_index: 98,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 99,
    };
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            std::slice::from_ref(&indexed_owner),
        ),
        None
    );

    let revolve_start = bytes.len();
    let mut revolve = vec![0; 386];
    revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&revolve);
    let revolve_scalar_start = bytes.len();
    for (record_index, ordinal, value) in [(1_779u32, 0u8, 3.5f64), (1_780, 1, 0.0)] {
        let mut scalar = vec![0; 105];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"321");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"265");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut revolve_scope = scope.clone();
    revolve_scope.byte_offset = revolve_start as u64;
    revolve_scope.kind = "Revolve".into();
    revolve_scope.frame_length = 386;
    revolve_scope.reference_members = vec![200, 201, 202, 203, 1_779, 1_780, 204];
    let revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &revolve_scope,
        &[],
    );
    assert_eq!(
        revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (revolve_start + 25) as u64,
            angle: 3.5,
            angle_record_index: 1_779,
            angle_offset: (revolve_scalar_start + 40) as u64,
            opposite_angle_record_index: Some(1_780),
            opposite_angle_offset: Some((revolve_scalar_start + 116 + 40) as u64),
        })
    );

    let indexed_revolve_start = bytes.len();
    let indexed_angle_record_index = 1_790u32;
    let mut indexed_revolve = vec![0; 377];
    indexed_revolve[21..25].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[25..29].copy_from_slice(&2u32.to_le_bytes());
    indexed_revolve[30..34].copy_from_slice(&1u32.to_le_bytes());
    indexed_revolve[34] = 1;
    indexed_revolve[35..43].copy_from_slice(&u64::from(indexed_angle_record_index).to_le_bytes());
    bytes.extend_from_slice(&indexed_revolve);
    let mut indexed_revolve_scope = revolve_scope.clone();
    indexed_revolve_scope.byte_offset = indexed_revolve_start as u64;
    indexed_revolve_scope.class_tag = "407".into();
    indexed_revolve_scope.paired_class_tag = "258".into();
    indexed_revolve_scope.frame_length = 377;
    indexed_revolve_scope.reference_members = vec![200, 201, 202, 203, 204, 205, 1_790, 1_791];
    let indexed_angle = DesignParameterOwner {
        id: indexed_revolve_scope.id.clone(),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "372".into(),
        record_index: indexed_angle_record_index,
        scope_record_index: indexed_revolve_scope.record_index,
        local_ordinal: 0,
        evaluated_value: std::f64::consts::TAU,
        evaluated_value_offset: 45,
        parameter_record_index: 1_792,
        owned_ordinal: 8,
        variant: None,
        companion_record_index: 1_793,
    };
    let indexed_revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &indexed_revolve_scope,
        std::slice::from_ref(&indexed_angle),
    );
    assert_eq!(
        indexed_revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::Cut,
            operation_offset: (indexed_revolve_start + 21) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: indexed_angle_record_index,
            angle_offset: 45,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    let legacy_revolve_start = bytes.len();
    let legacy_angle_record_index = 1_800u32;
    let mut legacy_revolve = vec![0; 359];
    legacy_revolve[20] = 1;
    legacy_revolve[25..29].copy_from_slice(&4u32.to_le_bytes());
    legacy_revolve[29..33].copy_from_slice(&2u32.to_le_bytes());
    legacy_revolve[34..38].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&legacy_revolve);
    let mut legacy_revolve_scope = revolve_scope.clone();
    legacy_revolve_scope.byte_offset = legacy_revolve_start as u64;
    legacy_revolve_scope.class_tag = "409".into();
    legacy_revolve_scope.paired_class_tag = "257".into();
    legacy_revolve_scope.frame_length = 359;
    legacy_revolve_scope.reference_members =
        vec![200, 201, 202, 203, legacy_angle_record_index, 204];
    let legacy_angle = DesignParameterOwner {
        id: legacy_revolve_scope.id.clone(),
        byte_offset: 0,
        frame_length: 104,
        class_tag: "372".into(),
        record_index: legacy_angle_record_index,
        scope_record_index: legacy_revolve_scope.record_index,
        local_ordinal: 0,
        evaluated_value: std::f64::consts::TAU,
        evaluated_value_offset: 55,
        parameter_record_index: 1_801,
        owned_ordinal: 8,
        variant: None,
        companion_record_index: 1_802,
    };
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (legacy_revolve_start + 25) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: legacy_angle_record_index,
            angle_offset: 55,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    legacy_revolve_scope.class_tag = "323".into();
    legacy_revolve_scope.paired_class_tag = "260".into();
    legacy_revolve_scope.frame_length = 381;
    legacy_revolve_scope.reference_members =
        vec![legacy_angle_record_index, 200, 201, 202, 203, 204, 205, 206];
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (legacy_revolve_start + 25) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: legacy_angle_record_index,
            angle_offset: 55,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    legacy_revolve_scope.class_tag = "385".into();
    legacy_revolve_scope.paired_class_tag = "262".into();
    legacy_revolve_scope.frame_length = 369;
    legacy_revolve_scope.reference_members =
        vec![200, 201, 202, 203, legacy_angle_record_index, 204];
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (legacy_revolve_start + 25) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: legacy_angle_record_index,
            angle_offset: 55,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    revolve_scope.id = "stream:scope".into();
    revolve_scope.path_feature_construction = revolve_construction;
    let mut revolve_profile = thicken_group.clone();
    revolve_profile.id = "stream:profile".into();
    revolve_profile.scope_record_index = revolve_scope.record_index;
    revolve_profile.role = 0x0000_0041_0000_0000;
    let mut revolve_axis = revolve_profile.clone();
    revolve_axis.id = "stream:axis".into();
    revolve_axis.role = 0x0000_0021_0000_0000;
    assert_eq!(
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &revolve_scope,
            &[revolve_profile, revolve_axis],
            &[],
            &[],
            &[],
            &[],
            &[],
        ),
        None
    );

    indexed_revolve_scope.id = "stream:indexed-revolve".into();
    indexed_revolve_scope.path_feature_construction = indexed_revolve_construction;
    let mut indexed_profile = thicken_group.clone();
    indexed_profile.id = "stream:indexed-profile".into();
    indexed_profile.scope_record_index = indexed_revolve_scope.record_index;
    indexed_profile.role = 0x0000_0041_0000_0000;
    let mut indexed_axis = indexed_profile.clone();
    indexed_axis.id = "stream:indexed-axis".into();
    indexed_axis.record_index = 899;
    indexed_axis.members = vec![900];
    indexed_axis.role = 0x0000_0021_0000_0000;
    let mut indexed_bodies = indexed_profile.clone();
    indexed_bodies.id = "stream:indexed-bodies".into();
    indexed_bodies.record_index = 901;
    indexed_bodies.role = 0x0000_0004_0000_0000;
    let mut axis_selection = crate::records::DesignEntitySelectionOperand {
        id: "stream:indexed-axis-selection".into(),
        scope_record_index: indexed_revolve_scope.record_index,
        group_record_index: indexed_axis.record_index,
        group_member_ordinal: 0,
        record_index: 900,
        byte_offset: 0,
        class_tag: "377".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 902,
        identity_record_offset: 0,
        primary_identity: 100,
        primary_identity_offset: 0,
        secondary_identity: Some(104),
        secondary_identity_offset: Some(0),
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: 903,
        next_byte_offset: 0,
    };
    let axis_placement = DesignSketchPlacement {
        member_run_head: false,
        id: "stream:indexed-axis-placement".into(),
        scope_record_index: Some(10),
        entity_id: "Sketch_100".into(),
        entity_suffix: 100,
        visibility: None,
        byte_offset: 0,
        class_tag: "305".into(),
        record_index: 904,
        frame_length: 201,
        transform: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        transform_offset: Some(0),
        paired_class_tag: "258".into(),
        paired_byte_offset: 0,
    };
    let axis_curve = SketchCurveIdentity {
        id: "stream:indexed-axis-curve".into(),
        record_index: 905,
        owner_reference: Some(100),
        class_tag: "450".into(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: 104,
        secondary_id: 0,
        geometry: Some(SketchCurveGeometry::Line {
            start: Point3::new(1.0, 2.0, 3.0),
            end: Point3::new(1.0, -3.0, 3.0),
            direction: Vector3::new(0.0, -1.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
        }),
    };
    let projected = crate::design::feature_project::project_fixed_revolve_with_entities(
        &indexed_revolve_scope,
        &[
            indexed_profile.clone(),
            indexed_axis.clone(),
            indexed_bodies.clone(),
        ],
        &[],
        std::slice::from_ref(&axis_selection),
        &[],
        &[axis_placement],
        &[axis_curve],
    );
    assert!(matches!(
        projected,
        Some(FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(cadmpeg_ir::features::RevolutionAxis { origin, direction }),
                ..
            },
            op: cadmpeg_ir::features::BooleanOp::Cut,
        }) if origin == Point3::new(1.0, 2.0, 3.0)
            && direction == Vector3::new(0.0, -1.0, 0.0)
    ));
    axis_selection.secondary_identity = None;
    axis_selection.historical_face_candidates =
        vec![crate::records::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical_entity_kind: crate::records::AsmHistoricalEntityKind::Face,
            historical_entity_ref: 40,
            historical_state_ids: vec![1],
            face_slot: 40,
        }];
    let historical_definition =
        crate::design::feature_project::project_fixed_revolve_with_entities(
            &indexed_revolve_scope,
            &[
                indexed_profile.clone(),
                indexed_axis.clone(),
                indexed_bodies.clone(),
            ],
            &[],
            std::slice::from_ref(&axis_selection),
            &[],
            &[],
            &[],
        )
        .unwrap();
    assert!(matches!(
        historical_definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction { axis: None, .. },
            ..
        }
    ));
    let mut feature = cadmpeg_ir::features::Feature {
        id: crate::ids::neutral_feature_id(&indexed_revolve_scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        parent: None,
        dependencies: Vec::new(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Vec::new(),
        outputs: Vec::new(),
        definition: historical_definition,
        native_ref: Some(indexed_revolve_scope.id.clone()),
    };
    let surface_id = cadmpeg_ir::ids::SurfaceId("surface:53".into());
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile.clone(), indexed_axis.clone()],
        std::slice::from_ref(&axis_selection),
        &[],
        &[cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId("f3d:brep:entity#40".into()),
            shell: cadmpeg_ir::ids::ShellId("shell:1".into()),
            surface: surface_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        }],
        &[cadmpeg_ir::geometry::Surface {
            id: surface_id,
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: Point3::new(4.0, 5.0, 6.0),
                normal: Vector3::new(0.0, 0.0, -2.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        }],
    );
    assert!(matches!(
        feature.definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(cadmpeg_ir::features::RevolutionAxis { origin, direction }),
                ..
            },
            ..
        } if origin == Point3::new(4.0, 5.0, 6.0)
            && direction == Vector3::new(0.0, 0.0, -1.0)
    ));

    let face_axis_operand = DesignFaceOperand {
        id: "stream:indexed-face-axis".into(),
        scope_record_index: indexed_revolve_scope.record_index,
        scope_reference_ordinal: 2,
        group_record_index: Some(indexed_axis.record_index),
        group_member_ordinal: Some(0),
        record_index: 900,
        byte_offset: 0,
        class_tag: "256".into(),
        paired_byte_offset: 0,
        paired_class_tag: "262".into(),
        recipe_record_index: 903,
        recipe_record_byte_offset: 0,
        recipe_id: "stream:indexed-face-axis-recipe".into(),
        recipe_prefix_offset: 0,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 0,
        recipe_program: vec![0, -1],
        recipe_node_offsets: Vec::new(),
        recipe_nodes: Vec::new(),
        candidate_faces: vec![
            cadmpeg_ir::ids::FaceId("face:axis-a".into()),
            cadmpeg_ir::ids::FaceId("face:axis-b".into()),
        ],
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        next_record_index: 905,
        next_byte_offset: 0,
    };
    let face_axis_definition = crate::design::feature_project::project_fixed_revolve_with_entities(
        &indexed_revolve_scope,
        &[
            indexed_profile.clone(),
            indexed_axis.clone(),
            indexed_bodies,
        ],
        &[],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &[],
        &[],
    )
    .expect("face-recipe axis retains a neutral Revolve before geometry binding");
    let mut face_axis_feature = cadmpeg_ir::features::Feature {
        definition: face_axis_definition.clone(),
        ..feature.clone()
    };
    let axis_faces = [
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId("face:axis-a".into()),
            shell: cadmpeg_ir::ids::ShellId("shell:axis".into()),
            surface: cadmpeg_ir::ids::SurfaceId("surface:axis-a".into()),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId("face:axis-b".into()),
            shell: cadmpeg_ir::ids::ShellId("shell:axis".into()),
            surface: cadmpeg_ir::ids::SurfaceId("surface:axis-b".into()),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new(),
            name: None,
            color: None,
            tolerance: None,
        },
    ];
    let mut axis_surfaces = [
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:axis-a".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius: 4.0,
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("surface:axis-b".into()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin: Point3::new(1.0, 2.0, 8.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                radius: 5.0,
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
    ];
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut face_axis_feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile.clone(), indexed_axis.clone()],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &axis_faces,
        &axis_surfaces,
    );
    assert!(matches!(
        face_axis_feature.definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction {
                axis: Some(cadmpeg_ir::features::RevolutionAxis { origin, direction }),
                ..
            },
            ..
        } if origin == Point3::new(1.0, 2.0, 3.0)
            && direction == Vector3::new(0.0, 0.0, 1.0)
    ));
    let mut conflicting_face_axis_feature = cadmpeg_ir::features::Feature {
        definition: face_axis_definition,
        ..feature
    };
    axis_surfaces[1].geometry = cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
        origin: Point3::new(2.0, 2.0, 8.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        radius: 5.0,
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
    };
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut conflicting_face_axis_feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile, indexed_axis],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &axis_faces,
        &axis_surfaces,
    );
    assert!(matches!(
        conflicting_face_axis_feature.definition,
        FeatureDefinition::Revolve {
            construction: cadmpeg_ir::features::RevolutionConstruction { axis: None, .. },
            ..
        }
    ));

    let loft_start = bytes.len();
    let mut loft = vec![0; 376];
    loft[29..33].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&loft);
    let mut loft_scope = scope.clone();
    loft_scope.byte_offset = loft_start as u64;
    loft_scope.kind = "Loft".into();
    loft_scope.frame_length = 376;
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &loft_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Loft {
            operation: DesignExtrudeOperation::Join,
            operation_offset: (loft_start + 29) as u64,
        })
    );
    loft_scope.id = "stream:loft-scope".into();
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (loft_start + 29) as u64,
    });
    let loft_group = |ordinal: u32, role: u64| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:loft-group-{ordinal}");
        group.scope_record_index = loft_scope.record_index;
        group.scope_reference_ordinal = ordinal;
        group.role = role;
        group
    };
    let role_41 = [loft_group(0, 0x41_0000_0000), loft_group(1, 0x41_0000_0000)];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &role_41, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 2 && guides.is_empty()
    ));
    let guided_role_41 = [
        loft_group(0, 0x41_0000_0000),
        loft_group(1, 0x41_0000_0000),
        loft_group(2, 0x41_0000_0000),
        loft_group(3, 0x5_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(
            &loft_scope,
            &guided_role_41,
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 3 && guides.len() == 1
    ));
    let role_shape = |groups: &[DesignConstructionOperandGroup]| {
        groups
            .iter()
            .map(|group| (group.role, group.members.len()))
            .collect::<Vec<_>>()
    };
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&guided_role_41),
    ));
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::Cut,
        operation_offset: (loft_start + 29) as u64,
    });
    let cut = [
        loft_group(0, 0x4_0000_0000),
        loft_group(1, 0x41_0000_0000),
        loft_group(2, 0x43_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &cut, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            op: cadmpeg_ir::features::BooleanOp::Cut,
            ..
        }) if sections.len() == 2
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::Cut,
        &role_shape(&cut),
    ));
    loft_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Loft {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (loft_start + 29) as u64,
    });
    let role_5 = [
        loft_group(0, 0x5_0000_0000),
        loft_group(1, 0x5_0000_0000),
        loft_group(2, 0x5_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &role_5, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft { sections, guides, .. })
            if sections.len() == 3 && guides.is_empty()
    ));
    let centered = [
        loft_group(0, 0x43_0000_0000),
        loft_group(1, 0x43_0000_0000),
        loft_group(2, 0x7_0000_0000),
    ];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &centered, &[], &[], &[]),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            guides,
            centerline: Some(cadmpeg_ir::features::PathRef::Native(centerline)),
            ..
        }) if sections.len() == 2 && guides.is_empty() && centerline == "stream:loft-group-2"
    ));
    let mixed = [
        loft_group(0, 0x43_0000_0000),
        loft_group(1, 0x43_0000_0000),
        loft_group(2, 0x5_0000_0000),
        loft_group(3, 0x7_0000_0000),
    ];
    assert_eq!(
        crate::design::feature_project::project_fixed_loft(&loft_scope, &mixed, &[], &[], &[]),
        None
    );
    assert!(!crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&mixed),
    ));
    let mut point = loft_group(0, 0x5_0000_0000);
    point.members = vec![10];
    let profile = loft_group(1, 0x43_0000_0000);
    let mut boundary = loft_group(2, 0x5_0000_0000);
    boundary.members = vec![20, 21, 22];
    assert!(matches!(
        crate::design::feature_project::project_fixed_loft(
            &loft_scope,
            &[point.clone(), profile.clone(), boundary.clone()],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Loft {
            sections,
            guides,
            centerline: None,
            ..
        }) if matches!(sections.as_slice(), [
            cadmpeg_ir::features::LoftSection::Point(
                cadmpeg_ir::features::LoftPointSection::Native(_)
            ),
            cadmpeg_ir::features::LoftSection::Profile(_),
            cadmpeg_ir::features::LoftSection::Profile(_),
        ]) && guides.is_empty()
    ));
    assert!(crate::validate::loft_operand_roles_are_valid(
        DesignExtrudeOperation::NewBody,
        &role_shape(&[point, profile, boundary]),
    ));

    let sweep_start = bytes.len();
    let mut sweep = vec![0; 499];
    sweep[25..29].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&sweep);
    let sweep_values: [f64; 6] = [0.8, 0.0, 1.0, 1.0, 6.632_251_157_578_453, 0.0];
    let sweep_scalar_start = bytes.len();
    for (ordinal, value) in sweep_values.into_iter().enumerate() {
        let record_index = 80 + ordinal as u32;
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal as u8;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut sweep_scope = scope.clone();
    sweep_scope.byte_offset = sweep_start as u64;
    sweep_scope.kind = "Sweep".into();
    sweep_scope.frame_length = 499;
    sweep_scope.reference_members = (80..86).collect();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &sweep_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Sweep {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (sweep_start + 25) as u64,
            values: sweep_values,
            record_indexes: [80, 81, 82, 83, 84, 85],
            value_offsets: std::array::from_fn(|ordinal| {
                (sweep_scalar_start + ordinal * 111 + 40) as u64
            }),
        })
    );
    sweep_scope.id = "stream:sweep-scope".into();
    sweep_scope.path_feature_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &sweep_scope,
        &[],
    );
    let sweep_group = |ordinal: u32, role: u64| {
        let mut group = thicken_group.clone();
        group.id = format!("stream:sweep-group-{ordinal}");
        group.scope_record_index = sweep_scope.record_index;
        group.scope_reference_ordinal = ordinal;
        group.role = role;
        group
    };
    let profile = sweep_group(0, 0x41_0000_0000);
    let path = sweep_group(1, 0x5_0000_0000);
    let body = sweep_group(2, 0x4_0000_0000);
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: 0.8,
                against_fraction: 0.0,
            }),
            twist: Some(cadmpeg_ir::features::Angle(6.632_251_157_578_453)),
            taper: None,
            ..
        })
    ));
    let rail = sweep_group(2, 0x5_0000_0000);
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (sweep_start + 25) as u64,
        values: [0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone(), rail],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            path: Some(cadmpeg_ir::features::PathRef::Native(path)),
            path_extent: Some(cadmpeg_ir::features::SweepPathExtent {
                along_fraction: 0.0,
                against_fraction: 1.0,
            }),
            guide_rail: Some(cadmpeg_ir::features::SweepGuideRail {
                path: cadmpeg_ir::features::PathRef::Native(rail),
                extent: cadmpeg_ir::features::SweepPathExtent {
                    along_fraction: 0.0,
                    against_fraction: 1.0,
                },
            }),
            ..
        }) if path == "stream:sweep-group-1" && rail == "stream:sweep-group-2"
    ));
    let complete_sweep_values = [1.0, 1.0, 1.0, 1.0, sweep_values[4], 0.0];
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::NewBody,
        operation_offset: (sweep_start + 25) as u64,
        values: complete_sweep_values,
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Unresolved,
            ..
        })
    ));
    assert_eq!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile.clone(), path.clone(), body.clone()],
            &[],
            &[],
            &[],
            &[],
        ),
        None
    );
    sweep_scope.sweep_profile = Some(crate::records::DesignSketchProfileOperand {
        scope_reference_ordinal: 3,
        record_index: 2795,
        byte_offset: 32_000,
        class_tag: "312".into(),
        asset_id: "asset".into(),
        asset_id_offset: 32_040,
        entity_id: "0_2718".into(),
        entity_suffix: 2718,
        entity_reference_offset: 32_080,
        region_selection: None,
        paired_class_tag: "258".into(),
        paired_byte_offset: 32_180,
    });
    let mut selected_profile = profile.clone();
    selected_profile.members = vec![2788];
    let mut profile_carrier = profile.clone();
    profile_carrier.id = "stream:sweep-profile-carrier".into();
    profile_carrier.scope_reference_ordinal = 3;
    profile_carrier.members = vec![2795];
    let mut guide_surface = sweep_group(4, 0x11_0000_0000);
    guide_surface.id = "stream:sweep-guide-surface".into();
    let entity_selection = crate::records::DesignEntitySelectionOperand {
        id: "stream:sweep-profile-selection".into(),
        scope_record_index: sweep_scope.record_index,
        group_record_index: selected_profile.record_index,
        group_member_ordinal: 0,
        record_index: 2788,
        byte_offset: 31_000,
        class_tag: "310".into(),
        asset_id: "asset".into(),
        asset_id_offset: 31_040,
        context_id: "context".into(),
        context_id_offset: 31_080,
        identity_record_index: 2791,
        identity_record_offset: 31_180,
        primary_identity: 2718,
        primary_identity_offset: 31_200,
        secondary_identity: Some(164),
        secondary_identity_offset: Some(31_208),
        curve_secondary_identity: None,
        curve_secondary_identity_offset: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: None,
        next_record_index: profile_carrier.record_index,
        next_byte_offset: profile_carrier.byte_offset,
    };
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[selected_profile, profile_carrier, path.clone(), guide_surface],
            &[],
            &[],
            &[entity_selection],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            section: cadmpeg_ir::features::SweepSection::Profile(
                cadmpeg_ir::features::ProfileRef::Native(profile)
            ),
            orientation: Some(cadmpeg_ir::features::SweepOrientation::GuideSurface {
                faces: cadmpeg_ir::features::FaceSelection::Native(faces),
            }),
            guide_rail: None,
            ..
        }) if profile == "stream:sweep-group-0" && faces == "stream:sweep-guide-surface"
    ));
    sweep_scope.sweep_profile = None;
    sweep_scope.path_feature_construction = Some(DesignPathFeatureConstruction::Sweep {
        operation: DesignExtrudeOperation::Cut,
        operation_offset: (sweep_start + 25) as u64,
        values: complete_sweep_values,
        record_indexes: [80, 81, 82, 83, 84, 85],
        value_offsets: std::array::from_fn(|ordinal| {
            (sweep_scalar_start + ordinal * 111 + 40) as u64
        }),
    });
    assert!(matches!(
        crate::design::feature_project::project_fixed_sweep(
            &sweep_scope,
            &[profile, path, body],
            &[],
            &[],
            &[],
            &[],
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::Sweep {
            mode: cadmpeg_ir::features::SweepMode::Solid {
                op: cadmpeg_ir::features::BooleanOp::Cut
            },
            ..
        })
    ));

    let pipe_start = bytes.len();
    let mut pipe = vec![0; 464];
    pipe[25..29].copy_from_slice(&4u32.to_le_bytes());
    pipe[29] = 1;
    pipe[30] = 1;
    bytes.extend_from_slice(&pipe);
    let pipe_values: [f64; 4] = [1.0, 1.0, 0.6, 0.15];
    let pipe_scalar_start = bytes.len();
    for (ordinal, value) in pipe_values.into_iter().enumerate() {
        let record_index = 170 + ordinal as u32;
        let mut scalar = vec![0; 100];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record_index.to_le_bytes());
        scalar[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&scope.record_index.to_le_bytes());
        scalar[35] = ordinal as u8;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        scalar.extend_from_slice(&3u32.to_le_bytes());
        scalar.extend_from_slice(b"261");
        scalar.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&scalar);
    }
    let mut pipe_scope = scope.clone();
    pipe_scope.byte_offset = pipe_start as u64;
    pipe_scope.kind = "Pipe".into();
    pipe_scope.frame_length = 464;
    pipe_scope.reference_members = (170..174).collect();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &pipe_scope,
            &[],
        ),
        Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (pipe_start + 25) as u64,
            section_shape: 1,
            section_shape_offset: (pipe_start + 29) as u64,
            filled: true,
            filled_offset: (pipe_start + 30) as u64,
            values: pipe_values,
            record_indexes: [170, 171, 172, 173],
            value_offsets: std::array::from_fn(|ordinal| {
                (pipe_scalar_start + ordinal * 111 + 40) as u64
            }),
        })
    );

    let mut companion = DesignParameterCompanion {
        id: "f3d:native:parameter-companion#11".into(),
        byte_offset: 0,
        class_tag: "300".into(),
        record_index: 11,
        owner_record_index: 10,
        timestamp_micros: 1,
        timestamp_micros_offset: 42,
        payload_byte_offset: 58,
        payload_byte_length: 0,
        owned_recipe_ids: Vec::new(),
    };
    scope.id = "f3d:native:parameter-scope#12".into();
    scope.byte_offset = 58;
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        ),
        Some((58, 58))
    );
    scope.byte_offset = 80;
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[],
            100,
        ),
        Some((58, 80))
    );
    scope.byte_offset = 90;
    let foreign_header = DesignRecordHeader {
        id: "f3d:native:record-header#55".into(),
        record_index: 55,
        class_tag: "301".into(),
        byte_offset: 70,
    };
    assert_eq!(
        companion_owned_interval(
            &companion,
            std::iter::empty(),
            &[],
            &[scope.clone()],
            &[foreign_header],
            100,
        ),
        Some((58, 70))
    );

    let mut parameter = parse_design_parameter(&parameter_record(
        None,
        "1",
        "User Parameter",
        None,
        "p",
        1.0,
    ))
    .expect("generated parameter");
    parameter.id = "f3d:native:design-parameter#65".into();
    parameter.byte_offset = 65;
    assert_eq!(
        companion_owned_interval(&companion, std::iter::once(&parameter), &[], &[], &[], 100,),
        Some((58, 65))
    );
    let recipe = ConstructionRecipe {
        id: "f3d:native:construction-recipe#60".into(),
        byte_offset: 60,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Edge,
        design_id: None,
        design_id_offset: None,
        design_selector: None,
        recipe_index: 0,
        record_index: 303,
    };
    bind_parameter_companion_payloads(
        std::slice::from_mut(&mut companion),
        std::slice::from_ref(&parameter),
        &[],
        &[],
        &[],
        &[],
        std::slice::from_ref(&recipe),
        &HashMap::from([("f3d:native".into(), 100)]),
    );
    assert_eq!(companion.payload_byte_offset, 58);
    assert_eq!(companion.payload_byte_length, 7);
    assert_eq!(companion.owned_recipe_ids, [recipe.id]);

    companion.payload_byte_length = 0;
    companion.owned_recipe_ids.clear();
    scope.entity_id = Some("Sketch_99".into());
    scope.entity_suffix = Some(99);
    let entity = crate::records::DesignEntityHeader {
        id: "f3d:native:design-entity-header#70".into(),
        byte_offset: 70,
        entity_suffix: 99,
        entity_id: "Sketch_99".into(),
        class_tag: "366".into(),
        optional_slot_present: false,
        module: Some("MSketch".into()),
        record_reference: None,
        record_reference_offset: None,
        declared_reference_count: None,
        reference_indices: Vec::new(),
        reference_offsets: Vec::new(),
        member_indices: Vec::new(),
        member_offsets: Vec::new(),
    };
    bind_parameter_companion_payloads(
        std::slice::from_mut(&mut companion),
        &[],
        &[],
        std::slice::from_ref(&scope),
        std::slice::from_ref(&entity),
        &[],
        &[],
        &HashMap::from([("f3d:native".into(), 100)]),
    );
    assert_eq!(companion.payload_byte_offset, 58);
    assert_eq!(companion.payload_byte_length, 12);
}
