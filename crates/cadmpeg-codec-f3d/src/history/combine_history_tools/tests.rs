// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::operation_settings::BooleanKind;
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::{
    CombineOperands, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};

use super::bind;
use crate::records::bodies::{DesignBodyBinding, DesignBodyBindingWire};
use crate::records::feature::combine::{
    DesignCombineBodyNode, DesignCombineBodySelection, DesignCombineForm, DesignCombineOperation,
    DesignCombineTools,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn scope_id(record_index: u32) -> String {
    format!("{STREAM}:design-parameter-scope#{record_index}")
}

fn scope(kind: DesignFeatureKind, record_index: u32) -> DesignParameterScope {
    DesignParameterScope::empty(&scope_id(record_index), kind, record_index)
}

fn feature(name: &str, ordinal: u64, native_ref: &str) -> Feature {
    Feature {
        id: FeatureId::mint(format!("test:model:feature#{name}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(
                    BodySelection::Native("target".into()),
                    BodySelection::NativeSet(
                        vec!["tool-a".to_owned(), "tool-b".to_owned()]
                            .try_into()
                            .unwrap(),
                    ),
                )
                .unwrap(),
                op: BooleanKind::Join,
                keep_tools: false,
                destination: Default::default(),
            }),
            DistinctMembers::try_from(vec![]).unwrap(),
        ),
        native_ref: Some(native_ref.to_owned()),
        product_definition: None,
    }
}

/// A Combine scope in the operand-list form with one browser node per tool.
fn combine_scope(tools: &[(u32, u32)]) -> DesignParameterScope {
    let mut scope = scope(DesignFeatureKind::Combine, 400);
    let selection = |(entity, creator): (u32, u32), record_index| DesignCombineBodySelection {
        record_index,
        external_identity: None,
        legacy_external_identity: None,
        body_node: Some(DesignCombineBodyNode {
            record_index: entity + 1,
            body_entity_suffix: entity,
            creating_scope_record_index: creator,
        }),
    };
    let mut rows = tools
        .iter()
        .zip(12u32..)
        .map(|(tool, at)| selection(*tool, at));
    if let DesignScopePayloadMut::Combine(slot) = scope.payload_mut() {
        slot.operation = Some(DesignCombineOperation {
            form: DesignCombineForm::OperandLists,
            operation: BooleanKind::Join,
            operation_offset: 0,
            keep_tools: false,
            keep_tools_offset: 0,
            destination: Default::default(),
            target_record_index: 11,
            target_body_node: None,
            tools: DesignCombineTools {
                first: rows.next().unwrap(),
                additional: rows.collect(),
            },
            created_body_nodes: Vec::new(),
            consumed_body_nodes: Vec::new(),
        });
    }
    scope
}

fn pair(entity_suffix: u64, history_only: bool) -> DesignBodyBinding {
    DesignBodyBinding::try_from(DesignBodyBindingWire {
        component_owner: None,
        component_owner_unresolved: false,
        history_only,
        id: format!("{STREAM}:design-body-binding#{}", entity_suffix * 100),
        stream: "Design/BulkStream.dat".into(),
        pair_count: 1,
        pair_ordinal: 0,
        asm_body_key: 9,
        asm_body_key_offset: entity_suffix * 100,
        entity_suffix,
        entity_suffix_offset: entity_suffix * 100 + 8,
        blob_name: "BREP.tool.smbh".into(),
        blob_name_offset: entity_suffix * 100 + 16,
        body: None,
    })
    .unwrap()
}

/// Assert that the one loss states that generated tools keep body-map pair
/// identities.
fn assert_result_state_unbound(losses: &[cadmpeg_ir::report::loss::LossNote]) {
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        "feature.combine-tool-result-state-unbound"
    );
}

fn tools_of(feature: &Feature) -> &BodySelection {
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        panic!("Combine feature");
    };
    operands.tools()
}

