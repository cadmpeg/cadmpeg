// SPDX-License-Identifier: Apache-2.0
//! Base results that the final model no longer holds (spec section 13.4).
use std::rc::Rc;

use super::derived::{decoded, derived_at_version};
use super::{decode, fixtures};
use crate::feature::history_states::{
    Binding, FeatureHistoryState, StateRef, UnboundReason, NATIVE_BASE_KIND, SPLIT_TAG,
};
use crate::feature::history_views::{ResultStates, StateModel};
use crate::loss::InventorLossCode;
use crate::rse::RseInventory;
use crate::test_support::history_carrier::introducing_carrier_bytes;
use crate::test_support::test_fixtures::{
    acis_sphere_kernel_stream_with_body_key, base_envelope_fixture,
};
use crate::InventorCodec;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossKind;
use cadmpeg_ir::report::Severity;

/// Source index of both Base features of the fixture.
const SOURCE_INDEX: u32 = 100;
/// Entity key of the result body of the fixture.
const RESULT_KEY: i64 = 7;

/// States of a fixed answer.
struct Fixed {
    introduces: bool,
    outcome: fn(&CadIr) -> StateModel,
    model: CadIr,
}

impl ResultStates for Fixed {
    fn after(
        &self,
        _: &DecodeContext<'_>,
        source_index: u32,
    ) -> Result<Rc<StateModel>, CodecError> {
        assert_eq!(source_index, SOURCE_INDEX);
        Ok(Rc::new((self.outcome)(&self.model)))
    }

    fn introduces(&self, source_index: u32, key: i64) -> bool {
        self.introduces && source_index == SOURCE_INDEX && key == RESULT_KEY
    }
}

fn holding(model: &CadIr) -> StateModel {
    StateModel::Model(Box::new(model.clone()))
}

/// Projects the two Base features of the fixture with a final model that
/// holds no body key, and returns the transferred count and the losses.
fn project_without_final_body(states: &Fixed) -> (usize, Vec<LossKind>) {
    let (design, base) = fixtures();
    let bytes = base_envelope_fixture(
        &acis_sphere_kernel_stream_with_body_key(21_800, 7),
        &design,
        &base,
    );
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("context");
    let snapshot = CompoundSnapshot::new(&ctx, root).expect("compound");
    let document = RseInventory::build(&ctx, &snapshot).expect("RSe inventory");
    let features = crate::feature::inventory(&ctx, &document).expect("feature inventory");
    let design = crate::design::inventory(&ctx, &document).expect("design inventory");
    let mut ir = decode(&design_records(), &base_records());
    ir.model.feature_result_topologies.clear();
    for feature in &mut ir.model.features {
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Base".into()),
                parameters: std::collections::BTreeMap::new(),
            }));
    }
    let keys: Vec<BodyNativeKey> = Vec::new();
    let projection = crate::feature::base::project(
        &ctx,
        &crate::feature::base::Inputs {
            document: &document,
            inventory: &features,
            design: &design,
            transforms: &[],
            keys: &keys,
            states: Some(states),
        },
        &mut ir,
    )
    .expect("Base projection");
    let codes = projection
        .losses
        .iter()
        .map(|note| note.code.clone())
        .collect();
    if projection.transferred == 2 {
        assert_eq!(ir.model.feature_result_topologies.len(), 2);
        for result in &ir.model.feature_result_topologies {
            assert_eq!(result.bodies().len(), 1);
            assert!(result.bodies()[0].as_str().ends_with("-3"));
        }
        for feature in &ir.model.features {
            assert!(matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
            ));
            assert!(feature.source_properties.contains_key("result_body_link"));
        }
    } else {
        assert!(ir.model.feature_result_topologies.is_empty());
    }
    (projection.transferred, codes)
}

fn design_records() -> super::SyntheticRecords {
    fixtures().0
}

fn base_records() -> crate::test_support::test_fixtures::BaseEnvelopeRecords {
    fixtures().1
}

/// The model of the state that introduces the result body: the decode of the
/// fixture holds one body with the result key.
fn state_model() -> CadIr {
    decode(&design_records(), &base_records())
}

