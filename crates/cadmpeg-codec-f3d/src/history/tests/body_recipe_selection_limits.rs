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
        members: vec![crate::records::identity::Located {
            value: 21,
            offset: 0,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
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
        operand_role: DesignConstructionOperandRole::Other(
            crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
        ),
        role_offset: 0,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap();
    let operand = DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: format!("{stream}:design-body-recipe-operand#21"),
        scope_record_index: 10,
        owner: DesignOperandOwner::Group {
            group_record_index: 20,
            group_member_ordinal: 0,
        },
        record_index: 21,
        byte_offset: 0,
        class_tag: "365".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 56,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 132,
        selector_tail: None,
        references: vec![
            crate::records::topology::body_recipe::DesignBodyRecipeReference {
                design_reference: 301,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: vec![cadmpeg_ir::ids::FaceId::mint("f3d:brep:entity#7").unwrap()],
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            },
        ],
        nested_record_index: 24,
        nested_record_index_offset: 38,
        recipe_id: format!("{stream}:construction-recipe#24"),
        resolved_face_slot: None,
        resolved_body_state_id: Some(1),
        resolved_body_slot: Some(7),
        resolved_body_face_slots: Vec::new(),
        next_record_index: 25,
        next_byte_offset: 200,
    })
    .unwrap();
    (
        scope,
        vec![group],
        vec![operand],
        BodySelection::Native(group_id),
    )
}

fn bind(
    max_items: u64,
    max_retained_bytes: u64,
) -> Result<BodySelection, cadmpeg_core::CodecError> {
    let (scope, groups, operands, mut selection) = selection_fixture();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let feature = FeatureId::mint("f3d:model:feature#body").unwrap();
    super::super::bind_body_recipe_body_selection(
        &ctx,
        &mut selection,
        &feature,
        1,
        &scope,
        &groups,
        &operands,
    )?;
    Ok(selection)
}

fn face_geometry() -> (
    cadmpeg_ir::topology::Body,
    cadmpeg_ir::topology::Region,
    cadmpeg_ir::topology::Shell,
) {
    use cadmpeg_ir::ids::{BodyId, RegionId, ShellId};
    use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};
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
    (body, region, shell)
}

fn bind_direct(
    max_items: u64,
    native_set: bool,
) -> Result<BodySelection, cadmpeg_core::CodecError> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = bind_direct_with_context(&ctx, native_set);
    if let Err(cadmpeg_core::CodecError::ResourceLimit(first)) = &result {
        assert!(matches!(ctx.finish_session(),
            Err(cadmpeg_core::CodecError::ResourceLimit(sticky)) if sticky == *first));
    }
    result
}

fn bind_direct_with_context(
    ctx: &DecodeContext<'_>,
    native_set: bool,
) -> Result<BodySelection, cadmpeg_core::CodecError> {
    let native_members = if native_set {
        &["f3d:Design/BulkStream.dat:design-record#21"][..]
    } else {
        &[]
    };
    bind_direct_with_native_members(ctx, native_members, false)
}

fn bind_direct_with_native_members(
    ctx: &DecodeContext<'_>,
    native_members: &[&str],
    duplicate_operand: bool,
) -> Result<BodySelection, cadmpeg_core::CodecError> {
    let (scope, groups, mut operands, mut selection) = selection_fixture();
    let (body, region, shell) = face_geometry();
    if !native_members.is_empty() {
        operands[0].owner =
            crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference {
                scope_reference_ordinal: 0,
            };
        if duplicate_operand {
            operands.push(operands[0].clone());
        }
        selection = BodySelection::NativeSet(
            native_members
                .iter()
                .map(|member| (*member).to_owned())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        );
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
    super::super::bind_direct_body_recipe_body_selection(ctx, &mut selection, &scope, &inputs)?;
    Ok(selection)
}

fn face_candidate_with_context(
    ctx: &DecodeContext<'_>,
) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    let (_, _, operands, _) = selection_fixture();
    let (body, region, shell) = face_geometry();
    super::super::body_recipe_face_body_candidates(
        ctx,
        &operands[0],
        &body.id,
        std::slice::from_ref(&body),
        std::slice::from_ref(&region),
        std::slice::from_ref(&shell),
    )
}

fn face_candidate(max_work_units: u64) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = max_work_units;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    face_candidate_with_context(&ctx)
}

fn external_body(
    max_items: u64,
    max_retained_bytes: u64,
    source: Option<&str>,
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let (_, _, operands, _) = selection_fixture();
    let (body, region, shell) = face_geometry();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::unique_external_body_candidate(
        &ctx,
        &operands[0],
        source,
        std::slice::from_ref(&body),
        std::slice::from_ref(&region),
        std::slice::from_ref(&shell),
    )
}

