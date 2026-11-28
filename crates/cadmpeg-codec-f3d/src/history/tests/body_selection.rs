// SPDX-License-Identifier: Apache-2.0
//! Body-selection history ownership tests.

#![allow(clippy::default_trait_access)]

use super::bind_feature_body_selections;
use crate::history::body_selections::FeatureBodySelectionInputs;
use crate::records::topology::{
    construction::DesignConstructionOperandGroup,
    construction::DesignConstructionOperandGroupFrame, extrude_selection::DesignOperandRole,
};

#[test]
fn move_body_selection_uses_unique_owning_history() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Move,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = Some(41);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index: 10,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
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
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_A,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 next_ref: Option<i64>|
     -> crate::test_support::history::State {
        (
            AsmDeltaState {
                id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
                parent: parent.into(),
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
            Some(topology()),
            vec![crate::history_records::AsmEntityVersion {
                entity_ref: 1,
                record_ref: state_id,
            }],
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(42, "history", Some(41)), state(41, "history", None)],
    );
    let unrelated_history = crate::test_support::history::history(
        "unrelated-history".into(),
        0,
        None,
        false,
        vec![state(41, "unrelated-history", None)],
    );
    let histories = [history, unrelated_history];
    let mut feature = Feature {
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
    feature.native_ref = Some(scope.id.clone());
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        scope_histories: &std::collections::HashMap::new(),
        bodies: &[],
        regions: &[],
        shells: &[],
        faces: &[],
    };
    let losses = bind_feature_body_selections(std::slice::from_mut(&mut feature), &inputs);
    assert!(losses.is_empty());

    let expected_body = crate::ids::history_input_body_id(
        &crate::ids::history_input_prefix(&cadmpeg_ir::identity_key!("move"), 41),
        1,
    );
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Historical { ref bodies, ref native, .. },
            ..
        }) if bodies.as_slice() == [expected_body] && native.as_str() == group_id
    ));
}

fn pattern_seed_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        patterns::PatternKind, Feature, FeatureDefinition, FeatureId, FeatureOperation,
    };
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CircularPattern,
        10,
    );
    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:design-construction-operand-group#20".into(),
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
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
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
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: "259".to_owned().try_into().unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap();
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#pattern").unwrap(),
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
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        scope_histories: &std::collections::HashMap::new(),
        bodies: &[],
        regions: &[],
        shells: &[],
        faces: &[],
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
    feature
}

#[test]
fn pattern_body_seed_preserves_native_selection() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let feature = pattern_seed_feature();
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(),
                [PatternSeed::Bodies(BodySelection::Native(id))]
                    if id == "f3d:Design/BulkStream.dat:design-construction-operand-group#20")));
}

/// Group record 20 of the scope with record index `scope_record_index`,
/// whose one member is body recipe operand 21.
fn body_operand_group(group_id: &str, scope_record_index: u32) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: group_id.into(),
            scope_record_index,
            scope_reference_ordinal: 0,
            record_index: 20,
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 21,
                offset: 0,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
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
                    closing_flag: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::BODIES_B,
                ),
            role_offset: 0,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

/// Body recipe operand 21, the first member of group 20, resolved to body
/// slot 1 in state 41.
fn body_recipe_operand(
    scope_record_index: u32,
) -> crate::records::topology::body_recipe::DesignBodyRecipeOperand {
    use crate::records::topology::body_recipe::{
        DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignOperandOwner,
    };
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: "f3d:Design/BulkStream.dat:body-recipe-operand#21".into(),
        scope_record_index,
        owner: DesignOperandOwner::Group {
            group_record_index: 20,
            group_member_ordinal: 0,
        },
        record_index: 21,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("350".to_owned()).unwrap(),
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
        references: vec![
            crate::records::topology::body_recipe::DesignBodyRecipeReference {
                design_reference: 301,
                design_reference_offset: 25,
                form: 33,
                form_offset: 33,
                candidate_faces: Vec::new(),
                preceding_candidate_faces: Vec::new(),
                preceding_body_slots: Vec::new(),
            },
        ],
        nested_record_index: 24,
        nested_record_index_offset: 38,
        recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#25".into(),
        resolved_face_slot: None,
        resolved_body_state_id: Some(41),
        resolved_body_slot: Some(1),
        resolved_body_face_slots: Vec::new(),
        next_record_index: 25,
        next_byte_offset: 256,
    })
    .unwrap()
}

