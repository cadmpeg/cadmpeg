// SPDX-License-Identifier: Apache-2.0

use super::{bodies_topology, native_base_feature, state};
use crate::history::feature_outputs::bind_feature_outputs;
use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use std::collections::HashMap;

type Bound = (Vec<cadmpeg_ir::ids::BodyId>, Vec<String>);

/// A scope that stores state 1 and no previous state, its feature, the
/// history that holds states 1 and 0, and the active body of its output.
fn unstored_previous() -> (
    cadmpeg_ir::features::Feature,
    crate::records::feature::scope::DesignParameterScope,
    AsmHistory,
    cadmpeg_ir::topology::Body,
) {
    let (feature, mut scope, history, body) = native_base_feature();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (feature, scope, history, body)
}

/// A second history that holds state 1 alone and links it to no state.
fn abstaining_tail() -> AsmHistory {
    crate::test_support::history::history(
        "f3d:asm-history#2".into(),
        0,
        None,
        false,
        vec![state(1, 1, None, bodies_topology(&[1]))],
    )
}

/// A second history that holds state 1 alone and links it to `previous`.
fn linked_tail(previous: i64) -> AsmHistory {
    let mut tail = abstaining_tail();
    tail.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(previous);
    tail
}

fn bind(
    feature: &cadmpeg_ir::features::Feature,
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &mut [AsmHistory],
    body: &cadmpeg_ir::topology::Body,
    scope_histories: &HashMap<String, String>,
) -> Bound {
    let mut feature = feature.clone();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(scope),
        histories,
        std::slice::from_ref(body),
        scope_histories,
        None,
    )
    .unwrap()
    .losses;
    let charged = losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::FeatureOutputStateChainUnbound.kind())
        .map(|loss| loss.message.clone())
        .collect();
    (feature.evaluation.outputs().clone(), charged)
}

fn unlinked(feature: &cadmpeg_ir::features::Feature, scope: &str, reason: &str) -> String {
    format!(
        "{} has state 1 of scope {scope} in the body history, and {reason}.",
        feature.id
    )
}

/// The previous state of a scope that stores none is the state to which its
/// bound history links its state, else the one state to which the histories
/// that hold its state link it. A history that links the state to no state
/// abstains. The feature binds its output through that pair. Histories that
/// link the state to different states, a history that links it to two, and
/// histories that link it to none charge the binding with that reason. When
/// no history holds the state, nothing is charged.
#[test]
fn a_held_state_binds_through_the_previous_state_its_histories_link() {
    let (feature, scope, history, body) = unstored_previous();
    let none = HashMap::new();
    let bound_to = |id: &str| HashMap::from([(scope.id.clone(), id.to_owned())]);
    let bound = (vec![body.id.clone()], Vec::new());
    assert_eq!(
        bind(&feature, &scope, &mut [history.clone()], &body, &none),
        bound
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(0)],
            &body,
            &none
        ),
        bound,
        "histories that agree on the previous state"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), abstaining_tail()],
            &body,
            &none
        ),
        bound,
        "a history that links the state to no state abstains"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &bound_to(&history.id),
        ),
        bound,
        "the scope's bound history"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), abstaining_tail()],
            &body,
            &bound_to("f3d:asm-history#2"),
        ),
        (
            Vec::new(),
            vec![format!(
                "{} has no complete unique output transition chain from state 0 to state 1.",
                feature.id
            )]
        ),
        "a bound history that links the state to no state abstains"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &none
        ),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "the histories that hold it link it to previous states 0 and 7"
            )]
        ),
        "histories that disagree"
    );
    let mut contradicting = history.clone();
    contradicting.states_mut()[0]
        .transition
        .as_mut()
        .unwrap()
        .previous_state_id = Some(7);
    assert_eq!(
        bind(&feature, &scope, &mut [contradicting], &body, &none),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "a history that holds it links it to previous state 7 by its transition and to previous state 0 by its header"
            )]
        ),
        "a history whose transition and header disagree"
    );
    let twice = crate::test_support::history::history(
        "f3d:asm-history#3".into(),
        0,
        None,
        false,
        vec![
            state(1, 2, None, bodies_topology(&[1])),
            state(1, 1, None, bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), twice],
            &body,
            &none
        ),
        (
            Vec::new(),
            vec![unlinked(&feature, &scope.id, "a history holds it twice")]
        ),
        "a history that holds the state twice"
    );
    assert_eq!(
        bind(&feature, &scope, &mut [abstaining_tail()], &body, &none),
        (
            Vec::new(),
            vec![unlinked(
                &feature,
                &scope.id,
                "no history that holds it links it to a previous state"
            )]
        ),
        "histories that link the state to no state"
    );
    assert_eq!(
        bind(&feature, &scope, &mut [], &body, &none),
        (Vec::new(), Vec::new())
    );
}