#[test]
fn history_only_tools_become_generated_bodies_of_their_creating_features() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::Extrude, 200),
        combine_scope(&[(150, 100), (250, 200)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("second", 2, &scope_id(200)),
        feature("combine", 3, &scope_id(400)),
    ];
    let bindings = [pair(150, true), pair(250, true)];
    let (losses, _, results) = bind(
        &mut features,
        &scopes,
        &bindings,
        (&[], &std::collections::HashMap::new()),
    );
    assert_result_state_unbound(&losses);
    assert!(results.is_empty());
    let BodySelection::Generated { bodies, native } = tools_of(&features[2]) else {
        panic!("generated tools");
    };
    let generators = bodies
        .iter()
        .map(|body| body.feature.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        generators,
        [features[0].id.as_str(), features[1].id.as_str()]
    );
    assert_eq!(bodies[0].local_id.as_str(), bindings[0].id());
    assert_eq!(bodies[1].local_id.as_str(), bindings[1].id());
    assert_eq!(native.as_str(), scope_id(400));
    for generator in [&features[0], &features[1]] {
        assert!(features[2].dependencies.contains(&generator.id));
    }
}

#[test]
fn a_tool_that_is_not_proven_leaves_every_tool_native_and_reports_the_reason() {
    let scopes = || {
        vec![
            scope(DesignFeatureKind::Extrude, 100),
            scope(DesignFeatureKind::Extrude, 200),
            combine_scope(&[(150, 100), (250, 200)]),
        ]
    };
    let features = || {
        vec![
            feature("first", 1, &scope_id(100)),
            feature("second", 2, &scope_id(200)),
            feature("combine", 3, &scope_id(400)),
        ]
    };
    // A pair that is not history-only, an absent pair, a creating scope that is
    // not a projected feature, and a generator that does not precede the Combine
    // each leave the selection native. The proven first tool keeps its generator.
    let cases = [
        (vec![pair(150, true), pair(250, false)], features()),
        (vec![pair(150, true)], features()),
        (vec![pair(150, true), pair(250, true)], {
            let mut rows = features();
            rows.remove(1);
            rows
        }),
        (vec![pair(150, true), pair(250, true)], {
            let mut rows = features();
            rows[1].ordinal = 3;
            rows
        }),
    ];
    for (bindings, mut features) in cases {
        let (losses, _, _) = bind(
            &mut features,
            &scopes(),
            &bindings,
            (&[], &std::collections::HashMap::new()),
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.combine-tool-body-unbound"
        );
        let combine = features.last().unwrap();
        assert!(matches!(tools_of(combine), BodySelection::NativeSet(_)));
        assert_eq!(
            combine.dependencies.iter().collect::<Vec<_>>(),
            [&features[0].id]
        );
    }
}

#[test]
fn a_derived_base_creator_stays_native_and_the_proven_tool_keeps_its_generator() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::native("Derived Base Feature"), 350),
        scope(DesignFeatureKind::DerivedInstance, 380),
        combine_scope(&[(150, 100), (360, 350)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("instance", 2, &scope_id(380)),
        feature("combine", 4, &scope_id(400)),
    ];
    let (losses, _, _) = bind(
        &mut features,
        &scopes,
        &[pair(150, true), pair(360, true)],
        (&[], &std::collections::HashMap::new()),
    );
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert_eq!(
        losses[0].code.local_code(),
        "feature.combine-tool-body-unbound"
    );
    let combine = &features[2];
    assert!(matches!(tools_of(combine), BodySelection::NativeSet(_)));
    assert_eq!(
        combine.dependencies.iter().collect::<Vec<_>>(),
        [&features[0].id]
    );
    assert_eq!(
        combine
            .source_properties
            .iter()
            .find(|(key, _)| key.as_str() == "combine:tool:0:generator")
            .map(|(_, value)| value.as_str()),
        Some(features[0].id.as_str())
    );
    assert!(!combine
        .source_properties
        .keys()
        .any(|key| key.as_str().starts_with("combine:tool:1:")));
}