#[test]
fn a_pattern_seed_without_a_stored_preceding_state_binds_through_its_history() {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed};
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::CircularPattern,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(42);
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let group = body_operand_group(group_id, 10);
    let topology = || AsmHistoricalTopology {
        bodies: vec![1],
        ..AsmHistoricalTopology::default()
    };
    let state = |state_id: i64,
                 parent: &str,
                 next_ref: Option<i64>|
     -> crate::test_support::history::State {
        (
            AsmDeltaState {
                id: format!("f3d:{parent}:asm-delta-state#{state_id}"),
                parent: parent.into(),
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
            Some(topology()),
            vec![crate::history_records::AsmEntityVersion {
                entity_ref: 1,
                record_ref: state_id,
            }],
        )
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![state(42, "history", Some(41)), state(41, "history", None)],
    );
    let unrelated_history = crate::test_support::history::history(
        "unrelated-history".into(),
        0,
        None,
        false,
        vec![state(41, "unrelated-history", None)],
    );
    let histories = [history, unrelated_history];

    let operand = body_recipe_operand(10);
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#pattern").unwrap(),
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
                seeds: vec![PatternSeed::Bodies(BodySelection::Native(group_id.into()))],
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let inputs = FeatureBodySelectionInputs {
        scopes: std::slice::from_ref(&scope),
        groups: std::slice::from_ref(&group),
        body_recipe_operands: std::slice::from_ref(&operand),
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        scope_histories: &std::collections::HashMap::new(),
        bodies: &[],
        regions: &[],
        shells: &[],
        faces: &[],
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
    let expected =
        crate::ids::history_input_body_id(&crate::ids::feature_input_prefix(&feature.id, 41), 1);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(),
                [PatternSeed::Bodies(BodySelection::Historical { bodies, .. })]
                    if bodies.as_slice() == [expected])
    ));
}

/// An extrude whose state 77 inserts body slot 1 and a feature that selects
/// that body: a pattern seed or `Move` in input state 79, or a `RemoveBody` whose
/// group 20 resolves to slot 1.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GeneratedSelector {
    Pattern,
    Remove,
    Move,
}

fn generated_case(
    live: &[cadmpeg_ir::topology::Body],
    selector: GeneratedSelector,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
) {
    generated_case_in(
        live,
        selector,
        GeneratedHistory::Single {
            id: "history",
            seed_state: 79,
            later_update: false,
        },
    )
}

#[derive(Clone, Copy)]
enum GeneratedHistory<'a> {
    Split,
    Single {
        id: &'a str,
        seed_state: i64,
        later_update: bool,
    },
}

