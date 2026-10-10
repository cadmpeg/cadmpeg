// SPDX-License-Identifier: Apache-2.0
//! Remaining unique queries preserve source and predicate boundaries.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn unique_query_remaining_stream_none_and_empty_stay_distinct() {
    crate::test_support::with_decode_context(|ctx| {
        assert!(super::super::super::feature_stream_matches(
            ctx,
            "undelimited",
            None,
            "test F3D stream absence"
        )
        .unwrap());
        assert!(!super::super::super::feature_stream_matches(
            ctx,
            "undelimited",
            Some(""),
            "test F3D stream absence"
        )
        .unwrap());
        assert!(super::super::super::feature_stream_matches(
            ctx,
            ":scope#1",
            Some(""),
            "test F3D empty stream"
        )
        .unwrap());
        assert!(!super::super::super::feature_stream_matches(
            ctx,
            ":scope#1",
            None,
            "test F3D empty stream"
        )
        .unwrap());
    });
}

#[test]
fn unique_query_remaining_stream_mismatch_leaves_long_stream_suffix_unvisited() {
    let source = format!("f3d:{}:scope#1", "x".repeat(4096));
    let expected = "y".repeat("f3d:".len() + 4096);
    // Reverse suffix scan reaches the colon after scope#1, then the first
    // mismatching stream byte. The equal-length suffix is never compared.
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::try_from("scope#1".len() + 1 + 1).unwrap();
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(!super::super::super::feature_stream_matches(
            ctx,
            &source,
            Some(&expected),
            "test F3D reached stream mismatch"
        )
        .unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    });
    policy.limits.max_work_units -= 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = super::super::super::feature_stream_matches(
            ctx,
            &source,
            Some(&expected),
            "test F3D reached stream mismatch",
        ) else {
            panic!("actual first mismatch byte must refuse");
        };
        assert_eq!(original.operation, "test F3D reached stream mismatch");
        assert_eq!(
            (original.used, original.additional),
            (policy.limits.max_work_units, 1)
        );
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::super::feature_stream_matches(ctx, "", None, "later F3D stream query"), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

fn assert_target_refusal(operation: &'static str) {
    let scope = super::scope();
    let group = super::group(
        77,
        0,
        100,
        vec![101],
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X21,
    );
    let selections = [super::selection(101, 0, 600)];
    let planes = [super::plane(601)];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, None);
        let Err(CodecError::ResourceLimit(original)) =
            super::super::super::selected_work_planes(ctx, &scope, &group, &selections, &planes)
        else {
            panic!("actual target plane query must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, operation);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(
            matches!(super::super::super::selected_work_planes(ctx, &scope, &group, &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn unique_query_remaining_target_source_preserves_original_refusal() {
    assert_target_refusal("visit F3D unique feature source");
}

#[test]
fn unique_query_remaining_target_stream_preserves_original_refusal() {
    assert_target_refusal("compare F3D unique feature stream");
}

#[test]
fn unique_query_remaining_empty_bind_preserves_preexisting_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) =
            ctx.charge_work(1, "original before Revolve binding")
        else {
            panic!("original charge must refuse");
        };
        assert!(
            matches!(super::super::super::bind_revolve_face_axes(ctx, &mut [], &[], &[], &[], &[], &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

fn unresolved_feature(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> cadmpeg_ir::features::Feature {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureEvaluation, FeatureOperation, PartialRevolveConstruction,
        PlanarProfileRef, RevolveConstruction,
    };
    cadmpeg_ir::features::Feature {
        id: crate::ids::neutral_feature_id(scope),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Revolve {
                construction: RevolveConstruction::Unresolved(PartialRevolveConstruction::Axis {
                    profile: PlanarProfileRef::Native(
                        "f3d:Design/BulkStream.dat:profile#10".into(),
                    ),
                    extent: None,
                    solid: None,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                }),
                op: cadmpeg_ir::features::BooleanOp::NewBody,
            },
        )),
        native_ref: Some(scope.id.clone()),
    }
}

#[test]
fn unique_query_remaining_bind_feature_visit_refuses_before_mutation() {
    let scope = super::scope();
    let mut feature = unresolved_feature(&scope);
    let original_definition = feature.evaluation.definition().clone();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = super::super::super::bind_revolve_face_axes(
            ctx,
            std::slice::from_mut(&mut feature),
            &[scope],
            &[],
            &[],
            &[],
            &[],
            &[],
        ) else {
            panic!("actual feature visit must refuse");
        };
        assert_eq!(original.operation, "visit F3D Revolve axis feature");
        assert_eq!((original.used, original.additional), (0, 1));
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert_eq!(feature.evaluation.definition(), &original_definition);
        assert!(
            matches!(super::super::super::bind_revolve_face_axes(ctx, &mut [], &[], &[], &[], &[], &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original)
        );
    });
}

#[test]
fn unique_query_remaining_bind_temporary_face_identity_refunds_actual_backing() {
    let scope = super::scope();
    let group = super::group(
        77,
        0,
        100,
        vec![101],
        crate::records::topology::extrude_selection::DesignOperandRole::ROLE_0X21,
    );
    let mut selection = super::selection(101, 0, 600);
    selection.historical_face_candidates.push(
        crate::records::topology::entity_selection::DesignEntitySelectionFaceCandidate {
            history_id: "history".into(),
            historical: crate::records::topology::fillet::HistoricalBinding {
                kind: crate::records::topology::body_recipe::AsmHistoricalEntityKind::Face,
                entity_ref: 600,
                state_ids: vec![1],
            },
            face_slot: 600,
        },
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        for _ in 0..7 {
            let mut feature = unresolved_feature(&scope);
            super::super::super::bind_revolve_face_axes(
                ctx,
                std::slice::from_mut(&mut feature),
                std::slice::from_ref(&scope),
                std::slice::from_ref(&group),
                std::slice::from_ref(&selection),
                &[],
                &[],
                &[],
            )
            .unwrap();
            assert!(
                matches!(feature.evaluation.definition(), cadmpeg_ir::features::FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Revolve { construction, .. }) if construction.axis().is_none())
            );
            drop(
                ctx.reserve_scoped(4096, "reuse actual Revolve temporary identity backing")
                    .unwrap(),
            );
        }
        assert_eq!(ctx.resource_refusal(), None);
    });
}