#[test]
fn every_proven_tool_records_its_generator() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::Extrude, 200),
        combine_scope(&[(150, 100), (250, 200)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("second", 2, &scope_id(200)),
        feature("combine", 3, &scope_id(400)),
    ];
    let (losses, _, _) = bind(
        &mut features,
        &scopes,
        &[pair(150, true), pair(250, true)],
        (&[], &std::collections::HashMap::new()),
    );
    assert_result_state_unbound(&losses);
    let properties = features[2]
        .source_properties
        .iter()
        .map(|(key, value)| (key.as_str().to_owned(), value.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        properties,
        [
            (
                "combine:tool:0:generator".to_owned(),
                features[0].id.as_str().to_owned()
            ),
            (
                "combine:tool:1:generator".to_owned(),
                features[1].id.as_str().to_owned()
            ),
        ]
    );
}

#[test]
fn a_combine_whose_tools_name_no_history_only_pair_is_left_alone() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        combine_scope(&[(150, 100)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("combine", 3, &scope_id(400)),
    ];
    let (losses, _, _) = bind(
        &mut features,
        &scopes,
        &[pair(150, false)],
        (&[], &std::collections::HashMap::new()),
    );
    assert!(losses.is_empty());
    assert!(matches!(
        tools_of(&features[1]),
        BodySelection::NativeSet(_)
    ));
}

#[test]
fn a_captured_base_feature_generates_the_second_combine_tool() {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::native("Derived Base Feature"), 200),
        combine_scope(&[(150, 100), (250, 200)]),
    ];
    let mut features = vec![
        feature("extrude", 1, &scope_id(100)),
        feature("derived", 2, &scope_id(200)),
        feature("combine", 3, &scope_id(400)),
    ];
    let prefix = crate::ids::feature_input_prefix(&features[1].id, 92);
    features[1]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Historical {
                    state: crate::ids::history_input_state_id(&prefix),
                    bodies: vec![crate::ids::history_input_body_id(&prefix, 7)]
                        .try_into()
                        .unwrap(),
                    native: scope_id(200).try_into().unwrap(),
                },
            },
        ));
    let (losses, _, _) = bind(
        &mut features,
        &scopes,
        &[pair(150, true), pair(250, true)],
        (&[], &std::collections::HashMap::new()),
    );
    assert_result_state_unbound(&losses);
    let BodySelection::Generated { bodies: tools, .. } = tools_of(&features[2]) else {
        panic!("generated tools")
    };
    assert_eq!(tools.len(), 2);
    assert_eq!(&tools[1].feature, &features[1].id);
    assert_eq!(
        features[2]
            .source_properties
            .iter()
            .find(|(key, _)| key.as_str() == "combine:tool:1:generator")
            .map(|(_, value)| value.as_str()),
        Some(features[1].id.as_str())
    );
}

