// SPDX-License-Identifier: Apache-2.0
//! Missing-magic errors retain admitted owned text.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::CodecBackend;

#[test]
fn missing_magic_admits_error_text_for_decode_and_inspect() {
    for inspect in [false, true] {
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>, root| {
            let codec = crate::codec::StepCodec::default();
            if inspect { codec.inspect_impl(ctx, root).map(|_| ()) }
            else { codec.decode_impl(ctx, root).map(|_| ()) }
        };
        crate::test_support::with_service_context(b"", |_, ctx| {
            assert!(matches!(run(ctx, cadmpeg_core::decode::View::over_retained(b"")),
                Err(CodecError::WrongFormat(message)) if message == "missing ISO-10303-21 magic"));
        });
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            assert!(matches!(run(ctx, cadmpeg_core::decode::View::over_retained(b"")),
                Err(CodecError::ResourceLimit(refusal))
                if refusal.dimension == ResourceDimension::RetainedBytes
                    && refusal.operation == "STEP missing magic error"));
        });
    }
}
