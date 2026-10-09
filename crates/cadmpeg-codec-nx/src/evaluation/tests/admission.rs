// SPDX-License-Identifier: Apache-2.0

use super::{attach_complete_active_configuration, complete_block_ir};
use crate::evaluation::{active_configuration_is_admitted, evaluate_saved_body_census};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use std::collections::BTreeSet;

#[test]
fn census_replay_and_configuration_work_refuse_at_the_named_route() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    for operation in [
        "NX saved body identity traversal",
        "NX body census replay traversal",
        "NX replay feature identity index",
        "NX new body replay identity",
        "NX body census identity comparison",
        "NX active configuration selection",
        "NX active configuration bodies",
        "NX active configuration neutral features",
    ] {
        let error = crate::test_support::resource_refusal_at(
            &[],
            ResourceDimension::WorkUnits,
            operation,
            |ctx| evaluate_saved_body_census(ctx, &ir),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn census_scratch_and_returned_identity_use_separate_storage_routes() {
    let ir = complete_block_ir();
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "NX body census feature order",
        ),
        (
            ResourceDimension::MaterializedBytes,
            "NX replay feature identity index",
        ),
        (
            ResourceDimension::RetainedBytes,
            "NX verified census body identity",
        ),
        (
            ResourceDimension::CollectionItems,
            "NX verified body census",
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(&[], dimension, operation, |ctx| {
            evaluate_saved_body_census(ctx, &ir)
        });
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.operation == operation));
    }
}

#[test]
fn configuration_without_a_body_list_does_not_visit_the_suffix() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    ir.model.configurations[0].bodies = None;
    let mut inactive = ir.model.configurations[0].clone();
    inactive.active = false;
    ir.model.configurations.extend(vec![inactive; 4096]);
    let saved = BTreeSet::from([&ir.model.bodies[0].id]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 1;
        },
        |ctx| {
            assert!(!active_configuration_is_admitted(ctx, &ir, &saved).unwrap());
        },
    );
}

#[test]
fn extra_active_configuration_does_not_visit_the_suffix() {
    let mut ir = complete_block_ir();
    attach_complete_active_configuration(&mut ir);
    let active = ir.model.configurations[0].clone();
    let mut inactive = active.clone();
    inactive.active = false;
    ir.model.configurations.push(active);
    ir.model.configurations.extend(vec![inactive; 4096]);
    let saved = BTreeSet::from([&ir.model.bodies[0].id]);
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_work_units = 2;
        },
        |ctx| {
            assert!(!active_configuration_is_admitted(ctx, &ir, &saved).unwrap());
        },
    );
}

#[test]
fn boundary_family_matches_the_serialized_definition_tag() {
    let ir = complete_block_ir();
    let feature = &ir.model.features[0];
    let wire = serde_json::to_value(feature.evaluation.definition()).unwrap();
    let boundary = crate::test_support::with_decode_context(|ctx| {
        crate::evaluation::feature_boundary(ctx, feature)
    })
    .unwrap();
    assert_eq!(
        boundary.family.as_deref(),
        wire.get("definition").and_then(serde_json::Value::as_str)
    );
}

// The profile caller constructs its own DecodeContext. This bound includes
// the feature-order vector, saved and replay body roots, and one feature root.
fn census_phase_peak(saved: usize, replay: usize) -> u64 {
    fn set_bytes<T>(count: usize) -> usize {
        let nodes = if count == 0 { 0 } else { (count - 1) / 5 + 1 };
        let alignment = std::mem::align_of::<T>().max(std::mem::align_of::<usize>());
        nodes * (11 * std::mem::size_of::<T>()
            + 16 * std::mem::size_of::<usize>() + 2 * alignment)
    }
    cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<&cadmpeg_ir::features::Feature>()
            + set_bytes::<&cadmpeg_ir::ids::BodyId>(saved)
            + set_bytes::<&cadmpeg_ir::ids::BodyId>(replay)
            + set_bytes::<&cadmpeg_ir::features::FeatureId>(1),
    )
}


