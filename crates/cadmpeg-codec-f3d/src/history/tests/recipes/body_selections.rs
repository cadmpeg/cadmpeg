// SPDX-License-Identifier: Apache-2.0

use super::direct_body_fixture;
use crate::records::topology::extrude_selection::DesignOperandRole;

#[test]
fn direct_body_recipe_selection_resolves_compact_coil_target() {
    use cadmpeg_ir::features::direct_edit::{ScaleCenter, ScaleFactors};
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;

    let (scope, group_id, group, operand, body, region, shell, face) = direct_body_fixture();
    let inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    let mut selection = BodySelection::Native(group_id.into());
    crate::history::body_selections::bind_direct_body_recipe_body_selection(
        &mut selection,
        &scope,
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.faces,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    assert_eq!(
        selection,
        BodySelection::Resolved {
            bodies: cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![
                BodyId::mint("f3d:brep:body#1").expect("identity grammar")
            ],)
            .expect("distinct bodies"),
            native: group_id.into(),
        }
    );

    let recipe = crate::records::recipes::ConstructionRecipe {
        id: operand.recipe_id.clone(),
        byte_offset: 0,
        kind: crate::records::recipes::ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "301".into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 9,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 0,
            offset: 0,
        }),
    };
    let link = crate::records::sketch_links::PersistentDesignLink {
        id: "link".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        design_id: "301".to_owned().try_into().unwrap(),

        design_reference: 9,
        ordinal: 0,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut candidates = crate::history::body_candidates::BodyCandidates::new(
        std::slice::from_ref(&body),
        std::slice::from_ref(&region),
        std::slice::from_ref(&shell),
        std::slice::from_ref(&face),
        std::slice::from_ref(&recipe),
        std::slice::from_ref(&link),
    );
    for _ in 0..1_000 {
        assert_eq!(candidates.linked(&operand), Some(body.id.clone()));
        assert_eq!(
            candidates.face_body_candidates(&operand, &body.id),
            (true, true)
        );
    }
    ctx.finish_session().unwrap();
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            std::slice::from_ref(&body),
            &[],
            &[],
            &[],
            std::slice::from_ref(&recipe),
            std::slice::from_ref(&link)
        )
        .linked(&operand),
        Some(body.id.clone())
    );

    let mut replacement = link.clone();
    replacement.ordinal = 1;
    replacement.design_reference = 99;
    let links = [link.clone(), replacement.clone()];
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            std::slice::from_ref(&body),
            &[],
            &[],
            &[],
            std::slice::from_ref(&recipe),
            &links
        )
        .linked(&operand),
        None
    );
    // Equal ordinals retain the first source link, regardless of its selector.
    replacement.ordinal = 0;
    let links = [link.clone(), replacement];
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            std::slice::from_ref(&body),
            &[],
            &[],
            &[],
            std::slice::from_ref(&recipe),
            &links
        )
        .linked(&operand),
        Some(body.id.clone())
    );
    let mut second_body = body.clone();
    second_body.id = BodyId::mint("f3d:brep:body#2").unwrap();
    let mut second_link = link.clone();
    second_link.target = cadmpeg_ir::attributes::AttributeTarget::Body(second_body.id.clone());
    assert_eq!(
        crate::history::body_candidates::BodyCandidates::new(
            &[body.clone(), second_body],
            &[],
            &[],
            &[],
            std::slice::from_ref(&recipe),
            &[link.clone(), second_link]
        )
        .linked(&operand),
        None
    );
    let mut direct_operand = operand.clone();
    direct_operand.owner =
        crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        };
    let direct_inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: &[],
        body_recipe_operands: std::slice::from_ref(&direct_operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    let native = format!(
        "{}:design-record#21",
        crate::ids::native_stream(&scope.id).expect("test scope stream")
    );
    let mut selection = BodySelection::NativeSet(vec![native.clone()].try_into().unwrap());
    crate::history::body_selections::bind_direct_body_recipe_body_selection(
        &mut selection,
        &scope,
        &direct_inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.faces,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    assert_eq!(
        selection,
        BodySelection::ResolvedSet {
            members: cadmpeg_ir::features::selections::BodyMembers::try_from_rows(vec![
                cadmpeg_ir::features::selections::BodyMember::new(
                    body.id.clone(),
                    cadmpeg_core::text::NonBlankString::new(native)
                        .expect("non-blank native fixture"),
                ),
            ],)
            .expect("valid body selection rows"),
        }
    );

    let mut scale_scope = scope.clone();
    scale_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Scale
                .try_into()
                .unwrap();
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut scale_group = group.clone();
    scale_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::BODIES_A,
        );
    let scale_inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scale_scope),
        groups: std::slice::from_ref(&scale_group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#scale").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Scale {
                bodies: BodySelection::Native(group_id.into()),
                center: Some(ScaleCenter::ModelOrigin),
                factors: ScaleFactors::Uniform {
                    factor: cadmpeg_ir::scalar::NonZeroReal::new(1.5).unwrap(),
                },
            }),
        ),
        native_ref: None,
    };
    feature.native_ref = Some(scale_scope.id.clone());
    let losses = crate::history::tests::bind_feature_body_selections(
        std::slice::from_mut(&mut feature),
        &scale_inputs,
    );
    assert!(losses.is_empty());
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Scale {
            bodies: BodySelection::Resolved { ref bodies, ref native },
            ..
        }) if bodies.as_slice() == [body.id.clone()] && native == group_id
    ));

    let mut move_scope = scope;
    move_scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Move
                .try_into()
                .unwrap();
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = Some(41);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let move_history =
        crate::test_support::history::history("f3d:history".into(), 0, None, false, Vec::new());
    let move_inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&move_scope),
        groups: std::slice::from_ref(&scale_group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: std::slice::from_ref(&move_history),
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    let mut move_feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#move").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: BodySelection::Native(group_id.into()),
                translation: cadmpeg_ir::features::geometry::FiniteVector3::new(
                    cadmpeg_ir::math::Vector3::new(1.0, 2.0, 3.0),
                )
                .unwrap(),
                rotation: None,
                copies: 0,
            }),
        ),
        native_ref: None,
    };
    move_feature.native_ref = Some(move_scope.id.clone());
    let losses = crate::history::tests::bind_feature_body_selections(
        std::slice::from_mut(&mut move_feature),
        &move_inputs,
    );
    assert!(losses.is_empty());
    assert!(matches!(
        move_feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Resolved { ref bodies, ref native },
            ..
        }) if bodies.as_slice() == [body.id.clone()] && native == group_id
    ));
}

