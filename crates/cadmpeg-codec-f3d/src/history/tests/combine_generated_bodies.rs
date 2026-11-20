// SPDX-License-Identifier: Apache-2.0
//! Combine operands named by the features that generate them.

#![allow(clippy::default_trait_access)]

use crate::history::FeatureBodySelectionInputs;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};
use cadmpeg_ir::features::{
    BodySelection, BooleanKind, CombineDestination, CombineOperands, Feature, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation,
};

fn scope(
    index: u32,
    state: i64,
    previous: i64,
) -> crate::records::feature::scope::DesignParameterScope {
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
}

fn feature(id: &str, scope: &str, definition: FeatureDefinition) -> Feature {
    Feature {
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
    }
}

fn state(state_id: i64, bodies: Vec<i64>) -> crate::test_support::history::State {
    let versions = bodies
        .iter()
        .map(|body| AsmEntityVersion {
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

/// Two spheres that insert body slots 1 and 2, and a `Combine` whose target
/// and tool are those slots in its input state 27.
fn combine_case(
    live: &[cadmpeg_ir::topology::Body],
) -> (Feature, Vec<cadmpeg_ir::features::FeatureResultTopology>) {
    let scopes = [scope(5, 23, 2), scope(6, 27, 23), scope(8, 30, 27)];
    let combine_id = FeatureId::mint("f3d:test:feature#combine").unwrap();
    let prefix = crate::ids::feature_input_prefix(&combine_id, 27);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let target = BodySelection::historical(
        state_id.clone(),
        vec![crate::ids::history_input_body_id(&prefix, 1)],
        "f3d:Design/BulkStream.dat:design-record#488".to_owned(),
    )
    .unwrap();
    let tools = BodySelection::historical(
        state_id,
        vec![crate::ids::history_input_body_id(&prefix, 2)],
        "f3d:Design/BulkStream.dat:design-record#481".to_owned(),
    )
    .unwrap();
    let unresolved = || {
        FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: cadmpeg_ir::features::UnresolvedFamily::Combine,
        })
    };
    let mut features = vec![
        feature("f3d:test:feature#sphere-one", &scopes[0].id, unresolved()),
        feature("f3d:test:feature#sphere-two", &scopes[1].id, unresolved()),
        feature(
            combine_id.as_str(),
            &scopes[2].id,
            FeatureDefinition::Operation(FeatureOperation::Combine {
                operands: CombineOperands::new(target, tools).unwrap(),
                op: BooleanKind::Cut,
                keep_tools: false,
                destination: CombineDestination::TargetComponent,
            }),
        ),
    ];
    let histories = [crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            state(30, vec![1]),
            state(27, vec![1, 2]),
            state(23, vec![1]),
            state(2, Vec::new()),
        ],
    )];
    let inputs = FeatureBodySelectionInputs {
        scopes: &scopes,
        groups: &[],
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &histories,
        bodies: live,
        regions: &[],
        shells: &[],
        faces: &[],
    };
    let results = crate::history::bind_generated_body_selections(
        &mut features,
        &inputs,
        &std::collections::HashMap::new(),
        None,
        &mut Vec::new(),
    );
    (features.remove(2), results)
}

#[test]
fn combine_operands_the_final_model_omits_are_generated_by_their_spheres() {
    let (combine, results) = combine_case(&[]);
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        combine.evaluation.definition()
    else {
        panic!("combine definition");
    };
    let generated = |selection: &BodySelection| match selection {
        BodySelection::Generated { bodies, native } => (
            bodies
                .iter()
                .map(|body| {
                    (
                        body.feature.as_str().to_owned(),
                        body.local_id.as_str().to_owned(),
                    )
                })
                .collect::<Vec<_>>(),
            native.as_str().to_owned(),
        ),
        other => panic!("generated selection: {other:?}"),
    };
    assert_eq!(
        generated(operands.target()),
        (
            vec![("f3d:test:feature#sphere-one".to_owned(), "1".to_owned())],
            "f3d:Design/BulkStream.dat:design-record#488".to_owned()
        )
    );
    assert_eq!(
        generated(operands.tools()),
        (
            vec![("f3d:test:feature#sphere-two".to_owned(), "2".to_owned())],
            "f3d:Design/BulkStream.dat:design-record#481".to_owned()
        )
    );
    for producer in ["f3d:test:feature#sphere-one", "f3d:test:feature#sphere-two"] {
        assert!(combine
            .dependencies
            .as_slice()
            .iter()
            .any(|dependency| dependency.as_str() == producer));
        assert!(results
            .iter()
            .any(|result| result.output_of.as_str() == producer));
    }
}

#[test]
fn a_combine_with_a_kept_operand_stays_historical() {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").unwrap();
    let (combine, results) = combine_case(std::slice::from_ref(&body));
    assert!(results.is_empty());
    assert!(matches!(
        combine.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. })
            if matches!(operands.target(), BodySelection::Historical { .. })
    ));
}
