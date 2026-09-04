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
fn body_recipe_operand_decodes_counted_and_empty_reference_tables() {
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
        extrude_face_role: None,
        role_offset: 953,

        paired_class_tag: "265".into(),
        paired_byte_offset: 1024,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "365".into(),
        record_index: 100,
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"365", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&2265u64.to_le_bytes());
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&2266u64.to_le_bytes());
    bytes.extend_from_slice(&32u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[7, 0, 0, 0]);
    header(&mut bytes, *b"259", 100);
    header(&mut bytes, *b"283", 101);
    header(&mut bytes, *b"463", 102);
    header(&mut bytes, *b"452", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    header(&mut bytes, *b"311", 104);
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: recipe_at as u64,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design_id: Some("2265".into()),
        design_id_offset: None,
        design_selector: Some(crate::records::ConstructionRecipeSelector {
            value: 9,
            byte_offset: 0,
        }),
        recipe_index: 0,
        record_index: 0,
    };
    let scope =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#80", "BoundaryFill", 80);

    let mut operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand");
    assert_eq!(operand.references.len(), 2);
    assert_eq!(operand.references[0].design_reference, 2265);
    assert_eq!(operand.references[0].form, 3);
    assert_eq!(operand.references[1].design_reference, 2266);
    assert_eq!(operand.references[1].form, 32);
    assert_eq!(operand.selector_tail, Some([7, 0, 0, 0]));
    assert_eq!(operand.selector_tail_offset, Some(220));
    assert_eq!(
        operand.owner,
        crate::records::DesignBodyRecipeOperandOwner::Group {
            group_record_index: 90,
            group_member_ordinal: 0,
        }
    );
    assert_eq!(operand.nested_record_index, 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset, next_at as u64);
    operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#0".into();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: String::new(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: String::new(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/Other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("other-stream").expect("identity grammar"),
                ),
                selector: 0,
                token: String::new(),
                design_references: vec![2265],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&scope),
    );
    assert_eq!(
        operand.references[0].candidate_faces,
        [
            FaceId::mint("other-selector").expect("identity grammar"),
            FaceId::mint("same-stream").expect("identity grammar")
        ]
    );

    // A legacy Combine tool keeps the same identity envelope with no
    // persistent Design-reference clauses. The marker therefore follows the
    // zero count at the ordinary reference-table cursor.
    let mut empty_bytes = bytes[..25].to_vec();
    empty_bytes[21..25].fill(0);
    empty_bytes.extend_from_slice(&bytes[49..]);
    let empty_recipe_at = recipe_at - 24;
    let empty_next_at = next_at - 24;
    let empty_recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{empty_recipe_at}"),
        byte_offset: empty_recipe_at as u64,
        ..recipe.clone()
    };
    let empty = parse_body_recipe_operand(&empty_bytes, &group, 0, &record, &empty_recipe)
        .expect("empty body recipe operand");
    assert!(empty.references.is_empty());
    assert_eq!(empty.nested_record_index, 103);
    assert_eq!(empty.next_byte_offset, empty_next_at as u64);

    let mut combine_scope =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#80", "Combine", 80);
    combine_scope.combine_operation = Some(crate::records::DesignCombineOperation {
        form: crate::records::DesignCombineForm::Standard,
        operation: crate::records::DesignExtrudeOperation::Join,
        operation_offset: 0,
        keep_tools: false,
        keep_tools_offset: 0,
        target: crate::records::DesignCombineBodySelection {
            record_index: 0,
            external_identity: None,
        },
        tools: Vec::new(),
    });
    let combine_recipe = ConstructionRecipe {
        design_selector: Some(crate::records::ConstructionRecipeSelector {
            value: 1,
            byte_offset: 0,
        }),
        ..recipe.clone()
    };
    let mut combine_operand = operand.clone();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut combine_operand),
        std::slice::from_ref(&combine_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: String::new(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: String::new(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&combine_scope),
    );
    assert_eq!(
        combine_operand.references[0].candidate_faces,
        [FaceId::mint("same-stream").expect("identity grammar")]
    );

    let mut nested = Vec::new();
    header(&mut nested, *b"302", 1);
    header(&mut nested, *b"305", 11);
    bytes.splice(next_at..next_at, nested.iter().copied());
    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand with nested recipe records");
    assert_eq!(operand.next_byte_offset, (next_at + nested.len()) as u64);
}

#[test]
fn class_367_body_recipe_operand_decodes_scale_member_frame() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"367", 100);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&301u64.to_le_bytes());
    bytes.extend_from_slice(&33u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&103u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "53aa8ab4-194a-434b-bd52-8c6d761dc147");
    lp_utf16(&mut bytes, "8e685642-4d68-4909-96d0-0dd4437491b6");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    header(&mut bytes, *b"264", 100);
    header(&mut bytes, *b"404", 101);
    header(&mut bytes, *b"416", 102);
    header(&mut bytes, *b"424", 103);
    let recipe_at = bytes.len();
    bytes.extend_from_slice(b"body_recipe_data");
    let next_at = bytes.len();
    header(&mut bytes, *b"280", 104);

    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 80,
        scope_reference_ordinal: 1,
        record_index: 90,
        byte_offset: 0,
        class_tag: "287".into(),
        members: vec![100],
        lost_edge_references: Vec::new(),
        member_offsets: vec![21],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 0,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role: 0x0000_0004_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 0,
        paired_class_tag: "264".into(),
        paired_byte_offset: 0,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "367".into(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: recipe_at as u64,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design_id: Some("301".into()),
        design_id_offset: None,
        design_selector: Some(crate::records::ConstructionRecipeSelector {
            value: 6,
            byte_offset: 0,
        }),
        recipe_index: 0,
        record_index: 0,
    };

    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("class-367 body recipe operand");
    assert_eq!(operand.references.len(), 1);
    assert_eq!(operand.references[0].design_reference, 301);
    assert_eq!(operand.references[0].form, 33);
    assert_eq!(operand.selector_tail, Some([1, 0, 0, 0]));
    assert_eq!(operand.selector_tail_offset, Some(208));
    assert_eq!(operand.nested_record_index, 103);
    assert_eq!(operand.next_byte_offset, next_at as u64);
}

