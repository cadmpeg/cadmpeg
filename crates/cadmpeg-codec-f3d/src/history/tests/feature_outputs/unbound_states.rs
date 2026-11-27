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

/// A second history that holds state 1 alone and links it to `previous`.
fn linked_tail(previous: i64) -> AsmHistory {
    let mut tail = crate::test_support::history::history(
        "f3d:asm-history#2".into(),
        0,
        None,
        false,
        vec![state(1, 1, None, bodies_topology(&[1]))],
    );
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

fn unlinked(feature: &cadmpeg_ir::features::Feature, scope: &str) -> String {
    format!(
        "{} has state 1 of scope {scope} in the body history, and the histories that hold it link it to no single previous state.",
        feature.id
    )
}

/// The previous state of a scope that stores none is the link of its state
/// in its bound history, else the one link of every history that holds the
/// state. The feature binds its output through that pair. When the
/// histories link the state to different states, the binding is charged with
/// that reason. When no history holds the state, nothing is charged.
#[test]
fn a_held_state_binds_through_the_previous_state_its_histories_link() {
    let (feature, scope, history, body) = unstored_previous();
    let none = HashMap::new();
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
    let mut copy = history.clone();
    copy.id = "f3d:asm-history#2".into();
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), copy],
            &body,
            &HashMap::from([(scope.id.clone(), "f3d:asm-history#2".to_owned())]),
        ),
        bound,
        "the scope's bound history"
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history.clone(), linked_tail(7)],
            &body,
            &none
        ),
        (Vec::new(), vec![unlinked(&feature, &scope.id)]),
        "histories that disagree"
    );
    assert_eq!(
        bind(&feature, &scope, &mut [], &body, &none),
        (Vec::new(), Vec::new())
    );
}

/// A derived base group reads the previous state of its first scope by the
/// same rule, and the charge names that scope.
#[test]
fn a_derived_base_group_reads_the_linked_previous_state_of_its_first_scope() {
    let (mut feature, scope, history, body) = unstored_previous();
    feature.source_properties.insert(
        crate::records::feature::derived_base::derived_base_scopes_property(),
        scope.record_index.to_string(),
    );
    let none = HashMap::new();
    assert_eq!(
        bind(&feature, &scope, &mut [history.clone()], &body, &none),
        (vec![body.id.clone()], Vec::new())
    );
    assert_eq!(
        bind(
            &feature,
            &scope,
            &mut [history, linked_tail(7)],
            &body,
            &none
        ),
        (Vec::new(), vec![unlinked(&feature, &scope.id)])
    );
}

/// A Base Feature whose stored bodies resolved takes those bodies as its
/// outputs after the history pass, so its unlinked state drops nothing.
#[test]
fn a_resolved_base_feature_with_an_unlinked_state_is_not_charged() {
    use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let (mut feature, scope, history, body) = unstored_previous();
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
            &mut [history, linked_tail(7)],
            &body,
            &HashMap::new()
        ),
        (vec![body.id.clone()], Vec::new())
    );
}
