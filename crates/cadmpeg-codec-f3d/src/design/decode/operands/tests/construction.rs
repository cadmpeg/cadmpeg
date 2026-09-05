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
use crate::design::decode::operands::parse_loft_legacy_body_carrier;

#[test]
fn localized_edge_treatment_group_retention_is_language_independent() {
    use crate::records::DesignFeatureKind as Kind;
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
        Kind::Native("unknown".into()),
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
        reference_members: crate::records::ReferenceRun::from_columns(vec![100, 200, 201], vec![1085, 1096, 1107], "reference_members").unwrap(),
        payload: crate::records::DesignFeatureKind::Extrude.into(),
        unclosed_construction_operand_groups: Vec::new(),
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "332".into(),
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

    let group = parse_construction_operand_group(&bytes, &scope, 0, &record)
        .complete()
        .expect("counted Extrude operand group");
    assert_eq!(group.members.iter().map(|member| member.value).collect::<Vec<_>>(), [200, 201]);
    assert_eq!(group.members.iter().map(|member| member.offset).collect::<Vec<_>>(), [26, 37]);
    assert_eq!(group.role, 0x0000_0008_0000_0000);
    assert_eq!(group.extrude_role, Some(DesignExtrudeOperandRole::Bodies));
    assert_eq!(group.frame.member_count_offset, 21);
    assert!(group.frame.auxiliary_records.is_empty());
    assert_eq!(group.frame.trailing_records.iter().map(|record| record.value).collect::<Vec<_>>(), [300]);
    assert_eq!(group.frame.opaque_index, 180);
    assert_eq!(group.frame.opaque_scalar, 0.125);
    assert!(group.frame.variant);
    assert_eq!(group.paired_byte_offset, paired_at as u64);

    let mut whole_body_bytes = bytes.clone();
    whole_body_bytes[group.role_offset as usize..group.role_offset as usize + 8]
        .copy_from_slice(&0x0000_0004_0000_0000u64.to_le_bytes());
    let whole_body = parse_construction_operand_group(&whole_body_bytes, &scope, 0, &record)
        .complete()
        .expect("counted Extrude whole-body group");
    assert_eq!(whole_body.role, 0x0000_0004_0000_0000);
    assert_eq!(
        whole_body.extrude_role,
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
    let flagged = parse_construction_operand_group(&flagged, &scope, 0, &record)
        .complete()
        .expect("operation-flagged counted operand group");
    assert_eq!(flagged.frame.member_count_offset, flagged_count_at as u64);
    assert_eq!(flagged.members.iter().map(|member| member.value).collect::<Vec<_>>(), [200, 201]);
    assert_eq!(flagged.role, 0x0000_0008_0000_0000);

    let mut start_face_bytes = bytes.clone();
    start_face_bytes[group.role_offset as usize..group.role_offset as usize + 8]
        .copy_from_slice(&0x0000_0005_0000_0000u64.to_le_bytes());
    let retained_role_five =
        parse_construction_operand_group(&start_face_bytes, &scope, 0, &record)
            .complete()
            .expect("counted Extrude retained role-five group");
    assert_eq!(retained_role_five.extrude_role, None);

    let mut from_face_scope = scope.clone();
    if let crate::records::DesignScopePayload::Extrude(slot)
    | crate::records::DesignScopePayload::Extrusion(slot)
    | crate::records::DesignScopePayload::Extrusao(slot) = &mut from_face_scope.payload
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
    let start_face =
        parse_construction_operand_group(&start_face_bytes, &from_face_scope, 0, &record)
            .complete()
            .expect("counted Extrude start-face group");
    assert_eq!(start_face.role, 0x0000_0005_0000_0000);
    assert_eq!(
        start_face.extrude_role,
        Some(DesignExtrudeOperandRole::Faces(None))
    );

    let mut to_face_scope = from_face_scope.clone();
    if let crate::records::DesignScopePayload::Extrude(slot)
    | crate::records::DesignScopePayload::Extrusion(slot)
    | crate::records::DesignScopePayload::Extrusao(slot) = &mut to_face_scope.payload
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
    to_face_bytes[group.role_offset as usize..group.role_offset as usize + 8]
        .copy_from_slice(&0x0000_0012_0000_0000u64.to_le_bytes());
    let legacy_to_face =
        parse_construction_operand_group(&to_face_bytes, &to_face_scope, 0, &record)
            .complete()
            .expect("counted Extrude legacy to-face group");
    assert_eq!(legacy_to_face.role, 0x0000_0012_0000_0000);
    assert_eq!(
        legacy_to_face.extrude_role,
        Some(DesignExtrudeOperandRole::Faces(None))
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
    let flagless = parse_construction_operand_group(&flagless, &scope, 0, &record)
        .complete()
        .expect("flagless counted operand group");
    assert_eq!(flagless.members.iter().map(|member| member.value).collect::<Vec<_>>(), [200, 201]);
    assert_eq!(flagless.role, 0x0000_0008_0000_0000);
    assert!(!flagless.frame.variant);
    assert_eq!(
        flagless.paired_byte_offset,
        u64::try_from(flagless_paired_at).unwrap()
    );

    let mut bombed = bytes.clone();
    bombed[21..25].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        parse_construction_operand_group(&bombed, &scope, 0, &record),
        ConstructionOperandGroupParse::NotAGroup
    ));

    // A record that opens the grammar but whose tail names another record is a
    // group this reader cannot read, not a reference member that is not a group.
    let mut truncated = bytes.clone();
    let tail_at = truncated.len() - 40;
    truncated[tail_at..].fill(0x5a);
    assert!(matches!(
        parse_construction_operand_group(&truncated, &scope, 0, &record),
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
        class_tag: "283".into(),
        ..record.clone()
    };
    let auxiliary = parse_construction_operand_group(&auxiliary, &scope, 0, &auxiliary_record)
        .complete()
        .expect("Extrude face group carrying both optional references");
    assert_eq!(auxiliary.members.iter().map(|member| member.value).collect::<Vec<_>>(), [109]);
    assert_eq!(auxiliary.members.iter().map(|member| member.offset).collect::<Vec<_>>(), [26]);
    assert_eq!(auxiliary.frame.auxiliary_records.iter().map(|record| record.value).collect::<Vec<_>>(), [103, 106]);
    assert_eq!(auxiliary.frame.auxiliary_records.iter().map(|record| record.offset).collect::<Vec<_>>(), [37, 48]);
    assert!(auxiliary.frame.trailing_records.is_empty());
    assert_eq!(auxiliary.role, 0x0000_0011_0000_0000);
    assert_eq!(
        auxiliary.extrude_role,
        Some(DesignExtrudeOperandRole::Faces(None))
    );
    assert_eq!(auxiliary.paired_byte_offset, auxiliary_paired_at as u64);

    let mut split_scope = scope.clone();
    split_scope.payload = crate::records::DesignFeatureKind::SplitFace.into();
    split_scope.frame_length = 334;
    split_scope.reference_members = crate::records::ReferenceRun::from_columns(vec![100, 200, 201, 400, 500], vec![1085, 1096, 1107, 1118, 1129], "reference_members").unwrap();
    let mut tool_group = group.clone();
    tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    tool_group.role = 0x0000_0021_0000_0000;
    let mut target_group = group.clone();
    target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    target_group.record_index = 400;
    target_group.scope_reference_ordinal = 3;
    target_group.members = vec![crate::records::Located { value: 500, offset: 1129 }];
    target_group.role = 0x0000_0010_0000_0000;
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
        &features[0].definition,
        FeatureDefinition::SplitFace {
            targets: cadmpeg_ir::features::FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool),
            ),
        } if targets.ends_with("#400") && tool.ends_with("#100")
    ));

    let mut compact_split_scope = split_scope.clone();
    compact_split_scope.class_tag = "418".into();
    compact_split_scope.paired_class_tag = "266".into();
    compact_split_scope.frame_length = 330;
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
        &compact_features[0].definition,
        FeatureDefinition::SplitFace { .. }
    ));

    let mut first_plane = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#601",
        crate::records::DesignFeatureKind::WorkPlane,
        601,
    );
    first_plane.feature_ordinal = 0;
    first_plane.with_work_plane_transform([
        [1.0, 0.0, 0.0, -0.8],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    let mut second_plane = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#701",
        crate::records::DesignFeatureKind::WorkPlane,
        701,
    );
    second_plane.feature_ordinal = 1;
    second_plane.with_work_plane_transform([
        [1.0, 0.0, 0.0, -1.4],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    compact_split_scope.feature_ordinal = 2;
    let plane_selection = |record_index, group_member_ordinal, primary_identity| {
        crate::records::DesignEntitySelectionOperand {
            id: format!("f3d:Design/BulkStream.dat:design-entity-selection-operand#{record_index}"),
            scope_record_index: compact_split_scope.record_index,
            group_record_index: split_groups[0].record_index,
            group_member_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: "372".into(),
            asset_id: "asset".into(),
            asset_id_offset: 0,
            context_id: "context".into(),
            context_id_offset: 0,
            identity_record_index: record_index + 3,
            identity_record_offset: 0,
            primary_identity,
            primary_identity_offset: 0,
            secondary: None,
            historical_edge_candidates: Vec::new(),
            historical_face_candidates: Vec::new(),
            resolved_edge_slot: None,
            next_record_index: record_index + 4,
            next_byte_offset: 0,
        }
    };
    let plane_selections = [plane_selection(200, 0, 600), plane_selection(201, 1, 700)];
    let expected_planes = [
        crate::ids::neutral_feature_id(&first_plane),
        crate::ids::neutral_feature_id(&second_plane),
    ];
    let plane_scopes = vec![first_plane, second_plane, compact_split_scope.clone()];
    let plane_timeline = DesignFeatureTimeline {
        id: crate::ids::native_design_feature_timeline_id_in_stream("f3d:Design/BulkStream.dat", 0),
        byte_offset: 0,
        class_tag: "256".into(),
        record_index: 1,
        source_ordinal: 0,
        frame_length: 0,
        context_record_index: 1,
        context_record_index_offset: 0,
        item_count_offset: 0,
        items: plane_scopes.iter().map(|scope| crate::records::Located { value: u64::from(scope.record_index), offset: 0 }).collect(),
    };
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
        &plane_split.definition,
        FeatureDefinition::SplitFace {
            tool: cadmpeg_ir::features::SplitFaceTool::Planes { planes },
            ..
        } if planes == &expected_planes
    ));
    assert_eq!(plane_split.dependencies, expected_planes);

    compact_split_scope.class_tag = "375".into();
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
        &mismatched_features[0].definition,
        FeatureDefinition::Native { .. }
    ));

    let mut split_body_scope = scope.clone();
    split_body_scope.payload = crate::records::DesignFeatureKind::Split.into();
    split_body_scope.frame_length = 325;
    split_body_scope.reference_members = crate::records::ReferenceRun::from_columns(vec![100, 200, 400, 500], vec![1085, 1096, 1107, 1118], "reference_members").unwrap();
    let mut split_tool_group = group.clone();
    split_tool_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    split_tool_group.record_index = 100;
    split_tool_group.scope_reference_ordinal = 0;
    split_tool_group.members = vec![crate::records::Located { value: 200, offset: 1096 }];
    split_tool_group.role = 0x0000_0009_0000_0000;
    let mut split_target_group = group.clone();
    split_target_group.id = "f3d:Design/BulkStream.dat:operand-group#400".into();
    split_target_group.record_index = 400;
    split_target_group.scope_reference_ordinal = 2;
    split_target_group.members = vec![crate::records::Located { value: 500, offset: 1118 }];
    split_target_group.role = 0x0000_0004_0000_0000;
    let split_tool = DesignFaceOperand {
        id: "f3d:Design/BulkStream.dat:face-operand#200".into(),
        scope_record_index: split_body_scope.record_index,
        scope_reference_ordinal: 1,
        group: Some(crate::records::DesignOperandGroup {
            group_record_index: 100,
            group_member_ordinal: 0,
        }),
        record_index: 200,
        byte_offset: 1200,
        class_tag: "297".into(),
        paired_byte_offset: 1400,
        paired_class_tag: "259".into(),
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
    };
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
    historical_split_scope.previous_history_state_id = Some(7);
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
        }) if faces.len() == 1 && native == historical_split_tool.id
    ));

    let mut multiple_targets_scope = split_body_scope.clone();
    multiple_targets_scope.frame_length = 358;
    multiple_targets_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 400, 500, 501]);
    let mut multiple_targets = split_target_group.clone();
    multiple_targets.members = vec![500, 501].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
    assert!(matches!(
        project_split(
            &multiple_targets_scope,
            &[split_tool_group.clone(), multiple_targets],
            std::slice::from_ref(&split_tool)
        ),
        Some(FeatureDefinition::SplitBody { .. })
    ));

    let mut construction_tool_scope = split_body_scope.clone();
    construction_tool_scope.frame_length = 347;
    construction_tool_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 201, 400, 500]);
    let mut construction_tool = split_tool_group.clone();
    construction_tool.role = 0x0000_0021_0000_0000;
    construction_tool.members = vec![200, 201].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
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
    oversized_tool.members = vec![200, 201, 202, 203].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
    invalid_groups.push(vec![oversized_tool, split_target_group.clone()]);
    for mutate in 0..4 {
        let mut tool = split_tool_group.clone();
        match mutate {
            0 => tool.scope_reference_ordinal = 1,
            1 => tool.record_index = 101,
            2 => tool.role = 0x0000_0008_0000_0000,
            3 => tool.members = vec![crate::records::Located { value: 201, offset: tool.members[0].offset }],
            _ => unreachable!(),
        }
        invalid_groups.push(vec![tool, split_target_group.clone()]);
    }
    for mutate in 0..4 {
        let mut target = split_target_group.clone();
        match mutate {
            0 => target.scope_reference_ordinal = 3,
            1 => target.record_index = 401,
            2 => target.role = 0x0000_0005_0000_0000,
            3 => target.members = vec![crate::records::Located { value: 501, offset: target.members[0].offset }],
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
    delete_scope.payload = crate::records::DesignFeatureKind::DeleteFace.into();
    delete_scope.frame_length = 258;
    delete_scope.kind_offset = 1161;
    delete_scope.reference_members = crate::records::ReferenceRun::from_columns(vec![100, 200], vec![1085, 1096], "reference_members").unwrap();
    let mut delete_group = group.clone();
    delete_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    delete_group.members = vec![crate::records::Located { value: 200, offset: 1096 }];
    delete_group.role = 0x0000_0010_0000_0000;
    let mut delete_face_operand = split_tool.clone();
    delete_face_operand.id = "f3d:Design/BulkStream.dat:face-operand#200".into();
    delete_face_operand.scope_record_index = delete_scope.record_index;
    delete_face_operand.scope_reference_ordinal = 1;
    delete_face_operand.group = Some(crate::records::DesignOperandGroup {
        group_record_index: delete_group.record_index,
        group_member_ordinal: 0,
    });
    delete_face_operand.record_index = 200;
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
        features[0].definition,
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
        features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved {
                faces: vec![FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")],
                native: delete_group.id.clone(),
            },
            heal: true,
        }
    );
    delete_scope.frame_length = 263;
    delete_scope.kind_offset = 1165;
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
        features[0].definition,
        FeatureDefinition::DeleteFace { heal: true, .. }
    ));
    delete_scope.frame_length += 1;
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
        features[0].definition,
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::DeleteFace,
            ..
        }
    ));

    let mut surface_scope = delete_scope.clone();
    let reference_bytes = 11 * surface_scope.reference_members.len() as u64;
    surface_scope.payload = crate::records::DesignFeatureKind::SurfaceDeleteFace.into();
    surface_scope.frame_length = 250 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 140 + reference_bytes;
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
        features[0].definition,
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
        features[0].definition,
        FeatureDefinition::DeleteFace {
            faces: FaceSelection::Resolved {
                faces: vec![FaceId::mint(crate::ids::brep_entity_id(7)).expect("identity grammar")],
                native: delete_group.id.clone(),
            },
            heal: false,
        }
    );
    surface_scope.frame_length = 251 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 139 + reference_bytes;
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
        features[0].definition,
        FeatureDefinition::DeleteFace { heal: false, .. }
    ));
    surface_scope.frame_length = 236 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 139 + reference_bytes;
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
        features[0].definition,
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
        surface_scope.class_tag = class_tag.into();
        surface_scope.paired_class_tag = paired_class_tag.into();
        surface_scope.frame_length = base_frame + reference_bytes;
        surface_scope.kind_offset = surface_scope.byte_offset + base_kind + reference_bytes;
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
            features[0].definition,
            FeatureDefinition::DeleteFace { heal: false, .. }
        ));
    }

    surface_scope.class_tag = "327".into();
    surface_scope.paired_class_tag = "258".into();
    surface_scope.frame_length = 250 + reference_bytes;
    surface_scope.kind_offset = surface_scope.byte_offset + 139 + reference_bytes;
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
        features[0].definition,
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::SurfaceDeleteFace,
            ..
        }
    ));

    for (class_tag, paired_class_tag) in [("264", "262"), ("383", "263")] {
        delete_scope.payload = crate::records::DesignFeatureKind::DeleteFace.into();
        delete_scope.class_tag = class_tag.into();
        delete_scope.paired_class_tag = paired_class_tag.into();
        delete_scope.frame_length = 232 + reference_bytes;
        delete_scope.kind_offset = delete_scope.byte_offset + 135 + reference_bytes;
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
            features[0].definition,
            FeatureDefinition::DeleteFace { heal: true, .. }
        ));
    }

    delete_scope.class_tag = "264".into();
    delete_scope.paired_class_tag = "263".into();
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
        features[0].definition,
        FeatureDefinition::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::DeleteFace,
            ..
        }
    ));

    let mut remove_scope = scope.clone();
    remove_scope.payload = crate::records::DesignFeatureKind::RemoveBody.into();
    let mut remove_group = group;
    remove_group.id = "f3d:Design/BulkStream.dat:operand-group#100".into();
    remove_group.role = 0x0000_0004_0000_0000;
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
    stitch_scope.payload = crate::records::DesignFeatureKind::SurfaceStitch.into();
    stitch_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![100, 200, 300, 301]);
    if let crate::records::DesignScopePayload::SurfaceStitch(slot) = &mut stitch_scope.payload {
        *slot = Some(DesignSurfaceStitchOperation {
            gap_tolerance: 0.01,
            gap_tolerance_offset: 40,
            tolerance_record_index: 300,
            settings_record_index: 301,
        });
    }
    let mut stitch_group = remove_group;
    stitch_group.members = vec![200].into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect();
    stitch_group.role = 0x0000_0005_0000_0000;
    assert_eq!(
        crate::design::feature_project::project_surface_stitch(
            &stitch_scope,
            std::slice::from_ref(&stitch_group)
        ),
        Some(cadmpeg_ir::features::FeatureDefinition::KnitSurface {
            faces: cadmpeg_ir::features::FaceSelection::Native(stitch_scope.id),
            merge_entities: Some(true),
            create_solid: Some(true),
            gap_tolerance: Some(cadmpeg_ir::features::Length(0.1)),
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
        ("323", crate::records::DesignFeatureKind::Move),
        ("328", crate::records::DesignFeatureKind::Move),
        ("257", crate::records::DesignFeatureKind::Move),
        ("338", crate::records::DesignFeatureKind::RemoveBody),
        ("282", crate::records::DesignFeatureKind::Move),
        ("302", crate::records::DesignFeatureKind::Move),
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
        scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![group_record_index]);
        let record = DesignRecordHeader {
            id: format!("f3d:test:legacy-body-record#{group_record_index}"),
            byte_offset: frame_at,
            class_tag: class_tag.to_owned(),
            record_index: group_record_index,
        };
        let group = parse_construction_operand_group(&bytes, &scope, 0, &record)
            .complete()
            .expect("legacy body construction group");

        assert_eq!(group.members.iter().map(|member| member.value).collect::<Vec<_>>(), [group_record_index + 3]);
        assert_eq!(group.role, 0x0000_0004_0000_0000);
        assert_eq!(group.frame.variant, flag_pair == [1, 1]);
        assert_eq!(group.paired_byte_offset, paired_at as u64);
    }
}