#[test]
fn topology_operands_follow_consecutive_nested_records_to_their_recipes() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) -> u64 {
        let offset = u64::try_from(bytes.len()).expect("generated frame length fits u64");
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
        offset
    }

    let mut bytes = Vec::new();
    header(&mut bytes, *b"306", 100);
    let paired_at = header(&mut bytes, *b"259", 100);
    header(&mut bytes, *b"408", 101);
    header(&mut bytes, *b"414", 102);
    let recipe_record_at = header(&mut bytes, *b"423", 103);
    // A recipe prefix can contain header-shaped scalar bytes. The consumer's
    // exact closing index, not the first header-like run, closes the envelope.
    header(&mut bytes, *b"122", 0);
    let recipe_name_at = bytes.len() + 4;
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(b"edge_recipe_data");
    for value in [-1i32, -1, 2, 0, -1, 1, -1, 7] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let next_at = header(&mut bytes, *b"306", 104);
    let scope = DesignParameterScope {
        id: "f3d:Design/BulkStream.dat:scope#1".into(),
        byte_offset: 1000,
        class_tag: "301".into(),
        record_index: 1,
        frame_length: 200,
        kind: "Fillet".into(),
        kind_offset: 1100,
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
        reference_count_offset: 1080,
        reference_members: vec![100],
        reference_member_offsets: vec![1085],
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
        circular_pattern_construction: None,
        rectangular_pattern_construction: None,
        assembly_alignment: None,
        component_insert_construction: None,
        derived_instance_construction: None,
        copy_paste_component_operation: None,
        mirror_construction: None,
        base_flange_profile: None,
        entity_id: None,
        entity_suffix: None,
        entity_reference_offset: None,
        paired_class_tag: "261".into(),
        paired_byte_offset: 1200,
    };
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: "306".into(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#60".into(),
        byte_offset: recipe_name_at as u64,
        record_index_offset: Some(recipe_record_at + 8),
        kind: ConstructionRecipeKind::Edge,
        design_id: None,
        design_id_offset: None,
        design_selector: None,
        recipe_index: 7,
        record_index: 303,
    };

    let mut edge_operand = parse_edge_operand(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("edge recipe operand");
    assert_eq!(edge_operand.record_index, 100);
    assert_eq!(edge_operand.paired_byte_offset, paired_at);
    assert_eq!(edge_operand.recipe_record_index, 103);
    assert_eq!(edge_operand.recipe_record_byte_offset, recipe_record_at);
    assert_eq!(edge_operand.recipe_id, recipe.id);
    assert_eq!(edge_operand.resolved_edge_slot, None);
    bytes[next_at as usize + 7..next_at as usize + 11].copy_from_slice(&105u32.to_le_bytes());
    let mut work_point_scope = scope.clone();
    work_point_scope.kind = "WorkPoint".into();
    let work_point_operand = parse_edge_operand(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &work_point_scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("WorkPoint edge recipe operand");
    assert_eq!(work_point_operand.next_record_index, 105);
    bytes[next_at as usize + 7..next_at as usize + 11].copy_from_slice(&107u32.to_le_bytes());
    let mut sweep_scope = scope.clone();
    sweep_scope.kind = "Sweep".into();
    let sweep_operand = parse_edge_operand(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &sweep_scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        None,
    )
    .expect("Sweep edge recipe operand");
    assert_eq!(sweep_operand.next_record_index, 107);
    bytes[next_at as usize + 7..next_at as usize + 11].copy_from_slice(&160u32.to_le_bytes());
    assert_eq!(
        parse_edge_operand(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            0,
            &record,
            std::slice::from_ref(&recipe),
            None,
        ),
        None
    );
    assert_eq!(
        parse_edge_operand(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            &scope,
            0,
            &record,
            std::slice::from_ref(&recipe),
            Some(recipe.byte_offset),
        ),
        None
    );
    let terminal_group_operand = parse_edge_operand(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &scope,
        0,
        &record,
        std::slice::from_ref(&recipe),
        Some(u64::try_from(bytes.len()).expect("generated stream length fits u64")),
    )
    .expect("terminal construction-group edge recipe operand");
    assert_eq!(terminal_group_operand.next_record_index, 160);
    assert_eq!(terminal_group_operand.next_byte_offset, next_at);

    let mut vertex_bytes = Vec::new();
    header(&mut vertex_bytes, *b"369", 200);
    let vertex_paired_at = header(&mut vertex_bytes, *b"261", 200);
    header(&mut vertex_bytes, *b"408", 201);
    header(&mut vertex_bytes, *b"414", 202);
    let vertex_recipe_record_at = header(&mut vertex_bytes, *b"423", 203);
    let vertex_recipe_name_at = vertex_bytes.len() + 4;
    vertex_bytes.extend_from_slice(&18u32.to_le_bytes());
    vertex_bytes.extend_from_slice(b"vertex_recipe_data");
    for value in [-1i32, 3, 1, -1, 2, -1, 0, -1, 0, 0] {
        vertex_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let vertex_next_at = header(&mut vertex_bytes, *b"370", 205);
    let vertex_header = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#200".into(),
        byte_offset: 0,
        class_tag: "369".into(),
        record_index: 200,
    };
    let vertex_recipe = ConstructionRecipe {
        id: "f3d:Design/BulkStream.dat:construction-recipe#200".into(),
        byte_offset: u64::try_from(vertex_recipe_name_at).expect("generated offset fits u64"),
        record_index_offset: Some(vertex_recipe_record_at + 8),
        kind: ConstructionRecipeKind::Vertex,
        design_id: None,
        design_id_offset: None,
        design_selector: None,
        recipe_index: 9,
        record_index: 303,
    };
    let parsed_vertex = parse_vertex_recipe(
        &vertex_bytes,
        &IndexedRecordOffsets::build(&vertex_bytes),
        crate::ids::native_stream(&scope.id).expect("scope stream"),
        &vertex_header,
        std::slice::from_ref(&vertex_recipe),
    )
    .expect("WorkPoint vertex recipe operand");
    assert_eq!(parsed_vertex.paired_byte_offset, vertex_paired_at);
    assert_eq!(parsed_vertex.recipe_record_index, 203);
    assert_eq!(parsed_vertex.next_record_index, 205);
    assert_eq!(parsed_vertex.next_byte_offset, vertex_next_at);
    assert_eq!(
        parsed_vertex.recipe_program,
        [-1, 3, 1, -1, 2, -1, 0, -1, 0, 0]
    );
    edge_operand.terminal_reference_edge_slots = vec![vec![17], vec![18, 19]];
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand),
        vec![&[17][..], &[18, 19][..]]
    );
    let reference_context = |reference_ordinal, changed_reference_edge_slots| {
        crate::records::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces: Vec::new(),
            result_face_boundaries: Vec::new(),
            result_shared_edge_slots: Vec::new(),
            preceding_faces: Vec::new(),
            preceding_face_boundaries: Vec::new(),
            preceding_support_face_slots: Vec::new(),
            preceding_support_face_boundaries: Vec::new(),
            shared_edge_slots: Vec::new(),
            changed_shared_edge_slots: Vec::new(),
            changed_reference_edge_slots,
        }
    };
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, vec![17]),
        reference_context(1, vec![18, 19]),
    ];
    edge_operand.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(2).unwrap(),
        std::num::NonZeroU32::new(1).unwrap(),
        std::num::NonZeroU32::new(2).unwrap(),
    ]);
    assert_eq!(
        crate::design::edge_resolve::edge_operand_reference_edge_sets(&edge_operand),
        vec![&[18, 19][..], &[17][..], &[18, 19][..]]
    );
    edge_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![17]),
    ];
    let mut second_changed_operand = edge_operand.clone();
    second_changed_operand.recipe_reference_contexts = vec![
        reference_context(0, Vec::new()),
        reference_context(1, vec![18]),
    ];
    assert_eq!(
        crate::design::edge_resolve::changed_reference_edge_group_candidates(&[
            &edge_operand,
            &second_changed_operand,
        ]),
        Some(vec![17, 18])
    );
    second_changed_operand.recipe_reference_contexts[0].changed_reference_edge_slots = vec![17];
    assert_eq!(
        crate::design::edge_resolve::changed_reference_edge_group_candidates(&[
            &edge_operand,
            &second_changed_operand,
        ]),
        None
    );
    edge_operand.recipe_reference_contexts.clear();
    edge_operand.local_topology_references = None;
    edge_operand.terminal_reference_edge_slots.clear();
    edge_operand.resolved_edge_slot = Some(17);
    assert_eq!(
        crate::design::edge_resolve::resolved_edge_operand(&edge_operand),
        None
    );
    edge_operand.resolved_edge_slot = None;
    edge_operand.changed_boundary_edge_slots = vec![17, 18];
    edge_operand.deleted_boundary_edge_slots = vec![17, 18];
    edge_operand.treatment_radius_candidates = vec![
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 17,
            radius: 3.0,
        },
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 18,
            radius: 3.0,
        },
    ];
    let second_operand = edge_operand.clone();
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            3.0
        ),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &second_operand],
            4.0
        ),
        None
    );
    let mut chain_left = edge_operand.clone();
    chain_left.treatment_radius_candidates.push(
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: 3.0,
        },
    );
    let mut chain_right = edge_operand.clone();
    chain_right.treatment_radius_candidates = vec![
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 19,
            radius: 3.0,
        },
        crate::records::DesignEdgeTreatmentRadiusCandidate {
            edge_slot: 20,
            radius: 3.0,
        },
    ];
    chain_right.deleted_boundary_edge_slots = vec![19, 20];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&chain_left, &chain_right],
            3.0
        ),
        Some(vec![17, 18, 19, 20])
    );
    let mut context_operand = edge_operand.clone();
    context_operand.treatment_radius_candidates.clear();
    context_operand.changed_boundary_edge_slots = vec![16, 17];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0
        ),
        None
    );
    context_operand.changed_boundary_edge_slots.clear();
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0
        ),
        Some(vec![17, 18])
    );
    context_operand.changed_boundary_edge_slots = vec![15, 16];
    assert_eq!(
        crate::design::edge_resolve::radius_edge_group_candidates(
            &[&edge_operand, &context_operand],
            3.0
        ),
        None
    );
    let mut resolved_operand = edge_operand.clone();
    resolved_operand.id = "resolved".into();
    resolved_operand.resolved_edge_slot = Some(17);
    let mut proven_operand = edge_operand.clone();
    proven_operand.resolved_edge_slot = Some(17);
    let recovered_group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "288".into(),
        members: vec![100],
        lost_edge_references: vec!["f3d:Design/BulkStream.dat:lost-edge#1".into()],
        member_offsets: vec![926],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 921,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![91],
            trailing_record_offsets: vec![950],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 968,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 972,
            variant: false,
        },
        role: 0x0000_0008_0000_0000,
        extrude_role: None,
        extrude_face_role: None,
        role_offset: 960,

        paired_class_tag: "259".into(),
        paired_byte_offset: 1_000,
    };
    let recovered = crate::design::edge_resolve::resolved_edge_group(
        &recovered_group,
        std::slice::from_ref(&recovered_group),
        std::slice::from_ref(&proven_operand),
        &[],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
    );
    assert!(matches!(
        recovered,
        cadmpeg_ir::features::EdgeSelection::Unresolved
    ));
    let mut terminal_group = recovered_group.clone();
    terminal_group.lost_edge_references.clear();
    terminal_group.members = vec![100, 104];
    let mut terminal_resolved = proven_operand.clone();
    terminal_resolved.recipe_state_id = Some(8);
    let mut terminal_unresolved = proven_operand.clone();
    terminal_unresolved.id = "f3d:Design/BulkStream.dat:edge-operand#104".into();
    terminal_unresolved.record_index = 104;
    terminal_unresolved.recipe_state_id = Some(8);
    terminal_unresolved.resolved_edge_slot = None;
    terminal_unresolved.changed_boundary_edge_slots.clear();
    terminal_unresolved.deleted_boundary_edge_slots.clear();
    terminal_unresolved.treatment_radius_candidates.clear();
    terminal_unresolved.recipe_selectors = vec![crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: vec![18, 19],
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: vec![18, 19],
    }];
    let terminal = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[terminal_resolved, terminal_unresolved.clone()],
        &[],
        None,
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
    );
    assert!(
        matches!(terminal, cadmpeg_ir::features::EdgeSelection::Native(_)),
        "{terminal:?}"
    );
    let identity = |record_index, ordinal, edge| DesignEdgeIdentityOperand {
        id: format!("f3d:Design/BulkStream.dat:edge-identity#{record_index}"),
        scope_record_index: 1,
        group_record_index: 90,
        group_member_ordinal: ordinal,
        record_index,
        byte_offset: u64::from(record_index),
        class_tag: "297".into(),
        compact_layout: false,
        local_id: u64::from(record_index),
        local_id_offset: 0,
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        historical_entity_kind: None,
        historical_entity_ref: None,
        historical_state_ids: Vec::new(),
        treatment_radius_candidates: Vec::new(),
        transition_edge_candidates: Vec::new(),
        resolved_edge_slots: Vec::new(),
        resolved_edge_slot: edge,
        resolution_identity_id: None,
    };
    let mut recipe_unresolved = proven_operand.clone();
    recipe_unresolved.resolved_edge_slot = None;
    recipe_unresolved.recipe_state_id = Some(8);
    recipe_unresolved.changed_boundary_edge_slots.clear();
    let merged = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone(), terminal_unresolved.clone()],
        &[identity(100, 0, Some(17)), identity(104, 1, None)],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
    );
    assert!(matches!(
        merged,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let complete = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone(), terminal_unresolved],
        &[identity(100, 0, Some(17)), identity(104, 1, Some(18))],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
    );
    assert!(matches!(
        complete,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let mut first_rule = identity(100, 0, None);
    first_rule.resolved_edge_slots = vec![17, 18];
    let mut second_rule = identity(104, 1, None);
    second_rule.resolved_edge_slots = vec![18, 19];
    let face_rules = crate::design::edge_resolve::resolved_edge_group(
        &terminal_group,
        std::slice::from_ref(&terminal_group),
        &[recipe_unresolved.clone()],
        &[first_rule, second_rule],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
    );
    assert!(matches!(
        face_rules,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges == &[
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:18").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:8:19").expect("identity grammar"),
            ]
    ));
    let mut chain_group = terminal_group.clone();
    chain_group.members = vec![100];
    let mut chain_recipe = recipe_unresolved.clone();
    chain_recipe.changed_boundary_edge_slots = vec![17, 18];
    let mut chain_identity = identity(100, 0, None);
    chain_identity.transition_edge_candidates = vec![18, 17];
    let chain = crate::design::edge_resolve::resolved_edge_treatment_group(
        &chain_group,
        std::slice::from_ref(&chain_group),
        &[chain_recipe],
        &[chain_identity],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into()),
        None,
    );
    assert!(matches!(
        chain,
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    assert_eq!(
        edge_operand.recipe_program_offset,
        recipe_name_at as u64 + 16
    );
    assert_eq!(edge_operand.recipe_program, [-1, -1, 2, 0, -1, 1, -1, 7]);
    assert!(edge_operand.recipe_structure.is_none());
    let structured = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 1, 1, 5, 4, 4, 4, 4, 3, 4, -1,
        3, 0, -1, 1, -1, 3, -1, 0, 1, 2, 5, 3, 3, 3, 1, 1, 1, -1,
    ])
    .expect("standard two-side recipe structure");
    assert_eq!(structured.root, 2);
    assert_eq!(structured.sides[0].field_count.get(), 3);
    assert_eq!(structured.sides[0].header_value, 0);
    assert_eq!(structured.sides[0].scalars, [2, 1]);
    assert_eq!(structured.sides[0].payload_entry_count, 1);
    assert_eq!(structured.sides[0].entries[0].selector, 1);
    assert_eq!(structured.sides[0].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].middle,
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].vertex_ordinal,
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].incident_edge_ordinal,
        Some(3)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[0].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Following)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1]
            .outer
            .get(),
        4
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].middle,
        3
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].incident_edge_ordinal,
        Some(2)
    );
    assert_eq!(
        structured.sides[0].entries[0].topology_triplets[1].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Preceding)
    );
    assert_eq!(structured.sides[1].field_count.get(), 3);
    assert_eq!(structured.sides[1].header_value, 0);
    assert_eq!(structured.sides[1].scalars, [1, 3]);
    assert_eq!(structured.sides[1].payload_entry_count, 1);
    assert_eq!(structured.sides[1].entries[0].selector, 2);
    assert_eq!(structured.sides[1].entries[0].boundary_edge_count.get(), 5);
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0]
            .outer
            .get(),
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[0].middle,
        3
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1]
            .outer
            .get(),
        1
    );
    assert_eq!(
        structured.sides[1].entries[0].topology_triplets[1].middle,
        1
    );
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 3),
        Some(
            [2, 1, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    assert!(
        crate::design::decode::operands::edge_recipe_local_topology_references(&structured, 2)
            .is_none()
    );
    let signed_middle =
        crate::design::decode::operands::edge_recipe_entries(&[1, 4, 1, -2, 1, 4, 4, 4])
            .expect("signed topology middle is retained");
    assert_eq!(signed_middle[0].topology_triplets[0].middle, -2);
    assert_eq!(
        signed_middle[0].topology_triplets[0].incident_edge_ordinal,
        None
    );
    let signed_face = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, -2, 1, 4, 4, 4, -1, 3, 0, -1,
        0, -1, 0, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1,
    ])
    .expect("signed face-node topology recipe structure");
    assert_eq!(
        signed_face.sides[0].entries[0].topology_triplets[0].middle,
        -2
    );
    assert_eq!(
        signed_face.sides[0].entries[0].topology_triplets[0].incident_edge_ordinal,
        None
    );
    let postlude_face = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 0, -1, 0, -1, 0, 1, 1, 4, 1, -2, 1, 4, 4, 4, -1, 3, 0, -1,
        0, -1, 0, -1, 0, 1, 1, 4, 1, 1, 1, 4, 4, 4, -1, 4, -1, 0, 0, -1,
    ])
    .expect("face-node topology postlude");
    assert_eq!(postlude_face.postlude, [-1, 4, -1, 0, 0, -1]);
    let unambiguous_payload_face = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 1, -1, 0, -1, 0, 2, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
        1, 1, 1, -1, 3, 0, -1, 0, -1, 1, -1, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, -1,
    ])
    .expect("face-node payload prefix grammar");
    assert_eq!(unambiguous_payload_face.sides[0].entries.len(), 2);
    let ambiguous_extended_payload_face =
        crate::design::decode::operands::face_recipe_structure(&[
            0, -1, 1, -1, 2, -1, 3, 0, -1, 1, -1, 0, -1, 0, 2, -1, 1, 1, 1, 1, 1, 1, 1, 1, -1, 3,
            0, -1, 1, -1, 0, -1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, -1,
        ]);
    assert!(ambiguous_extended_payload_face.is_none());
    let mut referenced_headers = structured.clone();
    referenced_headers.sides[0].header_value = 2;
    referenced_headers.sides[1].header_value = 3;
    assert_eq!(
        crate::design::decode::operands::edge_recipe_local_topology_references(
            &referenced_headers,
            3
        ),
        Some(
            [2, 2, 1, 3, 1, 3]
                .into_iter()
                .map(|value| std::num::NonZeroU32::new(value).unwrap())
                .collect()
        )
    );
    let wrap =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 0, 1, 1, 1, 1]).unwrap();
    assert_eq!(wrap[0].topology_triplets[0].vertex_ordinal, 0);
    assert_eq!(wrap[0].topology_triplets[0].incident_edge_ordinal, Some(4));
    assert_eq!(wrap[0].common_incident_edge_ordinal, None);
    assert_eq!(
        wrap[0].topology_triplets[0].incident_side,
        Some(crate::records::DesignTopologyIncidentSide::Preceding)
    );
    let common =
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 1, 1, 1, 1, 1, 1]).unwrap();
    assert_eq!(common[0].common_incident_edge_ordinal, Some(0));
    let underived =
        crate::design::decode::operands::edge_recipe_entries(&[0, 6, 6, 4, 6, 1, 1, 1]).unwrap();
    assert_eq!(underived[0].topology_triplets[0].vertex_ordinal, 5);
    assert_eq!(
        underived[0].topology_triplets[0].incident_edge_ordinal,
        None
    );
    assert_eq!(underived[0].topology_triplets[0].incident_side, None);
    assert_eq!(
        crate::design::decode::operands::edge_recipe_entries(&[3, 5, 1, 1, 1, 2, 1, 2]).unwrap()[0]
            .selector,
        3
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[-1, 5, 1, 1, 1, 2, 1, 2]).is_none()
    );
    assert!(
        crate::design::decode::operands::edge_recipe_entries(&[1, 5, 6, 5, 6, 2, 1, 2]).is_none()
    );
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        1, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    assert!(crate::design::decode::operands::edge_recipe_entries(&[
        2, 5, 1, 1, 1, 2, 1, 2, 1, 5, 2, 1, 2, 3, 2, 3,
    ])
    .is_none());
    let extended = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 2, -1, 1, -1, 0, -1, 0, 0, -1, 4, 3, -1, 0, -1, 1, -1,
        4, -1, 0, 0, -1,
    ])
    .expect("recipe structure with a third scalar on its second side");
    assert_eq!(extended.sides[0].scalars, [1, 0]);
    assert_eq!(extended.sides[1].scalars, [0, 1, 4]);
    assert_eq!(extended.sides[1].field_count.get(), 4);
    assert!(extended.sides[0].entries.is_empty());
    assert!(extended.sides[1].entries.is_empty());
    let zero_delimited = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 1, 0, 0, 0, 2, 0, 0, 0, -1, 4, 1, 0, 3, 0, 4, 0, 0, 0, 0,
        1, 2, 3, 2, 1, 2, 1, 1, 1, -1,
    ])
    .expect("recipe structure with zero-delimited side fields");
    assert_eq!(zero_delimited.root, 2);
    assert_eq!(zero_delimited.sides[0].field_count.get(), 3);
    assert_eq!(zero_delimited.sides[0].header_value, 1);
    assert_eq!(zero_delimited.sides[0].scalars, [0, 2]);
    assert!(zero_delimited.sides[0].entries.is_empty());
    assert_eq!(zero_delimited.sides[1].field_count.get(), 4);
    assert_eq!(zero_delimited.sides[1].scalars, [3, 4, 0]);
    assert_eq!(zero_delimited.sides[1].entries.len(), 1);
    assert_eq!(zero_delimited.sides[1].entries[0].selector, 2);
    assert_eq!(
        zero_delimited.sides[1].entries[0].boundary_edge_count.get(),
        3
    );
    let mixed_delimiters = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, -1, 2, -1, 3, 2, 0, 1, -1, 0, 0, 0, 0, -1, 3, 0, 0, 1, -1, 3, 0, 0, 0,
        -1,
    ])
    .expect("recipe structure with field-local delimiters");
    assert_eq!(mixed_delimiters.root, 2);
    assert_eq!(mixed_delimiters.sides[0].header_value, 2);
    assert_eq!(mixed_delimiters.sides[0].scalars, [1, 0]);
    assert_eq!(mixed_delimiters.sides[1].header_value, 0);
    assert_eq!(mixed_delimiters.sides[1].scalars, [1, 3]);
    let revolution_axis = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, 0, 1, 0, 2, -1, 3, 0, 0, 2, -1, 1, 0, 0, 1, 1, 7, 1, 1, 1, 4, 4, 4, -1, 3, 0,
        0, 1, 0, 3, 0, 0, 0, 0,
    ])
    .expect("revolution-axis edge recipe structure");
    assert_eq!(revolution_axis.sides[0].scalars, [2, 1]);
    assert_eq!(revolution_axis.sides[0].entries.len(), 1);
    assert!(revolution_axis.sides[1].entries.is_empty());
    let variable_scalars = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 5, 1, -1, 0, -1, 2, -1, 3, -1, 4, -1, 0, 0, -1, 3, 0, -1,
        1, -1, 2, -1, 0, 0, -1,
    ])
    .expect("recipe structure with four scalar fields");
    assert_eq!(variable_scalars.sides[0].field_count.get(), 5);
    assert_eq!(variable_scalars.sides[0].scalars, [0, 2, 3, 4]);
    let extended_payload = crate::design::decode::operands::edge_recipe_structure(&[
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 2, 3, -1, 0, 0, -1, 4, -1, 0, 0,
        -1, 1, 0, 4, 1, 1, 1, 2, 2, 2, -1, 3, 0, -1, 1, -1, 2, -1, 0, 0, -1,
    ])
    .expect("recipe structure with an extended payload field program");
    assert_eq!(
        extended_payload.sides[0].payload_prefix,
        [2, 3, -1, 0, 0, -1, 4, -1, 0, 0, -1]
    );
    assert_eq!(extended_payload.sides[0].entries.len(), 1);
    let surface_patch = crate::design::decode::operands::surface_patch_recipe_structure(
        &[
            0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0, 2,
            1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0, -1,
        ],
        4,
    )
    .expect("SurfacePatch two-clause recipe structure");
    assert_eq!(surface_patch.root, 2);
    assert_eq!(surface_patch.clauses.len(), 2);
    assert_eq!(surface_patch.clauses[0].face_reference_ordinals, [2, 1]);
    assert_eq!(surface_patch.clauses[0].edge_reference_ordinals, [0, 2]);
    assert_eq!(surface_patch.clauses[0].payload_entry_count, 1);
    assert_eq!(surface_patch.clauses[0].entries[0].selector, 0);
    assert_eq!(surface_patch.clauses[1].face_reference_ordinals, [3, 1]);
    assert_eq!(surface_patch.clauses[1].edge_reference_ordinals, [0, 3]);
    assert_eq!(surface_patch.clauses[1].payload_entry_count, 0);
    assert!(surface_patch.clauses[1].entries.is_empty());
    assert!(
        crate::design::decode::operands::surface_patch_recipe_structure(
            &[
                0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0,
                2, 1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0,
                -1,
            ],
            3,
        )
        .is_none()
    );
    assert!(
        crate::design::decode::operands::surface_patch_recipe_structure(
            &[
                0, -1, 1, 1, -1, 2, -1, 2, 2, -1, 1, -1, 2, 0, -1, 0, 0, -1, 2, -1, 0, 0, -1, 1, 0,
                2, 1, 1, 1, 2, 1, 2, -1, 2, 3, -1, 1, -1, 2, 0, -1, 0, 0, -1, 3, -1, 0, 0, -1, 0,
                7,
            ],
            4,
        )
        .is_none()
    );
    let mut surface_patch_operand = edge_operand.clone();
    surface_patch_operand.surface_patch_recipe_structure = Some(surface_patch.clone());
    surface_patch_operand.recipe_state_id = Some(8);
    surface_patch_operand.resolved_edge_slot = Some(17);
    let mut surface_patch_group = terminal_group.clone();
    surface_patch_group.members = vec![100];
    let surface_selection = crate::design::edge_resolve::resolved_edge_group(
        &surface_patch_group,
        std::slice::from_ref(&surface_patch_group),
        std::slice::from_ref(&surface_patch_operand),
        &[],
        Some(8),
        &cadmpeg_ir::features::FeatureId("f3d:model:feature#surface-patch".into()),
    );
    assert!(matches!(
        surface_selection,
        cadmpeg_ir::features::EdgeSelection::Historical { ref edges, .. }
            if edges == &[cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#13:surface-patch:8:17").expect("identity grammar")]
    ));
    surface_patch_operand.resolved_edge_slot = None;
    assert!(matches!(
        crate::design::edge_resolve::resolved_edge_group(
            &surface_patch_group,
            std::slice::from_ref(&surface_patch_group),
            std::slice::from_ref(&surface_patch_operand),
            &[],
            Some(8),
            &cadmpeg_ir::features::FeatureId("f3d:model:feature#surface-patch".into()),
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let face = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0, 0, -1,
    ])
    .expect("face node topology recipe structure");
    assert_eq!(face.root, 0);
    assert_eq!(face.prelude, [1, 2]);
    assert_eq!(face.sides[0].field_count.get(), 3);
    assert_eq!(face.sides[0].header_value, 0);
    assert_eq!(face.sides[0].scalars, [2, 1]);
    assert_eq!(face.sides[1].field_count.get(), 3);
    assert_eq!(face.sides[1].header_value, 0);
    assert_eq!(face.sides[1].scalars, [1, 3]);
    let zero_delimited_face = crate::design::decode::operands::face_recipe_structure(&[
        0, 0, 1, 0, 2, -1, 3, 0, 0, 2, 0, 1, 0, 0, 0, -1, 3, 0, 0, 1, 0, 3, 0, 0, 0, -1,
    ])
    .expect("zero-delimited face node topology recipe structure");
    assert_eq!(zero_delimited_face, face);
    assert_eq!(edge_operand.next_record_index, 104);
    assert_eq!(edge_operand.next_byte_offset, next_at);
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:asm:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
                ),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#xref").expect("identity grammar"),
                ),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        edge_operand.candidate_faces,
        [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")]
    );
    let mut local_recipe = recipe.clone();
    local_recipe.record_index = -1335;
    bind_edge_operand_candidates(
        std::slice::from_mut(&mut edge_operand),
        std::slice::from_ref(&local_recipe),
        &[PersistentSubentityTag {
            id: "f3d:asm:persistent-subentity-tag#1".into(),
            target: AttributeTarget::Face(
                FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
            ),
            selector: 1,
            token: "3".into(),
            design_references: vec![303],
            ordinal: 0,
        }],
    );
    assert!(edge_operand.candidate_faces.is_empty());
    let mut embedded_program = vec![99];
    embedded_program.extend_from_slice(&edge_operand.recipe_program[7..]);
    embedded_program.push(88);
    let dimension_recipe = DesignDimensionRecipeRecord {
        id: "f3d:Design/BulkStream.dat:dimension-recipe#1".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "recipe".into(),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: "423".into(),
        record_index: 1,
        frame_length: 4,
        prefix_offset: 0,
        prefix_bytes: vec![1],
        references: Vec::new(),
        program_offset: 0,
        program: embedded_program,
        matching_edge_operand_ids: Vec::new(),
    };
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            std::slice::from_ref(&edge_operand),
        ),
        [edge_operand.id.clone()]
    );
    let mut other_stream_operand = edge_operand.clone();
    other_stream_operand.id = "f3d:Other/BulkStream.dat:edge-operand#100".into();
    assert_eq!(
        crate::design::decode::dimension_frames::dimension_recipe_matching_edge_operand_ids(
            &dimension_recipe,
            &[edge_operand.clone(), other_stream_operand],
        ),
        [edge_operand.id.clone()]
    );

    let mut face_bytes = Vec::new();
    header(&mut face_bytes, *b"306", 100);
    let face_paired_at = header(&mut face_bytes, *b"259", 100);
    header(&mut face_bytes, *b"408", 101);
    header(&mut face_bytes, *b"414", 102);
    let face_recipe_record_at = header(&mut face_bytes, *b"423", 103);
    let face_recipe_name_at = face_bytes.len() + 4;
    face_bytes.extend_from_slice(&24u32.to_le_bytes());
    face_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, -1, -1, 2, 7, -1, -1, 2, 8, -1, -1, 2, 9] {
        face_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let face_next_at = header(&mut face_bytes, *b"306", 104);
    let mut face_scope = scope;
    face_scope.kind = "Extrude".into();
    let mut face_recipe = recipe;
    face_recipe.kind = ConstructionRecipeKind::BoundedFace;
    face_recipe.design_id = Some("303".into());
    face_recipe.byte_offset = face_recipe_name_at as u64;
    face_recipe.record_index_offset = Some(face_recipe_record_at + 8);
    let mut operand = parse_face_operand(
        &face_bytes,
        &IndexedRecordOffsets::build(&face_bytes),
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand");
    assert_eq!(operand.record_index, 100);
    assert_eq!(operand.paired_byte_offset, face_paired_at);
    assert_eq!(operand.recipe_record_index, 103);
    assert_eq!(operand.recipe_kind, ConstructionRecipeKind::BoundedFace);
    assert_eq!(operand.recipe_id, face_recipe.id);
    assert!(operand.resolved_face_slots.is_empty());
    assert_eq!(
        operand.recipe_program_offset,
        face_recipe_name_at as u64 + 24
    );
    assert_eq!(operand.recipe_program[0..3], [0, -1, 4]);
    let face_program_at = face_recipe_name_at + 24;
    face_bytes[face_program_at + 4..face_program_at + 8].copy_from_slice(&0i32.to_le_bytes());
    let zero_prelude = parse_face_operand(
        &face_bytes,
        &IndexedRecordOffsets::build(&face_bytes),
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("zero-prelude face recipe operand");
    assert_eq!(zero_prelude.recipe_program[0..3], [0, 0, 4]);
    assert_eq!(
        face_recipe_program_kind(&zero_prelude.recipe_program),
        Some(FaceRecipeProgramKind::Counted { header_value: 4 })
    );
    assert_eq!(
        operand.recipe_node_offsets,
        [
            face_recipe_name_at as u64 + 36,
            face_recipe_name_at as u64 + 52,
            face_recipe_name_at as u64 + 68,
        ]
    );
    assert_eq!(operand.recipe_nodes.len(), 3);
    assert_eq!(
        operand.recipe_nodes[0].byte_offset,
        face_recipe_name_at as u64 + 36
    );
    assert_eq!(
        operand.recipe_nodes[0].end_byte_offset,
        face_recipe_name_at as u64 + 52
    );
    assert_eq!(operand.recipe_nodes[0].program, [-1, -1, 2, 7]);
    assert_eq!(operand.next_record_index, 104);
    assert_eq!(operand.next_byte_offset, face_next_at);

    let mut prelude_bytes = face_bytes.clone();
    let prelude_words = [4i32, 5, 6, 7];
    let prelude_bytes_at = prelude_words
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    prelude_bytes.splice(
        face_program_at + 12..face_program_at + 12,
        prelude_bytes_at.iter().copied(),
    );
    let prelude = parse_face_operand(
        &prelude_bytes,
        &IndexedRecordOffsets::build(&prelude_bytes),
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe operand with counted prelude");
    assert_eq!(prelude.recipe_program[0..7], [0, 0, 4, 4, 5, 6, 7]);
    assert_eq!(
        prelude.recipe_node_offsets[0],
        prelude.recipe_program_offset + 28
    );
    assert_eq!(prelude.recipe_nodes[0].program, [-1, -1, 2, 7]);

    let enclosing_limit = header(&mut face_bytes, *b"306", 105);
    let bounded = parse_face_operand(
        &face_bytes,
        &IndexedRecordOffsets::build(&face_bytes),
        &face_scope,
        0,
        None,
        Some(enclosing_limit),
        &record,
        std::slice::from_ref(&face_recipe),
    )
    .expect("face recipe bounded before its enclosing member limit");
    assert_eq!(bounded.next_record_index, 104);
    assert_eq!(bounded.next_byte_offset, face_next_at);

    let mut compact_bytes = Vec::new();
    header(&mut compact_bytes, *b"306", 100);
    header(&mut compact_bytes, *b"259", 100);
    header(&mut compact_bytes, *b"408", 101);
    header(&mut compact_bytes, *b"414", 102);
    let compact_record_at = header(&mut compact_bytes, *b"423", 103);
    let compact_name_at = compact_bytes.len() + 4;
    compact_bytes.extend_from_slice(&24u32.to_le_bytes());
    compact_bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1, 4, 1, -1, 1, 0, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let mut compact_recipe = face_recipe.clone();
    compact_recipe.byte_offset = compact_name_at as u64;
    compact_recipe.record_index_offset = Some(compact_record_at + 8);
    let compact = parse_face_operand(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("compact face recipe operand");
    assert_eq!(compact.recipe_program, [0, -1, 4, 1, -1, 1, 0, -1]);
    assert!(compact.recipe_nodes.is_empty());

    let terminal_program_at = compact_name_at + 24;
    compact_bytes.truncate(terminal_program_at);
    for value in [0i32, -1] {
        compact_bytes.extend_from_slice(&value.to_le_bytes());
    }
    header(&mut compact_bytes, *b"306", 104);
    let terminal = parse_face_operand(
        &compact_bytes,
        &IndexedRecordOffsets::build(&compact_bytes),
        &face_scope,
        0,
        None,
        None,
        &record,
        std::slice::from_ref(&compact_recipe),
    )
    .expect("terminal face recipe operand");
    assert_eq!(terminal.recipe_program, [0, -1]);
    assert!(terminal.recipe_nodes.is_empty());
    assert_eq!(
        face_recipe_program_kind(&terminal.recipe_program),
        Some(FaceRecipeProgramKind::Terminal)
    );
    assert_eq!(
        face_recipe_program_kind(&[0, 0]),
        Some(FaceRecipeProgramKind::Terminal)
    );
    assert_eq!(face_recipe_program_kind(&[0, 1, 4]), None);
    assert_eq!(face_recipe_program_kind(&[0, -1, 0]), None);
    operand.recipe_references.push(DesignRecipeReference {
        selector: 1,
        selector_offset: 1_101,
        token: "3".into(),
        token_offset: 1,
        design_reference: 303,
        design_reference_offset: 2,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    });
    bind_face_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&face_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
                ),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#51").expect("identity grammar"),
                ),
                selector: 1,
                token: "4".into(),
                design_references: vec![303],
                ordinal: 1,
            },
            PersistentSubentityTag {
                id: "f3d:xref/other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("f3d:brep:entity#xref").expect("identity grammar"),
                ),
                selector: 1,
                token: "3".into(),
                design_references: vec![303],
                ordinal: 0,
            },
        ],
    );
    assert_eq!(
        operand.candidate_faces,
        [
            FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
            FaceId::mint("f3d:brep:entity#51").expect("identity grammar")
        ]
    );
    assert_eq!(
        operand.unreferenced_candidate_faces,
        [FaceId::mint("f3d:brep:entity#51").expect("identity grammar")]
    );
    let mut direct_face = operand.clone();
    direct_face.recipe_kind = ConstructionRecipeKind::Face;
    direct_face.recipe_references = vec![DesignRecipeReference {
        selector: 1,
        selector_offset: 1_201,
        token: "3".into(),
        token_offset: 1_202,
        design_reference: 303,
        design_reference_offset: 1_203,
        candidate_faces: vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }];
    direct_face.alternate_selector_candidate_faces.clear();
    direct_face.resolved_face_slots.clear();
    let group = DesignConstructionOperandGroup {
        id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
        scope_record_index: face_scope.record_index,
        scope_reference_ordinal: 0,
        record_index: 90,
        byte_offset: 900,
        class_tag: "306".into(),
        members: vec![operand.record_index],
        lost_edge_references: Vec::new(),
        member_offsets: vec![924],
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 920,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: vec![91],
            trailing_record_offsets: vec![935],
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 954,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 958,
            variant: false,
        },
        role: 0x0000_0011_0000_0000,
        extrude_role: Some(DesignExtrudeOperandRole::Faces),
        extrude_face_role: Some(DesignExtrudeFaceRole::Termination),
        role_offset: 946,

        paired_class_tag: "259".into(),
        paired_byte_offset: 980,
    };
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&direct_face)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")] && native == group.id
    ));
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#51").expect("identity grammar")] && native == group.id
    ));
    operand
        .unreferenced_candidate_faces
        .push(FaceId::mint("f3d:brep:entity#50").expect("identity grammar"));
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand)).is_none());
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_kind = ConstructionRecipeKind::BoundedFace;
    operand.recipe_nodes.clear();
    operand.recipe_nodes.push(DesignFaceRecipeNode {
        byte_offset: 1_200,
        end_byte_offset: 1_300,
        program: Vec::new(),
        recipe_structure: Some(DesignFaceRecipeStructure {
            root: 0,
            prelude: [0, 2],
            sides: [
                DesignTopologyRecipeSide {
                    field_count: std::num::NonZeroU32::new(3).unwrap(),
                    header_value: 0,
                    scalars: vec![0, 1],
                    payload_prefix: vec![0],
                    payload_entry_count: 0,
                    entries: Vec::new(),
                },
                DesignTopologyRecipeSide {
                    field_count: std::num::NonZeroU32::new(3).unwrap(),
                    header_value: 1,
                    scalars: vec![1, 0],
                    payload_prefix: vec![0],
                    payload_entry_count: 0,
                    entries: Vec::new(),
                },
            ],
            postlude: Vec::new(),
        }),
    });
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.unreferenced_candidate_faces && native == group.id
    ));
    operand.recipe_nodes[0].recipe_structure = None;
    assert!(resolved_face_group(&group, std::slice::from_ref(&operand)).is_none());
    operand.preceding_candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&operand),
        Some(50)
    );
    operand.resolved_face_slots = vec![50];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep:entity#50").expect("identity grammar")] && native == group.id
    ));
    let mut namespaced_slot = operand.clone();
    namespaced_slot.candidate_faces =
        vec![FaceId::mint("f3d:brep/example.smbh/brep:entity#50").expect("identity grammar")];
    namespaced_slot.unreferenced_candidate_faces.clear();
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&namespaced_slot)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == [FaceId::mint("f3d:brep/example.smbh/brep:entity#50").expect("identity grammar")]
                && native == group.id
    ));
    namespaced_slot.resolved_face_slots = vec![51];
    assert!(resolved_face_group(&group, std::slice::from_ref(&namespaced_slot)).is_none());
    let mut historical_face_scope = face_scope.clone();
    historical_face_scope.previous_history_state_id = Some(49);
    assert!(matches!(
        crate::design::feature_project::direct_face_selection(
            &historical_face_scope,
            std::slice::from_ref(&operand)
        ),
        Some(FaceSelection::Historical { state, faces, native })
            if state == feature_input_topology_id(&crate::ids::neutral_feature_id(&historical_face_scope), 49)
                && faces.len() == 1
                && faces[0].0.ends_with(":49:50")
                && native == historical_face_scope.id
    ));
    operand.resolved_face_slots.clear();
    assert!(crate::design::face_resolve::retain_face_operand_resolution(
        &group,
        std::slice::from_mut(&mut operand),
        &FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
    ));
    assert_eq!(operand.resolved_face_slots, [50]);
    operand.resolved_face_slots.clear();
    operand.alternate_selector_candidate_faces = vec![
        FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
        FaceId::mint("f3d:brep:entity#51").expect("identity grammar"),
    ];
    assert!(matches!(
        resolved_face_group(&group, std::slice::from_ref(&operand)),
        Some(FaceSelection::Resolved { faces, native })
            if faces == operand.alternate_selector_candidate_faces && native == group.id
    ));
    operand.alternate_selector_candidate_faces.clear();
    operand.resolved_face_slots = vec![50];
    let mut ambiguous = [operand.clone(), operand.clone()];
    assert!(
        !crate::design::face_resolve::retain_face_operand_resolution(
            &group,
            &mut ambiguous,
            &FaceId::mint("f3d:brep:entity#50").expect("identity grammar"),
        )
    );

    let split_structure = crate::design::decode::operands::face_recipe_structure(&[
        0, -1, 1, -1, 2, -1, 3, 0, -1, 2, -1, 1, -1, 0, 0, -1, 3, 0, -1, 1, -1, 3, -1, 0, 0, -1,
    ])
    .expect("split-face context recipe structure");
    let mut split_scope = face_scope.clone();
    split_scope.kind = "SplitFace".into();
    split_scope.previous_history_state_id = Some(49);
    let mut split_group = group.clone();
    split_group.scope_reference_ordinal = 2;
    split_group.role = 0x0000_0010_0000_0000;
    split_group.members = vec![operand.record_index, operand.record_index + 1];
    let mut split_selected = operand.clone();
    split_selected.group_record_index = Some(split_group.record_index);
    split_selected.group_member_ordinal = Some(0);
    split_selected.resolved_face_slots = vec![50];
    for node in &mut split_selected.recipe_nodes {
        node.recipe_structure = Some(split_structure.clone());
    }
    let mut split_context = split_selected.clone();
    split_context.record_index += 1;
    split_context.group_member_ordinal = Some(1);
    split_context.candidate_faces.clear();
    split_context.unreferenced_candidate_faces.clear();
    split_context.alternate_selector_candidate_faces.clear();
    split_context.preceding_candidate_faces.clear();
    split_context.changed_candidate_faces.clear();
    split_context.resolved_face_slots.clear();
    let nested_candidate = split_context
        .recipe_references
        .iter()
        .flat_map(|reference| {
            reference
                .candidate_faces
                .iter()
                .chain(&reference.alternate_selector_faces)
        })
        .next()
        .cloned()
        .expect("nested bounded-face candidate");
    let nested_slot = nested_candidate
        .0
        .rsplit_once('#')
        .and_then(|(_, slot)| slot.parse::<i64>().ok())
        .expect("nested bounded-face slot");
    split_context.changed_candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#999").expect("identity grammar")];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&split_context),
        None
    );
    split_context.changed_candidate_faces = vec![nested_candidate];
    assert_eq!(
        crate::design::face_resolve::resolve_face_operand_history_candidates(&split_context),
        Some(nested_slot)
    );
    split_context.changed_candidate_faces.clear();
    assert!(matches!(
        resolved_historical_split_face_target_group(
            &split_scope,
            &split_group,
            &[split_selected.clone(), split_context.clone()],
        ),
        Some(FaceSelection::Historical { state, faces, native })
            if state == feature_input_topology_id(&crate::ids::neutral_feature_id(&split_scope), 49)
                && faces.len() == 1
                && faces[0].0.ends_with(":49:50")
                && native == split_group.id
    ));
    let mut candidate_context = split_context.clone();
    candidate_context.candidate_faces =
        vec![FaceId::mint("f3d:brep:entity#50").expect("identity grammar")];
    candidate_context.unreferenced_candidate_faces = candidate_context.candidate_faces.clone();
    candidate_context.preceding_candidate_faces = candidate_context.candidate_faces.clone();
    candidate_context
        .recipe_nodes
        .push(candidate_context.recipe_nodes[0].clone());
    candidate_context.recipe_program = vec![0, -1, 2];
    assert!(resolved_historical_split_face_target_group(
        &split_scope,
        &split_group,
        &[split_selected.clone(), candidate_context],
    )
    .is_some());
    let mut unresolved_context = split_context;
    for reference in &mut unresolved_context.recipe_references {
        reference.candidate_faces.clear();
        reference.alternate_selector_faces.clear();
    }
    assert!(resolved_historical_split_face_target_group(
        &split_scope,
        &split_group,
        &[split_selected, unresolved_context],
    )
    .is_none());
}

#[test]
fn face_recipe_boundary_accepts_omitted_n_plus_four() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let mut ordinary = Vec::new();
    for record_index in 100..=104 {
        header(&mut ordinary, *b"306", record_index);
    }
    let ordinary_position = ordinary.len() - 11;
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &ordinary,
            ordinary_position,
            100,
            None,
        ),
        Some((ordinary_position, 104))
    );

    let mut omitted = Vec::new();
    for record_index in 100..=103 {
        header(&mut omitted, *b"306", record_index);
    }
    let position = omitted.len();
    header(&mut omitted, *b"124", 0);
    let next = omitted.len();
    header(&mut omitted, *b"317", 105);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(&omitted, position, 100, None),
        Some((next, 105))
    );

    let mut arbitrary = Vec::new();
    for record_index in 100..=103 {
        header(&mut arbitrary, *b"306", record_index);
    }
    let arbitrary_position = arbitrary.len();
    header(&mut arbitrary, *b"124", 205);
    assert_eq!(
        crate::design::decode::operands::face_recipe_next_boundary(
            &arbitrary,
            arbitrary_position,
            100,
            None,
        ),
        Some((arbitrary_position, 205))
    );
}