fn generated_case_in(
    live: &[cadmpeg_ir::topology::Body],
    selector: GeneratedSelector,
    history: GeneratedHistory<'_>,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};

    let state =
        |state_id: i64, version: i64, bodies: Vec<i64>| -> crate::test_support::history::State {
            let versions = bodies
                .iter()
                .map(|body| crate::history_records::AsmEntityVersion {
                    entity_ref: *body,
                    record_ref: version,
                })
                .collect();
            (
                AsmDeltaState {
                    id: format!("f3d:history:asm-delta-state#{state_id}"),
                    parent: "history".into(),
                    byte_offset: 0,
                    state_id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: state_id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(AsmHistoricalTopology {
                    bodies,
                    ..AsmHistoricalTopology::default()
                }),
                versions,
            )
        };
    let build_history =
        |id: &str, states| crate::test_support::history::history(id.into(), 0, None, false, states);
    let histories = match history {
        GeneratedHistory::Split => vec![
            build_history(
                "history",
                vec![state(80, 80, Vec::new()), state(79, 79, Vec::new())],
            ),
            build_history(
                "other-history",
                vec![state(77, 77, vec![1]), state(2, 2, Vec::new())],
            ),
        ],
        GeneratedHistory::Single {
            id, later_update, ..
        } => vec![build_history(
            id,
            later_update
                .then(|| state(81, 81, vec![1]))
                .into_iter()
                .chain([
                    state(80, 79, vec![1]),
                    state(79, 79, vec![1]),
                    state(77, 77, vec![1]),
                    state(2, 2, Vec::new()),
                ])
                .collect(),
        )],
    };
    let seed_state = match history {
        GeneratedHistory::Split => 79,
        GeneratedHistory::Single { seed_state, .. } => seed_state,
    };
    let (feature, results, losses) = generated_selection_in_state(
        live,
        selector,
        &histories,
        &std::collections::HashMap::new(),
        seed_state,
        1,
    );
    assert!(losses.is_empty(), "{losses:?}");
    (feature, results)
}

/// One state of a test history: `state_id` holds the body slots `bodies`.
fn history_state(state_id: i64, bodies: Vec<i64>) -> crate::test_support::history::State {
    use crate::history_records::{AsmDeltaState, AsmHistoricalTopology};

    let versions = bodies
        .iter()
        .map(|body| crate::history_records::AsmEntityVersion {
            entity_ref: *body,
            record_ref: state_id,
        })
        .collect();
    (
        AsmDeltaState {
            id: format!("f3d:history:asm-delta-state#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology {
            bodies,
            ..AsmHistoricalTopology::default()
        }),
        versions,
    )
}

/// [`generated_case`] over `histories`: the extrude scope 5 has state 77 after
/// state 2, and the selecting scope 8 has state 80 after state 79.
fn generated_case_in_histories(
    live: &[cadmpeg_ir::topology::Body],
    remove: bool,
    histories: &[crate::history_records::AsmHistory],
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
) {
    let (feature, results, losses) =
        generated_case_with_bindings(live, remove, histories, &std::collections::HashMap::new());
    assert!(losses.is_empty(), "{losses:?}");
    (feature, results)
}

fn generated_case_with_bindings(
    live: &[cadmpeg_ir::topology::Body],
    remove: bool,
    histories: &[crate::history_records::AsmHistory],
    scope_histories: &std::collections::HashMap<String, String>,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    generated_selection_with_bindings(
        live,
        if remove {
            GeneratedSelector::Remove
        } else {
            GeneratedSelector::Pattern
        },
        histories,
        scope_histories,
    )
}

fn generated_selection_with_bindings(
    live: &[cadmpeg_ir::topology::Body],
    selector: GeneratedSelector,
    histories: &[crate::history_records::AsmHistory],
    scope_histories: &std::collections::HashMap<String, String>,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    generated_selection_in_state(live, selector, histories, scope_histories, 79, 0)
}

fn generated_selection_in_state(
    live: &[cadmpeg_ir::topology::Body],
    selector: GeneratedSelector,
    histories: &[crate::history_records::AsmHistory],
    scope_histories: &std::collections::HashMap<String, String>,
    seed_state: i64,
    selector_ordinal: u64,
) -> (
    cadmpeg_ir::features::Feature,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::operation_settings::BodyRetentionMode;
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed};
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let scope = |index: u32, state: i64, previous: i64| {
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
            crate::records::feature::scope::DesignFeatureKind::Move,
            index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(state);
                draft.previous_history_state_id = Some(previous);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let scopes = [scope(5, 77, 2), scope(8, 80, 79)];
    let feature = |id: &str, scope: &str, definition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(scope.to_owned()),
    };
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let pattern_id = FeatureId::mint("f3d:test:feature#mirror").unwrap();
    let seed_body = crate::ids::history_input_body_id(
        &crate::ids::feature_input_prefix(&pattern_id, seed_state),
        1,
    );
    let seed = BodySelection::historical(
        crate::ids::history_input_state_id(&crate::ids::feature_input_prefix(
            &pattern_id,
            seed_state,
        )),
        vec![seed_body],
        group_id.to_owned(),
    )
    .unwrap();
    let mut features = vec![
        feature(
            "f3d:test:feature#extrude",
            &scopes[0].id,
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        feature(
            pattern_id.as_str(),
            &scopes[1].id,
            FeatureDefinition::Operation(match selector {
                GeneratedSelector::Remove => FeatureOperation::DeleteBody {
                    bodies: BodySelection::Native(group_id.into()),
                    mode: BodyRetentionMode::DeleteSelected,
                },
                GeneratedSelector::Pattern => FeatureOperation::Pattern {
                    seeds: vec![PatternSeed::Bodies(seed)],
                    pattern: PatternKind::UNRESOLVED_CIRCULAR,
                },
                GeneratedSelector::Move => FeatureOperation::MoveBody {
                    bodies: seed,
                    translation: cadmpeg_ir::features::geometry::FiniteVector3::new(
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    )
                    .unwrap(),
                    rotation: None,
                    copies: 0,
                },
            }),
        ),
    ];
    features[1].ordinal = selector_ordinal;
    let groups = [body_operand_group(group_id, 8)];
    let operands = [body_recipe_operand(8)];
    let inputs = FeatureBodySelectionInputs {
        scopes: &scopes,
        groups: &groups,
        body_recipe_operands: &operands,
        construction_recipes: &[],
        persistent_design_links: &[],
        histories,
        scope_histories: &std::collections::HashMap::new(),
        bodies: live,
        regions: &[],
        shells: &[],
        faces: &[],
    };
    let mut losses = Vec::new();
    let results = crate::history::bind_generated_body_selections(
        &mut features,
        &inputs,
        scope_histories,
        None,
        &mut losses,
    );
    (features.remove(1), results, losses)
}

#[test]
fn a_seed_body_the_final_model_omits_is_generated_by_the_feature_that_inserts_it() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    let (pattern, results) = generated_case(&[], GeneratedSelector::Pattern);
    let FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern {
        seeds, ..
    }) = pattern.evaluation.definition()
    else {
        panic!("pattern definition");
    };
    let [PatternSeed::Bodies(BodySelection::Generated { bodies, native })] = seeds.as_slice()
    else {
        panic!("generated seed: {seeds:?}");
    };
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].feature.as_str(), "f3d:test:feature#extrude");
    assert_eq!(bodies[0].local_id.as_str(), "1");
    assert_eq!(
        native.as_str(),
        "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    );
    assert!(pattern
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == "f3d:test:feature#extrude"));
    let [result] = results.as_slice() else {
        panic!("one result state");
    };
    assert_eq!(result.output_of.as_str(), "f3d:test:feature#extrude");
    assert_eq!(
        result
            .bodies()
            .iter()
            .map(cadmpeg_core::text::NonBlankString::as_str)
            .collect::<Vec<_>>(),
        ["1"]
    );
}

