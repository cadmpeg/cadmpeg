// SPDX-License-Identifier: Apache-2.0
use crate::records::dimensions::{DesignDimensionRecipeRecord, DesignRecipeReference};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::sketch_links::PersistentSubentityTag;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::ids::FaceId;

fn reference(token: &str, design_reference: i64) -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: token.into(),
        token_offset: 4,
        design_reference,
        design_reference_offset: 8,
        candidate_faces: Vec::new(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

fn face_tag(id: &str, face: &str, token: &str, design_reference: i64) -> PersistentSubentityTag {
    PersistentSubentityTag {
        id: id.into(),
        target: AttributeTarget::Face(FaceId::mint(face).expect("identity grammar")),
        selector: 1,
        token: cadmpeg_core::text::NonBlankString::try_from(token).unwrap(),
        design_references: vec![design_reference],
        ordinal: 0,
    }
}

#[test]
fn recipe_record_references_bind_the_tags_of_their_own_token() {
    let tags = [
        face_tag("f3d:design:tag#1", "test:model:face#a", "13", 331),
        face_tag("f3d:design:tag#2", "test:model:face#b", "7", 331),
        face_tag("f3d:design:tag#3", "test:model:face#c", "13", 999),
        face_tag("f3d:design:tag#4", "test:model:face#d", "7", 405),
        face_tag("f3d:design:tag#5", "test:model:face#e", "13", 331),
    ];
    let mut records = [DesignDimensionRecipeRecord {
        id: "f3d:design:design-dimension-recipe-record#0".into(),
        companion_record_index: 1,
        recipe_ordinal: 0,
        recipe_id: "recipe".into(),
        recipe_kind: ConstructionRecipeKind::Edge,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: 1,
        frame_length: 4,
        prefix_offset: 0,
        prefix_bytes: vec![1],
        references: vec![
            reference("13", 331),
            reference("7", 405),
            reference("9", 331),
        ],
        program_offset: 0,
        program: vec![0],
        matching_edge_operand_ids: Vec::new(),
    }];
    let ctx = cadmpeg_test_support::service_decode_context();
    super::super::bind_dimension_recipe_reference_candidates(&ctx, &mut records, &tags).unwrap();
    let faces = |references: &[DesignRecipeReference]| {
        references
            .iter()
            .map(|reference| reference.candidate_faces.clone())
            .collect::<Vec<_>>()
    };
    let face = |id: &str| FaceId::mint(id).expect("identity grammar");
    assert_eq!(
        faces(&records[0].references),
        [
            vec![face("test:model:face#a"), face("test:model:face#e")],
            vec![face("test:model:face#d")],
            Vec::new(),
        ]
    );
    // Each reference binds as it does against every tag.
    let mut ungrouped = records[0].references.clone();
    for reference in &mut ungrouped {
        super::super::bind_recipe_reference_candidates_charged(
            &ctx,
            reference,
            &tags,
            Some(&records[0].id),
        )
        .unwrap();
    }
    assert_eq!(faces(&ungrouped), faces(&records[0].references));
}
