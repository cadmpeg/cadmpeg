// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::feature::scope::DesignFeatureKind;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::products::ProductDefinition;

fn body(slot: i64) -> BodyId {
    BodyId::mint(format!("f3d:brep/panel.smbh/brep:entity#{slot}")).unwrap()
}

fn topology(extra_face: bool) -> AsmHistoricalTopology {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let faces = if extra_face {
        vec![10, 11, 20]
    } else {
        vec![10, 20]
    };
    AsmHistoricalTopology {
        bodies: vec![1, 2],
        regions: vec![3, 4],
        shells: vec![5, 6],
        body_regions: vec![relation(1, vec![3]), relation(2, vec![4])],
        region_shells: vec![relation(3, vec![5]), relation(4, vec![6])],
        shell_faces: vec![
            relation(5, if extra_face { vec![10, 11] } else { vec![10] }),
            relation(6, vec![20]),
        ],
        shell_wire_edges: vec![relation(5, Vec::new()), relation(6, Vec::new())],
        shell_free_vertices: vec![relation(5, Vec::new()), relation(6, Vec::new())],
        face_loops: faces
            .iter()
            .map(|&face| relation(face, Vec::new()))
            .collect(),
        face_surfaces: faces
            .iter()
            .map(|&entity| AsmHistoricalCarrierBinding {
                entity,
                carrier: entity + 100,
            })
            .collect(),
        surfaces: faces.iter().map(|face| face + 100).collect(),
        faces,
        ..Default::default()
    }
}

