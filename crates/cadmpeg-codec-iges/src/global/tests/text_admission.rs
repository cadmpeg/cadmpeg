// SPDX-License-Identifier: Apache-2.0
use crate::global::{Resolution, Value};
use cadmpeg_core::{CodecError, decode::{DecodePolicy, ResourceDimension}};

#[test]
fn global_declaration_lossy_text_refuses_before_expanded_storage() {
    let bytes = b"a\xffb";
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    crate::test_support::with_policy_context(bytes, &policy, |ctx| {
        let resolution = Resolution { ctx, values: vec![Value::Malformed(bytes.to_vec())], losses: Vec::new() };
        let result = resolution.declaration_text(0);
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0
                && limit.additional == 5 && limit.operation == "iges global declaration text"));
    });
}

#[test]
fn global_declaration_lossy_text_preserves_replacement_boundaries() {
    for (bytes, expected) in [
        (&b"a\xffb"[..],"a\u{fffd}b"),
        (&b"a\xe2\x82"[..],"a\u{fffd}"),
        (&b"a\xff\xffb"[..],"a\u{fffd}\u{fffd}b"),
        (&b"valid"[..],"valid"),
    ] {
        crate::test_support::with_service_context(bytes, |ctx| {
            let resolution = Resolution { ctx, values: vec![Value::Malformed(bytes.to_vec())], losses: Vec::new() };
            assert_eq!(resolution.declaration_text(0).unwrap(),expected);
        });
    }
}
