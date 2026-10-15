// SPDX-License-Identifier: Apache-2.0
//! Indefinite BER extents are indexed once and reused.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::test_support::{with_policy_context, with_service_context};

fn nested_cms(depth: usize) -> Vec<u8> {
    let source = super::BER_CMS_INDEFINITE;
    let signature = source
        .windows(4)
        .rposition(|bytes| bytes == [0x05, 0, 0x04, 0])
        .expect("algorithm parameters precede the signature")
        + 2;
    let mut cms = source[..signature].to_vec();
    for _ in 0..depth {
        cms.extend_from_slice(&[0x24, 0x80]);
    }
    cms.extend_from_slice(&[0x04, 0x01, 0]);
    for _ in 0..depth {
        cms.extend_from_slice(&[0, 0]);
    }
    cms.extend_from_slice(&source[signature + 2..]);
    cms
}

fn validate(ctx: &cadmpeg_core::decode::DecodeContext<'_>, input: &[u8]) -> Result<(), CodecError> {
    match crate::signature::validate_detached_cms(ctx, input) {
        Ok(()) => Ok(()),
        Err(crate::signature::CmsError::Resource(error)) => Err(error),
        error => panic!("valid constructed OCTET STRING: {error:?}"),
    }
}

#[test]
fn nested_indefinite_octet_strings_do_not_repeat_descendant_scans() {
    let work = |depth| {
        let cms = nested_cms(depth);
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1024;
        with_policy_context(&cms, &policy, |input, ctx| {
            validate(ctx, input).expect("nested signature is valid");
            let CodecError::ResourceLimit(refusal) = ctx
                .charge_work(u64::MAX, "test BER work")
                .expect_err("work probe refuses")
            else {
                panic!("work refusal required");
            };
            refusal.used
        })
    };
    // Doubling the nesting permits linear visits and logarithmic keyed work.
    // Repeated descendant scans approach four times the work instead.
    assert!(work(128) < 3 * work(64));
}

#[test]
fn ber_extent_entries_preserve_materialized_refusal() {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "STEP BER extent entries",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            with_policy_context(super::BER_CMS_INDEFINITE, &policy, |input, ctx| {
                validate(ctx, input)
            })
        },
    );
}

#[test]
fn ber_extent_index_releases_storage_without_retaining_it() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 65_536;
    with_policy_context(super::BER_CMS_INDEFINITE, &policy, |input, ctx| {
        validate(ctx, input).expect("extent index is temporary");
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test released BER index",
        )
        .expect("extent storage is released when validation returns");
    });
    with_service_context(super::BER_CMS_INDEFINITE, |input, ctx| {
        validate(ctx, input).expect("ordinary service admission");
    });
}