fn linked_body_with_context(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    duplicate_recipe_candidate: bool,
    multiple_body_links: bool,
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    use crate::records::identity::RecordedValue;
    use crate::records::recipes::{
        ConstructionRecipe, ConstructionRecipeDesign, ConstructionRecipeKind,
        ConstructionRecipeSelector,
    };
    let (_, _, operands, _) = selection_fixture();
    let body = |slot| cadmpeg_ir::topology::Body {
        id: cadmpeg_ir::ids::BodyId::mint(format!("f3d:brep:body#{slot}")).unwrap(),
        kind: cadmpeg_ir::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: Some(true),
    };
    let first_body = body(1);
    let recipe = ConstructionRecipe {
        id: operands[0].recipe_id.clone(),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Body,
        design: Some(ConstructionRecipeDesign {
            id: RecordedValue {
                value: "301".into(),
                offset: 0,
            },
            selector: Some(ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let mut recipes = vec![recipe];
    if duplicate_recipe_candidate {
        recipes.push(ConstructionRecipe {
            id: operands[0].recipe_id.clone(),
            byte_offset: 0,
            kind: ConstructionRecipeKind::Body,
            design: None,
            recipe_index: 1,
            record_index: Some(RecordedValue {
                value: 1,
                offset: 0,
            }),
        });
    }
    let link = |body: &cadmpeg_ir::topology::Body, ordinal| {
        crate::records::sketch_links::PersistentDesignLink {
            id: if ordinal == 0 {
                "link".into()
            } else {
                format!("link-{ordinal}")
            },
            target: cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
            design_id: "301".to_owned().try_into().unwrap(),
            design_reference: 9,
            ordinal,
        }
    };
    let mut bodies = vec![first_body];
    let mut links = vec![link(&bodies[0], 0)];
    if multiple_body_links {
        bodies.push(body(2));
        links.push(link(&bodies[1], 1));
    }
    super::super::body_recipe_link_candidate(ctx, &operands[0], &recipes, &links, &bodies)
}

fn linked_body(
    max_work_units: u64,
    max_retained_bytes: u64,
) -> Result<Option<cadmpeg_ir::ids::BodyId>, cadmpeg_core::CodecError> {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = max_work_units;
    policy.limits.max_retained_bytes = max_retained_bytes;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    linked_body_with_context(&ctx, false, false)
}

#[test]
fn body_recipe_slots_refuse_collection_limit() {
    let error = bind(0, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body recipe slots")
    );
}

#[test]
fn body_recipe_ids_refuse_collection_limit() {
    let error = bind(1, u64::MAX).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D body recipe identities")
    );
}

#[test]
fn body_recipe_validation_refuses_collection_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "validate BodySelection historical members",
        |cap| bind(cap, u64::MAX),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate BodySelection historical members")
    );
}

#[test]
fn body_recipe_identity_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D history input identity",
        |cap| bind(u64::MAX, cap).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D history input identity")
    );
}

#[test]
fn body_recipe_keeps_historical_selection() {
    assert!(matches!(
        bind(3, u64::MAX).unwrap(),
        BodySelection::Historical { .. }
    ));
}

#[test]
fn direct_body_recipe_selection_refuses_collection_limit() {
    let error = bind_direct(5, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe selections")
    );
}

#[test]
fn direct_body_recipe_selection_validation_refuses_collection_limit() {
    let error = bind_direct(6, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate distinct decoded members")
    );
}

#[test]
fn direct_body_recipe_native_members_refuse_collection_limit() {
    let error = bind_direct(0, true).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe native members")
    );
}

#[test]
fn direct_body_recipe_rows_refuse_collection_limit() {
    let error = bind_direct(6, true).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D direct body recipe rows")
    );
}

#[test]
fn direct_body_recipe_rows_validation_refuses_collection_limit() {
    let error = bind_direct(7, true).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "validate body selection members")
    );
}