fn history() -> AsmHistory {
    let history_id = "f3d:BREP.panel.smbh:asm-history#0";
    let states = [(64, 1, Some(0), true), (59, 0, None, false)]
        .into_iter()
        .map(|(state_id, node_index, next_ref, extra)| {
            (
                AsmDeltaState {
                    id: format!("f3d:BREP.panel.smbh:asm-delta-state#{state_id}"),
                    parent: history_id.into(),
                    byte_offset: 0,
                    state_id,
                    node_index,
                    next_ref,
                    previous_ref: None,
                    version_flag: 1,
                    state_flag: 0,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(topology(extra)),
                Vec::new(),
            )
        })
        .collect();
    crate::test_support::history::history(history_id.into(), 0, None, false, states)
}

fn scope(current: i64) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::Extrude,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(current);
            draft.previous_history_state_id = Some(2);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn feature(scope: &DesignParameterScope, slot: i64) -> Feature {
    Feature {
        id: FeatureId::mint(format!("test:model:feature#{slot}")).unwrap(),
        ordinal: 1,
        product_definition: None,
        name: Some("Panel feature".into()),
        suppressed: Some(false),
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Synthetic".into(),
                parameters: BTreeMap::new(),
            }),
            DistinctMembers::try_from(vec![body(slot)]).unwrap(),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

fn definition() -> ProductDefinition {
    json::from_value(cadmpeg_core::json!({
        "id": "test:model:component#panel", "kind": "part", "bodies": [body(1)],
        "bom_properties": {}, "storage": {"kind": "embedded"},
        "occurrences": [], "instance_bodies": [], "representations": []
    }))
    .unwrap()
}

/// One body history tail, and the body it retains, as `collect` finds it.
pub(crate) fn one_tail() -> (BodyHistoryTails, BodyId) {
    let scope = scope(59);
    let found = collect(
        &[feature(&scope, 1)],
        &[scope],
        &[history()],
        &Default::default(),
        None,
    );
    assert_eq!(found.tails.len(), 1);
    (found, body(1))
}

/// A feature whose histories link its state to different previous states
/// still states that state, so a body whose history ends at it retains no
/// change after it.
#[test]
fn a_feature_at_the_head_state_retains_no_change_when_its_previous_state_is_unlinked() {
    let mut scope = scope(64);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut other = history();
    other.id = "f3d:BREP.other.smbh:asm-history#0".into();
    other.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(7);
    let found = collect(
        &[feature(&scope, 1)],
        &[scope],
        &[history(), other],
        &Default::default(),
        None,
    );
    assert!(found.tails.is_empty());
    assert!(found.unread.is_empty());
}

#[test]
fn a_repeated_state_id_does_not_place_the_feature_at_the_history_head() {
    let mut scope = scope(64);
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut history = history();
    history.states_mut()[1].state_id = 64;
    history.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(64);
    let mut model = Model::default();
    model.features.push(feature(&scope, 1));
    model.product_definitions.push(definition());
    let found = collect(
        &model.features,
        &[scope],
        &[history],
        &Default::default(),
        None,
    );
    let mut losses = Vec::new();
    append(&mut model, &found, &mut losses).unwrap();
    assert_eq!(
        model.features[1].evaluation.outputs().as_slice(),
        &[body(1)]
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureBodyHistoryIncomplete.kind()
    );
    assert!(losses[0]
        .message
        .contains("from an ambiguous occurrence of state 64 to 64"));
}

#[test]
fn a_later_body_change_retains_a_native_operation_owned_by_the_target() {
    let scope = scope(59);
    let before = feature(&scope, 1);
    let mut foreign = feature(&scope, 2);
    foreign.id = FeatureId::mint("test:model:feature#foreign-cut").unwrap();
    foreign.ordinal = 2;
    foreign.product_definition = Some("test:model:component#guide".try_into().unwrap());
    foreign.evaluation.set_outputs(Default::default());
    let features = vec![before, foreign];
    let found = collect(&features, &[scope], &[history()], &Default::default(), None);
    assert_eq!(found.tails.len(), 1);
    let definition = definition();
    let mut guide = definition.clone();
    guide.id = features[1].product_definition.clone().unwrap();
    guide.bodies.clear();
    let mut model = Model::default();
    model.features.clone_from(&features);
    model.product_definitions = vec![definition.clone(), guide];
    let mut losses = Vec::new();
    append(&mut model, &found, &mut losses).unwrap();
    assert_eq!(&model.features[..2], features.as_slice());
    let retained = &model.features[2];
    assert_eq!(retained.product_definition, Some(definition.id.clone()));
    assert_eq!(retained.evaluation.outputs(), &[body(1)]);
    assert_eq!(
        retained.dependencies.as_slice(),
        std::slice::from_ref(&features[0].id)
    );
    assert_eq!(retained.ordinal, 3);
    assert_eq!(
        retained.native_ref.as_deref(),
        Some("f3d:BREP.panel.smbh:asm-delta-state#64")
    );
    assert!(matches!(
        retained.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureBodyHistoryIncomplete.kind()
    );
    assert!(losses[0].message.contains(&format!(
        "Product definition {} holds the body and owns the native history interval.",
        definition.id
    )));
}

#[test]
fn a_change_to_another_body_preserves_the_linked_history() {
    let scope = scope(59);
    assert!(collect(
        &[feature(&scope, 2)],
        &[scope],
        &[history()],
        &Default::default(),
        None
    )
    .tails
    .is_empty());
}

#[test]
fn a_feature_that_reaches_the_head_has_no_tail() {
    let scope = scope(64);
    assert!(collect(
        &[feature(&scope, 1)],
        &[scope],
        &[history()],
        &Default::default(),
        None
    )
    .tails
    .is_empty());
}

#[test]
fn a_geometry_unchanged_tail_preserves_the_linked_history() {
    let scope = scope(59);
    let mut history = history();
    history.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .geometry_unchanged = true;
    assert!(collect(
        &[feature(&scope, 1)],
        &[scope],
        &[history],
        &Default::default(),
        None
    )
    .tails
    .is_empty());
}

#[test]
fn a_suppressed_feature_does_not_claim_the_tail() {
    let old = scope(59);
    let mut new = scope(64);
    new.id.push_str("-suppressed");
    let before = feature(&old, 1);
    let mut suppressed = feature(&new, 1);
    suppressed.ordinal = 2;
    suppressed.suppressed = Some(true);
    assert_eq!(
        collect(
            &[before, suppressed],
            &[old, new],
            &[history()],
            &Default::default(),
            None
        )
        .tails
        .len(),
        1
    );
}

#[test]
fn an_unproved_interval_or_exhausted_work_keeps_the_tail_native() {
    let history = history();
    let histories = std::slice::from_ref(&history);
    assert!(changed_after(
        &history,
        histories,
        58,
        64,
        1,
        &WorkBudget::new(MAX_HISTORY_TAIL_WORK)
    ));
    assert!(changed_after(
        &history,
        histories,
        59,
        64,
        1,
        &WorkBudget::new(0)
    ));
}

#[test]
fn matching_state_numbers_in_another_brep_do_not_claim_the_target() {
    let scope = scope(59);
    let mut other = history();
    other.id = "f3d:BREP.other.smbh:asm-history#0".into();
    assert!(collect(
        &[feature(&scope, 1)],
        &[scope],
        &[other],
        &Default::default(),
        None
    )
    .tails
    .is_empty());
}

#[test]
fn a_body_no_decoded_history_holds_gets_no_interval_and_the_unread_loss() {
    let scope = scope(59);
    let before = feature(&scope, 1);
    let mut other = history();
    other.id = "f3d:BREP.other.smbh:asm-history#0".into();
    let found = collect(
        std::slice::from_ref(&before),
        &[scope],
        &[other],
        &Default::default(),
        None,
    );
    assert!(found.tails.is_empty());
    assert_eq!(found.unread, vec![(body(1), before.id.clone())]);
    let mut model = Model::default();
    model.features.push(before.clone());
    let mut losses = Vec::new();
    append(&mut model, &found, &mut losses).unwrap();
    assert_eq!(model.features.len(), 1);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::FeatureBodyHistoryUnread.kind());
    assert!(losses[0].message.contains(&body(1).to_string()));
}

#[test]
fn a_body_in_zero_product_definitions_retains_an_unowned_native_operation() {
    assert_unresolved_owner(Vec::new(), "no owner");
}

#[test]
fn a_body_in_two_product_definitions_retains_an_unowned_native_operation() {
    let first = definition();
    let mut second = first.clone();
    second.id = "test:model:component#guide".try_into().unwrap();
    assert_unresolved_owner(vec![first, second], "several owners");
}

fn assert_unresolved_owner(definitions: Vec<ProductDefinition>, gap: &str) {
    let scope = scope(59);
    let before = feature(&scope, 1);
    let found = collect(
        std::slice::from_ref(&before),
        &[scope],
        &[history()],
        &Default::default(),
        None,
    );
    assert_eq!(found.tails.len(), 1);
    let mut model = Model::default();
    model.features.push(before.clone());
    model.product_definitions = definitions;
    let mut losses = Vec::new();
    append(&mut model, &found, &mut losses).unwrap();
    assert_eq!(model.features.len(), 2);
    assert_eq!(model.features[0], before);
    let retained = &model.features[1];
    assert_eq!(retained.product_definition, None);
    assert_eq!(retained.evaluation.outputs(), &[body(1)]);
    assert_eq!(retained.dependencies.as_slice(), &[before.id]);
    assert_eq!(retained.ordinal, 2);
    assert_eq!(
        retained.native_ref.as_deref(),
        Some("f3d:BREP.panel.smbh:asm-delta-state#64")
    );
    assert!(matches!(
        retained.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert_eq!(losses.len(), 2);
    let ownership = losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::FeatureBodyHistoryOwnerUnresolved.kind())
        .unwrap();
    assert_eq!(
        ownership.message,
        format!(
            "Body {} has {gap} in product definitions; the native history interval from state 59 to 64 remains unowned and retains the imported final shape.",
            body(1)
        )
    );
    assert!(losses
        .iter()
        .any(|loss| loss.code == F3dLossCode::FeatureBodyHistoryIncomplete.kind()));
}

fn assert_unproved(
    features: &[Feature],
    scopes: &[DesignParameterScope],
    histories: &[AsmHistory],
    native: &str,
) {
    let found = collect(features, scopes, histories, &Default::default(), None);
    assert_eq!(found.tails.len(), 1);
    let mut model = Model::default();
    model.features.extend_from_slice(features);
    let mut owner = definition();
    owner.bodies = features[0].evaluation.outputs().clone();
    model.product_definitions = vec![owner];
    let mut losses = Vec::new();
    append(&mut model, &found, &mut losses).unwrap();
    let retained = model.features.last().unwrap();
    assert_eq!(retained.source_tag.as_deref(), Some("Body history change"));
    assert_eq!(retained.native_ref.as_deref(), Some(native));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        F3dLossCode::FeatureBodyHistoryIncomplete.kind()
    );
}

#[test]
fn a_feature_without_a_readable_history_state_retains_the_tail_native() {
    let scope = scope(59);
    assert_unproved(
        &[feature(&scope, 1)],
        &[],
        &[history()],
        "f3d:BREP.panel.smbh:asm-delta-state#64",
    );
}

#[test]
fn a_history_without_a_state_order_retains_the_tail_native() {
    let scope = scope(59);
    let empty = crate::test_support::history::history(
        "f3d:BREP.panel.smbh:asm-history#0".into(),
        0,
        None,
        false,
        Vec::new(),
    );
    assert_unproved(
        &[feature(&scope, 1)],
        &[scope],
        &[empty],
        "f3d:BREP.panel.smbh:asm-history#0",
    );
}

#[test]
fn a_body_with_an_unknown_slot_retains_the_tail_native() {
    let scope = scope(59);
    let mut feature = feature(&scope, 1);
    let unknown = BodyId::mint("f3d:brep/panel.smbh/brep:entity#unread").unwrap();
    feature
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![unknown]).unwrap());
    assert_unproved(
        &[feature],
        &[scope],
        &[history()],
        "f3d:BREP.panel.smbh:asm-delta-state#64",
    );
}

