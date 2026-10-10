// SPDX-License-Identifier: Apache-2.0
//! Discovery queries preserve the context that owns their cached storage.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::super::AnnotationDiscoveryIndex;

fn original_context_refusal(placement: bool) {
    let exchange = super::exchange("#1=ITEM();");
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &setup_arena, &DecodePolicy::service())
        .expect("geometry fixture context");
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup)
        .expect("geometry fixture");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut index = AnnotationDiscoveryIndex::new(&ctx).expect("original discovery context");
    let mut claim_storage = ctx.reserve_scoped(0, "test discovery claims").expect("scope");
    let reports = std::cell::RefCell::new(
        ctx.reserve_scoped(0, "test discovery reports").expect("scope"),
    );
    let mut claims = BTreeSet::new();
    let mut losses = Vec::new();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original discovery refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    let result = if placement {
        index.placement(exchange.records().get(&1).expect("record"), &exchange, &geometry.value).map(|_| ())
    } else {
        index.text(
            1,
            &exchange,
            &geometry.value,
            (&mut claims, &mut claim_storage),
            (&mut losses, &reports),
        ).map(|_| ())
    };
    assert!(matches!(result, Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(index.aliases.is_empty());
    assert!(index.summaries.is_empty());
    assert!(index.placements.is_empty());
    assert!(index.texts.is_empty());
    assert!(claims.is_empty());
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(losses);
    drop(claims);
    drop(reports);
    drop(claim_storage);
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn annotation_discovery_text_preserves_original_session_refusal() {
    original_context_refusal(false);
}

#[test]
fn annotation_discovery_placements_preserves_original_session_refusal() {
    original_context_refusal(true);
}