#[test]
fn direct_body_recipe_native_identity_text_queries_refuse_work_limits() {
    const ONE_MEMBER: &[&str] = &["f3d:Design/BulkStream.dat:design-record#21"];
    const TWO_MEMBERS: &[&str] = &[
        "f3d:Design/BulkStream.dat:design-record#21",
        "f3d:Design/BulkStream.dat:design-record#22",
    ];
    for (operation, native_members, duplicate_operand) in [
        (
            "scan F3D direct body recipe native members",
            ONE_MEMBER,
            false,
        ),
        (
            "scan previous F3D direct body recipe native members",
            TWO_MEMBERS,
            false,
        ),
        (
            "compare F3D direct body recipe native members",
            TWO_MEMBERS,
            false,
        ),
        (
            "split F3D direct body recipe native identity",
            ONE_MEMBER,
            false,
        ),
        (
            "parse F3D direct body recipe record index",
            ONE_MEMBER,
            false,
        ),
        (
            "compare F3D direct body recipe stream identity",
            ONE_MEMBER,
            false,
        ),
        (
            "compare F3D direct body recipe operand stream",
            ONE_MEMBER,
            false,
        ),
        (
            "find F3D direct body recipe operand",
            ONE_MEMBER,
            false,
        ),
        (
            "find duplicate F3D direct body recipe operand",
            ONE_MEMBER,
            true,
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                bind_direct_with_native_members(ctx, native_members, duplicate_operand)
                    .map(|_| ())
            },
        );
        assert!(matches!(error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn direct_body_recipe_keeps_resolved_selection() {
    assert!(matches!(
        bind_direct(7, false).unwrap(),
        BodySelection::Resolved { .. }
    ));
    assert!(matches!(bind_direct(8, true),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "validate body selection members"));
    // A resolved row indexes its body and its native identity separately.
    assert!(matches!(
        bind_direct(9, true).unwrap(),
        BodySelection::ResolvedSet { .. }
    ));
}

#[test]
fn persistent_body_link_refuses_work_limit() {
    let operation = "resolve F3D persistent body link";
    // The WorkUnits ceiling includes recipe and link charges before this request.
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| linked_body_with_context(ctx, false, false),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn body_recipe_link_searches_and_comparisons_refuse_work() {
    let operations = [
        ("scan F3D body recipe candidates", false, false),
        ("compare F3D body recipe identity", false, false),
        ("compare F3D body recipe stream", false, false),
        ("scan F3D duplicate body recipe candidates", true, false),
        ("compare F3D persistent design identifier", false, false),
        ("scan F3D persistent body link versions", false, false),
        ("compare F3D persistent body link target", false, false),
        ("scan F3D persistent body candidates", false, false),
        ("compare F3D persistent body candidate identity", false, false),
        ("compare F3D selected persistent body", false, true),
    ];
    for (operation, duplicate_recipe_candidate, multiple_body_links) in operations {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                linked_body_with_context(
                    ctx,
                    duplicate_recipe_candidate,
                    multiple_body_links,
                )
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn persistent_body_link_id_refuses_retained_limit() {
    let error = linked_body(u64::MAX, 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D persistent body link identity")
    );
}

#[test]
fn persistent_body_link_preserves_identity() {
    assert_eq!(
        linked_body(u64::MAX, u64::MAX).unwrap().unwrap().as_str(),
        "f3d:brep:body#1"
    );
}

#[test]
fn body_recipe_face_carrier_refuses_work_limit() {
    let operation = "resolve F3D body recipe face carrier";
    // The WorkUnits ceiling includes face-reference admissions before this request.
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| face_candidate_with_context(ctx),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn body_recipe_face_carrier_preserves_selected_candidate() {
    assert_eq!(face_candidate(u64::MAX).unwrap(), (true, true));
}

#[test]
fn body_recipe_face_scans_and_comparisons_refuse_work() {
    for operation in [
        "scan F3D body recipe references",
        "scan F3D body recipe candidate faces",
        "compare F3D body recipe region",
        "compare F3D selected body recipe candidate",
    ] {
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| face_candidate_with_context(ctx),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn external_body_region_index_refuses_collection_limit() {
    let error = external_body(0, u64::MAX, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D external body regions")
    );
}

#[test]
fn external_body_face_index_refuses_collection_limit() {
    let error = external_body(1, u64::MAX, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D external body faces")
    );
}

#[test]
fn external_body_metadata_index_refuses_collection_limit() {
    let error = external_body(2, u64::MAX, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D external body metadata")
    );
}

#[test]
fn external_body_candidate_refuses_collection_limit() {
    let error = external_body(3, u64::MAX, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D external body candidates")
    );
}

#[test]
fn external_body_displayed_refuses_collection_limit() {
    let error = external_body(4, u64::MAX, None).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D displayed external bodies")
    );
}

#[test]
fn external_body_prefix_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "retain F3D current history prefix",
        |cap| external_body(u64::MAX, cap, Some("other")).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D current history prefix")
    );
}

#[test]
fn external_body_candidate_id_refuses_retained_limit() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "copy F3D external body candidate",
        |cap| external_body(u64::MAX, cap, None).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "copy F3D external body candidate")
    );
}

#[test]
fn external_body_preserves_selected_identity() {
    assert_eq!(
        external_body(5, u64::MAX, None).unwrap().unwrap().as_str(),
        "f3d:brep:body#1"
    );
}