#[test]
fn base_feature_body_selection_uses_active_transition_outputs() {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    use cadmpeg_ir::ids::BodyId;

    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#feature").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),

        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::selections::DistinctMembers::try_from(vec![
                BodyId::mint("test:model:body#2").expect("identity grammar"),
                BodyId::mint("test:model:body#1").expect("identity grammar"),
            ])
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    };
    crate::history::feature_outputs::bind_base_feature_output_selection(&mut feature).unwrap();
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { ref bodies, ref native }
        }) if bodies.as_slice() == [BodyId::mint("test:model:body#2").expect("identity grammar"), BodyId::mint("test:model:body#1").expect("identity grammar")]
            && native == "native:scope"
    ));
}

/// A `RemoveBody` in the coil fixture's scope whose operand resolved body
/// slot `slot` through its recipe identity. Its face match selects the
/// displayed fixture body, slot 1.
fn direct_remove_case(slot: Option<i64>) -> cadmpeg_ir::features::selections::BodySelection {
    use cadmpeg_ir::features::operation_settings::BodyRetentionMode;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
    let (scope, group_id, group, mut operand, body, region, shell, face) = direct_body_fixture();
    operand.resolved_body_slot = slot;
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#remove").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native(group_id.into()),
                mode: BodyRetentionMode::DeleteSelected,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    crate::history::tests::bind_feature_body_selections(
        std::slice::from_mut(&mut feature),
        &inputs,
    );
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) => bodies.clone(),
        definition => panic!("removal definition: {definition:?}"),
    }
}

