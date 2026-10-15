// SPDX-License-Identifier: Apache-2.0
//! Nesting limits for drawing discovery.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_drawing_references(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}