#[test]
fn class_296_two_sided_to_faces_role_0x12_is_a_face_group_only_in_its_exact_scope() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#296536",
        crate::records::DesignFeatureKind::Extrude,
        296_536,
    );
    scope.byte_offset = 1000;
    scope.class_tag = "296".into();
    scope.paired_class_tag = "261".into();
    scope.frame_length = 536;
    scope.reference_count_offset = 1291;
    scope.reference_members = crate::records::ReferenceRun::Unlocated((0..13).map(|index| 296_500 + index).collect());
    if let crate::records::DesignScopePayload::Extrude(slot)
    | crate::records::DesignScopePayload::Extrusion(slot)
    | crate::records::DesignScopePayload::Extrusao(slot) = &mut scope.payload
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
        class_tag: "323".into(),
        record_index: 296_501,
    };
    let group = parse_construction_operand_group(&bytes, &scope, 0, &header)
        .complete()
        .expect("class-296 two-sided-to-faces construction group");
    assert_eq!(
        group.extrude_role,
        Some(DesignExtrudeOperandRole::Faces(None))
    );

    let mut wrong_length = scope.clone();
    wrong_length.frame_length = 537;
    let group = parse_construction_operand_group(&bytes, &wrong_length, 0, &header)
        .complete()
        .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role, None);

    let mut wrong_extent = scope;
    let Some(DesignExtrudePrologue::LegacyShifted { extent, .. }) =
        wrong_extent.extrude_prologue_mut()
    else {
        panic!("synthetic class-296 two-sided-to-faces prologue");
    };
    *extent = Some(DesignExtrudeExtent::SymmetricDistance);
    let group = parse_construction_operand_group(&bytes, &wrong_extent, 0, &header)
        .complete()
        .expect("construction group with otherwise valid frame");
    assert_eq!(group.extrude_role, None);
}

