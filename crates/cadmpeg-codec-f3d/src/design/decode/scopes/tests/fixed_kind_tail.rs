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
use crate::layout::joint_origin_legacy_class_337_266_frame as joint_origin_class_337_266;
use crate::layout::shell_class_369_261_scope_frame as shell_369_261;
use crate::layout::work_plane_legacy_321_opaque_matrix_frame as work_plane_321_opaque;
use crate::layout::work_plane_legacy_325_matrix_frame as work_plane_325;
use crate::layout::work_plane_legacy_337_matrix_frame as work_plane_337;
use crate::layout::work_plane_legacy_class_256_matrix_frame as work_plane_class_256;
use crate::layout::work_plane_legacy_class_290_matrix_frame as work_plane_class_290;
use crate::layout::work_plane_legacy_class_322_332_matrix_frame as work_plane_class_322_332;
use crate::layout::work_plane_legacy_class_337_325_matrix_frame as work_plane_class_337_325;

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

    let scope =
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap();
    assert_eq!(scope.kind, crate::records::DesignFeatureKind::Sketch);
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
    assert_eq!(compact.kind, crate::records::DesignFeatureKind::Sketch);
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
        assert_eq!(decoded.kind, crate::records::DesignFeatureKind::Sketch);
        assert_eq!(decoded.previous_history_state_id, Some(2));
        assert_eq!(
            decoded.previous_history_state_id_offset,
            Some((feature_ordinal_at + 30) as u64)
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
        Some((feature_ordinal_at + 41) as u64)
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
        assert_eq!(decoded.kind, crate::records::DesignFeatureKind::Sketch);
        assert_eq!(decoded.previous_history_state_id, None);
        assert_eq!(decoded.previous_history_state_id_offset, None);
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
    assert_eq!(
        copy.kind,
        crate::records::DesignFeatureKind::CopyPasteBodies
    );
    assert_eq!(copy.feature_ordinal, 2);
    assert_eq!(copy.feature_ordinal_offset, copy_feature_ordinal_at as u64);
    assert_eq!(copy.history_state_id, None);
    assert_eq!(copy.previous_history_state_id, None);
    assert_eq!(
        copy.previous_history_state_id_offset,
        Some((copy_feature_ordinal_at + 53) as u64)
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
    assert_eq!(
        generic_scope.kind,
        crate::records::DesignFeatureKind::Sketch
    );
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
    axis_scope.kind = crate::records::DesignFeatureKind::WorkAxis;
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
    assert!(matches!(
        construction.source,
        Some(crate::records::DesignWorkAxisSource::TwoPoint {
            point_record_indices: [102, 104],
            ..
        })
    ));
    axis_scope.set_work_axis_construction(Some(construction));
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
    joint_origin_scope.kind = crate::records::DesignFeatureKind::JointOrigin;
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
    compact_joint_origin_scope.kind = crate::records::DesignFeatureKind::JointOrigin;
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

    let legacy_joint_origin_at = bytes.len();
    let mut legacy_joint_origin = vec![0; joint_origin_class_337_266::LEN];
    legacy_joint_origin[0..4].copy_from_slice(&3u32.to_le_bytes());
    legacy_joint_origin[4..7].copy_from_slice(b"337");
    legacy_joint_origin[7..11].copy_from_slice(&72u32.to_le_bytes());
    legacy_joint_origin
        [joint_origin_class_337_266::MATRIX_PREFIX..joint_origin_class_337_266::MATRIX]
        .copy_from_slice(&joint_origin_class_337_266::MATRIX_PREFIX_VALUE);
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = joint_origin_class_337_266::MATRIX + ordinal * 8;
        legacy_joint_origin[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    legacy_joint_origin.extend_from_slice(&3u32.to_le_bytes());
    legacy_joint_origin.extend_from_slice(b"266");
    legacy_joint_origin.extend_from_slice(&72u32.to_le_bytes());
    bytes.extend_from_slice(&legacy_joint_origin);
    let mut legacy_joint_origin_scope = scope.clone();
    legacy_joint_origin_scope.kind = crate::records::DesignFeatureKind::JointOrigin;
    legacy_joint_origin_scope.reference_members = vec![72];
    let decoded = exact_joint_origin_frame(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &legacy_joint_origin_scope,
    )
    .expect("exact class-337/266 JointOrigin frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(
        decoded.transform_offset,
        (legacy_joint_origin_at + joint_origin_class_337_266::MATRIX) as u64
    );
    assert_eq!(decoded.reference, None);

    let mut invalid_legacy_joint_origin = legacy_joint_origin.clone();
    invalid_legacy_joint_origin[joint_origin_class_337_266::MATRIX_PREFIX] = 0;
    let mut invalid_bytes = bytes[..legacy_joint_origin_at].to_vec();
    invalid_bytes.extend_from_slice(&invalid_legacy_joint_origin);
    assert_eq!(
        exact_joint_origin_frame(
            &invalid_bytes,
            &IndexedRecordOffsets::build(&invalid_bytes),
            &legacy_joint_origin_scope,
        ),
        None
    );

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
    move_scope.kind = crate::records::DesignFeatureKind::Move;
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
    compact_move_scope.kind = crate::records::DesignFeatureKind::Move;
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
    class_433_move_scope.kind = crate::records::DesignFeatureKind::Move;
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
    scale_scope.kind = crate::records::DesignFeatureKind::Massstab;
    scale_scope.frame_length = 317;
    scale_scope.reference_members = vec![101, 102, 103, 104, 105];
    let scale_records = IndexedRecordOffsets::build(&bytes);
    assert_eq!(
        exact_scale_operation(&bytes, &scale_records, &scale_scope, &HashMap::new()),
        Some(DesignScaleOperation {
            body_group_record_index: 102,
            center_record_index: 105,
            center_position: None,
            center_position_offset: None,
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
    sphere_scope.kind = crate::records::DesignFeatureKind::Native("SpherePrimitive".into());
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
    torus_scope.kind = crate::records::DesignFeatureKind::Native("TorusPrimitive".into());
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
    offset_scope.kind = crate::records::DesignFeatureKind::OffsetFaces;
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
    thicken_scope.kind = crate::records::DesignFeatureKind::Thicken;
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
    thicken_scope.set_direct_face_operation(exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &thicken_scope,
    ));
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
    shell_scope.kind = crate::records::DesignFeatureKind::Shell;
    shell_scope.frame_length = 278;
    shell_scope.reference_members = vec![200, 201, 1_778];
    shell_scope.set_direct_face_operation(exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &shell_scope,
    ));
    assert!(matches!(
        shell_scope.direct_face_operation(),
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
    compact_shell_scope.set_direct_face_operation(exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &compact_shell_scope,
    ));
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
    offset_scope.set_direct_face_operation(exact_direct_face_operation(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &offset_scope,
    ));
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
    extrude_scope.kind = crate::records::DesignFeatureKind::Extrude;
    extrude_scope.ensure_extrude().extrude_prologue = Some(DesignExtrudePrologue::ReferenceAware {
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
            &extrude_scope,
            &[],
            &[],
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
            &extrude_scope,
            &[],
            &[],
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
            &extrude_scope,
            &[],
            &[],
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
    extend_scope.kind = crate::records::DesignFeatureKind::SurfaceExtend;
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
    extend_scope.set_surface_extend_operation(Some(operation));
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
    extend_scope.kind = crate::records::DesignFeatureKind::SurfaceOffset;
    extend_scope.set_surface_extend_operation(None);
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
    extend_scope.set_surface_offset_operation(Some(operation));
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
            &extrude_scope,
            &[],
            &[],
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
            &extrude_scope,
            &[],
            &[],
        ),
        None
    );

    super::fixed_kind_operations::continue_fixed_kind_operations(bytes, scope, &thicken_group);
}

#[test]
fn generated_copy_paste_bodies_scope_matches_operation_layout() {
    let (bytes, _) = crate::test_support::generated_design_copy_paste_bodies_bulkstream();
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    let headers =
        crate::design::decode::scopes::parameter_scope_candidate_headers(&bytes, &records)
            .into_iter()
            .filter(|header| header.record_index == 1_400)
            .collect::<Vec<_>>();
    assert_eq!(headers.len(), 1);
    let scope = crate::design::decode::scopes::parse_parameter_scope(&bytes, &records, &headers[0])
        .expect("scope");
    assert_eq!(
        scope.kind,
        crate::records::DesignFeatureKind::CopyPasteBodies
    );
    assert_eq!(scope.reference_members, [1_500, 1_600]);
    assert_eq!(scope.frame_length, 225);
    let operation =
        crate::design::decode::scopes::exact_copy_paste_bodies_operation(&bytes, &records, &scope)
            .expect("CopyPasteBodies operation");
    assert_eq!(operation.body_group_record_index, 1_500);
    assert_eq!(operation.relation_record_index, 1_700);
    assert_eq!(operation.source_body_entity_suffixes, [985]);
    assert_eq!(operation.copied_body_entity_suffixes, [8_422]);
}

#[test]
fn legacy_work_plane_class_380_frame_decodes_its_matrix() {
    let mut bytes = vec![0; 325];
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"380");
    bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
    let transform = identity_matrix();
    for (ordinal, value) in transform.into_iter().flatten().enumerate() {
        let at = 49 + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&71u32.to_le_bytes());

    let scope = DesignParameterScope::empty(
        "f3d:test:scope#1",
        crate::records::DesignFeatureKind::WorkPlane,
        1,
    );
    let mut scope = scope;
    scope.reference_members = vec![71];
    let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
        .expect("class-380 WorkPlane frame");
    assert_eq!(decoded.transform, transform);
    assert_eq!(decoded.transform_offset, 49);
    assert_eq!(decoded.reference, None);
}

#[test]
fn legacy_work_plane_class_256_frame_decodes_its_opaque_prefix_lane() {
    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for opaque_u16 in [[0, 0], [0x9b, 0xdc]] {
        let mut bytes = vec![0; work_plane_class_256::LEN];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"256");
        bytes[7..11].copy_from_slice(&71u32.to_le_bytes());
        bytes
            [work_plane_class_256::OPAQUE_U16..work_plane_class_256::OPAQUE_U16 + opaque_u16.len()]
            .copy_from_slice(&opaque_u16);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = work_plane_class_256::MATRIX + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"262");
        bytes.extend_from_slice(&71u32.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#1",
            crate::records::DesignFeatureKind::WorkPlane,
            1,
        );
        scope.reference_members = vec![71];
        let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("class-256 WorkPlane frame");
        assert_eq!(decoded.transform, transform);
        assert_eq!(
            decoded.transform_offset,
            work_plane_class_256::MATRIX as u64
        );
        assert_eq!(decoded.reference, None);
    }

    let mut invalid = vec![0; work_plane_class_256::LEN];
    invalid[0..4].copy_from_slice(&3u32.to_le_bytes());
    invalid[4..7].copy_from_slice(b"256");
    invalid[7..11].copy_from_slice(&71u32.to_le_bytes());
    invalid[work_plane_class_256::ZERO_PAIR] = 1;
    invalid.extend_from_slice(&3u32.to_le_bytes());
    invalid.extend_from_slice(b"262");
    invalid.extend_from_slice(&71u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#2",
        crate::records::DesignFeatureKind::WorkPlane,
        2,
    );
    scope.reference_members = vec![71];
    assert_eq!(
        exact_work_plane_frame(&invalid, &IndexedRecordOffsets::build(&invalid), &scope),
        None
    );
}

#[test]
fn legacy_work_plane_opaque_prefix_frames_use_class_pair_admission() {
    type WorkPlaneOpaqueCase = (&'static [u8; 3], &'static [u8; 3], usize, [u8; 2], u32);

    let transform: [[f64; 4]; 4] = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let cases: [WorkPlaneOpaqueCase; 3] = [
        (b"341", b"261", work_plane_321_opaque::LEN, [0xea, 0x20], 81),
        (b"346", b"262", work_plane_321_opaque::LEN, [0xae, 0x70], 82),
        (
            b"337",
            b"266",
            work_plane_class_337_325::LEN,
            [0x6d, 0x00],
            83,
        ),
    ];

    for (class_tag, paired_class_tag, frame_length, opaque_u16, record_index) in cases {
        let matrix = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::MATRIX
        } else {
            work_plane_class_337_325::MATRIX
        };
        let opaque = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::OPAQUE_U16
        } else {
            work_plane_class_337_325::OPAQUE_U16
        };
        let zero_pair = if frame_length == work_plane_321_opaque::LEN {
            work_plane_321_opaque::ZERO_PAIR
        } else {
            work_plane_class_337_325::ZERO_PAIR
        };
        let mut bytes = vec![0; frame_length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(class_tag);
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[opaque..opaque + opaque_u16.len()].copy_from_slice(&opaque_u16);
        assert_eq!(&bytes[zero_pair..zero_pair + 2], &[0, 0]);
        for (ordinal, value) in transform.into_iter().flatten().enumerate() {
            let at = matrix + ordinal * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(paired_class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());

        let mut scope = DesignParameterScope::empty(
            "f3d:test:scope#opaque",
            crate::records::DesignFeatureKind::WorkPlane,
            1,
        );
        scope.reference_members = vec![record_index];
        let decoded = exact_work_plane_frame(&bytes, &IndexedRecordOffsets::build(&bytes), &scope)
            .expect("opaque-prefix WorkPlane frame");
        assert_eq!(decoded.transform, transform);
        assert_eq!(decoded.transform_offset, matrix as u64);
        assert_eq!(decoded.reference, None);
    }

    let mut invalid = vec![0; work_plane_321_opaque::LEN];
    invalid[0..4].copy_from_slice(&3u32.to_le_bytes());
    invalid[4..7].copy_from_slice(b"341");
    invalid[7..11].copy_from_slice(&84u32.to_le_bytes());
    invalid[work_plane_321_opaque::OPAQUE_U16..work_plane_321_opaque::OPAQUE_U16 + 2]
        .copy_from_slice(&[0xea, 0x20]);
    invalid[work_plane_321_opaque::ZERO_PAIR] = 1;
    invalid.extend_from_slice(&3u32.to_le_bytes());
    invalid.extend_from_slice(b"262");
    invalid.extend_from_slice(&84u32.to_le_bytes());
    let mut scope = DesignParameterScope::empty(
        "f3d:test:scope#invalid",
        crate::records::DesignFeatureKind::WorkPlane,
        2,
    );
    scope.reference_members = vec![84];
    assert_eq!(
        exact_work_plane_frame(&invalid, &IndexedRecordOffsets::build(&invalid), &scope),
        None
    );
}
