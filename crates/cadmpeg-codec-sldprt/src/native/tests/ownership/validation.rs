// SPDX-License-Identifier: Apache-2.0
//! Completed native validation tables release before attachment work.
#![cfg(target_pointer_width = "64")]

use crate::native::SldprtNative;
use crate::records::FeatureInputLane;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture() -> (SldprtNative, cadmpeg_ir::NativeNamespace) {
    let native = SldprtNative {
        feature_input_lanes: vec![FeatureInputLane {
            id: "sldprt:test:lane#root".into(),
            configuration: None,
            native_payload: Vec::new(),
            classes: Vec::new(),
            names: Vec::new(),
            scalars: Vec::new(),
            relation_bindings: Vec::new(),
            relation_instances: Vec::new(),
            body_selections: Vec::new(),
            edge_selections: Vec::new(),
            surface_selections: Vec::new(),
            generated_surface_identities: Vec::new(),
            references: Vec::new(),
            sketch_entities: Vec::new(),
        }],
        ..SldprtNative::default()
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native.store(&ctx, &mut namespace).unwrap();
    ctx.finish_session().unwrap();
    (native, namespace)
}

#[test]
fn native_load_releases_workspace_with_owned_lanes_alive() {
    let (expected, namespace) = fixture();
    // The effective empty-input workspace is MATERIALIZED_BASE = 16 MiB.
    // This is a full release check, not a whole-load peak estimate.
    const EMPTY_INPUT_WORKSPACE: u64 = 16 * 1024 * 1024;
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = EMPTY_INPUT_WORKSPACE;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        let loaded = SldprtNative::load_charged(&ctx, &namespace).unwrap();
        assert_eq!(loaded, expected);
        drop(
            ctx.reserve_scoped(EMPTY_INPUT_WORKSPACE, "reuse native validation workspace")
                .unwrap(),
        );
        assert_eq!(loaded.feature_input_lanes[0].id, "sldprt:test:lane#root");
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn native_load_attachment_refusal_excludes_completed_validation_tables() {
    let (_, namespace) = fixture();
    // Validation reaches lane-set99 + payload-map163 =262, then releases.
    // The borrowed lane-id vector holds64. Each attachment positions map
    // holds195 + one exact8-byte group; its outer vector holds one24-byte
    // slot. The first positions group refuses its exact8-byte slot while
    // the borrowed lane-id vector and map hold64 +195 =259.
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 262;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error: CodecError = SldprtNative::load_charged(&ctx, &namespace)
        .unwrap_err()
        .into();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("required native attachment backing refuses");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "attach SLDPRT native records to owners");
    assert_eq!(limit.limit, 262);
    assert_eq!(limit.used, 259);
    assert_eq!(limit.additional, 8);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(
        matches!(ctx.charge_work(0, "after native attachment refusal"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
    );
}