fn result_state(
    generator: &Feature,
    bodies: &[&str],
) -> cadmpeg_ir::features::evaluation::FeatureResultTopology {
    let members = cadmpeg_ir::features::evaluation::FeatureResultMembers::new(
        bodies
            .iter()
            .map(|body| cadmpeg_core::text::NonBlankString::new((*body).to_owned()).unwrap())
            .collect(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .unwrap();
    cadmpeg_ir::features::evaluation::FeatureResultTopology::new(
        crate::ids::history_result_state_id(&generator.id, 4),
        generator.id.clone(),
        members,
        None,
    )
}

fn bound_two_tool_combine() -> (Vec<Feature>, Vec<super::ReplacedTools>) {
    let scopes = vec![
        scope(DesignFeatureKind::Extrude, 100),
        scope(DesignFeatureKind::Extrude, 200),
        combine_scope(&[(150, 100), (250, 200)]),
    ];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("second", 2, &scope_id(200)),
        feature("combine", 3, &scope_id(400)),
    ];
    let (losses, replaced, _) = bind(
        &mut features,
        &scopes,
        &[pair(150, true), pair(250, true)],
        (&[], &std::collections::HashMap::new()),
    );
    assert_result_state_unbound(&losses);
    (features, replaced)
}

#[test]
fn a_tool_of_a_one_body_result_state_takes_the_listed_identity() {
    let (mut features, replaced) = bound_two_tool_combine();
    let results = [result_state(&features[0], &["2"])];
    let losses = super::align_with_result_states(&mut features, &results, replaced);
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Generated { bodies, .. } = tools_of(&features[2]) else {
        panic!("generated tools");
    };
    assert_eq!(bodies[0].local_id.as_str(), "2");
    assert_eq!(bodies[1].local_id.as_str(), pair(250, true).id());
}

#[test]
fn a_tool_absent_from_a_several_body_result_state_restores_the_native_tools() {
    let (mut features, replaced) = bound_two_tool_combine();
    let results = [result_state(&features[0], &["2", "3"])];
    let losses = super::align_with_result_states(&mut features, &results, replaced);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(!matches!(
        tools_of(&features[2]),
        BodySelection::Generated { .. }
    ));
}

const TOOL_HISTORY: &str = "f3d:asset/Breps.BlobParts/BREP.tool.smbh:asm-history#2";

fn live_pair(entity_suffix: u64) -> DesignBodyBinding {
    let mut value = json::to_value(pair(entity_suffix, false)).unwrap();
    value["body"] = "f3d:brep:body#7".into();
    json::from_value(value).unwrap()
}

/// A component history whose state 1 inserts body slot 7, then the given
/// later topologies.
fn tool_history(later: &[&[i64]]) -> crate::history_records::AsmHistory {
    let topologies = [&[][..], &[7][..]]
        .into_iter()
        .chain(later.iter().copied())
        .collect::<Vec<_>>();
    let rows = topologies
        .iter()
        .enumerate()
        .rev()
        .map(|(id, bodies)| {
            let id = i64::try_from(id).unwrap();
            (
                crate::history_records::AsmDeltaState {
                    id: format!("{TOOL_HISTORY}:asm-delta-state#{id}"),
                    parent: TOOL_HISTORY.into(),
                    byte_offset: 0,
                    state_id: id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(crate::history_records::AsmHistoricalTopology {
                    bodies: bodies.to_vec(),
                    ..Default::default()
                }),
                Vec::new(),
            )
        })
        .collect();
    crate::test_support::history::history(TOOL_HISTORY.into(), 0, None, false, rows)
}

fn component_scope() -> DesignParameterScope {
    let mut creator = scope(DesignFeatureKind::Extrude, 100);
    creator
        .try_edit(|draft| {
            draft.history_state_id = Some(1);
            draft.previous_history_state_id = Some(0);
            draft.layout_fixture_tail();
        })
        .unwrap();
    creator
}

fn bind_component_tool(
    history: &crate::history_records::AsmHistory,
) -> (Vec<Feature>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let scopes = vec![component_scope(), combine_scope(&[(150, 100)])];
    let mut features = vec![
        feature("first", 1, &scope_id(100)),
        feature("combine", 3, &scope_id(400)),
    ];
    let scope_histories =
        std::collections::HashMap::from([(scope_id(100), TOOL_HISTORY.to_owned())]);
    let (losses, _, _) = bind(
        &mut features,
        &scopes,
        &[live_pair(150)],
        (std::slice::from_ref(history), &scope_histories),
    );
    (features, losses)
}

#[test]
fn a_live_tool_of_another_history_selects_its_model_body() {
    let (features, losses) = bind_component_tool(&tool_history(&[]));
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Resolved { bodies, native } = tools_of(&features[1]) else {
        panic!("model-body tools: {:?}", tools_of(&features[1]));
    };
    assert_eq!(
        bodies
            .iter()
            .map(cadmpeg_ir::ids::BodyId::as_str)
            .collect::<Vec<_>>(),
        ["f3d:brep:body#7"]
    );
    assert_eq!(native.as_str(), scope_id(400));
    assert!(features[1].dependencies.contains(&features[0].id));
}

#[test]
fn a_live_tool_that_a_later_transition_changes_stays_native() {
    let (features, losses) = bind_component_tool(&tool_history(&[&[]]));
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].message.contains("later transition"), "{losses:?}");
    assert!(matches!(
        tools_of(&features[1]),
        BodySelection::NativeSet(_)
    ));
}

