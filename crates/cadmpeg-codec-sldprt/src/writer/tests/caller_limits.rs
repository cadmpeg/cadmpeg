// SPDX-License-Identifier: Apache-2.0
//! Reads of the write path run under the caller's resource limits.

use std::sync::{Arc, OnceLock};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceLimits};
use cadmpeg_core::CodecError;

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::writer::{retained_source_baseline, SemanticWriteCache};

#[test]
fn a_baseline_is_reused_under_equal_limits_and_not_under_tighter_ones() {
    let image = sldprt_with_body(&triangle_body());
    let limits = ResourceLimits::desktop();
    let arena = DecodeArena::new();
    let policy = DecodePolicy { limits };
    let source = DecodeContext::from_root_bytes(&image, &arena, &policy).unwrap();
    let mut write = SemanticWriteCache::default();
    assert!(
        retained_source_baseline(&mut write.source_baseline, Some(&source))
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
    let source = DecodeContext::from_root_bytes(&image, &arena, &policy).unwrap();
    let mut baseline = None;
    assert!(matches!(
        retained_source_baseline(&mut baseline, Some(&source)),
        Err(CodecError::ResourceLimit(_))
    ));
}
