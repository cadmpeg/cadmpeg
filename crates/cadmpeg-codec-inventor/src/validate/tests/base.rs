// SPDX-License-Identifier: Apache-2.0
//! Validation of stored-body Base results that the final model no longer
//! holds (spec section 13.4).
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::Severity;

use crate::feature::history_states::{
    Binding, FeatureHistoryState, StateRef, UnboundReason, BASE_TAG, NATIVE_BASE_KIND, SPLIT_TAG,
};
use crate::test_support::base_envelope::{
    decoded, derived_at_version, state_model, DERIVED_RESULT_KEY, RESULT_KEY,
};
use crate::InventorCodec;
fn bound(ir: &CadIr, key: i64) -> Vec<FeatureHistoryState> {
    let namespace = ir.native.namespace("inventor").expect("native namespace");
    let features: Vec<serde_json::Value> = namespace.arena_as("pm_dc_features").expect("features");
    features
        .iter()
        .map(|feature| FeatureHistoryState {
            id: format!("{}-history", feature["id"].as_str().expect("id")),
            feature: feature["id"].as_str().expect("id").to_owned(),
            source_index: u32::try_from(
                feature["header"]["source_index"]
                    .as_u64()
                    .expect("source index"),
            )
            .expect("source index fits"),
            kind: "base".into(),
            state_after: Some(StateRef {
                ordinal: 1,
                state_id: 5,
            }),
            state_before: Some(0),
            body_key: Some(key),
            history_body: None,
            binding: Binding::Bound,
        })
        .collect()
}

/// Base findings of `ir`, whose final model holds no result body key, with
/// the history states of `ir`, bound to `key`, changed by `mutate`.
pub(super) fn base_findings_of(
    mut ir: CadIr,
    key: i64,
    mutate: impl FnOnce(&mut Vec<FeatureHistoryState>),
) -> Vec<String> {
    // The final model joins no body to the result wrapper, so the feature
    // lists no output body.
    for feature in &mut ir.model.features {
        feature
            .evaluation
            .set_outputs(std::iter::empty::<cadmpeg_ir::ids::BodyId>().collect());
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut states = bound(&ir, key);
    mutate(&mut states);
    let namespace = ir.native.namespace_mut(cadmpeg_ir::codec_id!("inventor"));
    namespace
        .set_arena(&ctx, "body_native_keys", &Vec::<BodyNativeKey>::new())
        .expect("keys");
    namespace
        .set_arena(&ctx, "feature_history_states", &states)
        .expect("states");
    InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("native validation")
        .into_iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .map(|finding| finding.message)
        // The v11 design of the decoded model still lists the output body
        // that the setup removed; the Base check is the one under test.
        .filter(|message| message.contains("Base result"))
        .collect()
}

fn base_findings(mutate: impl FnOnce(&mut Vec<FeatureHistoryState>)) -> Vec<String> {
    base_findings_of(state_model(), RESULT_KEY, mutate)
}

#[test]
fn validation_accepts_a_result_with_a_bound_state_and_no_final_body() {
    assert_eq!(base_findings(|_| {}), Vec::<String>::new());
}

#[test]
fn validation_refuses_a_result_without_a_matching_bound_state() {
    let refused = |findings: Vec<String>| !findings.is_empty();
    assert!(refused(base_findings(Vec::clear)));
    assert!(refused(
        base_findings(|states| states[0].body_key = Some(8))
    ));
    assert!(refused(
        base_findings(|states| states[0].state_after = None)
    ));
    assert!(refused(base_findings(|states| {
        states[0].binding = Binding::Unbound {
            reason: UnboundReason::BodyKeyMismatch,
        }
    })));
    assert!(refused(
        base_findings(|states| states[0].source_index = 101)
    ));
}

#[test]
fn validation_trusts_bound_only_for_a_base_state() {
    // Bound means introduced for a Base state; for a Split state it means
    // consumed, so a Split record with a matching key is no evidence.
    assert!(!base_findings(|states| states[0].kind = SPLIT_TAG.into()).is_empty());
    assert!(base_findings(|states| states[0].kind = NATIVE_BASE_KIND.into()).is_empty());
}

#[test]
fn validation_accepts_a_derived_result_only_on_a_body_creating_state() {
    // A derived result with no final body needs a bound state of a
    // body-creating kind; a Split state means consumed.
    let (design, base) = derived_at_version(false, 19);
    let ir = decoded(&design, &base).into_parts().0;
    assert!(!ir.geometry.bodies.is_empty());
    // The result link of the derived fixture has entity key DERIVED_RESULT_KEY.
    let key = DERIVED_RESULT_KEY;
    assert_eq!(
        base_findings_of(ir.clone(), key, |_| {}),
        Vec::<String>::new()
    );
    assert!(!base_findings_of(ir.clone(), key, |states| states[0].body_key = Some(8)).is_empty());
    // A Base state does not bind a derived feature: the kinds differ.
    assert!(!base_findings_of(ir, key, |states| states[0].kind = BASE_TAG.into()).is_empty());
}