#[test]
fn a_result_body_that_its_state_introduces_types_the_base() {
    let states = Fixed {
        introduces: true,
        outcome: holding,
        model: state_model(),
    };
    assert_eq!(project_without_final_body(&states), (2, Vec::new()));
}

#[test]
fn a_result_key_that_no_state_introduces_stays_native_without_a_loss() {
    let states = Fixed {
        introduces: false,
        outcome: holding,
        model: state_model(),
    };
    assert_eq!(project_without_final_body(&states), (0, Vec::new()));
}

#[test]
fn a_state_without_a_body_of_the_result_key_stays_native_with_its_loss() {
    let mut model = state_model();
    model
        .native
        .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
        .arenas_mut()
        .remove("body_native_keys");
    let states = Fixed {
        introduces: true,
        outcome: holding,
        model,
    };
    let (transferred, codes) = project_without_final_body(&states);
    assert_eq!(transferred, 0);
    assert_eq!(codes, vec![state_loss(); 2]);
}

#[test]
fn a_state_that_does_not_decode_or_passed_its_cap_stays_native_with_its_loss() {
    for outcome in [
        |_: &CadIr| StateModel::Failed,
        |_: &CadIr| StateModel::Bounded,
    ] {
        let states = Fixed {
            introduces: true,
            outcome,
            model: CadIr::empty(),
        };
        let (transferred, codes) = project_without_final_body(&states);
        assert_eq!(transferred, 0);
        assert_eq!(codes, vec![state_loss(); 2]);
    }
    let states = Fixed {
        introduces: true,
        outcome: |_| StateModel::Unavailable,
        model: CadIr::empty(),
    };
    let (transferred, codes) = project_without_final_body(&states);
    assert_eq!(transferred, 0);
    assert_eq!(codes, vec![state_loss(); 2]);
}

fn state_loss() -> LossKind {
    InventorLossCode::FeatureBaseResultStateNative
        .note("state")
        .code
}

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
/// the history states of `ir` changed by `mutate`.
pub(super) fn base_findings_of(
    mut ir: CadIr,
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
    let mut states = bound(&ir, RESULT_KEY);
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
    base_findings_of(state_model(), mutate)
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
fn validation_refuses_a_derived_result_without_a_final_body() {
    let (design, base) = derived_at_version(false, 19);
    let ir = decoded(&design, &base).into_parts().0;
    assert!(!ir.geometry.bodies.is_empty());
    assert!(!base_findings_of(ir, |_| {}).is_empty());
}

/// Decodes the Base fixture over a closed synthetic chain whose current
/// model holds no body: the state that first lists the source index holds
/// the sphere solid with `key`, and the state before it holds none.
fn decode_over_chain(key: i64) -> CadIr {
    let (design, base) = fixtures();
    let carrier = introducing_carrier_bytes(RefWidth::Eight, 4, i64::from(SOURCE_INDEX), key);
    let bytes = base_envelope_fixture(&carrier, &design, &base);
    InventorCodec
        .decode(&mut std::io::Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic Base decode")
        .into_parts()
        .0
}

#[test]
fn a_closed_chain_types_a_base_whose_final_model_holds_no_body() {
    let ir = decode_over_chain(RESULT_KEY);
    assert!(ir.geometry.bodies.is_empty());
    assert_eq!(ir.model.features.len(), 2);
    assert_eq!(ir.model.feature_result_topologies.len(), 2);
    for feature in &ir.model.features {
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        assert!(feature.source_properties.contains_key("result_body_link"));
    }
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let findings = InventorCodec
        .validate_native(&ctx, &ir, None)
        .expect("native validation");
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity >= Severity::Error),
        "{findings:?}"
    );
}

#[test]
fn a_closed_chain_that_introduces_another_key_leaves_the_base_native() {
    let ir = decode_over_chain(RESULT_KEY + 1);
    assert!(ir.model.feature_result_topologies.is_empty());
    assert!(ir.model.features.iter().all(|feature| matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    )));
}
