// SPDX-License-Identifier: Apache-2.0

use super::{canonical_json_sha256, canonical_json_sha256_with_charge};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn canonical_hash_byte_charge_preserves_digest() {
    let value = std::collections::BTreeMap::from([("escaped", "a\\b\"c\n".repeat(4096)), ("unicode", "æΩ🎛".repeat(4096))]);
    let expected = canonical_json_sha256(&value).unwrap();
    let ctx = cadmpeg_test_support::service_decode_context();
    let actual: Result<String, CodecError> = canonical_json_sha256_with_charge(&value, |bytes| ctx.charge_work(bytes, "test canonical hash bytes"));
    assert_eq!(actual.unwrap(), expected);
}

#[test]
fn canonical_hash_byte_charge_propagates_resource_refusal() {
    let value = vec!["retained record text"; 1024];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result: Result<String, CodecError> = canonical_json_sha256_with_charge(&value, |bytes| ctx.charge_work(bytes, "test canonical hash bytes"));
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "test canonical hash bytes"));
}
