// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::assembly::assembly_operand_frame_fixture;
use super::prelude::*;

#[test]
fn circular_pattern_axis_prefers_one_inline_carrier() {
    use crate::records::DesignCircularPatternAxis;

    let historical = DesignCircularPatternAxis::HistoricalEdge {
        wrapper_record_indices: vec![11],
        persistent_identities: vec![17],
        identity_offsets: vec![23],
        resolved_origin: None,
        resolved_direction: None,
    };
    let inline = DesignCircularPatternAxis::Inline {
        origin: [1.0, 2.0, 3.0],
        origin_offset: 29,
        direction: [0.0, 0.0, 1.0],
        direction_offset: 53,
    };

    let historical_only = [(historical.clone(), 10, 11)];
    assert_eq!(
        select_circular_pattern_axis(&historical_only).map(|candidate| (candidate.1, candidate.2)),
        Some((10, 11))
    );

    let mixed = [(historical.clone(), 10, 11), (inline.clone(), 20, 21)];
    assert_eq!(
        select_circular_pattern_axis(&mixed).map(|candidate| (candidate.1, candidate.2)),
        Some((20, 21))
    );

    let duplicate_inline = [(inline.clone(), 20, 21), (inline, 30, 31)];
    assert!(select_circular_pattern_axis(&duplicate_inline).is_none());

    let duplicate_historical = [(historical.clone(), 10, 11), (historical, 12, 13)];
    assert!(select_circular_pattern_axis(&duplicate_historical).is_none());
}

