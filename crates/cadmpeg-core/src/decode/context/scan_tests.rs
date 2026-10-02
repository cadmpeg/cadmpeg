// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn scan_text_and_concat_passes_refuse_on_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(ctx.copy_retained_lossy_utf8(b"a", "UTF-8"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(ctx.concat_retained(&[Vec::new()], "concat"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits));
}

