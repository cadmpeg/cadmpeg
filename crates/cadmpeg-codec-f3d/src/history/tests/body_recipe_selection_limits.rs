// SPDX-License-Identifier: Apache-2.0
//! Decode limits for historical body recipe selections.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{BodySelection, FeatureId};

fn selection_fixture() -> (
    crate::records::feature::scope::DesignParameterScope,
    Vec<crate::records::topology::construction::DesignConstructionOperandGroup>,
    Vec<crate::records::topology::body_recipe::DesignBodyRecipeOperand>,
    BodySelection,
) {
    use crate::records::topology::body_recipe::{
        DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignOperandOwner,
    };
    use crate::records::topology::construction::{
        DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
        DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
        DesignConstructionOperandRole,
    };
    let stream = "f3d:Design/BulkStream.dat";
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        &format!("{stream}:design-parameter-scope#10"),
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    let group_id = format!("{stream}:design-construction-operand-group#20");
    let group = DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: group_id.clone(),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 0,
        class_tag: "280".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located { value: 21, offset: 0 }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(DesignConstructionOperandGroupFrameDraft {
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
        }).unwrap(),
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
        ),
        role_offset: 0,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    }).unwrap();
    let operand = DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: format!("{stream}:design-body-recipe-operand#21"),
        scope_record_index: 10,
        owner: DesignOperandOwner::Group { group_record_index: 20, group_member_ordinal: 0 },
        record_index: 21,
        byte_offset: 0,
        class_tag: "365".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned().try_into().unwrap(),
        asset_id_offset: 56,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned().try_into().unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![crate::records::topology::body_recipe::DesignBodyRecipeReference {
            design_reference: 301,
            design_reference_offset: 25,
            form: 33,
            form_offset: 33,
            candidate_faces: vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#7").unwrap()],
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 24,
        nested_record_index_offset: 38,
        recipe_id: format!("{stream}:construction-recipe#24"),
        resolved_face_slot: None,
        resolved_body_state_id: Some(1),
        resolved_body_slot: Some(7),
        resolved_body_face_slots: Vec::new(),
        next_record_index: 25,
        next_byte_offset: 200,
    }).unwrap();
    (scope, vec![group], vec![operand], BodySelection::Native(group_id))
}

fn bind(max_items: u64, max_retained_bytes: u64) -> Result<BodySelection, cadmpeg_core::CodecError> {
    let (scope, groups, operands, mut selection) = selection_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let feature = FeatureId::mint("f3d:model:feature#body").unwrap();
    super::super::bind_body_recipe_body_selection(
        &ctx, &mut selection, &feature, 1, &scope, &groups, &operands,
    )?;
    Ok(selection)
}

fn bind_direct(max_items: u64, native_set: bool) -> Result<BodySelection, cadmpeg_core::CodecError> {
    use cadmpeg_ir::ids::{BodyId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};
    let (scope, groups, mut operands, mut selection) = selection_fixture();
    let body = Body {
        id: BodyId::mint("f3d:brep:body#1").unwrap(),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("f3d:model:region#1").unwrap()],
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let region = Region {
        id: RegionId::mint("f3d:model:region#1").unwrap(),
        body: body.id.clone(),
        shells: vec![ShellId::mint("f3d:model:shell#1").unwrap()],
    };
    let shell = Shell::with_face(
        ShellId::mint("f3d:model:shell#1").unwrap(),
        region.id.clone(),
        cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#7").unwrap(),
    );
    if native_set {
        operands[0].owner = crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        };
        selection = BodySelection::NativeSet(vec![
            "f3d:Design/BulkStream.dat:design-record#21".to_owned(),
        ].try_into().unwrap());
    }
    let inputs = super::super::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: &groups,
        body_recipe_operands: &operands,
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::bind_direct_body_recipe_body_selection(
        &ctx, &mut selection, &scope, &inputs,
    )?;
    Ok(selection)
}

#[test]
fn body_recipe_slots_refuse_collection_limit() {
    let error = bind(0, u64::MAX).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body recipe slots"));
}

#[test]
fn body_recipe_ids_refuse_collection_limit() {
    let error = bind(1, u64::MAX).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body recipe identities"));
}

#[test]
fn body_recipe_validation_refuses_collection_limit() {
    let error = bind(2, u64::MAX).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate F3D body recipe identities"));
}

#[test]
fn body_recipe_identity_refuses_retained_limit() {
    let error = bind(u64::MAX, 0).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D history input identity"));
}

#[test]
fn body_recipe_keeps_historical_selection() {
    assert!(matches!(bind(3, u64::MAX).unwrap(), BodySelection::Historical { .. }));
}

#[test]
fn direct_body_recipe_selection_refuses_collection_limit() {
    let error = bind_direct(0, false).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe selections"));
}

#[test]
fn direct_body_recipe_selection_validation_refuses_collection_limit() {
    let error = bind_direct(1, false).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate F3D direct body recipe selections"));
}

#[test]
fn direct_body_recipe_native_members_refuse_collection_limit() {
    let error = bind_direct(0, true).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe native members"));
}

#[test]
fn direct_body_recipe_rows_refuse_collection_limit() {
    let error = bind_direct(1, true).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe rows"));
}

#[test]
fn direct_body_recipe_rows_validation_refuses_collection_limit() {
    let error = bind_direct(2, true).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate F3D direct body recipe rows"));
}

#[test]
fn direct_body_recipe_keeps_resolved_selection() {
    assert!(matches!(bind_direct(2, false).unwrap(), BodySelection::Resolved { .. }));
    assert!(matches!(bind_direct(3, true).unwrap(), BodySelection::ResolvedSet { .. }));
}
