// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::records::topology::DesignOperandRole;
use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

pub(super) fn continue_fixed_kind_operations(
    bytes: Vec<u8>,
    scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) {
    let (bytes, scope) = fixed_kind_edge_and_revolve_operations(bytes, scope, thicken_group);
    super::fixed_kind_path_operations::fixed_kind_path_operations(bytes, scope, thicken_group);
}

fn fixed_kind_edge_and_revolve_operations(
    mut bytes: Vec<u8>,
    scope: DesignParameterScope,
    thicken_group: &DesignConstructionOperandGroup,
) -> (Vec<u8>, DesignParameterScope) {
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
    draft_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Draft
                .try_into()
                .unwrap();
            draft.frame_length = 361;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![175, 176, 181, 182, 186, 190, 193]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let expected = Some(DesignDraftOperation {
        angle: crate::records::feature::DesignFiniteScalar::new(0.4).unwrap(),
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
    draft_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![181, 182, 186, 190, 193, 175, 176]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
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
    draft_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![175, 181, 182, 186, 190, 193]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );

    draft_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![175, 176, 181, 182, 186]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_draft_operation_with_owners(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &draft_scope,
            &[],
        ),
        None
    );
    draft_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![175, 176, 181, 182, 186, 190, 193]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

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
    fillet_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Fillet
                .try_into()
                .unwrap();
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![77, 50, 78, 79, 87, 88]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::feature::DesignFixedFilletGroup::try_new(
                Some(crate::records::feature::DesignFixedFilletScalar {
                    value: 1.0,
                    record_index: 77,
                    value_offset: (fillet_start + 40) as u64,
                }),
                crate::records::feature::DesignFixedFilletLaw::Variable {
                    start: crate::records::feature::DesignFixedFilletScalar {
                        value: 0.0,
                        record_index: 78,
                        value_offset: (fillet_start + 115 + 40) as u64
                    },
                    end: crate::records::feature::DesignFixedFilletScalar {
                        value: 0.65,
                        record_index: 79,
                        value_offset: (fillet_start + 230 + 40) as u64
                    },
                    intermediate: vec![crate::records::feature::DesignFixedFilletIntermediate {
                        radius: crate::records::feature::DesignFixedFilletScalar {
                            value: 0.4,
                            record_index: 87,
                            value_offset: (fillet_start + 345 + 40) as u64
                        },
                        parameter: crate::records::feature::DesignFixedFilletScalar {
                            value: 0.2,
                            record_index: 88,
                            value_offset: (fillet_start + 460 + 40) as u64
                        },
                    }],
                },
            )
            .unwrap()],
        })
    );
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![50, 77]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::feature::DesignFixedFilletGroup::try_new(
                None,
                crate::records::feature::DesignFixedFilletLaw::Constant(
                    crate::records::feature::DesignFixedFilletScalar {
                        value: 1.0,
                        record_index: 77,
                        value_offset: (fillet_start + 40) as u64
                    }
                ),
            )
            .unwrap()],
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
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![89]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope),
        Some(DesignFixedFilletParameters {
            groups: vec![crate::records::feature::DesignFixedFilletGroup::try_new(
                None,
                crate::records::feature::DesignFixedFilletLaw::Constant(
                    crate::records::feature::DesignFixedFilletScalar {
                        value: 0.5,
                        record_index: 89,
                        value_offset: (dynamic_scalar_at + 40) as u64
                    }
                ),
            )
            .unwrap()],
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
    fillet_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![92, 93, 94, 95]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let fixed =
        exact_fixed_fillet_parameters(&bytes, &IndexedRecordOffsets::build(&bytes), &fillet_scope)
            .expect("two constant-radius Fillet scalar groups");
    assert_eq!(fixed.groups.len(), 2);
    assert_eq!(
        fixed.groups[0]
            .law()
            .radii()
            .map(|scalar| scalar.value)
            .collect::<Vec<_>>(),
        [0.5]
    );
    assert_eq!(
        fixed.groups[1]
            .law()
            .radii()
            .map(|scalar| scalar.value)
            .collect::<Vec<_>>(),
        [0.25]
    );
    assert_eq!(
        fixed.groups[1]
            .tangency_weight()
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
    chamfer_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Chamfer
                .try_into()
                .unwrap();
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![86]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::EqualDistance {
            distance: crate::records::feature::DesignFixedChamferDistance {
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
    chamfer_scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![86, 96]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_fixed_chamfer_parameters(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &chamfer_scope,
            &[],
        ),
        Some(DesignFixedChamferParameters::TwoDistances {
            first: crate::records::feature::DesignFixedChamferDistance {
                value: 0.04,
                record_index: 86,
                value_offset: (chamfer_scalar_start + 40) as u64,
            },
            second: crate::records::feature::DesignFixedChamferDistance {
                value: 0.08,
                record_index: 96,
                value_offset: (second_chamfer_scalar_start + 40) as u64,
            },
        })
    );
    chamfer_scope.id = "f3d:Design/BulkStream.dat:scope#12".into();
    let indexed_owner =
        crate::records::DesignParameterOwner::try_from(crate::records::DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:parameter-owner#97".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: crate::records::DesignClassTag::try_from("292".to_owned()).unwrap(),
            record_index: 97,
            scope_record_index: chamfer_scope.record_index,
            local_ordinal: 0,
            evaluated_value: 0.04,
            evaluated_value_offset: 40,
            parameter_record_index: 98,
            owned_ordinal: 0,
            variant: Some(0),
            companion_record_index: 99,
        })
        .unwrap();
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
    revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = revolve_start as u64;
            draft.payload = crate::records::feature::DesignFeatureKind::Revolve
                .try_into()
                .unwrap();
            draft.frame_length = 386;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                200, 201, 202, 203, 1_779, 1_780, 204,
            ]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &revolve_scope,
        &[],
    );
    assert_eq!(
        revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: (revolve_start + 25) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(3.5).unwrap(),
                angle_record_index: 1_779,
                angle_offset: (revolve_scalar_start + 40) as u64,
                opposite_angle: Some(crate::records::Located {
                    value: 1_780,
                    offset: (revolve_scalar_start + 116 + 40) as u64
                }),
            }
        ))
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
    indexed_revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = indexed_revolve_start as u64;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    indexed_revolve_scope.class_tag =
        crate::records::DesignClassTag::try_from("407".to_owned()).unwrap();
    indexed_revolve_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();
    indexed_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 377;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                200, 201, 202, 203, 204, 205, 1_790, 1_791,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let indexed_angle =
        crate::records::DesignParameterOwner::try_from(crate::records::DesignParameterOwnerWire {
            id: indexed_revolve_scope.id.clone(),
            byte_offset: 5,
            frame_length: 104,
            class_tag: crate::records::DesignClassTag::try_from("372".to_owned()).unwrap(),
            record_index: indexed_angle_record_index,
            scope_record_index: indexed_revolve_scope.record_index,
            local_ordinal: 0,
            evaluated_value: std::f64::consts::TAU,
            evaluated_value_offset: 45,
            parameter_record_index: 1_791,
            owned_ordinal: 8,
            variant: Some(0),
            companion_record_index: 1_792,
        })
        .unwrap();
    let indexed_revolve_construction = exact_path_feature_construction(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &indexed_revolve_scope,
        std::slice::from_ref(&indexed_angle),
    );
    assert_eq!(
        indexed_revolve_construction,
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: (indexed_revolve_start + 21) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(std::f64::consts::TAU)
                    .unwrap(),
                angle_record_index: indexed_angle_record_index,
                angle_offset: 45,
                opposite_angle: None,
            }
        ))
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
    class403_scope.class_tag = crate::records::DesignClassTag::try_from("403".to_owned()).unwrap();
    class403_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();
    class403_scope
        .try_edit(|draft| {
            draft.byte_offset = class403_start as u64;
            draft.frame_length = 387;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                204,
                205,
                206,
                indexed_angle_record_index,
            ]);
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut class403_angle = indexed_angle.clone();
    {
        let mut wire = crate::records::DesignParameterOwnerWire::from(class403_angle.clone());
        wire.scope_record_index = class403_scope.record_index;
        wire.byte_offset = class403_start as u64;
        wire.evaluated_value_offset = (class403_start + 40) as u64;
        class403_angle = crate::records::DesignParameterOwner::try_from(wire).unwrap();
    }
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &class403_scope,
            std::slice::from_ref(&class403_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::Cut,
                operation_offset: (class403_start + 21) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(std::f64::consts::TAU)
                    .unwrap(),
                angle_record_index: indexed_angle_record_index,
                angle_offset: (class403_start + 40) as u64,
                opposite_angle: None,
            }
        ))
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
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.byte_offset = legacy_revolve_start as u64;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    legacy_revolve_scope.class_tag =
        crate::records::DesignClassTag::try_from("409".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("257".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 359;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                legacy_angle_record_index,
                204,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let legacy_angle =
        crate::records::DesignParameterOwner::try_from(crate::records::DesignParameterOwnerWire {
            id: legacy_revolve_scope.id.clone(),
            byte_offset: 15,
            frame_length: 104,
            class_tag: crate::records::DesignClassTag::try_from("372".to_owned()).unwrap(),
            record_index: legacy_angle_record_index,
            scope_record_index: legacy_revolve_scope.record_index,
            local_ordinal: 0,
            evaluated_value: std::f64::consts::TAU,
            evaluated_value_offset: 55,
            parameter_record_index: 1_801,
            owned_ordinal: 8,
            variant: Some(0),
            companion_record_index: 1_802,
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: (legacy_revolve_start + 25) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(std::f64::consts::TAU)
                    .unwrap(),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                opposite_angle: None,
            }
        ))
    );
    legacy_revolve_scope.class_tag =
        crate::records::DesignClassTag::try_from("323".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("260".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 381;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                legacy_angle_record_index,
                200,
                201,
                202,
                203,
                204,
                205,
                206,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: (legacy_revolve_start + 25) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(std::f64::consts::TAU)
                    .unwrap(),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                opposite_angle: None,
            }
        ))
    );
    legacy_revolve_scope.class_tag =
        crate::records::DesignClassTag::try_from("385".to_owned()).unwrap();
    legacy_revolve_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("262".to_owned()).unwrap();
    legacy_revolve_scope
        .try_edit(|draft| {
            draft.frame_length = 369;
            draft.reference_members = crate::records::ReferenceRun::unlocated(vec![
                200,
                201,
                202,
                203,
                legacy_angle_record_index,
                204,
            ]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    assert_eq!(
        exact_path_feature_construction(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &legacy_revolve_scope,
            std::slice::from_ref(&legacy_angle),
        ),
        Some(DesignPathFeatureConstruction::Revolve(
            crate::records::feature::DesignRevolveConstruction {
                operation: DesignExtrudeOperation::NewBody,
                operation_offset: (legacy_revolve_start + 25) as u64,
                angle: crate::records::feature::DesignPositiveScalar::new(std::f64::consts::TAU)
                    .unwrap(),
                angle_record_index: legacy_angle_record_index,
                angle_offset: 55,
                opposite_angle: None,
            }
        ))
    );
    revolve_scope.id = "stream:scope".into();
    {
        let value = revolve_construction;
        revolve_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let mut revolve_profile = thicken_group.clone();
    revolve_profile.id = "stream:profile".into();
    revolve_profile.scope_record_index = revolve_scope.record_index;
    revolve_profile.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    let mut revolve_axis = revolve_profile.clone();
    revolve_axis.id = "stream:axis".into();
    revolve_axis.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X21,
    );
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
    {
        let value = indexed_revolve_construction;
        indexed_revolve_scope
            .try_edit(|draft| {
                draft.payload =
                    value.map_or_else(|| draft.payload.kind().try_into().unwrap(), Into::into);
            })
            .unwrap();
    }
    let mut indexed_profile = thicken_group.clone();
    indexed_profile.id = "stream:indexed-profile".into();
    indexed_profile.scope_record_index = indexed_revolve_scope.record_index;
    indexed_profile.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::PROFILE);
    let mut indexed_axis = indexed_profile.clone();
    indexed_axis.id = "stream:indexed-axis".into();
    indexed_axis.record_index = 899;
    indexed_axis
        .try_set_members(vec![crate::records::Located {
            value: 900,
            offset: indexed_axis.members()[0].offset,
        }])
        .unwrap();
    indexed_axis.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X21,
    );
    let mut indexed_bodies = indexed_profile.clone();
    indexed_bodies.id = "stream:indexed-bodies".into();
    indexed_bodies.record_index = 901;
    indexed_bodies.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let mut axis_selection = crate::records::topology::DesignEntitySelectionOperand::try_new(
        crate::records::topology::DesignEntitySelectionOperandDraft {
            id: "stream:indexed-axis-selection".into(),
            scope_record_index: indexed_revolve_scope.record_index,
            group_record_index: indexed_axis.record_index,
            group_member_ordinal: 0,
            record_index: 900,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("377".to_owned()).unwrap(),
            asset_id: crate::records::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 0,
            context_id: crate::records::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 0,
            identity_record_index: 903,
            identity_record_offset: 0,
            primary_identity: 100,
            primary_identity_offset: 29,
            secondary: Some(crate::records::DesignSecondaryIdentity {
                identity: crate::records::Located {
                    value: 104,
                    offset: 37,
                },
                curve_identity: None,
            }),
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: 904,
            next_byte_offset: 45,
        },
    )
    .unwrap();
    let axis_placement = DesignSketchPlacement {
        frame: crate::records::DesignSketchFrame::new(
            0,
            crate::records::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),

        id: "stream:indexed-axis-placement".into(),
        scope_record_index: Some(10),
        entity_id: crate::records::DesignEntityId::try_from("Sketch_100".to_owned())
            .expect("valid entity ID"),

        visibility: None,

        class_tag: crate::records::DesignClassTag::try_from("305".to_owned()).unwrap(),
        record_index: 904,

        paired_class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
    };
    let axis_curve = SketchCurveIdentity {
        id: "stream:indexed-axis-curve".into(),
        record_index: 905,
        owner_reference: Some(100),
        class_tag: crate::records::DesignClassTag::try_from("450".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(104).unwrap(),
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
        Some(FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            op: cadmpeg_ir::features::BooleanOp::Cut,
        })) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(1.0, 2.0, 3.0)
                && axis.direction == Vector3::new(0.0, -1.0, 0.0))
    ));
    let mut draft = axis_selection.into_draft();
    draft.secondary = None;
    draft.primary_identity_offset = draft.identity_record_offset + 21;
    draft.next_byte_offset = draft.identity_record_offset + 29;
    axis_selection =
        crate::records::topology::DesignEntitySelectionOperand::try_new(draft).unwrap();
    axis_selection.historical_face_candidates = vec![
        crate::records::topology::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::HistoricalBinding {
                kind: crate::records::topology::AsmHistoricalEntityKind::Face,
                entity_ref: 40,
                state_ids: vec![1],
            },
            face_slot: 40,
        },
    ];
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
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_none()
    ));
    let mut feature = cadmpeg_ir::features::Feature {
        id: crate::ids::neutral_feature_id(&indexed_revolve_scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(historical_definition),
        native_ref: Some(indexed_revolve_scope.id.clone()),
    };
    let surface_id =
        cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#53").expect("identity grammar");
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile.clone(), indexed_axis.clone()],
        std::slice::from_ref(&axis_selection),
        &[],
        &[cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#40").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#1").expect("identity grammar"),
            surface: surface_id.clone(),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        }],
        &[cadmpeg_ir::geometry::Surface {
            id: surface_id,
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::PlaneSurface::try_new(
                    Point3::new(4.0, 5.0, 6.0),
                    Vector3::new(0.0, 0.0, -2.0).unit().unwrap(),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            source_object: None,
        }],
    )
    .unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(4.0, 5.0, 6.0)
                && axis.direction == Vector3::new(0.0, 0.0, -1.0))
    ));

    let face_axis_operand =
        DesignFaceOperand::try_new(crate::records::topology::DesignFaceOperandDraft {
            id: "stream:indexed-face-axis".into(),
            scope_record_index: indexed_revolve_scope.record_index,
            scope_reference_ordinal: 2,
            group: Some(crate::records::topology::DesignOperandGroup {
                group_record_index: indexed_axis.record_index,
                group_member_ordinal: 0,
            }),
            record_index: 900,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
            paired_byte_offset: 16,
            paired_class_tag: crate::records::DesignClassTag::try_from("262".to_owned()).unwrap(),
            recipe_record_index: 903,
            recipe_record_byte_offset: 32,
            recipe_id: "stream:indexed-face-axis-recipe".into(),
            recipe_prefix_offset: 43,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_kind: ConstructionRecipeKind::Face,
            recipe_program_offset: 0,
            recipe_program: vec![0, -1],

            recipe_nodes: Vec::new(),
            candidate_faces: vec![
                cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-a").expect("identity grammar"),
                cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-b").expect("identity grammar"),
            ],
            unreferenced_candidate_faces: Vec::new(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 905,
            next_byte_offset: 200,
        })
        .unwrap();
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
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            face_axis_definition.clone(),
            (feature.clone()).evaluation.outputs().clone(),
        ),
        ..feature.clone()
    };
    let axis_faces = [
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-a").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#axis")
                .expect("identity grammar"),
            surface: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-a")
                .expect("identity grammar"),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        },
        cadmpeg_ir::topology::Face {
            id: cadmpeg_ir::ids::FaceId::mint("test:model:face#axis-b").expect("identity grammar"),
            shell: cadmpeg_ir::ids::ShellId::mint("test:model:shell#axis")
                .expect("identity grammar"),
            surface: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-b")
                .expect("identity grammar"),
            sense: cadmpeg_ir::topology::Sense::Forward,
            loops: Vec::new().into(),
            name: None,
            color: None,
            tolerance: None,
        },
    ];
    let mut axis_surfaces = [
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-a")
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::CylinderSurface::try_new(
                        Point3::new(1.0, 2.0, 3.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        4.0,
                    )
                    .unwrap(),
                ),
            ),
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#axis-b")
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::CylinderSurface::try_new(
                        Point3::new(1.0, 2.0, 8.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                        5.0,
                    )
                    .unwrap(),
                ),
            ),
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
    )
    .unwrap();
    assert!(matches!(
        face_axis_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_some_and(|axis|
            axis.origin == Point3::new(1.0, 2.0, 3.0)
                && axis.direction == Vector3::new(0.0, 0.0, 1.0))
    ));
    let mut conflicting_face_axis_feature = cadmpeg_ir::features::Feature {
        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            face_axis_definition,
            (feature).evaluation.outputs().clone(),
        ),
        ..feature
    };
    axis_surfaces[1].geometry =
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                Point3::new(2.0, 2.0, 8.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
            )
            .unwrap(),
        ));
    crate::design::feature_project::bind_revolve_face_axes(
        std::slice::from_mut(&mut conflicting_face_axis_feature),
        std::slice::from_ref(&indexed_revolve_scope),
        &[indexed_profile, indexed_axis],
        &[],
        std::slice::from_ref(&face_axis_operand),
        &axis_faces,
        &axis_surfaces,
    )
    .unwrap();
    assert!(matches!(
        conflicting_face_axis_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Revolve {
            ref construction,
            ..
        }) if construction.axis().is_none()
    ));

    (bytes, scope)
}