fn body_update(lump: i64, transform: i64) -> AsmHistory {
    body_shape_update(0, -1, lump, -1, transform)
}

fn body_shape_update(key: i64, pattern: i64, lump: i64, wire: i64, transform: i64) -> AsmHistory {
    use cadmpeg_asm::sab::{Record, Token};
    let base = history();
    let states = base
        .states()
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let topology = self::topology(false);
            let versions = topology
                .slots()
                .into_iter()
                .map(|entity_ref| crate::history_records::AsmEntityVersion {
                    entity_ref,
                    record_ref: if entity_ref == 1 {
                        if index == 0 {
                            201
                        } else {
                            200
                        }
                    } else {
                        entity_ref
                    },
                })
                .collect();
            (state.clone(), Some(topology), versions)
        })
        .collect();
    let history = crate::test_support::history::history(base.id.clone(), 0, None, false, states);
    let record = |attribute, key, pattern, lump, wire, transform| Record {
        index: 1,
        name: "body".into(),
        offset: 0,
        len: 0,
        tokens: vec![
            Token::Ref(attribute),
            Token::Long(key),
            Token::Ref(pattern),
            Token::Ref(lump),
            Token::Ref(wire),
            Token::Ref(transform),
        ]
        .into(),
    };
    history.check_transitions().unwrap();
    history.cache_record_archive(crate::history_records::CachedRecordArchive {
        archive: HashMap::from([
            (200, record(149, 0, -1, 3, -1, 4)),
            (201, record(5, key, pattern, lump, wire, transform)),
        ]),
        subtypes: cadmpeg_asm::nurbs::toks::SubtypeTable::from_records(&[]),
    });
    history
}

#[test]
fn an_attribute_only_body_update_has_no_tail() {
    let scope = scope(59);
    assert!(
        collect(&[feature(&scope, 1)], &[scope], &[body_update(3, 4)], None)
            .tails
            .is_empty()
    );
}

#[test]
fn a_body_lump_update_retains_a_tail() {
    let scope = scope(59);
    assert_eq!(
        collect(&[feature(&scope, 1)], &[scope], &[body_update(8, 4)], None)
            .tails
            .len(),
        1
    );
}

#[test]
fn a_body_transform_update_retains_a_tail() {
    let scope = scope(59);
    assert_eq!(
        collect(&[feature(&scope, 1)], &[scope], &[body_update(3, 8)], None)
            .tails
            .len(),
        1
    );
}

#[test]
fn a_body_key_pattern_or_wire_update_retains_a_tail() {
    let scope = scope(59);
    for history in [
        body_shape_update(1, -1, 3, -1, 4),
        body_shape_update(0, 7, 3, -1, 4),
        body_shape_update(0, -1, 3, 9, 4),
    ] {
        assert_eq!(
            collect(&[feature(&scope, 1)], &[scope.clone()], &[history], None)
                .tails
                .len(),
            1
        );
    }
}