#[test]
fn a_remove_keeps_its_selection_native_when_the_face_match_contradicts_the_recipe_slot() {
    use cadmpeg_ir::features::selections::BodySelection;
    for slot in [None, Some(1)] {
        assert!(
            matches!(
                direct_remove_case(slot),
                BodySelection::Resolved { bodies, .. }
                    if bodies.as_slice().iter().map(cadmpeg_ir::ids::BodyId::as_str).eq(["f3d:brep:body#1"])
            ),
            "slot {slot:?}"
        );
    }
    assert!(matches!(
        direct_remove_case(Some(7)),
        BodySelection::Native(native)
            if native == "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    ));
}

/// A body pattern in the coil fixture's scope whose seed operand resolved
/// body slot `slot` through its recipe identity. Its face match selects the
/// displayed fixture body, slot 1.
fn direct_pattern_seed_case(slot: Option<i64>) -> cadmpeg_ir::features::selections::BodySelection {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed},
        Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    let (scope, _, group, mut operand, body, region, shell, face) = direct_body_fixture();
    operand.resolved_body_slot = slot;
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#mirror").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_MIRROR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = crate::history::body_selections::FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: std::slice::from_ref(&body),
        regions: std::slice::from_ref(&region),
        shells: std::slice::from_ref(&shell),
        faces: std::slice::from_ref(&face),
    };
    crate::history::pattern_bodies::bind_pattern_body_selections(
        std::slice::from_mut(&mut feature),
        &inputs,
        &mut crate::history::body_candidates::BodyCandidates::new(
            inputs.bodies,
            inputs.regions,
            inputs.shells,
            inputs.faces,
            inputs.construction_recipes,
            inputs.persistent_design_links,
        ),
    );
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
            match seeds.as_slice() {
                [PatternSeed::Bodies(selection)] => selection.clone(),
                seeds => panic!("one body seed: {seeds:?}"),
            }
        }
        definition => panic!("pattern definition: {definition:?}"),
    }
}

#[test]
fn a_pattern_seed_stays_native_when_the_face_match_contradicts_the_recipe_slot() {
    use cadmpeg_ir::features::selections::BodySelection;
    for slot in [None, Some(1)] {
        assert!(
            matches!(
                direct_pattern_seed_case(slot),
                BodySelection::Resolved { bodies, .. }
                    if bodies.as_slice().iter().map(cadmpeg_ir::ids::BodyId::as_str).eq(["f3d:brep:body#1"])
            ),
            "slot {slot:?}"
        );
    }
    assert!(matches!(
        direct_pattern_seed_case(Some(7)),
        BodySelection::Native(native)
            if native == "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    ));
}

#[test]
fn a_combine_keeps_its_direct_binding_where_its_header_predecessor_binds_no_body() {
    use crate::history_records::AsmDeltaState;
    use cadmpeg_ir::features::operation_settings::BooleanKind;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{
        CombineOperands, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };

    let (scope, group_id, group, operand, body, region, shell, face) = direct_body_fixture();
    let mut scope = scope;
    scope
        .try_edit(|draft| {
            draft.payload = crate::records::feature::scope::DesignFeatureKind::Combine
                .try_into()
                .unwrap();
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    // State 42 stores no predecessor; its header link names state 41. Neither
    // state stores a topology, so the historical binding finds no body.
    let state = |state_id: i64, next_ref: Option<i64>| -> crate::test_support::history::State {
        (
            AsmDeltaState {
                id: format!("f3d:history:asm-delta-state#{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            None,
            Vec::new(),
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(42, Some(41)), state(41, None)],
    );
    assert_eq!(
        crate::history::history_states::effective_scope_previous_history_state_id(
            &scope,
            std::slice::from_ref(&history)
        ),
        Some(41)
    );
    let combine = |histories: &[crate::history_records::AsmHistory]| {
        let inputs = crate::history::body_selections::FeatureBodySelectionInputs {
            scopes: std::slice::from_ref(&scope),
            groups: std::slice::from_ref(&group),
            body_recipe_operands: std::slice::from_ref(&operand),
            construction_recipes: &[],
            persistent_design_links: &[],
            histories,
            bodies: std::slice::from_ref(&body),
            regions: std::slice::from_ref(&region),
            shells: std::slice::from_ref(&shell),
            faces: std::slice::from_ref(&face),
        };
        let mut feature = Feature {
            product_definition: None,
            id: FeatureId::mint("f3d:test:feature#combine").expect("identity grammar"),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: Default::default(),
            source_properties: Default::default(),
            source_tag: None,
            source_text: None,
            source_content: Default::default(),
            evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Combine {
                    operands: CombineOperands::new(
                        BodySelection::Native(group_id.into()),
                        BodySelection::Native("tool".into()),
                    )
                    .unwrap(),
                    op: BooleanKind::Join,
                    keep_tools: false,
                    destination: Default::default(),
                }),
            ),
            native_ref: Some(scope.id.clone()),
        };
        crate::history::tests::bind_feature_body_selections(
            std::slice::from_mut(&mut feature),
            &inputs,
        );
        let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
            feature.evaluation.definition()
        else {
            panic!("Combine feature");
        };
        (operands.target().clone(), operands.tools().clone())
    };
    let direct = combine(&[]);
    assert!(matches!(
        &direct.0,
        BodySelection::Resolved { bodies, .. } if bodies.as_slice() == [body.id.clone()]
    ));
    assert_eq!(combine(std::slice::from_ref(&history)), direct);
}
