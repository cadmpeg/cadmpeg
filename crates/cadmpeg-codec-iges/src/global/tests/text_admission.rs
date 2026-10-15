// SPDX-License-Identifier: Apache-2.0
use crate::global::{Resolution, Value};
use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};

#[test]
fn global_declaration_lossy_text_refuses_before_expanded_storage() {
    let bytes = b"a\xffb";
    let result = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "iges global declaration text",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::test_support::with_policy_context(bytes, &policy, |ctx| {
                let resolution = Resolution {
                    ctx,
                    values: std::array::from_fn(|index| {
                        if index == 0 {
                            Value::Malformed(bytes)
                        } else {
                            Value::Omitted
                        }
                    }),
                    losses: Vec::new(),
                    loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
                };
                resolution.declaration_text(0)
            })
        },
    );
    assert!(matches!(result, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0
            && limit.additional == 5 && limit.operation == "iges global declaration text"));
}

#[test]
fn global_declaration_lossy_text_preserves_replacement_boundaries() {
    for (bytes, expected) in [
        (&b"a\xffb"[..], "a\u{fffd}b"),
        (&b"a\xe2\x82"[..], "a\u{fffd}"),
        (&b"a\xff\xffb"[..], "a\u{fffd}\u{fffd}b"),
        (&b"valid"[..], "valid"),
    ] {
        crate::test_support::with_service_context(bytes, |ctx| {
            let resolution = Resolution {
                ctx,
                values: std::array::from_fn(|index| {
                    if index == 0 {
                        Value::Malformed(bytes)
                    } else {
                        Value::Omitted
                    }
                }),
                losses: Vec::new(),
                loss_storage: ctx.reserve_scoped(0, "iges global loss notes").unwrap(),
            };
            assert_eq!(resolution.declaration_text(0).unwrap(), expected);
        });
    }
}