#[test]
fn a_seed_body_the_final_model_keeps_stays_historical() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let (pattern, results) =
        generated_case(std::slice::from_ref(&body), GeneratedSelector::Pattern);
    assert!(results.is_empty());
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Historical { .. })])
    ));
    assert!(pattern.dependencies.is_empty());
}

#[test]
fn a_removed_body_the_final_model_omits_is_generated_by_the_feature_that_inserts_it() {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (remove, results) = generated_case(&[], GeneratedSelector::Remove);
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody {
        bodies: BodySelection::Generated { bodies, native },
        ..
    }) = remove.evaluation.definition()
    else {
        panic!("generated removal: {:?}", remove.evaluation.definition());
    };
    assert_eq!(
        bodies
            .iter()
            .map(|body| (body.feature.as_str(), body.local_id.as_str()))
            .collect::<Vec<_>>(),
        [("f3d:test:feature#extrude", "1")]
    );
    assert_eq!(
        native.as_str(),
        "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    );
    assert!(remove
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == "f3d:test:feature#extrude"));
    assert_eq!(results.len(), 1);
}

/// A test history named `id` with `states`, newest first.
fn named_history(
    id: &str,
    states: Vec<crate::test_support::history::State>,
) -> crate::history_records::AsmHistory {
    crate::test_support::history::history(id.into(), 0, None, false, states)
}