/// A history whose states are listed from newest to oldest, each with its
/// body slots and the record of each slot.
fn versioned_history(states: &[(i64, &[(i64, i64)])]) -> crate::history_records::AsmHistory {
    let rows = states
        .iter()
        .map(|(id, bodies)| {
            (
                crate::history_records::AsmDeltaState {
                    id: format!("{TOOL_HISTORY}:asm-delta-state#{id}"),
                    parent: TOOL_HISTORY.into(),
                    byte_offset: 0,
                    state_id: *id,
                    version_flag: 1,
                    state_flag: 0,
                    previous_ref: None,
                    next_ref: None,
                    node_index: *id,
                    partner_ref: None,
                    owner_ref: 0,
                    bulletin_boards: Vec::new(),
                    records: Vec::new(),
                    transition: None,
                },
                Some(crate::history_records::AsmHistoricalTopology {
                    bodies: bodies.iter().map(|(body, _)| *body).collect(),
                    ..Default::default()
                }),
                bodies
                    .iter()
                    .map(|(body, record)| crate::history_records::AsmEntityVersion {
                        entity_ref: *body,
                        record_ref: *record,
                    })
                    .collect(),
            )
        })
        .collect();
    crate::test_support::history::history(TOOL_HISTORY.into(), 0, None, false, rows)
}

fn state_scope(
    kind: DesignFeatureKind,
    record_index: u32,
    state: i64,
    previous: i64,
) -> DesignParameterScope {
    let mut scope = scope(kind, record_index);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(state);
            draft.previous_history_state_id = Some(previous);
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

/// A component chain in `TOOL_HISTORY`: a new-body feature (state 1) inserts
/// slot 7, a cut (state 2) updates slot 7, and a body mirror (state 3) inserts
/// slot 8. A Combine takes the new body and the mirrored body as history-only
/// tools. `combine_history` binds the Combine scope to `TOOL_HISTORY`, and
/// `cut_ordinal` places the cut in the timeline.
fn bind_component_chain(
    combine_history: bool,
    cut_ordinal: u64,
) -> (
    Vec<Feature>,
    Vec<cadmpeg_ir::report::loss::LossNote>,
    Vec<cadmpeg_ir::features::evaluation::FeatureResultTopology>,
) {
    let history = versioned_history(&[
        (3, &[(7, 2), (8, 3)]),
        (2, &[(7, 2)]),
        (1, &[(7, 1)]),
        (0, &[]),
    ]);
    let scopes = vec![
        state_scope(DesignFeatureKind::Extrude, 100, 1, 0),
        state_scope(DesignFeatureKind::Extrude, 200, 2, 1),
        state_scope(DesignFeatureKind::native("Mirror"), 300, 3, 2),
        combine_scope(&[(150, 100), (350, 300)]),
    ];
    let mut features = vec![
        feature("new-body", 1, &scope_id(100)),
        feature("cut", cut_ordinal, &scope_id(200)),
        feature("mirror", 3, &scope_id(300)),
        feature("combine", 4, &scope_id(400)),
    ];
    let mut scope_histories = std::collections::HashMap::from([
        (scope_id(100), TOOL_HISTORY.to_owned()),
        (scope_id(200), TOOL_HISTORY.to_owned()),
        (scope_id(300), TOOL_HISTORY.to_owned()),
    ]);
    if combine_history {
        scope_histories.insert(scope_id(400), TOOL_HISTORY.to_owned());
    }
    let (mut losses, replaced, results) = bind(
        &mut features,
        &scopes,
        &[pair(150, true), pair(350, true)],
        (std::slice::from_ref(&history), &scope_histories),
    );
    let mut states = Vec::new();
    losses.extend(super::apply_result_states(
        &mut features,
        &mut states,
        (results, replaced),
    ));
    (features, losses, states)
}

/// The body members of the result state of `feature`.
fn result_bodies<'a>(
    states: &'a [cadmpeg_ir::features::evaluation::FeatureResultTopology],
    feature: &Feature,
) -> Vec<&'a str> {
    let mut rows = states.iter().filter(|state| state.output_of == feature.id);
    let state = rows.next().expect("one result state");
    assert!(rows.next().is_none(), "one result state");
    state
        .bodies()
        .iter()
        .map(cadmpeg_core::text::NonBlankString::as_str)
        .collect()
}

