// SPDX-License-Identifier: Apache-2.0
//! A scope's suppression comes from state bytes 0 and 1 of its tail, else
//! from its null history states. A tail with the no-result flag and no
//! suppression state byte is an active feature with no current result.

use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopeTailFlags,
};

/// The tail state of one scope: state bytes and the no-result flag.
#[derive(Clone, Copy)]
struct State {
    state: [bool; 3],
    no_result: bool,
}

/// The suppression of an Extrude scope, active (with a current state) or
/// not, whose tail stores `state` (`None` where the tail is unread), and the
/// tail-state loss codes its projection charges.
fn suppressed(
    active: bool,
    state: Option<State>,
) -> (Option<bool>, Vec<cadmpeg_ir::report::loss::LossKind>) {
    let mut scope = DesignParameterScope::empty(
        "f3d:native/BulkStream.dat:design-parameter-scope#12",
        DesignFeatureKind::Extrude,
        12,
    );
    if active {
        scope
            .try_edit(|draft| draft.history_state_id = Some(7))
            .unwrap();
    }
    scope.tail_flags = state.map(|State { state, no_result }| DesignScopeTailFlags {
        suppression: [state[0], state[1]],
        unevaluated: state[2],
        state_offset: scope.feature_ordinal_offset(),
        no_result,
        rolled_back: false,
        no_result_offset: scope.feature_ordinal_offset() + 3,
        envelope_offset: None,
    });
    let id = scope.id.clone();
    let crate::design::feature_project::ProjectedParameterDesign {
        features,
        projection_losses,
        ..
    } = crate::test_support::with_decode_context(|ctx| {
        let scopes = &[scope];
        let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
        crate::design::feature_project::project_parameter_design_with_edge_identities(
            ctx,
            &crate::design::feature_project::ProjectInputs {
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .expect("test projection has a synthetic exact timeline")
    });
    let suppressed = features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(id.as_str()))
        .expect("the scope's feature")
        .suppressed;
    let state_codes = [
        F3dLossCode::FeatureNoResultActiveAssumed.kind(),
        F3dLossCode::FeatureUnevaluatedStateAssumed.kind(),
    ];
    let codes = projection_losses
        .into_iter()
        .map(|loss| loss.code)
        .filter(|code| state_codes.contains(code))
        .collect();
    (suppressed, codes)
}

const fn state(state: [bool; 3], no_result: bool) -> State {
    State { state, no_result }
}

#[test]
fn either_suppression_state_byte_suppresses_the_feature() {
    for active in [false, true] {
        for bytes in [
            [true, false, false],
            [false, true, false],
            [true, true, true],
        ] {
            let (suppressed, codes) = suppressed(active, Some(state(bytes, true)));
            assert_eq!(suppressed, Some(true), "{bytes:?} active {active}");
            assert!(codes.is_empty(), "{codes:?}");
        }
    }
}

#[test]
fn the_no_result_flag_without_a_suppression_byte_is_an_active_feature_with_a_loss() {
    for active in [false, true] {
        let (suppressed, codes) = suppressed(active, Some(state([false; 3], true)));
        assert_eq!(suppressed, Some(false), "active {active}");
        assert_eq!(
            codes,
            [F3dLossCode::FeatureNoResultActiveAssumed.kind()],
            "active {active}"
        );
    }
}

#[test]
fn the_unevaluated_state_byte_alone_is_an_active_feature_with_a_loss() {
    let (suppressed, codes) = suppressed(false, Some(state([false, false, true], true)));
    assert_eq!(suppressed, Some(false));
    assert_eq!(codes, [F3dLossCode::FeatureUnevaluatedStateAssumed.kind()]);
}

#[test]
fn a_current_tail_is_an_active_feature_whatever_its_history_states() {
    for active in [false, true] {
        let (suppressed, codes) = suppressed(active, Some(state([false; 3], false)));
        assert_eq!(suppressed, Some(false), "active {active}");
        assert!(codes.is_empty(), "{codes:?}");
    }
}

#[test]
fn a_scope_without_tail_flags_is_suppressed_by_its_null_history_states() {
    assert_eq!(suppressed(false, None).0, Some(true));
    assert_eq!(suppressed(true, None).0, Some(false));
}