#[allow(
    clippy::large_stack_arrays,
    reason = "This pattern fixture keeps the decoded scope records inline for frame assertions."
)]
#[test]
fn pattern_constructions_require_exact_scalar_and_operand_frames() {
    fn append_header(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.extend_from_slice(&3_u32.to_le_bytes());
        bytes.extend_from_slice(b"999");
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn append_transform_record(bytes: &mut Vec<u8>, record_index: u32, translation: [f64; 3]) {
        append_header(bytes, record_index);
        for value in [
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
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let scope_record_index = 10_u32;
    let count_record_index = 20_u32;
    let angle_record_index = 30_u32;
    let axis_record_index = 40_u32;
    let selection_record_index = 43_u32;
    let mut bytes = Vec::new();

    let count_start = bytes.len();
    let mut count = vec![0; 99];
    count[0..4].copy_from_slice(&3_u32.to_le_bytes());
    count[4..7].copy_from_slice(b"357");
    count[7..11].copy_from_slice(&count_record_index.to_le_bytes());
    count[19] = 1;
    count[20..24].copy_from_slice(&1_u32.to_le_bytes());
    count[24] = 1;
    count[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    count[40..44].copy_from_slice(&25_u32.to_le_bytes());
    count[44] = 1;
    count[45..49].copy_from_slice(&(count_record_index + 2).to_le_bytes());
    count[55..59].copy_from_slice(&99_u32.to_le_bytes());
    count[63] = 1;
    count[64..68].copy_from_slice(&scope_record_index.to_le_bytes());
    count[76] = 1;
    count[77..81].copy_from_slice(&(count_record_index + 1).to_le_bytes());
    count[88] = 1;
    count[89..93].copy_from_slice(&scope_record_index.to_le_bytes());
    count.extend_from_slice(&3_u32.to_le_bytes());
    count.extend_from_slice(b"258");
    count.extend_from_slice(&count_record_index.to_le_bytes());
    bytes.extend_from_slice(&count);

    let angle_start = bytes.len();
    let mut angle = vec![0; 103];
    angle[0..4].copy_from_slice(&3_u32.to_le_bytes());
    angle[4..7].copy_from_slice(b"354");
    angle[7..11].copy_from_slice(&angle_record_index.to_le_bytes());
    angle[19..24].copy_from_slice(&[1, 1, 0, 0, 0]);
    angle[24] = 1;
    angle[25..29].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[35] = 1;
    angle[40..48].copy_from_slice(&std::f64::consts::TAU.to_le_bytes());
    angle[48] = 1;
    angle[49..53].copy_from_slice(&77_u32.to_le_bytes());
    angle[67] = 1;
    angle[68..72].copy_from_slice(&scope_record_index.to_le_bytes());
    angle[80] = 1;
    angle[81..85].copy_from_slice(&78_u32.to_le_bytes());
    angle[92] = 1;
    angle[93..97].copy_from_slice(&scope_record_index.to_le_bytes());
    angle.extend_from_slice(&3_u32.to_le_bytes());
    angle.extend_from_slice(b"258");
    angle.extend_from_slice(&angle_record_index.to_le_bytes());
    bytes.extend_from_slice(&angle);

    let axis_start = bytes.len();
    let mut axis = vec![0; 195];
    axis[0..4].copy_from_slice(&3_u32.to_le_bytes());
    axis[4..7].copy_from_slice(b"379");
    axis[7..11].copy_from_slice(&axis_record_index.to_le_bytes());
    axis[21..25].copy_from_slice(&8_u32.to_le_bytes());
    for (offset, value) in [1.0_f64, 2.0, 3.0].into_iter().enumerate() {
        axis[25 + offset * 8..33 + offset * 8].copy_from_slice(&value.to_le_bytes());
    }
    axis[49..57].copy_from_slice(&(-1.0_f64).to_le_bytes());
    axis[89..93].copy_from_slice(&9_u32.to_le_bytes());
    axis[93..97].copy_from_slice(&1_u32.to_le_bytes());
    axis[97] = 1;
    axis[98..102].copy_from_slice(&selection_record_index.to_le_bytes());
    axis[110..114].copy_from_slice(&1_u32.to_le_bytes());
    axis[114] = 1;
    axis[115..119].copy_from_slice(&79_u32.to_le_bytes());
    axis[125..133].copy_from_slice(&0x0000_0004_0000_0000_u64.to_le_bytes());
    axis[143..147].copy_from_slice(&99_u32.to_le_bytes());
    axis[147..155].copy_from_slice(&0.5_f64.to_le_bytes());
    axis[155..159].copy_from_slice(&99_u32.to_le_bytes());
    axis[159] = 1;
    axis[160..164].copy_from_slice(&(axis_record_index + 2).to_le_bytes());
    axis[172] = 1;
    axis[173..177].copy_from_slice(&(axis_record_index + 1).to_le_bytes());
    axis[184] = 1;
    axis[185..189].copy_from_slice(&scope_record_index.to_le_bytes());
    axis.extend_from_slice(&3_u32.to_le_bytes());
    axis.extend_from_slice(b"258");
    axis.extend_from_slice(&axis_record_index.to_le_bytes());
    bytes.extend_from_slice(&axis);

    let mut scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:design-parameter-scope#0".into(),
        byte_offset: 0,
        class_tag: "291".into(),
        record_index: scope_record_index,
        frame_length: 329,
        kind: "C-Pattern".into(),
        kind_offset: 0,
        extrude: None,
        coil: None,
        feature_ordinal: 1,
        feature_ordinal_offset: 0,
        history_state_id: Some(2),
        history_state_id_offset: 0,
        previous_history_state_id: Some(1),
        previous_history_state_id_offset: 0,
        reference_count_offset: 0,
        reference_members: vec![
            count_record_index,
            angle_record_index,
            axis_record_index,
            selection_record_index,
        ],
        reference_member_offsets: vec![0; 4],
        solid_primitive: None,
        direct_face_operation: None,
        move_operation: None,
        scale_operation: None,
        surface_stitch_operation: None,
        surface_extend_operation: None,
        surface_offset_operation: None,
        ruled_surface_operation: None,
        surface_patch_boundaries: Vec::new(),
        base_flange: None,
        edge_flange_operation: None,
        hem_operation: None,
        fixed_fillet_parameters: None,
        fixed_chamfer_parameters: None,
        path_feature_construction: None,
        combine_operation: None,
        thread_construction: None,
        draft_operation: None,
        copy_paste_bodies_operation: None,
        base_feature_construction: None,
        work_plane_frame: None,
        work_axis_construction: None,
        joint_origin_frame: None,
        work_point_construction: None,
        unclosed_construction_operand_groups: Vec::new(),
        hole_construction: None,
        sweep_profile: None,
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        derived_instance_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        sketch_entity: None,
        paired_class_tag: "258".into(),
        paired_byte_offset: 329,
    };
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        Some(DesignCircularPatternConstruction {
            count: 25,
            count_record_index,
            count_offset: (count_start + 40) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index,
            angle_offset: (angle_start + 40) as u64,
            axis: crate::records::DesignCircularPatternAxis::Inline {
                origin: [1.0, 2.0, 3.0],
                origin_offset: (axis_start + 25) as u64,
                direction: [-1.0, 0.0, 0.0],
                direction_offset: (axis_start + 49) as u64,
            },
            axis_record_index,
            selection_record_index,
        })
    );

    for (offset, value) in [0.0_f64, 0.0, 2.0].into_iter().enumerate() {
        bytes[axis_start + 49 + offset * 8..axis_start + 57 + offset * 8]
            .copy_from_slice(&value.to_le_bytes());
    }
    let normalized = exact_circular_pattern_construction_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &[],
    )
    .expect("non-unit axis displacement is normalized");
    let crate::records::DesignCircularPatternAxis::Inline { direction, .. } = normalized.axis
    else {
        panic!("inline axis expected");
    };
    assert_eq!(direction, [0.0, 0.0, 1.0]);

    let mut zero_displacement = bytes.clone();
    zero_displacement[axis_start + 49..axis_start + 73].fill(0);
    assert!(exact_circular_pattern_construction_with_owners(
        &zero_displacement,
        &IndexedRecordOffsets::build(&zero_displacement),
        &scope,
        &[],
    )
    .is_none());

    bytes[count_start + 4] = b'x';
    bytes[angle_start + 4] = b'x';
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
    let owners = [
        owner(count_record_index, 0, 25.0, 101),
        owner(angle_record_index, 1, std::f64::consts::TAU, 202),
    ];
    let owner_backed = exact_circular_pattern_construction_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &owners,
    )
    .unwrap();
    assert_eq!(owner_backed.count, 25);
    assert_eq!(owner_backed.count_offset, 101);
    assert_eq!(owner_backed.angle, std::f64::consts::TAU);
    assert_eq!(owner_backed.angle_offset, 202);
    bytes[count_start + 4] = b'3';
    bytes[angle_start + 4] = b'3';

    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&6_u32.to_le_bytes());
    assert!(exact_circular_pattern_construction_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &[]
    )
    .is_some());
    bytes[axis_start + 89..axis_start + 93].fill(0);
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 89..axis_start + 93].copy_from_slice(&9_u32.to_le_bytes());

    bytes[axis_start + 57..axis_start + 65].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );
    bytes[axis_start + 57..axis_start + 65].fill(0);
    scope
        .reference_members
        .extend([axis_record_index, selection_record_index]);
    assert_eq!(
        exact_circular_pattern_construction_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            &[]
        ),
        None
    );

    scope.kind = "R-Pattern".into();
    let rectangular_owners = [
        owner(50, 0, 3.0, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 10.0, 503),
        owner(53, 3, 0.0, 504),
    ];
    let rectangular = exact_rectangular_pattern_construction(
        &[],
        &IndexedRecordOffsets::build(&[]),
        &scope,
        &rectangular_owners,
    )
    .expect("exact rectangular-pattern scalar lanes");
    assert_eq!(rectangular.u_count, 3);
    assert_eq!(rectangular.v_count, 1);
    assert_eq!(rectangular.u_extent, 10.0);
    assert_eq!(rectangular.v_extent, 0.0);
    assert_eq!(rectangular.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(rectangular.value_offsets, [501, 502, 503, 504]);
    assert_eq!(rectangular.instances, None);

    append_transform_record(&mut bytes, 100, [2.0, 3.0, 4.0]);
    for record_index in 50..=53 {
        append_header(&mut bytes, record_index);
    }
    append_header(&mut bytes, 110);
    append_transform_record(&mut bytes, 120, [2.0, 3.0, 9.0]);
    append_transform_record(&mut bytes, 130, [2.0, 3.0, 14.0]);
    append_header(&mut bytes, 140);
    scope.reference_members = vec![100, 50, 51, 52, 53, 110, 120, 130, 140];
    let rectangular = exact_rectangular_pattern_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &rectangular_owners,
    )
    .expect("rectangular-pattern placement run");
    let instances = rectangular.instances.expect("exact placement run");
    assert_eq!(instances.record_indices, [100, 120, 130]);
    assert_eq!(
        instances
            .transforms
            .iter()
            .map(|transform| transform[2][3])
            .collect::<Vec<_>>(),
        [4.0, 9.0, 14.0]
    );

    let mut invalid_inactive_spacing = rectangular_owners.clone();
    invalid_inactive_spacing[3].evaluated_value = 1.0;
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &invalid_inactive_spacing
        ),
        None
    );
    let mut duplicate_lane = rectangular_owners.clone();
    duplicate_lane[3].local_ordinal = 2;
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &duplicate_lane
        ),
        None
    );
    let mut excess_lane = rectangular_owners.to_vec();
    excess_lane.push(owner(54, 4, 1.0, 505));
    assert_eq!(
        exact_rectangular_pattern_construction(
            &[],
            &IndexedRecordOffsets::build(&[]),
            &scope,
            &excess_lane
        ),
        None
    );

    scope.kind = "Assemble".into();
    scope.frame_length = 627;
    scope.reference_members = vec![50, 51, 52, 53];
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &rectangular_owners,
    )
    .expect("exact assembly scalar lanes");
    assert_eq!(alignment.angle, 3.0);
    assert_eq!(alignment.offset, [1.0, 10.0, 0.0]);
    assert_eq!(alignment.owner_record_indices, [50, 51, 52, 53]);
    assert_eq!(alignment.value_offsets, [501, 502, 503, 504]);
    assert_eq!(alignment.operand_frames, None);

    let mut placement_and_alignment_owners = rectangular_owners.to_vec();
    placement_and_alignment_owners.extend([
        owner(60, 4, 0.25, 601),
        owner(61, 5, 4.0, 602),
        owner(62, 6, 5.0, 603),
        owner(63, 7, 6.0, 604),
    ]);
    scope.reference_members = vec![50, 51, 52, 53, 60, 61, 62, 63];
    assert!(exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .is_none());
    scope.frame_length = 732;
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .expect("assembly alignment after four placement lanes");
    assert_eq!(alignment.angle, 0.25);
    assert_eq!(alignment.offset, [4.0, 5.0, 6.0]);
    assert_eq!(alignment.owner_record_indices, [60, 61, 62, 63]);
    scope.frame_length = 604;
    let datum_envelope_alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &placement_and_alignment_owners,
    )
    .expect("JointOrigin datum-envelope alignment after four placement lanes");
    assert_eq!(datum_envelope_alignment.angle, 0.25);
    assert_eq!(datum_envelope_alignment.offset, [4.0, 5.0, 6.0]);
    assert_eq!(
        datum_envelope_alignment.owner_record_indices,
        [60, 61, 62, 63]
    );
    assert!(datum_envelope_alignment.operand_frames.is_none());

    let mut short_axial_owners = rectangular_owners.to_vec();
    short_axial_owners.extend([owner(64, 4, 0.5, 605), owner(65, 5, 2.0, 606)]);
    scope.reference_members = vec![50, 51, 52, 53, 64, 65];
    assert!(exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &short_axial_owners,
    )
    .is_none());
    scope.frame_length = 705;
    let short_axial_alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &short_axial_owners,
    )
    .expect("six-owner alignment belongs to the 705-byte axial frame");
    assert_eq!(short_axial_alignment.angle, 0.5);
    assert_eq!(short_axial_alignment.offset, [0.0, 0.0, 2.0]);

    let mut legacy_alignment_owners = placement_and_alignment_owners.clone();
    legacy_alignment_owners.extend([owner(64, 8, 0.5, 605), owner(65, 9, 2.0, 606)]);
    scope.reference_members = vec![50, 51, 52, 53, 60, 61, 62, 63, 64, 65];
    assert!(exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &legacy_alignment_owners,
    )
    .is_none());
    scope.frame_length = 772;
    let alignment = exact_assembly_alignment(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        &legacy_alignment_owners,
    )
    .expect("legacy assembly axial alignment lanes");
    assert_eq!(alignment.angle, 0.5);
    assert_eq!(alignment.offset, [0.0, 0.0, 2.0]);
    assert_eq!(alignment.owner_record_indices, [64, 65]);
    assert_eq!(alignment.value_offsets, [605, 606]);
    scope.reference_members = vec![50, 51, 52, 53];

    let assembly_bytes = assembly_operand_frame_fixture(scope_record_index);
    scope.frame_length = 637;
    scope.paired_byte_offset = 637;
    scope.paired_class_tag = "259".into();
    let frames = exact_assembly_alignment(
        &assembly_bytes,
        &IndexedRecordOffsets::build(&assembly_bytes),
        &scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_frames)
    .expect("exact assembly operand frames");
    assert_eq!(
        frames.map(|frame| (
            frame.reference_record_index,
            frame.reference_offset,
            frame.transform_offset,
            [
                frame.transform[0][3],
                frame.transform[1][3],
                frame.transform[2][3]
            ]
        )),
        [
            (70, 29, 40, [1.0, 2.0, 3.0]),
            (80, 169, 180, [4.0, 5.0, 6.0]),
        ]
    );
    let mut legacy_assembly_bytes = vec![0_u8; 633];
    legacy_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    for (legacy_reference, legacy_transform, modern_reference, modern_transform) in
        [(24, 36, 28, 40), (164, 176, 168, 180)]
    {
        legacy_assembly_bytes[legacy_reference..legacy_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        legacy_assembly_bytes[legacy_transform..legacy_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    legacy_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    legacy_assembly_bytes.extend_from_slice(b"258");
    legacy_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let legacy_assembly_scope = DesignParameterScope {
        frame_length: 633,
        paired_byte_offset: 633,
        paired_class_tag: "258".into(),
        ..scope.clone()
    };
    let legacy_frames = exact_assembly_alignment(
        &legacy_assembly_bytes,
        &IndexedRecordOffsets::build(&legacy_assembly_bytes),
        &legacy_assembly_scope,
        &rectangular_owners,
    )
    .and_then(|alignment| alignment.operand_frames)
    .expect("compact assembly operand frames");
    assert_eq!(legacy_frames[0].reference_offset, 25);
    assert_eq!(legacy_frames[0].transform_offset, 36);

    let mut dynamic_standard_bytes = assembly_bytes.clone();
    dynamic_standard_bytes[641..644].copy_from_slice(b"262");
    let dynamic_standard_scope = DesignParameterScope {
        paired_class_tag: "262".into(),
        ..scope.clone()
    };
    assert!(exact_assembly_alignment(
        &dynamic_standard_bytes,
        &IndexedRecordOffsets::build(&dynamic_standard_bytes),
        &dynamic_standard_scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_frames.is_some()));

    let mut dynamic_compact_bytes = legacy_assembly_bytes.clone();
    dynamic_compact_bytes[637..640].copy_from_slice(b"262");
    let dynamic_compact_scope = DesignParameterScope {
        paired_class_tag: "262".into(),
        ..legacy_assembly_scope.clone()
    };
    assert!(exact_assembly_alignment(
        &dynamic_compact_bytes,
        &IndexedRecordOffsets::build(&dynamic_compact_bytes),
        &dynamic_compact_scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_frames.is_some()));

    let mut axial_assembly_bytes = vec![0_u8; 772];
    axial_assembly_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    axial_assembly_bytes[20..25].copy_from_slice(&[1, 0, 0, 0, 0]);
    for (axial_reference, axial_transform, modern_reference, modern_transform) in
        [(28, 39, 28, 40), (167, 178, 168, 180)]
    {
        axial_assembly_bytes[axial_reference..axial_reference + 5]
            .copy_from_slice(&assembly_bytes[modern_reference..modern_reference + 5]);
        axial_assembly_bytes[axial_transform..axial_transform + 128]
            .copy_from_slice(&assembly_bytes[modern_transform..modern_transform + 128]);
    }
    axial_assembly_bytes.extend_from_slice(&3_u32.to_le_bytes());
    axial_assembly_bytes.extend_from_slice(b"261");
    axial_assembly_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let axial_assembly_scope = DesignParameterScope {
        frame_length: 772,
        paired_byte_offset: 772,
        paired_class_tag: "261".into(),
        reference_members: vec![50, 51, 52, 53, 60, 61, 62, 63, 64, 65],
        ..scope.clone()
    };
    let axial_alignment = exact_assembly_alignment(
        &axial_assembly_bytes,
        &IndexedRecordOffsets::build(&axial_assembly_bytes),
        &axial_assembly_scope,
        &legacy_alignment_owners,
    )
    .expect("legacy assembly alignment and operand frames");
    assert_eq!(axial_alignment.angle, 0.5);
    assert_eq!(axial_alignment.offset, [0.0, 0.0, 2.0]);
    let axial_frames = axial_alignment.operand_frames.as_ref().unwrap();
    assert_eq!(axial_frames[0].reference_offset, 29);
    assert_eq!(axial_frames[0].transform_offset, 39);
    assert_eq!(axial_frames[1].reference_offset, 168);
    assert_eq!(axial_frames[1].transform_offset, 178);

    let mut short_axial_bytes = axial_assembly_bytes[..705].to_vec();
    short_axial_bytes.extend_from_slice(&3_u32.to_le_bytes());
    short_axial_bytes.extend_from_slice(b"261");
    short_axial_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let short_axial_scope = DesignParameterScope {
        frame_length: 705,
        paired_byte_offset: 705,
        paired_class_tag: "261".into(),
        reference_members: vec![50, 51, 52, 53, 64, 65],
        ..scope.clone()
    };
    let short_axial_alignment = exact_assembly_alignment(
        &short_axial_bytes,
        &IndexedRecordOffsets::build(&short_axial_bytes),
        &short_axial_scope,
        &short_axial_owners,
    )
    .expect("short axial assembly alignment and operand frames");
    assert_eq!(short_axial_alignment.angle, 0.5);
    assert_eq!(short_axial_alignment.offset, [0.0, 0.0, 2.0]);
    assert_eq!(short_axial_alignment.owner_record_indices, [64, 65]);
    assert!(short_axial_alignment.operand_paths().is_none());
    let short_axial_frames = short_axial_alignment
        .operand_frames
        .as_ref()
        .expect("short axial operand frames");
    assert_eq!(short_axial_frames[0].reference_offset, 29);
    assert_eq!(short_axial_frames[0].transform_offset, 39);
    assert_eq!(short_axial_frames[1].reference_offset, 168);
    assert_eq!(short_axial_frames[1].transform_offset, 178);

    let mut first_joint_origin = scope.clone();
    first_joint_origin.kind = "JointOrigin".into();
    first_joint_origin.record_index = 70;
    first_joint_origin.reference_members.clear();
    let mut second_joint_origin = first_joint_origin.clone();
    second_joint_origin.record_index = 80;
    let mut linked_assembly = axial_assembly_scope.clone();
    linked_assembly.assembly_alignment = Some(axial_alignment.clone());
    let mut linked_scopes = [linked_assembly, first_joint_origin, second_joint_origin];
    bind_joint_origin_frames_from_assemblies(&axial_assembly_bytes, &mut linked_scopes);
    assert_eq!(linked_scopes[1].joint_origin_transform_offset(), Some(39));
    assert_eq!(
        linked_scopes[1].joint_origin_transform(),
        Some(axial_frames[0].transform)
    );
    assert_eq!(linked_scopes[2].joint_origin_transform_offset(), Some(178));
    assert_eq!(
        linked_scopes[2].joint_origin_transform(),
        Some(axial_frames[1].transform)
    );
    assert_eq!(
        linked_scopes[0]
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index),
        None
    );

    let mut single_frame_bytes = vec![0_u8; 604];
    single_frame_bytes[..11].copy_from_slice(&assembly_bytes[..11]);
    single_frame_bytes[24] = 1;
    single_frame_bytes[25..29].copy_from_slice(&90_u32.to_le_bytes());
    single_frame_bytes[36..164].copy_from_slice(&assembly_bytes[40..168]);
    single_frame_bytes[164] = 1;
    single_frame_bytes[165..169].copy_from_slice(&91_u32.to_le_bytes());
    single_frame_bytes[175..179].copy_from_slice(&1_u32.to_le_bytes());
    let mut single_frame_assembly = scope.clone();
    single_frame_assembly.class_tag = "276".into();
    single_frame_assembly.paired_class_tag = "258".into();
    single_frame_assembly.frame_length = 604;
    single_frame_assembly.paired_byte_offset = 604;
    single_frame_assembly.reference_members = placement_and_alignment_owners
        .iter()
        .map(|owner| owner.record_index)
        .collect();
    single_frame_assembly.assembly_alignment = Some(datum_envelope_alignment);
    let mut single_frame_joint_origin = scope.clone();
    single_frame_joint_origin.kind = "JointOrigin".into();
    single_frame_joint_origin.record_index = 91;
    single_frame_joint_origin.reference_members.clear();
    let mut single_frame_scopes = [single_frame_assembly, single_frame_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut single_frame_scopes);
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform_offset(),
        Some(36)
    );
    assert_eq!(
        single_frame_scopes[1].joint_origin_transform(),
        Some(axial_frames[0].transform)
    );
    assert_eq!(single_frame_scopes[1].joint_origin_reference(), Some(90));
    assert_eq!(
        single_frame_scopes[1].joint_origin_reference_offset(),
        Some(25)
    );
    assert_eq!(
        single_frame_scopes[0]
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index),
        Some(91)
    );

    let mut conflicting_assembly = single_frame_scopes[0].clone();
    conflicting_assembly
        .assembly_alignment
        .as_mut()
        .unwrap()
        .joint_origin_scope_record_index = None;
    let mut conflicting_joint_origin = single_frame_scopes[1].clone();
    conflicting_joint_origin
        .joint_origin_frame
        .as_mut()
        .unwrap()
        .joint_origin_transform[2][3] += 1.0;
    let mut conflicting_scopes = [conflicting_assembly, conflicting_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut conflicting_scopes);
    assert_eq!(
        conflicting_scopes[0]
            .assembly_alignment
            .as_ref()
            .and_then(|alignment| alignment.joint_origin_scope_record_index),
        None
    );

    single_frame_bytes[175..179].copy_from_slice(&2_u32.to_le_bytes());
    let mut invalid_joint_origin = single_frame_scopes[1].clone();
    invalid_joint_origin.joint_origin_frame = None;
    let mut invalid_single_frame_scopes = [single_frame_scopes[0].clone(), invalid_joint_origin];
    bind_joint_origin_frames_from_assemblies(&single_frame_bytes, &mut invalid_single_frame_scopes);
    assert_eq!(
        invalid_single_frame_scopes[1].joint_origin_transform(),
        None
    );

    let mut compact_bytes = assembly_bytes[..627].to_vec();
    compact_bytes.extend_from_slice(&3_u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"264");
    compact_bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    let mut compact_scope = scope.clone();
    compact_scope.class_tag = "459".into();
    compact_scope.frame_length = 627;
    compact_scope.paired_byte_offset = 627;
    compact_scope.paired_class_tag = "264".into();
    assert!(exact_assembly_alignment(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &compact_scope,
        &rectangular_owners,
    )
    .is_some_and(|alignment| alignment.operand_frames.is_some()));
}
