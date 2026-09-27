// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod test_archive;

pub(crate) fn assert_retained_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    for _ in 0..1024 {
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
            .expect("test input is within the root limit");
        match decode(&ctx) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes => {
                let threshold = limit.used.checked_add(limit.additional)
                    .expect("retained admission fits u64");
                assert!(threshold > policy.limits.max_retained_bytes);
                if limit.operation == operation {
                    policy.limits.max_retained_bytes = threshold - 1;
                    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                        .expect("test input is within the root limit");
                    assert!(matches!(decode(&ctx),
                        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                            if refusal.dimension == ResourceDimension::RetainedBytes
                                && refusal.operation == operation
                                && refusal.used + refusal.additional == threshold));
                    return;
                }
                policy.limits.max_retained_bytes = threshold;
            }
            Err(error) => panic!("expected {operation} refusal; got {error:?}"),
            Ok(_) => panic!("{operation} did not refuse"),
        }
    }
    panic!("{operation} was not reached within 1024 retained admissions");
}

pub(crate) fn validate_native(ir: &cadmpeg_ir::document::CadIr) -> Vec<cadmpeg_ir::report::check::Finding> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within the input limit");
    crate::validate_native(&ctx, ir).expect("test validation is within resource limits")
}
