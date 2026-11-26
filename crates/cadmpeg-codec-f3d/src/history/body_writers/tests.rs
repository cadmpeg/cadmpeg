// SPDX-License-Identifier: Apache-2.0
use super::attach;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology, AsmHistory};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};

const HISTORY: &str = "f3d:Breps.BlobParts/BREP.root.smbh:asm-history#1";

fn scope(index: u32, state: i64, previous: i64) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
        DesignFeatureKind::Move,
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

fn feature(name: &str, ordinal: u64, scope: &DesignParameterScope) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("f3d:test:feature#{name}")).unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: "Extrude".into(),
                parameters: Default::default(),
            },
        )),
        native_ref: Some(scope.id.clone()),
    }
}

/// States `(state id, record version of slot 1)`, newest first; every state
/// holds body slot 1 except state 2.
fn history(states: &[(i64, i64)]) -> AsmHistory {
    let rows = states
        .iter()
        .map(|&(state_id, record)| {
            let bodies = if state_id == 2 { Vec::new() } else { vec![1] };
            let versions = bodies
                .iter()
                .map(|body| AsmEntityVersion {
                    entity_ref: *body,
                    record_ref: record,
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
        })
        .collect();
    crate::test_support::history::history(HISTORY.into(), 0, None, false, rows)
}

/// The writers of the input state of a feature at state 30 from 27, with
/// `FIRST` at state 23 from 2 and `SECOND` at state 27 from `second_from`.
fn writers(history: &AsmHistory, second_from: i64) -> Vec<(String, String)> {
    let scopes = [scope(5, 23, 2), scope(6, 27, second_from), scope(8, 30, 27)];
    let features = [
        feature("first", 0, &scopes[0]),
        feature("second", 1, &scopes[1]),
        feature("consumer", 2, &scopes[2]),
    ];
    let previous = history
        .states()
        .iter()
        .find(|candidate| candidate.state_id == 27)
        .unwrap();
    let mut topologies = vec![crate::history::input_topologies::project_input_topology(
        &features[2],
        history,
        previous,
        27,
    )
    .unwrap()];
    attach(
        &features,
        (&scopes, std::slice::from_ref(history)),
        &std::collections::HashMap::new(),
        &mut topologies,
    );
    topologies[0]
        .body_writers
        .iter()
        .map(|writer| {
            (
                writer.body.as_str().to_owned(),
                writer.writer.as_str().to_owned(),
            )
        })
        .collect()
}

#[test]
fn an_input_body_names_the_feature_that_last_updates_it() {
    let history = history(&[(30, 30), (27, 27), (23, 23), (2, 0)]);
    let found = writers(&history, 23);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, "f3d:test:feature#second");
}

#[test]
fn an_input_body_whose_last_update_has_no_scope_names_no_writer() {
    let history = history(&[(30, 30), (27, 25), (25, 25), (23, 23), (2, 0)]);
    let found = writers(&history, 25);
    assert!(found.is_empty(), "{found:?}");
}