#[test]
fn construction_operand_trailing_transform_has_exact_affine_frame() {
    let record_index = 300u32;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"339");
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[1, 0]);
    let following_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"432");
    bytes.extend_from_slice(&(record_index + 1).to_le_bytes());
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#300".into(),
        byte_offset: 0,
        class_tag: "339".into(),
        record_index,
    };

    let parsed = parse_construction_operand_transform(&bytes, &header)
        .expect("exact construction-operand transform");
    assert_eq!(parsed.transform, transform);
    assert_eq!(parsed.transform_offset, 22);
    assert_eq!(parsed.following_record_index, 301);
    assert_eq!(parsed.following_byte_offset, following_at as u64);
    assert_eq!(parsed.following_class_tag, "432");

    bytes[150] = 0;
    assert!(parse_construction_operand_transform(&bytes, &header).is_none());

    let secondary = [
        [1.0_f64, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut dual = bytes[..21].to_vec();
    for value in transform.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    for value in secondary.into_iter().flatten() {
        dual.extend_from_slice(&value.to_le_bytes());
    }
    dual.push(0);
    let dual_following_at = dual.len();
    dual.extend_from_slice(&3u32.to_le_bytes());
    dual.extend_from_slice(b"432");
    dual.extend_from_slice(&(record_index + 1).to_le_bytes());
    let parsed = parse_construction_operand_dual_transform(&dual, &header)
        .expect("exact dual construction-operand transform");
    assert_eq!(parsed.first_transform, transform);
    assert_eq!(parsed.first_transform_offset, 21);
    assert_eq!(parsed.second_transform, secondary);
    assert_eq!(parsed.second_transform_offset, 149);
    assert_eq!(dual_following_at, 278);
}

#[test]
fn construction_operand_trailing_flag_has_exact_compact_frame() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"374");
    bytes.extend_from_slice(&33602u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    let header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#33602".into(),
        byte_offset: 0,
        class_tag: "374".into(),
        record_index: 33602,
    };

    let flag = parse_construction_operand_flag(&bytes, &header).expect("compact trailing flag");
    assert!(flag.value);
    assert_eq!(flag.value_offset, 22);

    bytes[22] = 2;
    assert!(parse_construction_operand_flag(&bytes, &header).is_none());
}

