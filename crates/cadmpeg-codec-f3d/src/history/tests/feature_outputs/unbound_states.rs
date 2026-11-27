// SPDX-License-Identifier: Apache-2.0

use super::native_base_feature;
use crate::history::feature_outputs::bind_feature_outputs;
use crate::loss::F3dLossCode;

/// A scope that stores its state and no previous state. Its one history
/// derives the previous state, and the feature binds its output. Under two
/// histories that hold the state, no previous state is derivable: the
/// binding fails and is charged. Under no history that holds the state, the
/// body history holds nothing to bind.
#[test]
fn a_held_state_without_a_derivable_previous_state_is_charged() {
    let (feature, mut scope, history, body) = native_base_feature();
    scope
        .try_edit(|draft| {
            draft.previous_history_state_id = None;
            draft.layout_fixture_tail();
        })
        .unwrap();
    let unbound = |histories: &mut [crate::history_records::AsmHistory]| {
        let mut feature = feature.clone();
        let losses = bind_feature_outputs(
            std::slice::from_mut(&mut feature),
            std::slice::from_ref(&scope),
            histories,
            std::slice::from_ref(&body),
            &Default::default(),
            None,
        )
        .unwrap()
        .losses;
        let charged = losses
            .iter()
            .filter(|loss| loss.code == F3dLossCode::FeatureOutputStateChainUnbound.kind())
            .map(|loss| loss.message.clone())
            .collect::<Vec<_>>();
        (feature.evaluation.outputs().clone(), charged)
    };
    assert_eq!(
        unbound(&mut [history.clone()]),
        (vec![body.id.clone()], Vec::new())
    );
    assert_eq!(
        unbound(&mut [history.clone(), history]),
        (
            Vec::new(),
            vec![format!(
                "{} has state 1 in the body history and no stored or derivable previous state.",
                feature.id
            )]
        )
    );
    assert_eq!(unbound(&mut []), (Vec::new(), Vec::new()));
}
