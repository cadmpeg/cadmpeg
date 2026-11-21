// SPDX-License-Identifier: Apache-2.0
//! A scope's suppression comes from the suppressed flag of its tail, else
//! from its null history states.

use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopeTailFlags,
};

fn suppressed(active: bool, flag: Option<bool>) -> Option<bool> {
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
    scope.tail_flags = flag.map(|suppressed| DesignScopeTailFlags {
        suppressed,
        rolled_back: false,
        suppressed_offset: scope.feature_ordinal_offset(),
        envelope_offset: None,
    });
    let id = scope.id.clone();
    let crate::design::feature_project::ProjectedParameterDesign { features, .. } =
        crate::test_support::with_decode_context(|ctx| {
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
    features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(id.as_str()))
        .expect("the scope's feature")
        .suppressed
}

#[test]
fn the_tail_suppressed_flag_decides_suppression_over_the_history_states() {
    assert_eq!(suppressed(true, Some(true)), Some(true));
    assert_eq!(suppressed(false, Some(false)), Some(false));
}

#[test]
fn a_scope_without_tail_flags_is_suppressed_by_its_null_history_states() {
    assert_eq!(suppressed(false, None), Some(true));
    assert_eq!(suppressed(true, None), Some(false));
}
