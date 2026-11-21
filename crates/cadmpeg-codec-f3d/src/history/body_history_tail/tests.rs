// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::feature::scope::DesignFeatureKind;
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
    let tails = collect(&features, &[scope], &[history()], None);
    assert_eq!(tails.len(), 1);
    let definition: ProductDefinition = serde_json::from_value(serde_json::json!({
        "id": "test:model:component#panel", "kind": "part", "bodies": [body(1)],
        "bom_properties": {}, "storage": {"kind": "embedded"}, "tessellations": [],
        "occurrences": [], "instance_bodies": [], "representations": [], "sketches": []
    }))
    .unwrap();
    let mut guide = definition.clone();
    guide.id = features[1].product_definition.clone().unwrap();
    guide.bodies.clear();
    let mut model = Model::default();
    model.features.clone_from(&features);
    model.product_definitions = vec![definition.clone(), guide];
    let mut losses = Vec::new();
    append(&mut model, &tails, &mut losses).unwrap();
    assert_eq!(&model.features[..2], features.as_slice());
    let retained = &model.features[2];
    assert_eq!(retained.product_definition, Some(definition.id));
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
}

#[test]
fn a_change_to_another_body_preserves_the_linked_history() {
    let scope = scope(59);
    assert!(collect(&[feature(&scope, 2)], &[scope], &[history()], None).is_empty());
}

#[test]
fn a_feature_that_reaches_the_head_has_no_tail() {
    let scope = scope(64);
    assert!(collect(&[feature(&scope, 1)], &[scope], &[history()], None).is_empty());
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
    assert!(collect(&[feature(&scope, 1)], &[scope], &[history], None).is_empty());
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
        collect(&[before, suppressed], &[old, new], &[history()], None).len(),
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
    assert!(collect(&[feature(&scope, 1)], &[scope], &[other], None).is_empty());
}
