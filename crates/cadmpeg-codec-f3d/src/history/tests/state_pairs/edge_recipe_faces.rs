// SPDX-License-Identifier: Apache-2.0
//! Scope history binding through the faces of a scope's edge recipes.
#![allow(clippy::unwrap_used)]

use crate::history::bind_scope_histories;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
#[test]
fn scope_binding_reads_the_blob_of_its_edge_recipe_faces() {
    use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
    use crate::records::sketch_links::PersistentSubentityTag;
    use crate::records::topology::edge_identity::DesignEdgeOperand;
    use cadmpeg_ir::attributes::AttributeTarget;
    use cadmpeg_ir::ids::FaceId;

    let state = |history: &str, state_id: i64, previous_state_id: Option<i64>| {
        (
            AsmDeltaState {
                id: format!("{history}:asm-delta-state#{state_id}"),
                parent: history.into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: previous_state_id,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(AsmHistoricalTopology::default()),
            Vec::new(),
        )
    };
    let history = |source: &str| {
        let id = format!("f3d:asset/Breps.BlobParts/BREP.{source}.smbh:asm-history#1");
        crate::test_support::history::history(
            id.clone(),
            0,
            None,
            false,
            vec![state(&id, 9, Some(2)), state(&id, 2, None)],
        )
    };
    let histories = [history("first"), history("second")];
    let stream = "f3d:Design/BulkStream.dat";
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#100"),
        crate::records::feature::scope::DesignFeatureKind::Chamfer,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(9);
            draft.previous_history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let scopes = vec![scope.clone()];
    let recipe = ConstructionRecipe {
        id: format!("{stream}:construction-recipe#1"),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Edge,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 40,
            offset: 0,
        }),
    };
    let operand: DesignEdgeOperand = serde_json::from_value(serde_json::json!({
        "id": format!("{stream}:design-edge-operand#0"),
        "scope_record_index": 100,
        "scope_reference_ordinal": 0,
        "record_index": 101,
        "byte_offset": 0,
        "class_tag": "297",
        "paired_byte_offset": 16,
        "paired_class_tag": "259",
        "recipe_record_index": 104,
        "recipe_record_byte_offset": 32,
        "recipe_id": recipe.id,
        "recipe_prefix_offset": 43,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [],
        "next_record_index": 105,
        "next_byte_offset": 160,
    }))
    .unwrap();
    let tag = |ordinal: u32, face: &str| PersistentSubentityTag {
        id: format!("{stream}:persistent-subentity-tag#{ordinal}"),
        target: AttributeTarget::Face(FaceId::mint(face).unwrap()),
        selector: 1,
        token: cadmpeg_core::text::NonBlankString::new("0").unwrap(),
        token_integer: 0,
        design_references: vec![40],
        ordinal,
    };
    let bind = |tags: &[PersistentSubentityTag]| {
        bind_scope_histories(
            &scopes,
            &[],
            &[],
            crate::history::edge_recipe_scope::Sources {
                operands: std::slice::from_ref(&operand),
                recipes: std::slice::from_ref(&recipe),
                tags,
            },
            &histories,
            &mut Vec::new(),
        )
    };

    let second = [
        tag(1, "f3d:brep/second.smbh/brep:entity#4"),
        tag(2, "f3d:brep/second.smbh/brep:entity#7"),
    ];
    assert_eq!(bind(&second)[&scope.id], histories[1].id);
    // Faces of two blobs, or faces with no blob, select no history.
    let mixed = [
        tag(1, "f3d:brep/second.smbh/brep:entity#4"),
        tag(2, "f3d:brep/first.smbh/brep:entity#7"),
    ];
    assert!(!bind(&mixed).contains_key(&scope.id));
    let unqualified = [tag(1, "f3d:brep:entity#4")];
    assert!(!bind(&unqualified).contains_key(&scope.id));
    assert!(!bind(&[]).contains_key(&scope.id));
}
