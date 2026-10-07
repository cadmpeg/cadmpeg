// SPDX-License-Identifier: Apache-2.0

use super::canonical_json_sha256;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn canonical_hash_byte_charge_preserves_digest() {
    let value = std::collections::BTreeMap::from([
        ("escaped", "a\\b\"c\n".repeat(4096)),
        ("unicode", "æΩ🎛".repeat(4096)),
    ]);
    let expected = canonical_json_sha256(
        &cadmpeg_test_support::service_decode_context(),
        &value,
        "canonical hash fixture",
    )
    .unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let actual: Result<String, CodecError> =
        canonical_json_sha256(&ctx, &value, "test canonical hash bytes").map_err(Into::into);
    assert_eq!(actual.unwrap(), expected);
}

#[test]
fn canonical_hash_byte_charge_propagates_resource_refusal() {
    let value = vec!["retained record text"; 1024];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result: Result<String, CodecError> =
        canonical_json_sha256(&ctx, &value, "test canonical hash bytes").map_err(Into::into);
    assert!(
        matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "test canonical hash bytes")
    );
}

#[test]
fn canonical_hash_admits_buffer_output_work_and_depth_with_the_original_refusal() {
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::RetainedBytes,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("canonical hash refusal dimensions"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error: CodecError =
            canonical_json_sha256(&ctx, &"text", "canonical hash dimension fixture")
                .unwrap_err()
                .into();
        let CodecError::ResourceLimit(limit) = error else {
            panic!("hash must preserve resource refusal");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn canonical_hash_releases_its_buffer_before_the_next_digest() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 8192;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = canonical_json_sha256(&ctx, &"text", "canonical hash buffer fixture").unwrap();
    let second = canonical_json_sha256(&ctx, &"text", "canonical hash buffer fixture").unwrap();
    assert_eq!(first, second);
    let storage = ctx
        .reserve_scoped(8192, "canonical hash buffer released")
        .unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn canonical_hash_matches_standard_pretty_json_for_compound_shapes() {
    let value = serde_json::json!({
        "empty": [],
        "links": ["a\\b\"c\n", "æΩ🎛", null, true, -17, 0.125],
        "nested": {"first": [1, 2, 3], "second": "x".repeat(16384)},
    });
    let expected = super::sha256_hex(&serde_json::to_vec_pretty(&value).unwrap());
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(canonical_json_sha256(&ctx, &value, "standard pretty JSON digest").unwrap(), expected);
    ctx.finish_session().unwrap();
}
