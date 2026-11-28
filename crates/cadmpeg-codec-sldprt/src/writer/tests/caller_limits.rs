// SPDX-License-Identifier: Apache-2.0
//! Reads of the write path run under the caller's resource limits.

use std::sync::{Arc, OnceLock};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceLimits};
use cadmpeg_core::CodecError;

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::writer::{retained_source_baseline, SemanticWriteCache, SourceInput};

#[test]
fn a_baseline_is_reused_under_equal_limits_and_not_under_tighter_ones() {
    let image = sldprt_with_body(&triangle_body());
    let limits = ResourceLimits::desktop();
    let arena = DecodeArena::new();
    let policy = DecodePolicy { limits };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let input = SourceInput::new(&image);
    let source = input.image(&ctx).unwrap();
    let mut write = SemanticWriteCache::default();
    assert!(
        retained_source_baseline(&mut write.source_baseline, &ctx, Some(source))
            .unwrap()
            .is_some()
    );
    let kept = OnceLock::new();
    write.keep_source(Some(&kept));
    let decoded = kept.get().expect("the write keeps the source it decoded");

    let equal = SemanticWriteCache::of_source(Some(&kept), limits);
    assert!(equal
        .source_baseline
        .is_some_and(|baseline| Arc::ptr_eq(&baseline, decoded)));

    let mut tight = limits;
    tight.max_recursion_depth = 0;
    assert!(SemanticWriteCache::of_source(Some(&kept), tight)
        .source_baseline
        .is_none());
    let policy = DecodePolicy { limits: tight };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    assert!(matches!(
        SourceInput::new(&image).baseline(&ctx).map(drop),
        Err(CodecError::ResourceLimit(_))
    ));
}