#[test]
fn consumed_component_bodies_have_result_states_that_name_the_tools() {
    for combine_history in [false, true] {
        let (features, losses, states) = bind_component_chain(combine_history, 2);
        assert!(losses.is_empty(), "{losses:?}");
        let BodySelection::Generated { bodies, .. } = tools_of(&features[3]) else {
            panic!("generated tools: {:?}", tools_of(&features[3]));
        };
        let tools = bodies
            .iter()
            .map(|body| (body.feature.as_str(), body.local_id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            tools,
            [
                (features[0].id.as_str(), "7"),
                (features[2].id.as_str(), "8")
            ]
        );
        assert_eq!(states.len(), 3);
        assert_eq!(result_bodies(&states, &features[0]), ["7"]);
        assert_eq!(result_bodies(&states, &features[1]), ["7"]);
        assert_eq!(result_bodies(&states, &features[2]), ["8"]);
        assert_eq!(
            states
                .iter()
                .find(|state| state.output_of == features[1].id)
                .map(|state| state.id.clone()),
            Some(crate::ids::history_result_state_id(&features[1].id, 2))
        );
    }
}

#[test]
fn a_writer_after_the_combine_has_no_tool_result_state() {
    let (features, losses, states) = bind_component_chain(false, 5);
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(states.len(), 2);
    assert!(states.iter().all(|state| state.output_of != features[1].id));
}

#[test]
fn a_tool_result_state_joins_the_result_state_of_its_feature() {
    let generator = feature("first", 1, &scope_id(100));
    let mut states = vec![result_state(&generator, &["9"])];
    super::add_result_states(
        &mut states,
        vec![super::ToolResultState {
            feature: generator.id.clone(),
            state_id: 4,
            slots: vec![9, 7],
        }],
    );
    assert_eq!(states.len(), 1);
    assert_eq!(result_bodies(&states, &generator), ["9", "7"]);
}

#[test]
fn a_writer_result_state_of_this_pass_keeps_a_fallback_tool_identity() {
    let (mut features, replaced) = bound_two_tool_combine();
    let writer = super::ToolResultState {
        feature: features[0].id.clone(),
        state_id: 7,
        slots: vec![3, 4],
    };
    let mut states = Vec::new();
    let losses = super::apply_result_states(&mut features, &mut states, (vec![writer], replaced));
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Generated { bodies, .. } = tools_of(&features[2]) else {
        panic!("generated tools");
    };
    assert_eq!(bodies[0].local_id.as_str(), pair(150, true).id());
    assert_eq!(bodies[1].local_id.as_str(), pair(250, true).id());
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].output_of, features[0].id);
}

#[test]
fn a_tool_listed_by_a_several_body_result_state_keeps_its_identity() {
    let (mut features, replaced) = bound_two_tool_combine();
    let results = [
        result_state(&features[0], &["2", pair(150, true).id()]),
        result_state(&features[1], &["5", pair(250, true).id()]),
    ];
    let losses = super::align_with_result_states(&mut features, &results, replaced);
    assert!(losses.is_empty(), "{losses:?}");
    let BodySelection::Generated { bodies, .. } = tools_of(&features[2]) else {
        panic!("generated tools");
    };
    assert_eq!(bodies[0].local_id.as_str(), pair(150, true).id());
    assert_eq!(bodies[1].local_id.as_str(), pair(250, true).id());
}
