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

fn output_binding_inputs() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    use crate::history_records::{AsmDeltaState, AsmHistoricalRelation, AsmHistoricalTopology};
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    let mut feature = base_feature();
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::BaseFeature,
        1,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.previous_history_state_id = Some(0);
            draft.layout_fixture_tail();
        })
        .unwrap();
    feature.native_ref = Some(scope.id.clone());
    let mut current_topology = AsmHistoricalTopology::default();
    current_topology.bodies.push(1);
    current_topology.body_regions.push(AsmHistoricalRelation {
        owner_ref: 1,
        member_refs: Vec::new(),
    });
    let state = |state_id, node_index, next_ref, topology| {
        (
            AsmDeltaState {
                id: format!("f3d:asm-delta-state#{state_id}"),
                parent: "f3d:asm-history#1".into(),
                byte_offset: 0,
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
    };
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
fn feature_output_binding_preserves_base_feature_selection() {
    use cadmpeg_ir::features::{BodySelection, FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = output_binding_inputs();
    super::super::bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        &[scope],
        &[history],
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
