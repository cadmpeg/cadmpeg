// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;
use crate::design::decode::operands::RecordFrame;
use crate::records::topology::DesignOperandRole;

#[test]
fn localized_edge_treatment_group_retention_is_language_independent() {
    use crate::records::feature::DesignFeatureKind as Kind;
    for kind in [
        Kind::Conge,
        Kind::Abrundung,
        Kind::Arredondamento,
        Kind::Chanfrein,
    ] {
        assert!(!construction_operand_group_is_retained(Some(&kind), false));
        assert!(construction_operand_group_is_retained(Some(&kind), true));
    }
    for kind in [
        Kind::Fillet,
        Kind::Chamfer,
        Kind::Extrusion,
        Kind::try_from("unknown".to_owned()).expect("native family name"),
    ] {
        assert!(construction_operand_group_is_retained(Some(&kind), false));
    }
    assert!(construction_operand_group_is_retained(None, false));
}

#[test]
fn construction_operand_groups_have_exact_counted_and_direct_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let scope = DesignParameterScope::try_new(
        crate::records::feature::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:scope#12".into(),
            byte_offset: 1000,
            class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
            record_index: 12,
            frame_length: 200,
            kind_offset: 1100,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,

            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: 1080,
            reference_members: crate::records::ReferenceRun::from_columns(
                vec![100, 200, 201],
                vec![1085, 1096, 1107],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::DesignFeatureKind::Extrude
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: crate::records::DesignClassTag::try_from("261".to_owned()).unwrap(),
            paired_byte_offset: 1200,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("332".to_owned()).unwrap(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"332", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [200u32, 201] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&300u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&0x0000_0008_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&180u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&102u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[1, 1, 0, 1]);
    bytes.extend_from_slice(&101u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.push(1);
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let paired_at = bytes.len();
    header(&mut bytes, *b"259", 100);

    let group = parse_construction_operand_group(&bytes, &scope, 0, &RecordFrame::from(&record))
        .complete()
        .expect("counted Extrude operand group");
    assert_eq!(
        group
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>(),
        [200, 201]
    );
    assert_eq!(
        group
            .members()
            .iter()
            .map(|member| member.offset)
            .collect::<Vec<_>>(),
        [26, 37]
    );
    assert_eq!(group.role(), DesignOperandRole::BODIES_B);
    assert_eq!(group.extrude_role(), Some(DesignExtrudeOperandRole::Bodies));
    assert_eq!(group.frame.member_count_offset, 21);
    assert!(group.frame.auxiliary_records.is_empty());
    assert_eq!(
        group
            .frame
            .trailing_records()
            .iter()
            .map(|record| record.value)
            .collect::<Vec<_>>(),
        [300]
    );
    assert_eq!(group.frame.opaque_index.get(), 180);
    assert_eq!(group.frame.opaque_scalar(), 0.125);
    assert!(group.frame.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    let mut whole_body_bytes = bytes.clone();
    whole_body_bytes[group.role_offset() as usize..group.role_offset() as usize + 8]
        .copy_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
    let whole_body =
        parse_construction_operand_group(&whole_body_bytes, &scope, 0, &RecordFrame::from(&record))
            .complete()
            .expect("counted Extrude whole-body group");
    assert_eq!(whole_body.role(), DesignOperandRole::BODIES_A);
    assert_eq!(
        whole_body.extrude_role(),
        Some(DesignExtrudeOperandRole::Bodies)
    );

    let mut flagged = bytes[..11].to_vec();
    flagged.extend_from_slice(&[0; 9]);
    flagged.push(1);
    flagged.extend_from_slice(&1u32.to_le_bytes());
    for value in [
        b"DcFeatureOperationIdFlag".as_slice(),
        b"IntrinsicMetaTypeuint64".as_slice(),
    ] {
        flagged.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        flagged.extend_from_slice(value);
    }
    flagged.extend_from_slice(&445u64.to_le_bytes());
    let flagged_count_at = flagged.len();
    flagged.extend_from_slice(&bytes[21..]);
    let flagged =
        parse_construction_operand_group(&flagged, &scope, 0, &RecordFrame::from(&record))
            .complete()
            .expect("operation-flagged counted operand group");
    assert_eq!(flagged.frame.member_count_offset, flagged_count_at as u64);
    assert_eq!(
        flagged
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>(),
        [200, 201]
    );
    assert_eq!(flagged.role(), DesignOperandRole::BODIES_B);

    let mut start_face_bytes = bytes.clone();
    start_face_bytes[group.role_offset() as usize..group.role_offset() as usize + 8]
        .copy_from_slice(&0x0000_0005_0000_0000u64.to_le_bytes());
    let retained_role_five =
        parse_construction_operand_group(&start_face_bytes, &scope, 0, &RecordFrame::from(&record))
            .complete()
            .expect("counted Extrude retained role-five group");
    assert_eq!(retained_role_five.extrude_role(), None);

    let mut from_face_scope = scope.clone();
    if let crate::records::feature::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusao(slot) =
        from_face_scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::Cut,
                operation_offset: 1028,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [1, 0],
                side_extent_discriminator_offsets: [1077, 1090],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedDistance,
                direction_face_extend_offsets: [1032, 1036],
                direction_reversed: false,
                direction_reversed_offset: 1040,
                solid_operation: true,
                solid_operation_offset: 1041,
                start: DesignExtrudeStart::FromFace,
                start_offset: 1042,
            });
    }
    let mut start_face = parse_construction_operand_group(
        &start_face_bytes,
        &from_face_scope,
        0,
        &RecordFrame::from(&record),
    )
    .complete()
    .expect("counted Extrude start-face group");
    assert_eq!(start_face.role(), DesignOperandRole::ROLE_0X5);
    assert_eq!(start_face.extrude_role(), None);
    crate::design::decode::operands::assign_extrude_face_roles(
        &from_face_scope,
        std::slice::from_mut(&mut start_face),
    );
    assert_eq!(
        start_face.extrude_role(),
        Some(DesignExtrudeOperandRole::Faces(
            DesignExtrudeFaceRole::Start
        ))
    );

    let mut to_face_scope = from_face_scope.clone();
    if let crate::records::feature::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusao(slot) =
        to_face_scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::ReferenceAware {
                reference: None,
                operation: DesignExtrudeOperation::Cut,
                operation_offset: 1028,
                direction_face_extend_values: [1, 2],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [1077, 1090],
                first_side_target_ordinal: None,
                extent: DesignExtrudeExtent::OneSidedToFace,
                direction_face_extend_offsets: [1032, 1036],
                direction_reversed: false,
                direction_reversed_offset: 1040,
                solid_operation: true,
                solid_operation_offset: 1041,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 1042,
            });
    }
    let mut to_face_bytes = bytes.clone();
    to_face_bytes[group.role_offset() as usize..group.role_offset() as usize + 8]
        .copy_from_slice(&0x0000_0012_0000_0000u64.to_le_bytes());
    let mut legacy_to_face = parse_construction_operand_group(
        &to_face_bytes,
        &to_face_scope,
        0,
        &RecordFrame::from(&record),
    )
    .complete()
    .expect("counted Extrude legacy to-face group");
    assert_eq!(legacy_to_face.role(), DesignOperandRole::ROLE_0X12);
    assert_eq!(legacy_to_face.extrude_role(), None);
    crate::design::decode::operands::assign_extrude_face_roles(
        &to_face_scope,
        std::slice::from_mut(&mut legacy_to_face),
    );
    assert_eq!(
        legacy_to_face.extrude_role(),
        Some(DesignExtrudeOperandRole::Faces(
            DesignExtrudeFaceRole::Termination
        ))
    );

    let tail_at = 11 + 10 + 4 + 2 * 11;
    let mut flagless = bytes[..tail_at + 62].to_vec();
    flagless.extend_from_slice(&[0; 2]);
    flagless.push(1);
    flagless.extend_from_slice(&101u32.to_le_bytes());
    flagless.extend_from_slice(&[0; 7]);
    flagless.push(1);
    flagless.extend_from_slice(&12u32.to_le_bytes());
    flagless.extend_from_slice(&[0; 6]);
    let flagless_paired_at = flagless.len();
    header(&mut flagless, *b"259", 100);
    let flagless =
        parse_construction_operand_group(&flagless, &scope, 0, &RecordFrame::from(&record))
            .complete()
            .expect("flagless counted operand group");
    assert_eq!(
        flagless
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>(),
        [200, 201]
    );
    assert_eq!(flagless.role(), DesignOperandRole::BODIES_B);
    assert!(!flagless.frame.variant);
    assert_eq!(
        flagless.paired_byte_offset,
        u64::try_from(flagless_paired_at).unwrap()
    );

    let mut bombed = bytes.clone();
    bombed[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        parse_construction_operand_group(&bombed, &scope, 0, &RecordFrame::from(&record)),
        ConstructionOperandGroupParse::NotAGroup
    ));

    // A record that opens the grammar but whose tail names another record is a
    // group this reader cannot read, not a reference member that is not a group.
    let mut truncated = bytes.clone();
    let tail_at = truncated.len() - 40;
    truncated[tail_at..].fill(0x5a);
    assert!(matches!(
        parse_construction_operand_group(&truncated, &scope, 0, &RecordFrame::from(&record)),
        ConstructionOperandGroupParse::Unclosed
    ));

    // Both optional references after the member run are present and the counted
    // identity run is empty: the shape a fixed-offset reader cannot reach.
    let mut auxiliary = Vec::new();
    header(&mut auxiliary, *b"283", 100);
    auxiliary.extend_from_slice(&[0; 10]);
    auxiliary.extend_from_slice(&1u32.to_le_bytes());
    for record_index in [109u32, 103, 106] {
        auxiliary.push(1);
        auxiliary.extend_from_slice(&record_index.to_le_bytes());
        auxiliary.extend_from_slice(&[0; 6]);
    }
    auxiliary.extend_from_slice(&0u32.to_le_bytes());
    auxiliary.extend_from_slice(&0x0000_0011_0000_0000u64.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 10]);
    auxiliary.extend_from_slice(&31_003u32.to_le_bytes());
    auxiliary.extend_from_slice(&0.25f64.to_le_bytes());
    auxiliary.extend_from_slice(&31_003u32.to_le_bytes());
    auxiliary.push(1);
    auxiliary.extend_from_slice(&102u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 6]);
    auxiliary.extend_from_slice(&[0; 2]);
    auxiliary.push(1);
    auxiliary.extend_from_slice(&101u32.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 7]);
    auxiliary.push(1);
    auxiliary.extend_from_slice(&scope.record_index.to_le_bytes());
    auxiliary.extend_from_slice(&[0; 6]);
    let auxiliary_paired_at = auxiliary.len();
    header(&mut auxiliary, *b"259", 100);
    let auxiliary_record = DesignRecordHeader {
        class_tag: crate::records::DesignClassTag::try_from("283".to_owned()).unwrap(),
        ..record.clone()
    };
    let mut auxiliary = parse_construction_operand_group(
        &auxiliary,
        &scope,
        0,
        &RecordFrame::from(&auxiliary_record),
    )
    .complete()
    .expect("Extrude face group carrying both optional references");
    assert_eq!(
        auxiliary
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>(),
        [109]
    );
    assert_eq!(
        auxiliary
            .members()
            .iter()
            .map(|member| member.offset)
            .collect::<Vec<_>>(),
        [26]
    );
    assert_eq!(
        auxiliary
            .frame
            .auxiliary_records
            .iter()
            .map(|record| record.value)
            .collect::<Vec<_>>(),
        [103, 106]
    );
    assert_eq!(
        auxiliary
            .frame
            .auxiliary_records
            .iter()
            .map(|record| record.offset)
            .collect::<Vec<_>>(),
        [37, 48]
    );
    assert!(auxiliary.frame.trailing_records().is_empty());
    assert_eq!(auxiliary.role(), DesignOperandRole::FACES);
    assert_eq!(auxiliary.extrude_role(), None);
    crate::design::decode::operands::assign_extrude_face_roles(
        &scope,
        std::slice::from_mut(&mut auxiliary),
    );
    assert_eq!(
        auxiliary.extrude_role(),
        Some(DesignExtrudeOperandRole::Faces(
            DesignExtrudeFaceRole::Termination
        ))
    );
    assert_eq!(auxiliary.paired_byte_offset, auxiliary_paired_at as u64);

    let mut split_scope = scope.clone();
    split_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::SplitFace
                .try_into()
                .unwrap();
            draft.frame_length = 334;
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                vec![100, 200, 201, 400, 500],
                vec![1085, 1096, 1107, 1118, 1129],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut tool_group = group.clone();
    tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    tool_group.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X21,
    );
    let mut target_group = group.clone();
    target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    target_group.record_index = 400;
    target_group.scope_reference_ordinal = 3;
    target_group
        .try_set_members(vec![crate::records::Located {
            value: 500,
            offset: 1129,
        }])
        .unwrap();
    target_group.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X10,
    );
    let split_groups = [tool_group, target_group];
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&split_scope),
        &split_groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::SplitFace {
            targets: cadmpeg_ir::features::FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool),
            ),
        } if targets.ends_with("#400") && tool.ends_with("#100")
    ));

    let mut compact_split_scope = split_scope.clone();
    compact_split_scope.class_tag =
        crate::records::DesignClassTag::try_from("418".to_owned()).unwrap();
    compact_split_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("266".to_owned()).unwrap();
    compact_split_scope
        .try_edit(|draft| {
            draft.frame_length = 330;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (compact_features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&compact_split_scope),
        &split_groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        compact_features[0].evaluation.definition(),
        FeatureDefinition::SplitFace { .. }
    ));

    let mut first_plane = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#601",
        crate::records::feature::DesignFeatureKind::WorkPlane,
        601,
    );
    first_plane.feature_ordinal = std::num::NonZeroU32::new(1).expect("nonzero ordinal");
    first_plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, -0.8],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    let mut second_plane = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#701",
        crate::records::feature::DesignFeatureKind::WorkPlane,
        701,
    );
    second_plane.feature_ordinal = std::num::NonZeroU32::new(2).expect("nonzero ordinal");
    second_plane.with_work_plane_transform(
        [
            [1.0, 0.0, 0.0, -1.4],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap(),
    );
    compact_split_scope.feature_ordinal = std::num::NonZeroU32::new(3).expect("nonzero ordinal");
    let plane_selection = |record_index, group_member_ordinal, primary_identity| {
        crate::records::topology::DesignEntitySelectionOperand::try_new(
            crate::records::topology::DesignEntitySelectionOperandDraft {
                id: format!(
                    "f3d:Design/BulkStream.dat:design-entity-selection-operand#{record_index}"
                ),
                scope_record_index: compact_split_scope.record_index,
                group_record_index: split_groups[0].record_index,
                group_member_ordinal,
                record_index,
                byte_offset: 0,
                class_tag: crate::records::DesignClassTag::try_from("372".to_owned()).unwrap(),
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
                identity_record_index: record_index + 3,
                identity_record_offset: 0,
                primary_identity,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: None,
                next_record_index: record_index + 4,
                next_byte_offset: 29,
            },
        )
        .unwrap()
    };
    let plane_selections = [plane_selection(200, 0, 600), plane_selection(201, 1, 700)];
    let expected_planes = [
        crate::ids::neutral_feature_id(&first_plane),
        crate::ids::neutral_feature_id(&second_plane),
    ];
    let plane_scopes = vec![first_plane, second_plane, compact_split_scope.clone()];
    let plane_timeline = DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        crate::records::DesignTimelineFrame::test_items(
            0,
            plane_scopes
                .iter()
                .map(|scope| crate::records::Located {
                    value: u64::from(scope.record_index),
                    offset: 0,
                })
                .collect(),
        ),
        crate::records::DesignClassTag::try_from("256".to_owned()).unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    )
    .unwrap();
    let (plane_features, _) = project_parameter_design_with_edge_identities(
        &crate::design::feature_project::ProjectInputs {
            native: &[],
            owners: &[],
            scopes: &plane_scopes,
            timelines: std::slice::from_ref(&plane_timeline),
            construction_groups: &split_groups,
            fillet_radius_groups: &[],
            edge_operands: &[],
            edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[],
            entity_selection_operands: &plane_selections,
            curve_identities: &[],
            face_operands: &[],
            body_recipe_operands: &[],
            legacy_loft_body_carriers: &[],
            placements: &[],
            body_bindings: &[],
            component_naming_spaces: &[],
            histories: &[],
        },
    )
    .expect("exact synthetic feature timeline");
    let plane_split = plane_features
        .iter()
        .find(|feature| feature.source_tag.as_deref() == Some("SplitFace"))
        .expect("projected SplitFace");
    assert!(matches!(
        plane_split.evaluation.definition(),
        FeatureDefinition::SplitFace {
            tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
            ..
        } if planes[..] == expected_planes
    ));
    assert_eq!(plane_split.dependencies.as_slice(), expected_planes);

    compact_split_scope.class_tag =
        crate::records::DesignClassTag::try_from("375".to_owned()).unwrap();
    let (mismatched_features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&compact_split_scope),
        &split_groups,
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        mismatched_features[0].evaluation.definition(),
        FeatureDefinition::Native { .. }
    ));

    let mut split_body_scope = scope.clone();
    split_body_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::Split
                .try_into()
                .unwrap();
            draft.frame_length = 325;
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                vec![100, 200, 400, 500],
                vec![1085, 1096, 1107, 1118],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset = *draft.reference_members.offsets().next().unwrap() - 5;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut split_tool_group = group.clone();
    split_tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    split_tool_group.record_index = 100;
    split_tool_group.scope_reference_ordinal = 0;
    split_tool_group
        .try_set_members(vec![crate::records::Located {
            value: 200,
            offset: 1096,
        }])
        .unwrap();
    split_tool_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X9);
    let mut split_target_group = group.clone();
    split_target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    split_target_group.record_index = 400;
    split_target_group.scope_reference_ordinal = 2;
    split_target_group
        .try_set_members(vec![crate::records::Located {
            value: 500,
            offset: 1118,
        }])
        .unwrap();
    split_target_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    let split_tool = DesignFaceOperand::try_new(crate::records::topology::DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:face-operand#200".into(),
        scope_record_index: split_body_scope.record_index,
        scope_reference_ordinal: 1,
        group: Some(crate::records::topology::DesignOperandGroup {
            group_record_index: 100,
            group_member_ordinal: 0,
        }),
        record_index: 200,
        byte_offset: 1200,
        class_tag: crate::records::DesignClassTag::try_from("297".to_owned()).unwrap(),
        paired_byte_offset: 1250,
        paired_class_tag: crate::records::DesignClassTag::try_from("259".to_owned()).unwrap(),
        recipe_record_index: 203,
        recipe_record_byte_offset: 1300,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#1300".into(),
        recipe_prefix_offset: 1311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1350,
        recipe_program: vec![0, -1],

        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 204,
        next_byte_offset: 1411,
    })
    .unwrap();
    let split_groups = [split_target_group.clone(), split_tool_group.clone()];
    assert!(matches!(
        project_split(
            &split_body_scope,
            &split_groups,
            std::slice::from_ref(&split_tool)
        ),
        Some(FeatureDefinition::SplitBody {
            targets: cadmpeg_ir::features::BodySelection::Native(ref targets),
            tools: cadmpeg_ir::features::FaceSelection::Native(ref tool),
        }) if targets.ends_with("#400") && tool.ends_with("#200")
    ));

    let mut historical_split_scope = split_body_scope.clone();
    historical_split_scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut historical_split_tool = split_tool.clone();
    historical_split_tool.preceding_candidate_faces =
        vec![FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")];
    historical_split_tool.recipe_references = vec![DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "23".into(),
        token_offset: 0,
        design_reference: 332,
        design_reference_offset: 0,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: vec![
            FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")
        ],
        alternate_selector_edges: Vec::new(),
    }];
    assert!(matches!(
        project_split(
            &historical_split_scope,
            &split_groups,
            std::slice::from_ref(&historical_split_tool)
        ),
        Some(FeatureDefinition::SplitBody {
            tools: FaceSelection::Historical { faces, native, .. },
            ..
        }) if faces.len() == 1 && native.as_str() == historical_split_tool.id
    ));

    let mut multiple_targets_scope = split_body_scope.clone();
    multiple_targets_scope
        .try_edit(|draft| {
            draft.frame_length = 358;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 400, 500, 501]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut multiple_targets = split_target_group.clone();
    multiple_targets
        .try_set_members(
            vec![500, 501]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
        )
        .unwrap();
    assert!(matches!(
        project_split(
            &multiple_targets_scope,
            &[split_tool_group.clone(), multiple_targets],
            std::slice::from_ref(&split_tool)
        ),
        Some(FeatureDefinition::SplitBody { .. })
    ));

    let mut construction_tool_scope = split_body_scope.clone();
    construction_tool_scope
        .try_edit(|draft| {
            draft.frame_length = 347;
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 201, 400, 500]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut construction_tool = split_tool_group.clone();
    construction_tool.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X21,
    );
    construction_tool
        .try_set_members(
            vec![200, 201]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
        )
        .unwrap();
    split_target_group.scope_reference_ordinal = 3;
    assert!(matches!(
        project_split(
            &construction_tool_scope,
            &[split_target_group.clone(), construction_tool],
            &[]
        ),
        Some(FeatureDefinition::SplitBody {
            tools: cadmpeg_ir::features::FaceSelection::Native(ref tool),
            ..
        }) if tool.ends_with("#100")
    ));
    split_target_group.scope_reference_ordinal = 2;

    let mut invalid_groups = Vec::new();
    invalid_groups.push(vec![split_target_group.clone()]);
    let mut oversized_tool = split_tool_group.clone();
    oversized_tool
        .try_set_members(
            vec![200, 201, 202, 203]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
        )
        .unwrap();
    invalid_groups.push(vec![oversized_tool, split_target_group.clone()]);
    for mutate in 0..4 {
        let mut tool = split_tool_group.clone();
        match mutate {
            0 => tool.scope_reference_ordinal = 1,
            1 => tool.record_index = 101,
            2 => {
                tool.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                );
            }
            3 => {
                tool.try_set_members(vec![crate::records::Located {
                    value: 201,
                    offset: tool.members()[0].offset,
                }])
                .unwrap();
            }
            _ => unreachable!(),
        }
        invalid_groups.push(vec![tool, split_target_group.clone()]);
    }
    for mutate in 0..4 {
        let mut target = split_target_group.clone();
        match mutate {
            0 => target.scope_reference_ordinal = 3,
            1 => target.record_index = 401,
            2 => {
                target.operand_role =
                    crate::records::topology::DesignConstructionOperandRole::Other(
                        DesignOperandRole::ROLE_0X5,
                    );
            }
            3 => {
                target
                    .try_set_members(vec![crate::records::Located {
                        value: 501,
                        offset: target.members()[0].offset,
                    }])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        invalid_groups.push(vec![split_tool_group.clone(), target]);
    }
    assert!(invalid_groups.iter().all(|groups| project_split(
        &split_body_scope,
        groups,
        std::slice::from_ref(&split_tool)
    )
    .is_none()));
    let mut nonterminal_tool = split_tool.clone();
    nonterminal_tool.recipe_program = vec![0, -1, 2];
    assert!(project_split(
        &split_body_scope,
        &split_groups,
        std::slice::from_ref(&nonterminal_tool)
    )
    .is_none());

    let mut delete_scope = scope.clone();
    delete_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::DeleteFace
                .try_into()
                .unwrap();
            draft.frame_length = 258;
            draft.kind_offset = 1161;
            draft.reference_members = crate::records::ReferenceRun::from_columns(
                vec![100, 200],
                vec![1085, 1096],
                "reference_members",
            )
            .unwrap();
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut delete_group = group.clone();
    delete_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    delete_group
        .try_set_members(vec![crate::records::Located {
            value: 200,
            offset: 1096,
        }])
        .unwrap();
    delete_group.operand_role = crate::records::topology::DesignConstructionOperandRole::Other(
        DesignOperandRole::ROLE_0X10,
    );
    let mut delete_face_operand = split_tool.clone();
    delete_face_operand.id = "f3d:Design/BulkStream.dat:face-operand#200".into();
    delete_face_operand.scope_record_index = delete_scope.record_index;
    delete_face_operand.scope_reference_ordinal = 1;
    delete_face_operand.group = Some(crate::records::topology::DesignOperandGroup {
        group_record_index: delete_group.record_index,
        group_member_ordinal: 0,
    });
    let mut draft = delete_face_operand.into_draft();
    draft.record_index = 200;
    draft.recipe_record_index = draft.record_index + 3;
    delete_face_operand = crate::records::topology::DesignFaceOperand::try_new(draft).unwrap();
    delete_face_operand.resolved_face_slots = vec![7];
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(delete_group.id.clone()),
            heal: true,
        }
    );
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        std::slice::from_ref(&delete_face_operand),
        &[],
    );
    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved {
                faces: vec![FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")],
                native: delete_group.id.clone(),
            },
            heal: true,
        }
    );
    delete_scope
        .try_edit(|draft| {
            draft.frame_length = 263;
            draft.kind_offset = 1165;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace { heal: true, .. }
    ));
    delete_scope
        .try_edit(|draft| {
            draft.frame_length += 1;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::DeleteFace,
            ..
        }
    ));

    let mut surface_scope = delete_scope.clone();
    let reference_bytes = 11 * surface_scope.reference_members().len() as u64;
    surface_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::SurfaceDeleteFace
                .try_into()
                .unwrap();
            draft.frame_length = 250 + reference_bytes;
            draft.kind_offset = draft.byte_offset + 140 + reference_bytes;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace {
            faces: cadmpeg_ir::features::FaceSelection::Native(delete_group.id.clone()),
            heal: false,
        }
    );
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        std::slice::from_ref(&delete_face_operand),
        &[],
    );
    assert_eq!(
        *features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved {
                faces: vec![FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")],
                native: delete_group.id.clone(),
            },
            heal: false,
        }
    );
    surface_scope
        .try_edit(|draft| {
            draft.frame_length = 251 + reference_bytes;
            draft.kind_offset = draft.byte_offset + 139 + reference_bytes;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::DeleteFace { heal: false, .. }
    ));
    surface_scope
        .try_edit(|draft| {
            draft.frame_length = 236 + reference_bytes;
            draft.kind_offset = draft.byte_offset + 139 + reference_bytes;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::SurfaceDeleteFace,
            ..
        }
    ));

    for (class_tag, paired_class_tag, base_frame, base_kind) in [
        ("287", "270", 245_u64, 135_u64),
        ("287", "270", 256, 146),
        ("327", "257", 250, 139),
        ("414", "263", 250, 140),
        ("497", "259", 257, 146),
        ("545", "257", 246, 135),
        ("545", "257", 250, 139),
        ("545", "257", 257, 146),
    ] {
        surface_scope.class_tag =
            crate::records::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        surface_scope.paired_class_tag =
            crate::records::DesignClassTag::try_from(paired_class_tag.to_owned()).unwrap();
        surface_scope
            .try_edit(|draft| {
                draft.frame_length = base_frame + reference_bytes;
                draft.kind_offset = draft.byte_offset + base_kind + reference_bytes;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_count_offset =
                    draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let (features, _) = project_parameter_design(
            &[],
            &[],
            std::slice::from_ref(&surface_scope),
            std::slice::from_ref(&delete_group),
            &[],
            &[],
            &[],
            &[],
        );
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::DeleteFace { heal: false, .. }
        ));
    }

    surface_scope.class_tag = crate::records::DesignClassTag::try_from("327".to_owned()).unwrap();
    surface_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();
    surface_scope
        .try_edit(|draft| {
            draft.frame_length = 250 + reference_bytes;
            draft.kind_offset = draft.byte_offset + 139 + reference_bytes;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.reference_count_offset =
                draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&surface_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::SurfaceDeleteFace,
            ..
        }
    ));

    for (class_tag, paired_class_tag) in [("264", "262"), ("383", "263")] {
        delete_scope
            .try_edit(|draft| {
                draft.payload = crate::records::feature::DesignFeatureKind::DeleteFace
                    .try_into()
                    .unwrap();
            })
            .unwrap();
        delete_scope.class_tag =
            crate::records::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
        delete_scope.paired_class_tag =
            crate::records::DesignClassTag::try_from(paired_class_tag.to_owned()).unwrap();
        delete_scope
            .try_edit(|draft| {
                draft.frame_length = 232 + reference_bytes;
                draft.kind_offset = draft.byte_offset + 135 + reference_bytes;
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.reference_count_offset =
                    draft.kind_offset - 12 - 11 * draft.reference_members.len() as u64;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        let (features, _) = project_parameter_design(
            &[],
            &[],
            std::slice::from_ref(&delete_scope),
            std::slice::from_ref(&delete_group),
            &[],
            &[],
            &[],
            &[],
        );
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::DeleteFace { heal: true, .. }
        ));
    }

    delete_scope.class_tag = crate::records::DesignClassTag::try_from("264".to_owned()).unwrap();
    delete_scope.paired_class_tag =
        crate::records::DesignClassTag::try_from("263".to_owned()).unwrap();
    let (features, _) = project_parameter_design(
        &[],
        &[],
        std::slice::from_ref(&delete_scope),
        std::slice::from_ref(&delete_group),
        &[],
        &[],
        &[],
        &[],
    );
    assert!(matches!(
        features[0].evaluation.definition(),
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::DeleteFace,
            ..
        }
    ));

    let mut remove_scope = scope.clone();
    remove_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::DesignFeatureKind::RemoveBody
                .try_into()
                .unwrap();
        })
        .unwrap();
    let mut remove_group = group;
    remove_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    remove_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_A);
    assert_eq!(
        crate::design::feature_project::project_remove_body(
            &remove_scope,
            std::slice::from_ref(&remove_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::DeleteBody {
            bodies: cadmpeg_ir::features::BodySelection::Native(remove_group.id.clone()),
            mode: cadmpeg_ir::features::BodyRetentionMode::DeleteSelected,
        })
    );

    let mut stitch_scope = scope;
    stitch_scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::ReferenceRun::unlocated(vec![100, 200, 300, 301]);
            draft.payload = crate::records::feature::DesignScopePayload::SurfaceStitch(
                DesignSurfaceStitchOperation {
                    gap_tolerance: crate::records::feature::DesignPositiveScalar::new(0.01)
                        .unwrap(),
                    gap_tolerance_offset: 40,
                    tolerance_record_index: 300,
                    settings_record_index: 301,
                },
            );
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut stitch_group = remove_group;
    stitch_group
        .try_set_members(
            vec![200]
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
        )
        .unwrap();
    stitch_group.operand_role =
        crate::records::topology::DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5);
    assert_eq!(
        crate::design::feature_project::project_surface_stitch(
            &stitch_scope,
            std::slice::from_ref(&stitch_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::KnitSurface {
            faces: cadmpeg_ir::features::FaceSelection::Native(stitch_scope.id),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::scalar::NonNegativeLength::new(0.1).unwrap()),
        })
    );
}

