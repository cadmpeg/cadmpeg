// SPDX-License-Identifier: Apache-2.0
//! Storage and work for rejected optional history outputs.

use crate::history::test_support::{base_feature, one_delta_state};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn rejected_history_work(bytes: &[u8], stream: &str) -> u64 {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    assert!(crate::history::decode(
        &ctx,
        bytes,
        stream,
        cadmpeg_asm::kernel_header::RefWidth::Four,
        &policy.limits
    )
    .unwrap()
    .is_none());
    let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "measure rejected history work")
        .unwrap_err()
    else {
        panic!("expected work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

#[test]
fn empty_history_skips_source_identity_encoding() {
    assert_eq!(
        rejected_history_work(&[], "x"),
        rejected_history_work(&[], &"x".repeat(4096))
    );
}

#[test]
fn rejected_delta_marker_skips_source_identity_encoding() {
    let mut bytes = one_delta_state();
    let marker = bytes.len() - 2;
    bytes[marker] = 0xff;
    assert_eq!(
        rejected_history_work(&bytes, "x"),
        rejected_history_work(&bytes, &"x".repeat(4096))
    );
}

#[test]
fn malformed_history_tail_releases_accepted_prefix_storage() {
    let bytes = [one_delta_state(), crate::history::DELTA.to_vec()].concat();
    assert!(rejected_history_work(&bytes, "history") > 0);
}

#[test]
fn complete_history_refuses_output_storage_commit() {
    let bytes = one_delta_state();
    let operation = "retain F3D ASM history output";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::RetainedBytes,
        operation,
        0,
        |ctx| {
            crate::history::decode(
                ctx,
                &bytes,
                "history",
                cadmpeg_asm::kernel_header::RefWidth::Four,
                &ctx.policy().limits,
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation)
    );
}

fn rejected_pattern() -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        patterns::{PatternKind, PatternSeed, PatternTransform},
        BodySelection, DistinctMembers, FeatureDefinition, FeatureDirection3, FeatureEvaluation,
        FeatureOperation, FinitePoint3,
    };
    let mut feature = base_feature();
    let ctx = cadmpeg_test_support::service_decode_context();
    let seed = BodySelection::historical(
        crate::ids::history_input_state_id_charged(&ctx, &feature.id, 1).unwrap(),
        vec![crate::ids::history_input_body_id_charged(&ctx, &feature.id, 1, 7).unwrap()],
        "native".into(),
        &ctx,
    )
    .unwrap()
    .unwrap();
    let pattern = PatternKind::new(PatternTransform::Circular {
        axis_origin: FinitePoint3::new(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)).unwrap(),
        axis_dir: FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        angle: cadmpeg_ir::scalar::PositiveAngle::new(1.0).unwrap(),
        count: 2,
    })
    .unwrap();
    feature.evaluation = FeatureEvaluation::new(
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Bodies(seed)],
            pattern,
        }),
        DistinctMembers::try_from(
            vec![cadmpeg_ir::ids::BodyId::mint("test:model:body#bad").unwrap()],
            &ctx,
        )
        .unwrap(),
    );
    feature.native_ref = None;
    feature
}

fn bind_rejected_patterns(
    ctx: &DecodeContext<'_>,
    count: usize,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut features = vec![rejected_pattern(); count];
    let original = features.clone();
    let inputs = crate::history::FeatureBodySelectionInputs {
        scopes: &[],
        groups: &[],
        body_recipe_operands: &[],
        construction_recipes: &[],
        persistent_design_links: &[],
        histories: &[],
        bodies: &[],
        regions: &[],
        shells: &[],
    };
    crate::history::bind_feature_body_selections(ctx, &mut features, &inputs)?;
    assert_eq!(features, original);
    Ok(())
}

#[test]
fn rejected_pattern_candidates_release_each_temporary_set() {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        "index F3D pattern body slots",
        0,
        |ctx| bind_rejected_patterns(ctx, 1),
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected pattern set refusal");
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = limit.used.checked_add(limit.additional).unwrap();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_rejected_patterns(&ctx, 32).unwrap();
}

#[test]
fn rejected_combine_fallback_rows_release_identity_prefix() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let feature = base_feature();
    let rows = crate::history::combine_historical_rows(
        &ctx,
        &feature.id,
        1,
        vec![2, 3],
        vec!["native".into(), String::new()].into_iter().map(Ok),
    )
    .unwrap();
    assert!(rows.is_none());
}

#[test]
fn unique_index_preserves_projection_error_and_stops() {
    let values = [1_u32, 2, 3];
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = crate::history::UniqueIndex::new(
        &values,
        |_, value| match value {
            1 => Ok(Some(*value)),
            2 => Err(cadmpeg_core::CodecError::malformed("projection failed")),
            _ => panic!("projection continued after failure"),
        },
        "project F3D test index",
    );
    let error = index.get(&ctx, &1).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
        if message == "projection failed"));
}

#[test]
fn unique_index_tombstones_repeated_keys_and_keeps_unique_keys() {
    let values = [1_u32, 1, 1, 2];
    let ctx = cadmpeg_test_support::service_decode_context();
    let index = crate::history::UniqueIndex::new(
        &values,
        |_, value| Ok(Some(*value)),
        "index F3D test values",
    );
    assert!(index.get(&ctx, &1).unwrap().is_none());
    assert_eq!(index.get(&ctx, &2).unwrap(), Some(&2));
    assert!(index.get(&ctx, &3).unwrap().is_none());
}
