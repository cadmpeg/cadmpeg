// SPDX-License-Identifier: Apache-2.0
//! Body selections whose group members are whole-body reference tails.

use super::bind_feature_body_selections;
use super::body_selection::history_state;
use crate::design::decode::body_reference_tails::TailBodyNodes;
use crate::history::body_selections::FeatureBodySelectionInputs;
use crate::records::feature::combine::DesignCombineBodyNode;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed};
use cadmpeg_ir::features::selections::BodySelection;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const HISTORY: &str = "f3d:Design/Breps.BlobParts/BREP.a.smbh:asm-history#1";
const OTHER_HISTORY: &str = "f3d:Design/Breps.BlobParts/BREP.b.smbh:asm-history#2";

/// Group 20 of scope 10 whose members are the records `members`.
fn group(members: &[u32]) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#20"),
        scope_record_index: 10,
        scope_reference_ordinal: 0,
        record_index: 20,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("280".to_owned()).unwrap(),
        members: members
            .iter()
            .map(|value| crate::records::identity::Located {
                value: *value,
                offset: 0,
            })
            .collect(),
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
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::BODIES_B),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

/// The browser node of tail record `member`, which names body entity `entity`
/// created by scope `creator`.
fn tail(member: u32, entity: u32, creator: u32) -> TailBodyNodes {
    TailBodyNodes {
        tail_id: format!("{STREAM}:design-body-reference-tail#{member}"),
        tail_record_index: u64::from(member),
        nodes: vec![DesignCombineBodyNode {
            record_index: 40 + member,
            body_entity_suffix: entity,
            creating_scope_record_index: creator,
        }],
    }
}

fn binding(
    entity: u64,
    body: Option<cadmpeg_ir::ids::BodyId>,
    blob: &str,
) -> crate::records::bodies::DesignBodyBinding {
    crate::records::bodies::DesignBodyBinding::try_from(
        crate::records::bodies::DesignBodyBindingWire {
            component_owner: None,
            component_owner_unresolved: false,
            history_only: false,
            id: format!("{STREAM}:design-body-binding#{entity}"),
            stream: "Design/BulkStream.dat".into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 5,
            asm_body_key_offset: 0,
            entity_suffix: entity,
            entity_suffix_offset: 8,
            blob_name: blob.into(),
            blob_name_offset: 16,
            body,
        },
    )
    .expect("binding")
}

fn body(ordinal: u32) -> cadmpeg_ir::ids::BodyId {
    cadmpeg_ir::ids::BodyId::mint(format!("f3d:brep:entity#{ordinal}")).unwrap()
}

fn scope(index: u32, kind: DesignFeatureKind, states: Option<(i64, i64)>) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#{index}"),
        kind,
        index,
    );
    if let Some((state, previous)) = states {
        scope
            .try_edit(|draft| {
                draft.history_state_id = Some(state);
                draft.previous_history_state_id = Some(previous);
                draft.layout_fixture_tail();
            })
            .unwrap();
    }
    scope
}

fn feature(scope: &DesignParameterScope, definition: FeatureOperation) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:test:feature#consumer").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(definition),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

// ---- Remove Body through the solved body of a body-map pair ----

fn bind_remove(
    tails: &[TailBodyNodes],
    bindings: &[crate::records::bodies::DesignBodyBinding],
    members: &[u32],
) -> (Feature, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let scope = scope(10, DesignFeatureKind::RemoveBody, None);
    let group = group(members);
    let mut features = [feature(
        &scope,
        FeatureOperation::DeleteBody {
            bodies: BodySelection::Native(group.id.clone()),
            mode: cadmpeg_ir::features::operation_settings::BodyRetentionMode::DeleteSelected,
        },
    )];
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
        body_tail_nodes: tails,
        body_bindings: bindings,
    };
    let losses = bind_feature_body_selections(&mut features, &inputs);
    let [feature] = features;
    (feature, losses)
}

fn remove_selection(feature: &Feature) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
        feature.evaluation.definition()
    else {
        panic!("DeleteBody");
    };
    bodies
}

#[test]
fn a_stateless_remove_body_selects_the_solved_body_its_tail_node_names() {
    let (feature, losses) = bind_remove(
        &[tail(21, 7, 3)],
        &[binding(7, Some(body(4)), "BREP.a.smbh")],
        &[21],
    );
    assert!(losses.is_empty());
    let BodySelection::Resolved { bodies, .. } = remove_selection(&feature) else {
        panic!("the selection stays native");
    };
    assert_eq!(bodies.as_slice(), [body(4)]);
}

