// SPDX-License-Identifier: Apache-2.0
use super::{format_design_text, malformed_design};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn design_diagnostic_refuses_exact_retained_byte_limit() {
    let source = "input é\nsource";
    let expected = format!("source {source:?} at {}", u64::MAX);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(malformed_design(Some(&ctx), format_args!("source {source:?} at {}", u64::MAX)),
        CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "f3d Design diagnostic")
    );
    policy.limits.max_retained_bytes += 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(malformed_design(Some(&ctx), format_args!("source {source:?} at {}", u64::MAX)),
        CodecError::Malformed(message) if message == expected)
    );
}

#[test]
fn design_formatted_text_refuses_before_input_sized_allocation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(
        matches!(format_design_text(Some(&ctx), format_args!("é"), "formatted text"),
        Err(CodecError::ResourceLimit(failure))
            if failure.dimension == ResourceDimension::RetainedBytes
                && failure.operation == "formatted text")
    );
}