#[test]
fn a_slot_that_only_another_history_inserts_falls_back_with_a_history_loss() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    // The selecting scope's states 80 and 79 are in history `own`, which never
    // inserts slot 1. History `other` inserts its own slot 1 at state 77; the
    // pass falls back to the all-history search and charges a loss.
    let histories = [
        named_history(
            "own",
            vec![history_state(80, Vec::new()), history_state(79, Vec::new())],
        ),
        named_history(
            "other",
            vec![history_state(77, vec![1]), history_state(2, Vec::new())],
        ),
    ];
    let (pattern, results, losses) =
        generated_case_with_bindings(&[], false, &histories, &std::collections::HashMap::new());
    assert_eq!(results.len(), 1);
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Generated { .. })])
    ));
    assert!(pattern
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == "f3d:test:feature#extrude"));
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureGeneratedBodyHistoryUnresolved.kind()
    );
}

#[test]
fn a_colliding_slot_is_generated_in_the_selecting_scopes_own_history() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    // Both histories insert a slot 1 at a state 77. The selecting scope's
    // states 80 and 79 are in history `own`, so its slot 1 is the body that
    // the transition 2 -> 77 of `own` inserts.
    let histories = [
        named_history(
            "own",
            vec![
                history_state(80, vec![1]),
                history_state(79, vec![1]),
                history_state(77, vec![1]),
                history_state(2, Vec::new()),
            ],
        ),
        named_history(
            "other",
            vec![history_state(77, vec![1]), history_state(2, Vec::new())],
        ),
    ];
    let bindings = std::collections::HashMap::from([(
        "f3d:Design/BulkStream.dat:design-parameter-scope#5".into(),
        "own".into(),
    )]);
    let (pattern, results, losses) =
        generated_case_with_bindings(&[], false, &histories, &bindings);
    assert!(losses.is_empty(), "{losses:?}");
    let FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern {
        seeds, ..
    }) = pattern.evaluation.definition()
    else {
        panic!("pattern definition");
    };
    let [PatternSeed::Bodies(BodySelection::Generated { bodies, .. })] = seeds.as_slice() else {
        panic!("generated seed: {seeds:?}");
    };
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].feature.as_str(), "f3d:test:feature#extrude");
    assert_eq!(bodies[0].local_id.as_str(), "1");
    let [result] = results.as_slice() else {
        panic!("one result state");
    };
    assert_eq!(result.output_of.as_str(), "f3d:test:feature#extrude");
}

fn shared_pair_histories(indirect: Option<&str>) -> [crate::history_records::AsmHistory; 2] {
    let mut own = vec![history_state(80, vec![1])];
    if indirect == Some("own") {
        own.push(history_state(78, vec![1]));
    }
    own.extend([
        history_state(79, vec![1]),
        history_state(77, vec![1]),
        history_state(2, Vec::new()),
    ]);
    let mut other = vec![history_state(80, Vec::new())];
    if indirect == Some("other") {
        other.push(history_state(78, Vec::new()));
    }
    other.push(history_state(79, Vec::new()));
    [named_history("other", other), named_history("own", own)]
}

#[test]
fn a_shared_state_pair_uses_the_selecting_scopes_bound_history() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let bindings = std::collections::HashMap::from([(
        "f3d:Design/BulkStream.dat:design-parameter-scope#8".into(),
        "own".into(),
    )]);
    for indirect in [None, Some("own")] {
        let histories = shared_pair_histories(indirect);
        for remove in [false, true] {
            let (feature, results, losses) =
                generated_case_with_bindings(&[], remove, &histories, &bindings);
            let selection = match feature.evaluation.definition() {
                FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                    let [PatternSeed::Bodies(selection)] = seeds.as_slice() else {
                        panic!("body seed");
                    };
                    selection
                }
                FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) => bodies,
                _ => panic!("body selection"),
            };
            let BodySelection::Generated { bodies, .. } = selection else {
                panic!("generated body: {selection:?}");
            };
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0].feature.as_str(), "f3d:test:feature#extrude");
            assert_eq!(bodies[0].local_id.as_str(), "1");
            assert_eq!(feature.dependencies.as_slice(), [bodies[0].feature.clone()]);
            let [result] = results.as_slice() else {
                panic!("one result state");
            };
            assert_eq!(result.output_of, bodies[0].feature);
            assert_eq!(result.bodies()[0].as_str(), "1");
            assert!(losses.is_empty(), "{losses:?}");
        }
    }
}