fn assert_census_refusal_is_sticky(
    ir: &cadmpeg_ir::CadIr,
    policy: &cadmpeg_core::decode::DecodePolicy,
    dimension: ResourceDimension,
    operation: &str,
    need: u64,
) {
    crate::test_support::with_decode_context_over(&[], |actual| *actual = *policy, |ctx| {
        let error = evaluate_saved_body_census(ctx, ir).unwrap_err();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("the original caller must retain the resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used + limit.additional, need);
        assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
        assert!(matches!(ctx.charge_work(0, "census sticky refusal control"),
            Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    });
}

#[test]
fn verified_census_releases_saved_index_before_output_growth() {
    use cadmpeg_ir::features::{BodySelection, DistinctMembers, FeatureDefinition,
        FeatureEvaluation, FeatureOperation};
    let mut ir = complete_block_ir();
    ir.model.bodies = (0..32)
        .map(|index| super::model_body(&format!("test:model:entity#body-{index:02}")))
        .collect();
    let expected = ir.model.bodies.iter().map(|body| body.id.clone()).collect::<Vec<_>>();
    let members = DistinctMembers::try_from(expected.clone(),
        &cadmpeg_test_support::service_decode_context()).unwrap();
    ir.model.features[0].evaluation = FeatureEvaluation::new(
        FeatureDefinition::Operation(FeatureOperation::BaseFeature {
            bodies: BodySelection::Resolved {
                bodies: members.clone(), native: "synthetic:test:census".to_owned(),
            },
        }), members,
    );
    let peak = census_phase_peak(32, 32);
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_materialized_bytes = peak;
    let result = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap();
    let crate::evaluation::BodyCensusEvaluation::Verified { bodies } = result else {
        panic!("the complete initial body selection must verify");
    };
    assert_eq!(bodies.as_slice(), expected);

    policy.limits.max_materialized_bytes = peak - 1;
    let error = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "NX selected body replay identity"));
    assert_census_refusal_is_sticky(&ir, &policy, ResourceDimension::MaterializedBytes,
        "NX selected body replay identity", peak);
}

#[test]
fn mismatched_census_releases_replay_index_before_saved_output_growth() {
    let mut ir = complete_block_ir();
    let expected_rederived = ir.model.features[0].evaluation.outputs().as_slice().to_vec();
    ir.model.bodies = (0..32)
        .map(|index| super::model_body(&format!("test:model:entity#saved-{index:02}")))
        .collect();
    let expected_saved = ir.model.bodies.iter().map(|body| body.id.clone()).collect::<Vec<_>>();
    let peak = census_phase_peak(32, 1);
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_materialized_bytes = peak;
    let result = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap();
    let crate::evaluation::BodyCensusEvaluation::Mismatch { evidence } = result else {
        panic!("different complete saved and replay censuses must remain a mismatch");
    };
    assert_eq!(evidence.rederived(), expected_rederived);
    assert_eq!(evidence.saved(), expected_saved);

    policy.limits.max_materialized_bytes = peak - 1;
    let error = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes
            && limit.operation == "NX new body replay identity"));
    assert_census_refusal_is_sticky(&ir, &policy, ResourceDimension::MaterializedBytes,
        "NX new body replay identity", peak);
}

#[test]
fn unsupported_census_keeps_boundary_identity_after_scratch_release() {
    let mut ir = complete_block_ir();
    ir.model.features[0].suppressed = None;
    ir.model.features[0].name = Some("Synthetic unresolved block".to_owned());
    let source = &ir.model.features[0];
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_materialized_bytes = census_phase_peak(1, 0);
    let result = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap();
    let crate::evaluation::BodyCensusEvaluation::Unsupported { feature, reason } = result else {
        panic!("a new block with unresolved suppression must retain its boundary");
    };
    assert_eq!(reason, crate::evaluation::UnsupportedBodyCensusReason::UnresolvedSuppression);
    assert_eq!(feature.id, source.id);
    assert_eq!(feature.name, source.name);
    assert_eq!(feature.ordinal, source.ordinal);
    assert_eq!(feature.family.as_deref(), Some("block"));

    policy.limits.max_retained_bytes = 0;
    let error = crate::evaluation::saved_body_census_evidence(&ir, &policy).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "NX census boundary feature identity"));
    assert_census_refusal_is_sticky(&ir, &policy, ResourceDimension::RetainedBytes,
        "NX census boundary feature identity", cadmpeg_core::decode::u64_from_index(source.id.as_str().len()));
}
