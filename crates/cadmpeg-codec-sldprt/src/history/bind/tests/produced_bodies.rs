// SPDX-License-Identifier: Apache-2.0
//! Produced-body membership lifetime and original allocation refusals.
#![cfg(target_pointer_width = "64")]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::Feature;

use super::super::{derive_feature_outputs, FeatureOutputSources};
use super::{face_owner_fixture, ordering_feature};
use crate::records::FeatureHistory;

fn history() -> FeatureHistory {
    FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: std::collections::BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![crate::history::tests::feature("native", Some("700"), 0)],
    }
}

fn projected() -> Feature {
    let mut feature = ordering_feature();
    feature.native_ref = Some("native".into());
    feature
}

#[test]
fn produced_body_membership_retires_before_output_validation() {
    // One-entry tables: owners131, seen99, produced163 plus one exact8-byte
    // group slot, and record sources131. Seen storage dies before record sources.
    // Output validation needs99 while the remaining live tables hold433.
    const MATERIALIZED: u64 = 131 + 163 + 8 + 131 + 99;
    let fixture = face_owner_fixture();
    let model = &fixture.ir().model;
    let histories = [history()];
    let face = model.faces[0].id.as_str();
    let producers = [(face, 700); 64];
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = MATERIALIZED;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let mut features = [projected()];
        derive_feature_outputs(
            &ctx,
            &mut features,
            &histories,
            FeatureOutputSources {
                face_producers: &producers,
                body_modifiers: &[],
            },
            &model.faces,
            &model.shells,
            &model.regions,
        )
        .unwrap();
        assert_eq!(
            features[0].evaluation.outputs(),
            std::slice::from_ref(&model.bodies[0].id)
        );
        // Returned owned identities remain alive while all local tables release.
        drop(
            ctx.reserve_scoped(MATERIALIZED, "reuse produced body workspace")
                .unwrap(),
        );
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn produced_body_membership_preserves_actual_refusals() {
    let fixture = face_owner_fixture();
    let model = &fixture.ir().model;
    let histories = [history()];
    let producers = [(model.faces[0].id.as_str(), 700)];
    for (dimension, operation) in [
        (ResourceDimension::MaterializedBytes, "index SLDPRT face owner shells"),
        (ResourceDimension::CollectionItems, "collect SLDPRT produced bodies"),
        (ResourceDimension::MaterializedBytes, "validate distinct decoded members"),
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                _ => panic!("produced body resource dimension"),
            }
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut features = [projected()];
            let result = derive_feature_outputs(
                &ctx, &mut features, &histories,
                FeatureOutputSources { face_producers: &producers, body_modifiers: &[] },
                &model.faces, &model.shells, &model.regions,
            );
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert!(features[0].evaluation.outputs().is_empty());
                assert_eq!(ctx.resource_refusal(), Some(*limit));
                assert!(matches!(ctx.charge_work(0, "after produced body refusal"), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == *limit));
            }
            result
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("required produced body backing refuses before publication"); };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(limit.operation, operation);
    }
}