#[test]
fn an_unbound_shared_state_pair_binds_the_one_inserting_state_with_a_history_loss() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let histories = shared_pair_histories(None);
    for remove in [false, true] {
        let (feature, results, losses) = generated_case_with_bindings(
            &[],
            remove,
            &histories,
            &std::collections::HashMap::new(),
        );
        let selection = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                let [PatternSeed::Bodies(selection)] = seeds.as_slice() else {
                    panic!("one body seed");
                };
                selection
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) => bodies,
            _ => panic!("body selection"),
        };
        let BodySelection::Generated { bodies, .. } = selection else {
            panic!("generated body: {selection:?}");
        };
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].feature.as_str(), "f3d:test:feature#extrude");
        assert_eq!(bodies[0].local_id.as_str(), "1");
        assert_eq!(feature.dependencies.as_slice(), [bodies[0].feature.clone()]);
        let [result] = results.as_slice() else {
            panic!("one result state");
        };
        assert_eq!(result.output_of, bodies[0].feature);
        let [loss] = losses.as_slice() else {
            panic!("one unresolved history loss: {losses:?}");
        };
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureGeneratedBodyHistoryUnresolved
                .note("")
                .code
        );
        assert!(loss.message.contains(feature.id.as_str()));
        assert!(loss.message.contains("body slots [1]"));
        assert!(loss.message.contains("taken as its producer"));
    }
}

#[test]
fn a_shared_state_pair_uses_the_unique_direct_history() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let histories = shared_pair_histories(Some("other"));
    let (pattern, results) = generated_case_in_histories(&[], false, &histories);
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Generated { bodies, .. })]
                if bodies.len() == 1 && bodies[0].feature.as_str() == "f3d:test:feature#extrude"
                    && bodies[0].local_id.as_str() == "1")
    ));
    assert_eq!(pattern.dependencies.len(), 1);
    assert_eq!(results.len(), 1);
}

#[test]
fn a_partly_stored_state_pair_binds_through_every_history_with_a_loss() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let histories = [
        named_history(
            "own",
            vec![
                history_state(80, vec![1]),
                history_state(77, vec![1]),
                history_state(2, Vec::new()),
            ],
        ),
        named_history("other", vec![history_state(99, Vec::new())]),
    ];
    let (pattern, results, losses) =
        generated_case_with_bindings(&[], false, &histories, &std::collections::HashMap::new());
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Generated { bodies, .. })]
                if bodies.len() == 1 && bodies[0].feature.as_str() == "f3d:test:feature#extrude"
                    && bodies[0].local_id.as_str() == "1")
    ));
    assert_eq!(pattern.dependencies.len(), 1);
    assert_eq!(results.len(), 1);
    let [loss] = losses.as_slice() else {
        panic!("one unresolved history loss: {losses:?}");
    };
    assert!(loss.message.contains("taken as its producer"));
}

#[test]
fn a_moved_body_the_final_model_omits_is_generated_by_the_feature_that_inserts_it() {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (moved, results) = generated_case(&[], GeneratedSelector::Move);
    let FeatureDefinition::Operation(FeatureOperation::MoveBody {
        bodies: BodySelection::Generated { bodies, native },
        ..
    }) = moved.evaluation.definition()
    else {
        panic!("generated move: {:?}", moved.evaluation.definition());
    };
    assert_eq!(
        bodies
            .iter()
            .map(|body| (body.feature.as_str(), body.local_id.as_str()))
            .collect::<Vec<_>>(),
        [("f3d:test:feature#extrude", "1")]
    );
    assert_eq!(
        native.as_str(),
        "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    );
    assert!(moved
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == "f3d:test:feature#extrude"));
    assert_eq!(results.len(), 1);
}

#[test]
fn a_moved_body_that_another_history_inserts_stays_historical() {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (moved, results) = generated_case_in(&[], GeneratedSelector::Move, GeneratedHistory::Split);
    assert!(results.is_empty());
    assert!(matches!(
        moved.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Historical { .. },
            ..
        })
    ));
    assert!(moved.dependencies.is_empty());
}