#[test]
fn construction_operand_auxiliary_paths_decode_transform_and_compact_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let scope_record_index = 40u32;
    let record_index = 100u32;
    let transform = [
        [0.0_f64, -1.0, 0.0, 12.5],
        [1.0, 0.0, 0.0, -4.0],
        [0.0, 0.0, 1.0, 3.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let mut expanded = Vec::new();
    header(&mut expanded, b"304", record_index);
    expanded.extend_from_slice(&[0; 10]);
    expanded.push(1);
    expanded.extend_from_slice(&174u64.to_le_bytes());
    expanded.extend_from_slice(&[0; 3]);
    for value in transform.into_iter().flatten() {
        expanded.extend_from_slice(&value.to_le_bytes());
    }
    expanded.push(0);
    reference(&mut expanded, scope_record_index);
    reference(&mut expanded, record_index + 2);
    expanded.extend_from_slice(&[0; 6]);
    let expanded_following_at = expanded.len();
    header(&mut expanded, b"390", record_index + 1);
    let expanded_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "304".into(),
        record_index,
    };
    let expanded = parse_construction_operand_path(&expanded, scope_record_index, &expanded_header)
        .expect("expanded selection path");
    assert_eq!(expanded.entity_ref, 174);
    assert_eq!(expanded.placement, crate::records::DesignConstructionPathPlacement::Transform(crate::records::Located { value: transform, offset: 33 }));
    assert_eq!(expanded.scope_record_index_offset, 163);
    assert_eq!(expanded.nested_record_index, 102);
    assert_eq!(expanded.nested_record_index_offset, 174);
    assert_eq!(expanded.following_record_index, 101);
    assert_eq!(expanded.following_byte_offset, expanded_following_at as u64);

    let mut compact = Vec::new();
    header(&mut compact, b"304", record_index);
    compact.extend_from_slice(&[0; 10]);
    compact.push(1);
    compact.extend_from_slice(&18_064u64.to_le_bytes());
    compact.extend_from_slice(&[0, 0, 1, 0]);
    reference(&mut compact, scope_record_index);
    reference(&mut compact, record_index + 2);
    compact.extend_from_slice(&[0; 6]);
    let compact_following_at = compact.len();
    header(&mut compact, b"390", record_index + 1);
    let compact = parse_construction_operand_path(&compact, scope_record_index, &expanded_header)
        .expect("compact selection path");
    assert_eq!(compact.entity_ref, 18_064);
    assert_eq!(compact.placement, crate::records::DesignConstructionPathPlacement::Compact(true));
    assert_eq!(compact.scope_record_index_offset, 35);
    assert_eq!(compact.nested_record_index_offset, 46);
    assert_eq!(compact.following_byte_offset, compact_following_at as u64);
}

