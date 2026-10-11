// SPDX-License-Identifier: Apache-2.0
//! Recovered validation errors release their scratch text.

use cadmpeg_core::decode::DecodePolicy;
use super::super::{is_decodable_string, string_within_limit, ImplementationLevel, Value};

#[test]
fn header_string_validation_releases_discarded_error_text() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 64;
    let value = Value::String(b"\\X\\GG".to_vec());
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        for _ in 0..8 {
            assert!(!is_decodable_string(&value, ImplementationLevel::LegacyEdition1, ctx).unwrap());
            assert!(!string_within_limit(&value, ImplementationLevel::LegacyEdition1, 10, ctx).unwrap());
        }
        assert!(ctx.resource_refusal().is_none());
    });
}