/// Histories that share the scope's bound history id are the histories the
/// scope reads first, so a history with another id that holds the state
/// twice does not take part.
#[test]
fn histories_that_share_the_bound_history_id_link_the_previous_state() {
    let (feature, scope, history, body) = unstored_previous();
    let mut namesake = abstaining_tail();
    namesake.id.clone_from(&history.id);
    let twice = crate::test_support::history::history(
        "f3d:asm-history#3".into(),
        0,
        None,
        false,
        vec![
            state(1, 2, None, bodies_topology(&[1])),
            state(1, 1, None, bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    assert_eq!(
        crate::history::history_states::scope_states(
            &scope,
            &HashMap::from([(scope.id.clone(), history.id.clone())]),
            &[history.clone(), namesake.clone(), twice.clone()],
        ),
        Some(crate::history::history_states::ScopeStates {
            state_id: 1,
            previous: crate::history::history_states::PreviousState::Linked(0),
        })
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), namesake, twice],
            &body,
            &HashMap::from([(scope.id.clone(), history.id.clone())]),
        ),
        (
            Vec::new(),
            vec![format!(
                "{} has no complete unique output transition chain from state 0 to state 1.",
                feature.id
            )]
        )
    );
}

/// A derived base group produces the state of its last scope from the
/// previous state of its first scope, read by the same rule. The charge
/// names the first scope and its state.
#[test]
fn a_derived_base_group_reads_the_linked_previous_state_of_its_first_scope() {
    use crate::history::feature_outputs::{feature_history_states, FeatureStates};
    use crate::history::history_states::{PreviousState, ScopeStates, StateConflict};
    let (mut feature, first, _, body) = unstored_previous();
    let mut last = super::scope_between(2, 1);
    last.try_edit(|draft| {
        draft.record_index = 2;
        draft.layout_fixture_tail();
    })
    .unwrap();
    feature.native_ref = Some(last.id.clone());
    feature.source_properties.insert(
        crate::records::feature::derived_base::derived_base_scopes_property(),
        format!("{},{}", first.record_index, last.record_index),
    );
    let scopes = [first.clone(), last.clone()];
    let history = crate::test_support::history::history(
        "f3d:asm-history#1".into(),
        0,
        None,
        false,
        vec![
            state(2, 2, None, bodies_topology(&[1])),
            state(1, 1, None, bodies_topology(&[1])),
            state(
                0,
                0,
                None,
                crate::history_records::AsmHistoricalTopology::default(),
            ),
        ],
    );
    let none = HashMap::new();
    let placed = |previous| {
        Some(FeatureStates {
            state_id: 2,
            scope: &scopes[0].id,
            from: ScopeStates {
                state_id: 1,
                previous,
            },
        })
    };
    assert_eq!(
        feature_history_states(&feature, &scopes, std::slice::from_ref(&history), &none),
        placed(PreviousState::Linked(0))
    );
    let mut histories = [history, linked_tail(7)];
    assert_eq!(
        feature_history_states(&feature, &scopes, &histories, &none),
        placed(PreviousState::Conflicting(StateConflict::Histories(0, 7)))
    );
    let mut bound = feature.clone();
    let losses = bind_feature_outputs(
        std::slice::from_mut(&mut bound),
        &scopes,
        &mut histories,
        std::slice::from_ref(&body),
        &none,
        None,
    )
    .unwrap()
    .losses;
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::FeatureOutputStateChainUnbound.kind())
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>(),
        vec![unlinked(
            &feature,
            &first.id,
            "the histories that hold it link it to previous states 0 and 7"
        )]
    );
}

/// A Base Feature whose stored bodies resolved takes those bodies as its
/// outputs after the history pass, and a Base Feature projected as inserted
/// copies takes the copies, so neither drops anything at an unlinked state.
#[test]
fn a_resolved_base_feature_with_an_unlinked_state_is_not_charged() {
    use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers, InsertedBodies};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = unstored_previous();
    let mut inserted = feature.clone();
    feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::BaseFeature {
                bodies: BodySelection::Resolved {
                    bodies: DistinctMembers::try_from(vec![body.id.clone()]).unwrap(),
                    native: scope.id.clone(),
                },
            },
        ));
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &HashMap::new()
        ),
        (vec![body.id.clone()], Vec::new())
    );
    inserted
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::InsertBodies {
                bodies: InsertedBodies::Resolved {
                    native: scope.id.clone(),
                },
            },
        ));
    assert_eq!(
        bind(
            &inserted,
            &scope,
            &mut [history, linked_tail(7)],
            &body,
            &HashMap::new()
        ),
        (Vec::new(), Vec::new())
    );
}
