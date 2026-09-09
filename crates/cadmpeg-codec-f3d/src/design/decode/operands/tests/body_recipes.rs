// SPDX-License-Identifier: Apache-2.0
use super::prelude::*;
use crate::records::topology::DesignConstructionOperandGroupFrame;
use crate::records::topology::DesignOperandRole;

#[test]
fn body_recipe_operand_decodes_counted_and_empty_reference_tables() {
    fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&class_tag);
        bytes.extend_from_slice(&record_index.to_le_bytes());
    }

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 0,
            record_index: 90,
            byte_offset: 900,
            class_tag: crate::records::DesignClassTag::try_from("269".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 100,
                offset: 926,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 921,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: vec![crate::records::Located {
                        value: 200,
                        offset: 943,
                    }],
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 971,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 975,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::ROLE_0X5,
            ),
            role_offset: 953,

            paired_class_tag: crate::records::DesignClassTag::try_from("265".to_owned()).unwrap(),
            paired_byte_offset: 1024,
        },
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("365".to_owned()).unwrap(),
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
        design: Some(crate::records::ConstructionRecipeDesign {
            id: crate::records::RecordedValue {
                value: "2265".into(),
                offset: None,
            },
            selector: Some(crate::records::ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: 0,
    };
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::DesignFeatureKind::BoundaryFill,
        80,
    );

    let mut operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand");
    assert_eq!(operand.references().len(), 2);
    assert_eq!(operand.references()[0].design_reference, 2265);
    assert_eq!(operand.references()[0].form, 3);
    assert_eq!(operand.references()[1].design_reference, 2266);
    assert_eq!(operand.references()[1].form, 32);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([7, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(220)
    );
    assert_eq!(
        operand.owner,
        crate::records::topology::DesignOperandOwner::Group {
            group_record_index: 90,
            group_member_ordinal: 0,
        }
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.recipe_id, recipe.id);
    assert_eq!(operand.next_byte_offset(), next_at as u64);
    operand.id = "f3d:Design/BulkStream.dat:body-recipe-operand#0".into();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut operand),
        std::slice::from_ref(&recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_ir::NonEmptyString::new("0").unwrap(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: cadmpeg_ir::NonEmptyString::new("0").unwrap(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:xref/Other/occurrence-0/design:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#other-stream").expect("identity grammar"),
                ),
                selector: 0,
                token: cadmpeg_ir::NonEmptyString::new("0").unwrap(),
                design_references: vec![2265],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&scope),
    );
    assert_eq!(
        operand.references()[0].candidate_faces,
        [
            FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
            FaceId::mint("test:model:face#same-stream").expect("identity grammar")
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
    assert!(empty.references().is_empty());
    assert_eq!(empty.nested_record_index(), 103);
    assert_eq!(empty.next_byte_offset(), empty_next_at as u64);

    let mut combine_scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#80",
        crate::records::feature::DesignFeatureKind::Combine,
        80,
    );
    if let crate::records::feature::DesignScopePayloadMut::Combine(slot) =
        combine_scope.payload_mut()
    {
        *slot = Some(crate::records::feature::DesignCombineOperation {
            form: crate::records::feature::DesignCombineForm::Standard,
            operation: cadmpeg_ir::features::BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            target_record_index: 0,
            tools: crate::records::feature::DesignCombineTools {
                first: crate::records::feature::DesignCombineBodySelection {
                    record_index: record.record_index,
                    external_identity: None,
                },
                additional: Vec::new(),
            },
        });
    }
    let mut combine_recipe = recipe.clone();
    combine_recipe.design.as_mut().unwrap().selector =
        Some(crate::records::ConstructionRecipeSelector {
            value: 1,
            byte_offset: 0,
        });
    let mut combine_operand = operand.clone();
    crate::design::decode::operands::bind_body_recipe_operand_candidates(
        std::slice::from_mut(&mut combine_operand),
        std::slice::from_ref(&combine_recipe),
        &[
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#1".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#same-stream").expect("identity grammar"),
                ),
                selector: 1,
                token: cadmpeg_ir::NonEmptyString::new("0").unwrap(),
                design_references: vec![2265],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "f3d:Design/BulkStream.dat:persistent-subentity-tag#2".into(),
                target: AttributeTarget::Face(
                    FaceId::mint("test:model:face#other-selector").expect("identity grammar"),
                ),
                selector: 2,
                token: cadmpeg_ir::NonEmptyString::new("0").unwrap(),
                design_references: vec![2265, 2266],
                ordinal: 0,
            },
        ],
        std::slice::from_ref(&combine_scope),
    );
    assert_eq!(
        combine_operand.references()[0].candidate_faces,
        [FaceId::mint("test:model:face#same-stream").expect("identity grammar")]
    );

    let mut nested = Vec::new();
    header(&mut nested, *b"302", 1);
    header(&mut nested, *b"305", 11);
    bytes.splice(next_at..next_at, nested.iter().copied());
    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("body recipe operand with nested recipe records");
    assert_eq!(operand.next_byte_offset(), (next_at + nested.len()) as u64);
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

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#90".into(),
            scope_record_index: 80,
            scope_reference_ordinal: 1,
            record_index: 90,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("287".to_owned()).unwrap(),
            members: vec![crate::records::Located {
                value: 100,
                offset: 21,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(
                DesignOperandRole::BODIES_A,
            ),
            role_offset: 0,
            paired_class_tag: crate::records::DesignClassTag::try_from("264".to_owned()).unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let record = DesignRecordHeader {
        id: "f3d:Design/BulkStream.dat:record#100".into(),
        byte_offset: 0,
        class_tag: crate::records::DesignClassTag::try_from("367".to_owned()).unwrap(),
        record_index: 100,
    };
    let recipe = ConstructionRecipe {
        id: format!("f3d:Design/BulkStream.dat:construction-recipe#{recipe_at}"),
        byte_offset: recipe_at as u64,
        record_index_offset: None,
        kind: ConstructionRecipeKind::Body,
        design: Some(crate::records::ConstructionRecipeDesign {
            id: crate::records::RecordedValue {
                value: "301".into(),
                offset: None,
            },
            selector: Some(crate::records::ConstructionRecipeSelector {
                value: 6,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: 0,
    };

    let operand = parse_body_recipe_operand(&bytes, &group, 0, &record, &recipe)
        .expect("class-367 body recipe operand");
    assert_eq!(operand.references().len(), 1);
    assert_eq!(operand.references()[0].design_reference, 301);
    assert_eq!(operand.references()[0].form, 33);
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.value),
        Some([1, 0, 0, 0])
    );
    assert_eq!(
        operand
            .clone()
            .into_draft()
            .selector_tail
            .map(|tail| tail.offset),
        Some(208)
    );
    assert_eq!(operand.nested_record_index(), 103);
    assert_eq!(operand.next_byte_offset(), next_at as u64);
}