#[test]
fn legacy_move_body_groups_accept_the_unterminated_true_flag_pair() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    for (ordinal, (class_tag, scope_kind)) in [
        ("323", crate::records::feature::DesignFeatureKind::Move),
        ("328", crate::records::feature::DesignFeatureKind::Move),
        ("257", crate::records::feature::DesignFeatureKind::Move),
        (
            "338",
            crate::records::feature::DesignFeatureKind::RemoveBody,
        ),
        ("282", crate::records::feature::DesignFeatureKind::Move),
        ("302", crate::records::feature::DesignFeatureKind::Move),
    ]
    .into_iter()
    .enumerate()
    {
        let scope_record_index = 12 + u32::try_from(ordinal).expect("small test ordinal");
        let group_record_index = 100 + 4 * u32::try_from(ordinal).expect("small test ordinal");
        let frame_at = 0;
        let mut bytes = Vec::new();
        header(
            &mut bytes,
            class_tag.as_bytes().try_into().expect("three-digit class"),
            group_record_index,
        );
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut bytes, group_record_index + 3);
        if class_tag == "328" {
            bytes.push(0);
            reference(&mut bytes, group_record_index + 13);
        } else {
            bytes.extend_from_slice(&[0; 2]);
        }
        bytes.extend_from_slice(&0u32.to_le_bytes());
        if class_tag == "328" {
            bytes.push(0);
        }
        bytes.extend_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&180u32.to_le_bytes());
        bytes.extend_from_slice(&0.125f64.to_le_bytes());
        bytes.extend_from_slice(&180u32.to_le_bytes());
        reference(&mut bytes, group_record_index + 2);
        let flag_pair = matches!(class_tag, "282" | "302")
            .then_some([0, 1])
            .unwrap_or([1, 1]);
        if class_tag == "328" {
            bytes.push(0);
        }
        bytes.extend_from_slice(&flag_pair);
        if class_tag == "328" {
            bytes.extend_from_slice(&u64::from(group_record_index + 1).to_le_bytes());
            bytes.extend_from_slice(&[0; 3]);
        } else {
            reference(&mut bytes, group_record_index + 1);
            bytes.push(0);
        }
        reference(&mut bytes, scope_record_index);
        let paired_at = bytes.len();
        header(
            &mut bytes,
            if class_tag == "328" { *b"263" } else { *b"262" },
            group_record_index,
        );

        let mut scope = DesignParameterScope::empty(
            &format!("f3d:test:legacy-body-group#{scope_record_index}"),
            scope_kind,
            scope_record_index,
        );
        scope
            .try_edit(|draft| {
                draft.reference_members =
                    crate::records::ReferenceRun::unlocated(vec![group_record_index]);
                draft.layout_fixture_references();
                draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
                draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
                draft.layout_fixture_tail();
            })
            .unwrap();
        let record = DesignRecordHeader {
            id: format!("f3d:test:legacy-body-record#{group_record_index}"),
            byte_offset: frame_at,
            class_tag: crate::records::DesignClassTag::try_from(class_tag.to_owned()).unwrap(),
            record_index: group_record_index,
        };
        let group =
            parse_construction_operand_group(&bytes, &scope, 0, &RecordFrame::from(&record))
                .complete()
                .expect("legacy body construction group");

        assert_eq!(
            group
                .members()
                .iter()
                .map(|member| member.value)
                .collect::<Vec<_>>(),
            [group_record_index + 3]
        );
        assert_eq!(group.role(), DesignOperandRole::BODIES_A);
        assert_eq!(group.frame.variant, flag_pair == [1, 1]);
        assert_eq!(group.paired_byte_offset, paired_at as u64);
    }
}

