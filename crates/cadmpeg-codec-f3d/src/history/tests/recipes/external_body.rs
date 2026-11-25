// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

#[test]
fn external_body_candidate_requires_one_displayed_body_across_every_clause() {
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let reference =
        |faces: &[&str]| crate::records::topology::body_recipe::DesignBodyRecipeReference {
            design_reference: 1,
            design_reference_offset: 25,
            form: 3,
            form_offset: 33,
            candidate_faces: faces
                .iter()
                .map(|face| FaceId::mint((*face).to_owned()).expect("identity grammar"))
                .collect(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        };
    let mut operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
        crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
            id: "operand".into(),
            scope_record_index: 1,
            owner: crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            },
            record_index: 2,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("295".to_owned())
                .unwrap(),
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 56,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 132,
            selector_tail: None,

            references: vec![reference(&[
                "f3d:brep/current/brep:face#1",
                "f3d:brep/external/brep:face#1",
                "f3d:brep/cache/brep:face#1",
            ])],
            nested_record_index: 5,
            nested_record_index_offset: 38,
            recipe_id: "recipe".into(),
            resolved_face_slot: None,
            resolved_body_state_id: None,
            resolved_body_slot: None,
            resolved_body_face_slots: Vec::new(),
            next_record_index: 6,
            next_byte_offset: 256,
        },
    )
    .unwrap();
    let body = |id: &str, visible| Body {
        id: BodyId::mint(id).expect("identity grammar"),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible,
        native_ref: None,
        unstated: None,
    };
    let bodies = [
        body("f3d:brep/current/brep:body#1", Some(true)),
        body("f3d:brep/external/brep:body#1", Some(true)),
        body("f3d:brep/cache/brep:body#1", None),
    ];
    let regions = [
        Region::new(
            RegionId::mint("test:model:region#current-region").expect("identity grammar"),
            bodies[0].id.clone(),
            vec![ShellId::mint("test:model:shell#current-shell").expect("identity grammar")],
        ),
        Region::new(
            RegionId::mint("test:model:region#external-region").expect("identity grammar"),
            bodies[1].id.clone(),
            vec![ShellId::mint("test:model:shell#external-shell").expect("identity grammar")],
        ),
        Region::new(
            RegionId::mint("test:model:region#cache-region").expect("identity grammar"),
            bodies[2].id.clone(),
            vec![ShellId::mint("test:model:shell#cache-shell").expect("identity grammar")],
        ),
    ];
    let shell = |id: &str, region: &str| {
        Shell::new(
            ShellId::mint(id).expect("identity grammar"),
            RegionId::mint(region).expect("identity grammar"),
        )
    };
    let shells = [
        shell(
            "test:model:shell#current-shell",
            "test:model:region#current-region",
        ),
        shell(
            "test:model:shell#external-shell",
            "test:model:region#external-region",
        ),
        shell(
            "test:model:shell#cache-shell",
            "test:model:region#cache-region",
        ),
    ];
    let faces = [
        crate::test_support::bare_face(
            "f3d:brep/current/brep:face#1",
            "test:model:shell#current-shell",
        ),
        crate::test_support::bare_face(
            "f3d:brep/external/brep:face#1",
            "test:model:shell#external-shell",
        ),
        crate::test_support::bare_face(
            "f3d:brep/cache/brep:face#1",
            "test:model:shell#cache-shell",
        ),
    ];

    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            &bodies,
            &regions,
            &shells,
            &faces,
            &[],
            &[]
        )
        .external(&operand, Some("current")),
        Some(bodies[1].id.clone())
    );

    operand
        .reference_bindings_mut()
        .next()
        .unwrap()
        .candidate_faces
        .retain(|face| !face.as_str().contains("/cache/"));
    let mut draft = operand.into_draft();
    let mut added = reference(&["f3d:brep/cache/brep:face#1"]);
    added.design_reference_offset =
        draft.byte_offset + 25 + u64_from_index(draft.references.len()) * 12;
    added.form_offset = added.design_reference_offset + 8;
    draft.references.push(added);
    draft.nested_record_index_offset =
        draft.byte_offset + 26 + u64_from_index(draft.references.len()) * 12;
    draft.asset_id_offset = draft.nested_record_index_offset + 18;
    operand =
        crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(draft).unwrap();
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            &bodies,
            &regions,
            &shells,
            &faces,
            &[],
            &[]
        )
        .external(&operand, Some("current")),
        None
    );
}
