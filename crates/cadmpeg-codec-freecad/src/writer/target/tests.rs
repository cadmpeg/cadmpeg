// SPDX-License-Identifier: Apache-2.0
//! Retained-layout eligibility borrows the source-free encode caller throughout.

use super::{admit_unedited, projection};
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceDimension, ResourceFailure,
};
use cadmpeg_core::encode::EncodeSession;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions, SourceFidelity};
use std::io::Cursor;

#[test]
fn inherited_caller_refusal_precedes_retained_layout_and_projection_shortcuts() {
    let ir = CadIr::empty();
    let fidelity = SourceFidelity::default();
    let before_ir = serde_json::to_value(&ir).unwrap();
    let before_fidelity = serde_json::to_value(&fidelity).unwrap();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let mut working = Vec::<u64>::new();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut working, usize::MAX, "retained layout prior refusal")
        .unwrap_err()
    else {
        panic!("working capacity overflow must latch a resource refusal")
    };
    assert!(matches!(
        admit_unedited(&ctx, &ir, &fidelity),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(matches!(
        projection::agrees(&ctx, &ir),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(working.is_empty());
    assert_eq!(serde_json::to_value(&ir).unwrap(), before_ir);
    assert_eq!(serde_json::to_value(&fidelity).unwrap(), before_fidelity);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}

#[test]
fn represented_layout_admission_uses_the_actual_source_free_encode_context() {
    let decoded = crate::FcstdCodec
        .decode(
            &mut Cursor::new(crate::test_support::test_archive::CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .unwrap();
    assert!(!decoded.ir().model.occurrences.is_empty());
    // The eligibility helper receives only its fingerprint fact, with no retained
    // records, parts, source layouts or archive bytes available in this fidelity.
    let mut fidelity = SourceFidelity::default();
    fidelity.decode_digest = decoded.source_fidelity().decode_digest.clone();
    let before_ir = serde_json::to_value(decoded.ir()).unwrap();
    let before_fidelity = serde_json::to_value(&fidelity).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // #217 §5.0: this is the actual caller's retained-output ceiling. The
    // fixture's native Part file/side-entry properties can independently
    // decline projection; successful projection was never its guarantee.
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let mut output = session.buffer();
    assert!(ctx.is_encoding());
    assert_eq!(ctx.resource_refusal(), None);
    // Real output growth must be admitted before allocation. No hash charge
    // or manually constructed refusal is used to manufacture this condition.
    let refusal = output
        .append(b"x", "retained layout actual caller output")
        .unwrap_err();
    assert_eq!(refusal.scope, LimitScope::Session);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (0, 0, 1));
    assert!(output.is_empty());
    assert_eq!(output.capacity(), 0);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(
        projection::agrees(&ctx, decoded.ir()),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(matches!(
        admit_unedited(&ctx, decoded.ir(), &fidelity),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert_eq!(serde_json::to_value(decoded.ir()).unwrap(), before_ir);
    assert_eq!(serde_json::to_value(&fidelity).unwrap(), before_fidelity);
    assert!(ctx.is_encoding());
    assert!(output.is_empty());
    assert_eq!(output.capacity(), 0);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}