#[test]
fn construction_tracking_path_decodes_absent_and_present_related_identities() {
    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn tracking_path(first: Option<u64>, second: Option<u64>) -> Vec<u8> {
        let wrapper_record_index = 300u32;
        let mut bytes = Vec::new();
        header(&mut bytes, b"361", wrapper_record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(wrapper_record_index + 1).to_le_bytes());
        bytes.extend_from_slice(&[0; 3]);
        header(&mut bytes, b"363", wrapper_record_index + 1);
        bytes.extend_from_slice(&[0; 10]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&268u64.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        for identity in [first, second] {
            bytes.extend_from_slice(&u32::from(identity.is_some()).to_le_bytes());
            if let Some(identity) = identity {
                bytes.extend_from_slice(&identity.to_le_bytes());
            }
        }
        header(&mut bytes, b"301", wrapper_record_index + 2);
        bytes
    }

    let absent = tracking_path(None, None);
    let absent = parse_construction_tracking_path(&absent, 0, 300, "361")
        .expect("tracking path without related identities");
    assert_eq!(absent.carrier_record_index, 301);
    assert_eq!(absent.carrier_byte_offset, 33);
    assert_eq!(absent.primary_identity, 268);
    assert_eq!(absent.primary_identity_offset, 70);
    assert_eq!(absent.selector, -1);
    assert_eq!(absent.kind, 3);
    assert_eq!(absent.first_related_identity, None);
    assert_eq!(absent.second_related_identity, None);
    assert_eq!(absent.following_record_index, 302);
    assert_eq!(absent.following_byte_offset, 114);

    let present = tracking_path(Some(113), Some(119));
    let present = parse_construction_tracking_path(&present, 0, 300, "361")
        .expect("tracking path with related identities");
    assert_eq!(present.first_related_identity.map(|identity| identity.value), Some(113));
    assert_eq!(present.first_related_identity.map(|identity| identity.offset), Some(110));
    assert_eq!(present.second_related_identity.map(|identity| identity.value), Some(119));
    assert_eq!(present.second_related_identity.map(|identity| identity.offset), Some(122));
    assert_eq!(present.following_byte_offset, 130);
}

#[test]
fn legacy_loft_body_carriers_admit_only_the_class_keyed_frames() {
    fn header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    fn reference(bytes: &mut Vec<u8>, record_index: u32) {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(record_index).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }

    fn carrier(
        primary_class: &[u8; 3],
        paired_class: &[u8; 3],
        scope_record_index: u32,
        record_index: u32,
        with_scope_tail: bool,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        header(&mut bytes, primary_class, record_index);
        bytes.extend_from_slice(&[0; 10]);
        bytes.push(1);
        bytes.extend_from_slice(&scope_record_index.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        reference(&mut bytes, 900);
        bytes.extend_from_slice(&89u32.to_le_bytes());
        bytes.extend_from_slice(&1.25f64.to_le_bytes());
        bytes.extend_from_slice(&89u32.to_le_bytes());
        reference(&mut bytes, record_index + 2);
        bytes.extend_from_slice(&[0, 0]);
        reference(&mut bytes, record_index + 1);
        if with_scope_tail {
            bytes.push(0);
            reference(&mut bytes, scope_record_index);
        }
        header(&mut bytes, paired_class, record_index);
        bytes
    }

    let mut scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat",
        crate::records::DesignFeatureKind::Loft,
        12,
    );
    {
        let value = Some(crate::records::DesignPathFeatureConstruction::Loft(crate::records::DesignLoftConstruction {
            operation: crate::records::DesignExtrudeOperation::Cut,
            operation_offset: 0,
        }));
        scope.payload = value.map_or_else(|| scope.kind().into(), Into::into);
    }

    let class_322 = carrier(b"322", b"262", 12, 100, false);
    let parsed_322 = parse_loft_legacy_body_carrier(
        &class_322,
        &scope,
        &crate::records::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 100,
            class_tag: "322".into(),
            byte_offset: 0,
        },
    )
    .expect("class-322 legacy Loft carrier");
    assert_eq!(parsed_322.paired_class_tag, "262");
    assert_eq!(parsed_322.paired_byte_offset, 87);
    assert_eq!(parsed_322.member, 900);
    assert_eq!(parsed_322.member_offset, 36);
    assert_eq!(parsed_322.opaque_index, 89);
    assert_eq!(parsed_322.opaque_scalar, 1.25);
    assert_eq!(parsed_322.next_next_record_index, 102);
    assert_eq!(parsed_322.next_record_index, 101);
    assert_eq!(parsed_322.trailing_scope_record_index.map(|reference| reference.value), None);

    let class_322_tail = carrier(b"322", b"262", 12, 200, true);
    let parsed_322_tail = parse_loft_legacy_body_carrier(
        &class_322_tail,
        &scope,
        &crate::records::DesignRecordHeader {
            id: "header-322-tail".into(),
            record_index: 200,
            class_tag: "322".into(),
            byte_offset: 0,
        },
    )
    .expect("class-322 legacy Loft carrier with scope tail");
    assert_eq!(parsed_322_tail.paired_class_tag, "262");
    assert_eq!(parsed_322_tail.paired_byte_offset, 99);
    assert_eq!(parsed_322_tail.trailing_scope_record_index.map(|reference| reference.value), Some(12));
    assert_eq!(parsed_322_tail.trailing_scope_record_index.map(|reference| reference.offset), Some(88));

    let class_411 = carrier(b"411", b"266", 12, 300, true);
    let parsed_411 = parse_loft_legacy_body_carrier(
        &class_411,
        &scope,
        &crate::records::DesignRecordHeader {
            id: "header-411".into(),
            record_index: 300,
            class_tag: "411".into(),
            byte_offset: 0,
        },
    )
    .expect("class-411 legacy Loft carrier");
    assert_eq!(parsed_411.paired_class_tag, "266");
    assert_eq!(parsed_411.paired_byte_offset, 99);
    assert_eq!(parsed_411.trailing_scope_record_index.map(|reference| reference.value), Some(12));
    assert_eq!(parsed_411.trailing_scope_record_index.map(|reference| reference.offset), Some(88));

    let mut wrong_presence = class_322.clone();
    wrong_presence[21] = 0;
    assert!(parse_loft_legacy_body_carrier(
        &wrong_presence,
        &scope,
        &crate::records::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 100,
            class_tag: "322".into(),
            byte_offset: 0,
        },
    )
    .is_none());

    let wrong_pair = carrier(b"322", b"266", 12, 400, false);
    assert!(parse_loft_legacy_body_carrier(
        &wrong_pair,
        &scope,
        &crate::records::DesignRecordHeader {
            id: "header-322".into(),
            record_index: 400,
            class_tag: "322".into(),
            byte_offset: 0,
        },
    )
    .is_none());
}
