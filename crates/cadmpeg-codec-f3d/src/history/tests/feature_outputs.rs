// SPDX-License-Identifier: Apache-2.0

fn active_body() -> cadmpeg_ir::topology::Body {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .model
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    body
}

fn base_feature() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    use cadmpeg_ir::ids::BodyId;
    Feature {
        needs_recompute: false,
        id: FeatureId::mint("test:model:feature#feature").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Base Feature".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::BaseFeature {
                bodies: BodySelection::Native("native:scope".into()),
            }),
            cadmpeg_ir::features::DistinctMembers::try_from(vec![
                BodyId::mint("test:model:body#2").unwrap(),
                BodyId::mint("test:model:body#1").unwrap(),
            ])
            .unwrap(),
        ),
        native_ref: Some("native:scope".into()),
    }
}

fn state(
    state_id: i64,
    node_index: i64,
    next_ref: Option<i64>,
    topology: crate::history_records::AsmHistoricalTopology,
) -> crate::test_support::history::State {
    (
        crate::history_records::AsmDeltaState {
            id: format!("f3d:asm-delta-state#{state_id}"),
            parent: "f3d:asm-history#1".into(),
            byte_offset: Some(0),
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        Vec::new(),
    )
}

fn scope_between(
    state_id: i64,
    previous_state_id: i64,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{state_id}"),
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state_id);
            draft.previous_history_state_id = Some(previous_state_id);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

fn bodies_topology(bodies: &[i64]) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: bodies.to_vec(),
        body_regions: bodies
            .iter()
            .map(|&owner_ref| AsmHistoricalRelation {
                owner_ref,
                member_refs: Vec::new(),
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::AsmHistoricalTopology;
    let mut feature = base_feature();
    let scope = scope_between(1, 0);
    feature.native_ref = Some(scope.id.clone());
    let current_topology = bodies_topology(&[1]);
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(1, 1, Some(0), current_topology),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ],
    );
    (feature, scope, history, active_body())
}

#[test]
fn feature_outputs_name_the_bodies_a_state_deletes_from_its_previous_state() {
    use crate::history_records::AsmHistoricalTopology;
    let history = |states| {
        crate::test_support::history::history("f3d:asm-history#1".into(), 0, None, false, states)
    };
    let body = |slot: i64| {
        let mut body = active_body();
        body.id = cadmpeg_ir::ids::BodyId::mint(format!("f3d:brep:entity#{slot}")).unwrap();
        body
    };
    let bodies = [body(1), body(2), body(3)];
    let chain = || {
        history(vec![
            state(3, 3, Some(2), bodies_topology(&[3])),
            state(2, 2, Some(1), bodies_topology(&[1, 3])),
            state(1, 1, Some(0), bodies_topology(&[1, 2])),
            state(0, 0, None, AsmHistoricalTopology::default()),
        ])
    };
    let expected = [(1, vec![1, 2]), (2, vec![2, 3]), (3, vec![1])];
    let feature_scope = |state_id| {
        let mut feature = base_feature();
        let scope = scope_between(state_id, state_id - 1);
        feature.native_ref = Some(scope.id.clone());
        (feature, scope)
    };
    let check = |feature: &cadmpeg_ir::features::Feature,
                 (state_id, expected): &(i64, Vec<i64>)| {
        let outputs = feature
            .evaluation
            .outputs()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        let expected = expected
            .iter()
            .map(|slot| format!("f3d:brep:entity#{slot}"))
            .collect::<Vec<_>>();
        assert_eq!(outputs, expected, "state {state_id}");
    };
    for case in &expected {
        let (mut feature, scope) = feature_scope(case.0);
        super::super::bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            &[scope],
            &mut [chain()],
            &bodies,
        )
        .unwrap();
        check(&feature, case);
    }
    let (mut features, scopes): (Vec<_>, Vec<_>) = expected
        .iter()
        .map(|(state_id, _)| feature_scope(*state_id))
        .unzip();
    super::super::bind_feature_outputs(&mut features, &scopes, &mut [chain()], &bodies).unwrap();
    for (feature, case) in features.iter().zip(&expected) {
        check(feature, case);
    }
}

#[test]
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &mut [history],
        std::slice::from_ref(&body),
    )
    .unwrap();
    assert_eq!(feature.evaluation.outputs().len(), 1);
    assert_eq!(feature.evaluation.outputs()[0], body.id);
    assert!(matches!(feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved { bodies, native }
        }) if bodies.as_slice() == [body.id] && native == "native:scope"));
}
