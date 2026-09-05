// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used, unused_imports)]

use super::super::*;

#[test]
fn form33_without_unique_body_proof_remains_unresolved() {
    use cadmpeg_ir::features::BodySelection;
    use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};

    let body = |slot| Body {
        id: BodyId::mint(format!("f3d:brep:body#{slot}")).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint(format!("region#{slot}")).expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let bodies = [body(1), body(2)];
    let regions = [
        Region {
            id: RegionId::mint("region#1").expect("identity grammar"),
            body: bodies[0].id.clone(),
            shells: vec![ShellId::mint("shell#1").expect("identity grammar")],
        },
        Region {
            id: RegionId::mint("region#2").expect("identity grammar"),
            body: bodies[1].id.clone(),
            shells: vec![ShellId::mint("shell#2").expect("identity grammar")],
        },
    ];
    let shells = [
        Shell {
            id: ShellId::mint("shell#1").expect("identity grammar"),
            region: RegionId::mint("region#1").expect("identity grammar"),
            faces: vec![FaceId::mint("face#1").expect("identity grammar")],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        },
        Shell {
            id: ShellId::mint("shell#2").expect("identity grammar"),
            region: RegionId::mint("region#2").expect("identity grammar"),
            faces: vec![FaceId::mint("face#2").expect("identity grammar")],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        },
    ];
    let operand = crate::records::DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:body-recipe#1".into(),
        scope_record_index: 10,
        owner: crate::records::DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        },
        record_index: 1,
        byte_offset: 0,
        class_tag: "365".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![crate::records::DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: vec![
                FaceId::mint("face#1").expect("identity grammar"),
                FaceId::mint("face#2").expect("identity grammar"),
            ],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 2,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#3".into(),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 4,
        next_byte_offset: 0,
    };

    assert_eq!(
        unique_external_body_candidate(&operand, None, &bodies, &regions, &shells),
        None
    );

    let scope = crate::records::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        crate::records::DesignFeatureKind::Combine,
        10,
    );
    let native = "f3d:Design/BulkStream.dat:design-record#1".to_owned();
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: &[],
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &bodies,
        regions: &regions,
        shells: &shells,
    };
    let mut selection = BodySelection::NativeSet(vec![native.clone()]);
    bind_direct_body_recipe_body_selection(&mut selection, &scope, &inputs);
    assert_eq!(selection, BodySelection::NativeSet(vec![native]));
}