#[test]
fn a_kept_seed_body_outside_a_brep_history_stays_historical() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let (pattern, results) =
        generated_case(std::slice::from_ref(&body), GeneratedSelector::Pattern);
    assert!(results.is_empty());
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Historical { .. })])
    ));
    assert!(pattern.dependencies.is_empty());
}

#[test]
fn a_seed_body_the_final_model_keeps_selects_that_model_body() {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::FeatureDefinition;
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let (pattern, results) = generated_case_in(
        std::slice::from_ref(&body),
        GeneratedSelector::Pattern,
        GeneratedHistory::Single {
            id: "f3d:Assets/BREP.source.smbh:asm-history#0",
            seed_state: 79,
            later_update: false,
        },
    );
    assert!(results.is_empty());
    let FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern {
        seeds, ..
    }) = pattern.evaluation.definition()
    else {
        panic!("pattern definition");
    };
    let [PatternSeed::Bodies(BodySelection::Resolved { bodies, native })] = seeds.as_slice() else {
        panic!("resolved seed: {seeds:?}");
    };
    assert_eq!(bodies.as_slice(), [body.id.clone()]);
    assert_eq!(
        native.as_str(),
        "f3d:Design/BulkStream.dat:design-construction-operand-group#20"
    );
    assert!(pattern
        .dependencies
        .as_slice()
        .iter()
        .any(|dependency| dependency.as_str() == "f3d:test:feature#extrude"));

    let mut other = body.clone();
    other.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#2").unwrap();
    let (pattern, _) = generated_case_in(
        &[body, other],
        GeneratedSelector::Pattern,
        GeneratedHistory::Single {
            id: "f3d:Assets/BREP.source.smbh:asm-history#0",
            seed_state: 79,
            later_update: false,
        },
    );
    assert!(matches!(
        pattern.evaluation.definition(),
        FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. })
            if matches!(seeds.as_slice(), [PatternSeed::Bodies(BodySelection::Resolved { .. })])
    ));
}

#[test]
fn a_seed_body_updated_after_its_seed_state_is_generated_by_its_writer_at_that_state() {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let (pattern, _) = generated_case_in(
        std::slice::from_ref(&body),
        GeneratedSelector::Pattern,
        GeneratedHistory::Single {
            id: "f3d:Assets/BREP.source.smbh:asm-history#0",
            seed_state: 77,
            later_update: false,
        },
    );
    assert_eq!(
        seed_generators(&pattern),
        [("f3d:test:feature#extrude", "1")]
    );
}

#[test]
fn a_seed_body_that_an_unscoped_state_updates_is_generated_by_its_inserting_feature() {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let histories = [crate::test_support::history::history(
        "f3d:Assets/BREP.source.smbh:asm-history#0".into(),
        0,
        None,
        false,
        [
            (81, vec![1]),
            (80, vec![1]),
            (79, vec![1]),
            (77, vec![1]),
            (2, Vec::new()),
        ]
        .into_iter()
        .map(|(state_id, bodies)| {
            let mut state = history_state(state_id, bodies);
            if state_id == 80 {
                for version in &mut state.2 {
                    version.record_ref = 79;
                }
            }
            state
        })
        .collect(),
    )];
    let (pattern, _, losses) = generated_selection_in_state(
        std::slice::from_ref(&body),
        GeneratedSelector::Pattern,
        &histories,
        &std::collections::HashMap::new(),
        79,
        1,
    );
    assert_eq!(
        seed_generators(&pattern),
        [("f3d:test:feature#extrude", "1")]
    );
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        "feature.body-operand-writer-assumed"
    );
}

#[test]
fn a_seed_body_with_a_face_updated_after_the_pattern_state_is_generated_by_its_writer() {
    assert_seed_with_updated_entity_is_generated(|topology| &mut topology.faces);
}

#[test]
fn a_seed_body_with_a_surface_updated_after_the_pattern_state_is_generated_by_its_writer() {
    assert_seed_with_updated_entity_is_generated(|topology| &mut topology.surfaces);
}

