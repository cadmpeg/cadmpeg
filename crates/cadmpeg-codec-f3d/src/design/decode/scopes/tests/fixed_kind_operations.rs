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
use crate::layout::fixed_pipe_operation_prefix as fixed_pipe_layout;
use crate::layout::legacy_pipe_operation_prefix as legacy_pipe_layout;

pub(super) fn continue_fixed_kind_operations(
    mut bytes: Vec<u8>,
    mut scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) {
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

    let class403_start = bytes.len();
    let mut class403_revolve = vec![0; 387];
    class403_revolve[21..25].copy_from_slice(&2u32.to_le_bytes());
    class403_revolve[25..29].copy_from_slice(&2u32.to_le_bytes());
    class403_revolve[29..31].copy_from_slice(&[0, 1]);
    class403_revolve[34] = 1;
    class403_revolve[35..39].copy_from_slice(&indexed_angle_record_index.to_le_bytes());
    let mut class403_guid = Vec::new();
    lp_utf16(&mut class403_guid, "00000000-0000-0000-0000-000000000000");
    class403_revolve[107..183].copy_from_slice(&class403_guid);
    bytes.extend_from_slice(&class403_revolve);
    let mut class403_scope = revolve_scope.clone();
    class403_scope.class_tag = "403".into();
    class403_scope.paired_class_tag = "258".into();
    class403_scope.byte_offset = class403_start as u64;
    class403_scope.frame_length = 387;
    class403_scope.reference_members = vec![
        200,
        201,
        202,
        203,
        204,
        205,
        206,
        indexed_angle_record_index,
    ];
    let mut class403_angle = indexed_angle.clone();
    class403_angle.scope_record_index = class403_scope.record_index;
    class403_angle.evaluated_value_offset = (class403_start + 40) as u64;
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &class403_scope,
            std::slice::from_ref(&class403_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve {
            operation: DesignExtrudeOperation::Cut,
            operation_offset: (class403_start + 21) as u64,
            angle: std::f64::consts::TAU,
            angle_record_index: indexed_angle_record_index,
            angle_offset: (class403_start + 40) as u64,
            opposite_angle_record_index: None,
            opposite_angle_offset: None,
        })
    );
    bytes[class403_start + 34] = 0;
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &class403_scope,
            std::slice::from_ref(&class403_angle),
        ),
        None
    );
    bytes[class403_start + 34] = 1;

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
        resolved_active_face: None,
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

    let owner_pipe_start = bytes.len();
    let mut owner_pipe = vec![0; fixed_pipe_layout::FILLED + 1];
    owner_pipe[fixed_pipe_layout::OPERATION..fixed_pipe_layout::OPERATION + 4]
        .copy_from_slice(&4u32.to_le_bytes());
    owner_pipe[fixed_pipe_layout::SECTION_SHAPE] = 1;
    owner_pipe[fixed_pipe_layout::FILLED] = 1;
    bytes.extend_from_slice(&owner_pipe);
    let owner_pipe_values: [f64; 4] = [1.0, 0.0, 0.175, 0.0438];
    let owner_pipe_record_indexes = [210, 211, 212, 213];
    let owner_pipe_owners = owner_pipe_values
        .into_iter()
        .enumerate()
        .map(|(ordinal, value)| DesignParameterOwner {
            id: format!(
                "f3d:Design/BulkStream.dat:parameter-owner#{}",
                owner_pipe_record_indexes[ordinal]
            ),
            byte_offset: 0,
            frame_length: 103,
            class_tag: "342".into(),
            record_index: owner_pipe_record_indexes[ordinal],
            scope_record_index: scope.record_index,
            local_ordinal: ordinal as u32,
            evaluated_value: value,
            evaluated_value_offset: 10_000 + ordinal as u64,
            parameter_record_index: owner_pipe_record_indexes[ordinal] + 1,
            owned_ordinal: ordinal as u32,
            variant: None,
            companion_record_index: owner_pipe_record_indexes[ordinal] + 2,
        })
        .collect::<Vec<_>>();
    let mut owner_pipe_scope = scope.clone();
    owner_pipe_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    owner_pipe_scope.byte_offset = owner_pipe_start as u64;
    owner_pipe_scope.class_tag = "421".into();
    owner_pipe_scope.paired_class_tag = "257".into();
    owner_pipe_scope.kind = "Pipe".into();
    owner_pipe_scope.frame_length = 405;
    owner_pipe_scope.reference_members = owner_pipe_record_indexes.into();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &owner_pipe_scope,
            &owner_pipe_owners,
        ),
        Some(DesignPathFeatureConstruction::Pipe {
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: (owner_pipe_start + fixed_pipe_layout::OPERATION) as u64,
            section_shape: 1,
            section_shape_offset: (owner_pipe_start + fixed_pipe_layout::SECTION_SHAPE) as u64,
            filled: true,
            filled_offset: (owner_pipe_start + fixed_pipe_layout::FILLED) as u64,
            values: owner_pipe_values,
            record_indexes: owner_pipe_record_indexes,
            value_offsets: [10_000, 10_001, 10_002, 10_003],
        })
    );
    let mut wrong_owner_class = owner_pipe_owners.clone();
    wrong_owner_class[0].class_tag = "341".into();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &owner_pipe_scope,
            &wrong_owner_class,
        ),
        None
    );

    for (pair_ordinal, (class_tag, paired_class_tag)) in
        [("405", "259"), ("475", "260")].into_iter().enumerate()
    {
        let legacy_pipe_start = bytes.len();
        let mut legacy_pipe = vec![0; 383];
        legacy_pipe[legacy_pipe_layout::ZERO_RUN_9..legacy_pipe_layout::PREFIX_MARKER]
            .copy_from_slice(&[0; 9]);
        legacy_pipe[legacy_pipe_layout::PREFIX_MARKER] = legacy_pipe_layout::PREFIX_MARKER_VALUE;
        legacy_pipe[legacy_pipe_layout::ZERO_RUN_5..legacy_pipe_layout::OPERATION]
            .copy_from_slice(&[0; 5]);
        legacy_pipe[legacy_pipe_layout::OPERATION..legacy_pipe_layout::SECTION_SHAPE]
            .copy_from_slice(&4u32.to_le_bytes());
        legacy_pipe[legacy_pipe_layout::SECTION_SHAPE] = 1;
        legacy_pipe[legacy_pipe_layout::FILLED] = 1;
        bytes.extend_from_slice(&legacy_pipe);
        let legacy_scalar_start = bytes.len();
        let legacy_values: [f64; 4] = [1.0, 1.0, 0.6, 0.15];
        let first_record_index = 180 + pair_ordinal as u32 * 4;
        for (ordinal, value) in legacy_values.into_iter().enumerate() {
            let record_index = first_record_index + ordinal as u32;
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
        let mut legacy_scope = scope.clone();
        legacy_scope.byte_offset = legacy_pipe_start as u64;
        legacy_scope.class_tag = class_tag.into();
        legacy_scope.paired_class_tag = paired_class_tag.into();
        legacy_scope.kind = "Pipe".into();
        legacy_scope.frame_length = 383;
        legacy_scope.reference_members = (first_record_index..first_record_index + 4).collect();
        assert_eq!(
            exact_path_feature_construction(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                &legacy_scope,
                &[],
            ),
            Some(DesignPathFeatureConstruction::Pipe {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: (legacy_pipe_start + legacy_pipe_layout::OPERATION) as u64,
                section_shape: 1,
                section_shape_offset: (legacy_pipe_start + legacy_pipe_layout::SECTION_SHAPE)
                    as u64,
                filled: true,
                filled_offset: (legacy_pipe_start + legacy_pipe_layout::FILLED) as u64,
                values: legacy_values,
                record_indexes: [
                    first_record_index,
                    first_record_index + 1,
                    first_record_index + 2,
                    first_record_index + 3,
                ],
                value_offsets: std::array::from_fn(|ordinal| {
                    (legacy_scalar_start + ordinal * 111 + 40) as u64
                }),
            })
        );
    }

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