#[test]
fn class_296_two_sided_to_faces_role_0x12_is_a_face_group_only_in_its_exact_scope() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#296536",
        crate::records::feature::DesignFeatureKind::Extrude,
        296_536,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 1000;
            draft.reference_count_offset = draft.byte_offset + 9;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.class_tag = crate::records::DesignClassTag::try_from("296".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("261".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 536;
            draft.reference_count_offset = 1291;
            draft.reference_members = crate::records::ReferenceRun::unlocated(
                (0..13).map(|index| 296_500 + index).collect(),
            );
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::DesignScopePayloadMut::Extrude(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusion(slot)
    | crate::records::feature::DesignScopePayloadMut::Extrusao(slot) = scope.payload_mut()
    {
        slot.get_or_insert_with(Default::default).extrude_prologue =
            Some(DesignExtrudePrologue::LegacyShifted {
                operation_prefix_marker_offset: None,
                operation: DesignExtrudeOperation::Join,
                operation_offset: 1026,
                direction_face_extend_values: [2, 2],
                side_extent_discriminators: [2, 0],
                side_extent_discriminator_offsets: [1115, 1287],
                extent: Some(DesignExtrudeExtent::TwoSidedToFaces),
                direction_face_extend_offsets: [1030, 1034],
                direction_reversed: false,
                direction_reversed_offset: 1038,
                solid_operation: true,
                solid_operation_offset: 1039,
                start: DesignExtrudeStart::ProfilePlane,
                start_offset: 1040,
            });
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"323");
    bytes.extend_from_slice(&296_501_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x0000_0012_0000_0000u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&91u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&91u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&296_503_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&[1, 1, 0]);
    bytes.push(1);
    bytes.extend_from_slice(&296_502_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&scope.record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&296_501_u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:group#296501".into(),
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("323".to_owned()).unwrap(),
        record_index: 296_501,
    };
    let mut group =
        parse_construction_operand_group(&bytes, &scope, 0, &RecordFrame::from(&header))
            .complete()
            .expect("class-296 two-sided-to-faces construction group");
    assert_eq!(group.extrude_role(), None);
    crate::design::decode::operands::assign_extrude_face_roles(
        &scope,
        std::slice::from_mut(&mut group),
    );
    assert_eq!(
        group.extrude_role(),
        Some(DesignExtrudeOperandRole::Faces(
            DesignExtrudeFaceRole::Termination
        ))
    );

    let mut wrong_length = scope.clone();
    wrong_length
        .try_edit(|draft| {
            draft.frame_length = 537;
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group =
        parse_construction_operand_group(&bytes, &wrong_length, 0, &RecordFrame::from(&header))
            .complete()
            .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role(), None);

    let mut wrong_extent = scope;
    let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
        wrong_extent.extrude_prologue_mut()
    else {
        panic!("synthetic class-296 two-sided-to-faces prologue");
    };
    *extent = Some(DesignExtrudeExtent::SymmetricDistance);
    let group =
        parse_construction_operand_group(&bytes, &wrong_extent, 0, &RecordFrame::from(&header))
            .complete()
            .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role(), None);
}