fn assert_seed_with_updated_entity_is_generated(
    entity_slots: impl Fn(&mut crate::history_records::AsmHistoricalTopology) -> &mut Vec<i64>,
) {
    use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
    use cadmpeg_ir::features::evaluation::FeatureEvaluation;
    use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed};
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let scope = |index: u32, state: i64, previous: i64| {
        let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
            &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
            crate::records::feature::scope::DesignFeatureKind::Move,
            index,
        );
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(state);
                draft.previous_history_state_id = Some(previous);
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let scopes = [scope(5, 79, 2), scope(8, 80, 79)];
    let feature = |id: &str, scope: &str, definition| Feature {
        product_definition: None,
        id: FeatureId::mint(id).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: Some(scope.to_owned()),
    };
    let pattern_id = FeatureId::mint("f3d:test:feature#mirror").unwrap();
    let group_id = "f3d:Design/BulkStream.dat:design-construction-operand-group#20";
    let seed_body =
        crate::ids::history_input_body_id(&crate::ids::feature_input_prefix(&pattern_id, 79), 1);
    let seed = BodySelection::historical(
        crate::ids::history_input_state_id(&crate::ids::feature_input_prefix(&pattern_id, 79)),
        vec![seed_body],
        group_id.to_owned(),
    )
    .unwrap();
    let mut features = vec![
        feature(
            "f3d:test:feature#extrude",
            &scopes[0].id,
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
        feature(
            pattern_id.as_str(),
            &scopes[1].id,
            FeatureDefinition::Operation(FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Bodies(seed)],
                pattern: PatternKind::UNRESOLVED_CIRCULAR,
            }),
        ),
    ];
    features[1].ordinal = 1;
    let groups = [body_operand_group(group_id, 8)];
    let operands = [body_recipe_operand(8)];
    let mk_state = |state_id: i64,
                    body_version: i64,
                    entity_version: Option<i64>,
                    bodies: Vec<i64>|
     -> crate::test_support::history::State {
        let mut versions: Vec<AsmEntityVersion> = bodies
            .iter()
            .map(|b| AsmEntityVersion {
                entity_ref: *b,
                record_ref: body_version,
            })
            .collect();
        let mut topology = AsmHistoricalTopology {
            bodies,
            ..AsmHistoricalTopology::default()
        };
        if let Some(record_ref) = entity_version {
            versions.push(AsmEntityVersion {
                entity_ref: 100,
                record_ref,
            });
            entity_slots(&mut topology).push(100);
        }
        (
            AsmDeltaState {
                id: format!("f3d:history:asm-delta-state#{state_id}"),
                parent: "history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            versions,
        )
    };
    let histories = [crate::test_support::history::history(
        "f3d:Assets/BREP.source.smbh:asm-history#0".into(),
        0,
        None,
        false,
        vec![
            mk_state(81, 79, Some(81), vec![1]),
            mk_state(80, 79, Some(79), vec![1]),
            mk_state(79, 79, Some(79), vec![1]),
            mk_state(2, 2, None, Vec::new()),
        ],
    )];
    let inputs = FeatureBodySelectionInputs {
        scopes: &scopes,
        groups: &groups,
        body_recipe_operands: &operands,
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        scope_histories: &std::collections::HashMap::new(),
        bodies: std::slice::from_ref(&body),
        regions: &[],
        shells: &[],
        faces: &[],
    };
    crate::history::bind_generated_body_selections(
        &mut features,
        &inputs,
        &std::collections::HashMap::new(),
        None,
        &mut Vec::new(),
    );
    assert_eq!(
        seed_generators(&features[1]),
        [("f3d:test:feature#extrude", "1")]
    );
}

/// The generating feature and local identity of each seed body of a body
/// pattern whose seed is a generated selection.
fn seed_generators(pattern: &cadmpeg_ir::features::Feature) -> Vec<(&str, &str)> {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        pattern.evaluation.definition()
    else {
        panic!("pattern definition");
    };
    let [PatternSeed::Bodies(BodySelection::Generated { bodies, .. })] = seeds.as_slice() else {
        panic!("generated seed: {seeds:?}");
    };
    bodies
        .iter()
        .map(|body| (body.feature.as_str(), body.local_id.as_str()))
        .collect()
}