#[test]
fn a_tail_node_whose_body_pair_is_unsolved_leaves_the_selection_native_with_a_loss() {
    let (feature, losses) =
        bind_remove(&[tail(21, 7, 3)], &[binding(7, None, "BREP.a.smbh")], &[21]);
    assert!(matches!(
        remove_selection(&feature),
        BodySelection::Native(_)
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureBodyTailUnbound.kind()
    );
}

#[test]
fn a_tail_node_naming_another_entity_leaves_the_selection_native() {
    let (feature, _) = bind_remove(
        &[tail(21, 7, 3)],
        &[binding(8, Some(body(4)), "BREP.a.smbh")],
        &[21],
    );
    assert!(matches!(
        remove_selection(&feature),
        BodySelection::Native(_)
    ));
}

#[test]
fn members_of_one_group_bind_to_their_own_bodies() {
    let (feature, _) = bind_remove(
        &[tail(21, 7, 3), tail(22, 8, 3)],
        &[
            binding(7, Some(body(4)), "BREP.a.smbh"),
            binding(8, Some(body(5)), "BREP.a.smbh"),
        ],
        &[21, 22],
    );
    let BodySelection::Resolved { bodies, .. } = remove_selection(&feature) else {
        panic!("the selection stays native");
    };
    assert_eq!(bodies.as_slice(), [body(4), body(5)]);
}

#[test]
fn two_members_naming_one_body_leave_the_selection_native() {
    let (feature, _) = bind_remove(
        &[tail(21, 7, 3), tail(22, 7, 3)],
        &[binding(7, Some(body(4)), "BREP.a.smbh")],
        &[21, 22],
    );
    assert!(matches!(
        remove_selection(&feature),
        BodySelection::Native(_)
    ));
}

#[test]
fn a_group_with_a_member_that_names_no_node_stays_native() {
    let (feature, _) = bind_remove(
        &[tail(21, 7, 3)],
        &[binding(7, Some(body(4)), "BREP.a.smbh")],
        &[21, 22],
    );
    assert!(matches!(
        remove_selection(&feature),
        BodySelection::Native(_)
    ));
}

// ---- Pattern seed through the history bodies of the preceding state ----

/// How the creating scope 3 and its history differ from the plain case.
#[derive(Clone, Copy)]
struct Variant {
    /// Bodies of state 10, the state of scope 3. State 9 holds body 1.
    creator_bodies: &'static [i64],
    /// Ordinals of the solved bodies, one member per entry.
    ordinals: &'static [u32],
    /// The history that scope 3 is bound to; `None` binds it to none.
    creator_history: Option<&'static str>,
    /// The state of scope 3.
    creator_state: i64,
    /// The BREP blob of the body-map pairs.
    blob: &'static str,
}

const PLAIN: Variant = Variant {
    creator_bodies: &[1, 2, 3],
    ordinals: &[3],
    creator_history: Some(HISTORY),
    creator_state: 10,
    blob: "BREP.a.smbh",
};

/// A Pattern scope 10 that reads state 12 after state 11 and selects, through
/// one tail per ordinal, bodies that scope 3 creates. When `direct` is false
/// the seed binds through `bind_feature_body_selections`.
fn bind_pattern(variant: Variant) -> Feature {
    bind_pattern_with(variant, false)
}

fn bind_pattern_with(variant: Variant, direct: bool) -> Feature {
    let creator = scope(3, DesignFeatureKind::Move, Some((variant.creator_state, 9)));
    let consumer = scope(10, DesignFeatureKind::CircularPattern, Some((12, 11)));
    let scopes = [creator, consumer];
    let held = |extra: Option<i64>| {
        let mut bodies = variant.creator_bodies.to_vec();
        bodies.extend(extra);
        bodies
    };
    let history = crate::test_support::history::history(
        HISTORY.into(),
        0,
        None,
        false,
        vec![
            history_state(12, held(Some(4))),
            history_state(11, held(None)),
            history_state(10, held(None)),
            history_state(9, vec![1]),
        ],
    );
    let members: Vec<u32> = (0..variant.ordinals.len())
        .map(|index| 21 + u32::try_from(index).unwrap())
        .collect();
    let group = group(&members);
    let tails: Vec<_> = members
        .iter()
        .map(|member| tail(*member, 7 + member - 21, 3))
        .collect();
    let bindings: Vec<_> = variant
        .ordinals
        .iter()
        .enumerate()
        .map(|(index, ordinal)| {
            binding(
                7 + u64::try_from(index).unwrap(),
                Some(body(*ordinal)),
                variant.blob,
            )
        })
        .collect();
    let mut scope_histories = std::collections::HashMap::new();
    scope_histories.insert(scopes[1].id.clone(), HISTORY.to_owned());
    if let Some(bound) = variant.creator_history {
        scope_histories.insert(scopes[0].id.clone(), bound.to_owned());
    }
    let mut features = [feature(
        &scopes[1],
        FeatureOperation::Pattern {
            seeds: Vec::new(),
            pattern: PatternKind::UNRESOLVED_CIRCULAR,
        },
    )];
    let inputs = FeatureBodySelectionInputs {
        scopes: &scopes,
        groups: std::slice::from_ref(&group),
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: std::slice::from_ref(&history),
        scope_histories: &scope_histories,
        bodies: &[],
        regions: &[],
        shells: &[],
        faces: &[],
        body_tail_nodes: &tails,
        body_bindings: &bindings,
    };
    if direct {
        crate::history::pattern_bodies::bind_pattern_body_selections(
            &mut features,
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
    } else {
        bind_feature_body_selections(&mut features, &inputs);
    }
    let [feature] = features;
    feature
}

/// The slots that the Pattern seed names in input state 11, or `None` when
/// the seed is native.
fn seed_slots(feature: &Feature) -> Option<Vec<cadmpeg_ir::ids::HistoricalBodyId>> {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
        feature.evaluation.definition()
    else {
        panic!("Pattern");
    };
    match seeds.as_slice() {
        [PatternSeed::Bodies(BodySelection::Historical { bodies, .. })] => {
            Some(bodies.as_slice().to_vec())
        }
        [PatternSeed::Bodies(BodySelection::Native(_))] => None,
        other => panic!("unexpected seeds {}", other.len()),
    }
}

fn input_body(feature: &Feature, slot: i64) -> cadmpeg_ir::ids::HistoricalBodyId {
    crate::ids::history_input_body_id(&crate::ids::feature_input_prefix(&feature.id, 11), slot)
}

#[test]
fn a_node_created_by_a_transition_inserting_two_bodies_takes_the_slot_of_its_solved_body() {
    let feature = bind_pattern(PLAIN);
    assert_eq!(seed_slots(&feature), Some(vec![input_body(&feature, 3)]));
}

#[test]
fn a_solved_body_outside_the_inserted_slots_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        ordinals: &[4],
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_node_created_by_a_transition_inserting_one_body_takes_that_slot() {
    let feature = bind_pattern(Variant {
        creator_bodies: &[1, 2],
        ordinals: &[2],
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), Some(vec![input_body(&feature, 2)]));
}

#[test]
fn a_solved_body_that_disagrees_with_the_one_inserted_slot_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        creator_bodies: &[1, 2],
        ordinals: &[1],
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_solved_body_of_another_blob_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        creator_bodies: &[1, 2],
        ordinals: &[2],
        blob: "BREP.b.smbh",
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_creator_bound_to_another_history_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        creator_history: Some(OTHER_HISTORY),
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_creator_bound_to_no_history_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        creator_history: None,
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_creator_state_that_does_not_precede_the_input_state_leaves_the_seed_native() {
    let feature = bind_pattern(Variant {
        creator_state: 12,
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn two_members_bind_distinct_slots_in_member_order() {
    let feature = bind_pattern(Variant {
        ordinals: &[3, 2],
        ..PLAIN
    });
    assert_eq!(
        seed_slots(&feature),
        Some(vec![input_body(&feature, 3), input_body(&feature, 2)])
    );
}

#[test]
fn two_members_naming_one_slot_leave_the_seed_native() {
    let feature = bind_pattern(Variant {
        ordinals: &[3, 3],
        ..PLAIN
    });
    assert_eq!(seed_slots(&feature), None);
}

#[test]
fn a_mirror_seed_binds_through_the_pattern_seed_binder() {
    let feature = bind_pattern_with(PLAIN, true);
    assert_eq!(seed_slots(&feature), Some(vec![input_body(&feature, 3)]));
}
