// SPDX-License-Identifier: Apache-2.0
//! Projected feature input topology.

use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation, UnresolvedFamily};

fn input_fixture() -> (
    Feature,
    crate::records::feature::scope::DesignParameterScope,
    crate::history_records::AsmHistory,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistory, AsmTopologyCache,
    };
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:design:scope#input",
        crate::records::feature::scope::DesignFeatureKind::WorkPoint,
        7,
    );
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = Some(4);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let feature = Feature {
        id: "f3d:model:feature#input".to_owned().try_into().unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("WorkPoint".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Unresolved {
                family: UnresolvedFamily::DatumPoint,
            }),
        ),
        native_ref: Some(scope.id.clone()),
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![AsmDeltaState {
            id: "f3d:history:state#4".into(),
            parent: "f3d:history".into(),
            byte_offset: 0,
            state_id: 4,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: None,
            node_index: 0,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            entity_versions: Vec::new(),
            topology_cache: AsmTopologyCache::Complete(AsmHistoricalTopology {
                bodies: vec![1],
                faces: vec![2],
                edges: vec![3],
                vertices: vec![4],
                ..AsmHistoricalTopology::default()
            }),
            transition: None,
        }],
    };
    (feature, scope, history)
}

fn project() -> Result<Vec<cadmpeg_ir::features::FeatureInputTopology>, cadmpeg_core::CodecError> {
    let (feature, scope, history) = input_fixture();
    super::super::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&history),
        &[],
        &std::collections::HashMap::new(),
    )
}

#[test]
fn input_projection_preserves_member_order() {
    let projected = project().unwrap();
    assert_eq!(projected.len(), 1);
    assert_eq!(
        projected[0].vertices.as_slice()[0].as_str(),
        "f3d:history-input:vertex#5:input:4:4"
    );
}

#[test]
fn input_topology_uses_scope_binding_when_global_state_numbers_repeat() {
    let (feature, mut scope, mut history) = input_fixture();
    scope
        .try_edit(|draft| draft.history_state_id = Some(5))
        .unwrap();
    let mut current = history.states[0].clone();
    current.id = "f3d:history:state#5".into();
    current.state_id = 5;
    current.node_index = 1;
    current.next_ref = Some(0);
    history.states.push(current);
    let mut other = history.clone();
    other.id = "f3d:other:history#0".into();
    other.states[0].id = "f3d:other:state#4".into();
    other.states[0].parent = other.id.clone();
    other.states[1].id = "f3d:other:state#5".into();
    other.states[1].parent = other.id.clone();
    other.states[0].topology_cache = crate::history_records::AsmTopologyCache::Complete(
        crate::history_records::AsmHistoricalTopology {
            faces: vec![99],
            ..Default::default()
        },
    );
    let histories = [history, other];
    let project = |bindings: &std::collections::HashMap<String, String>| {
        super::super::project_feature_input_topologies(
            std::slice::from_ref(&feature),
            std::slice::from_ref(&scope),
            &histories,
            &[],
            bindings,
        )
        .unwrap()
    };
    assert!(project(&std::collections::HashMap::new()).is_empty());
    let bindings = std::collections::HashMap::from([(scope.id.clone(), histories[0].id.clone())]);
    let inputs = project(&bindings);
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        inputs[0].faces.as_slice(),
        &[crate::ids::history_input_face_id_charged(&feature.id, 4, 2).unwrap()]
    );
    assert_eq!(
        inputs[0].native_ref.as_deref(),
        Some(histories[0].states[0].id.as_str())
    );
    let bindings =
        std::collections::HashMap::from([(scope.id.clone(), "f3d:missing:history#0".into())]);
    assert!(project(&bindings).is_empty());
    assert!(super::super::project_feature_input_topologies(
        std::slice::from_ref(&feature),
        std::slice::from_ref(&scope),
        &histories[..1],
        &[],
        &bindings,
    )
    .unwrap()
    .is_empty());
}
