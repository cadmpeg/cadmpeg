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

#[test]
fn decoded_header_string_releases_errors_and_retains_success() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    policy.limits.max_materialized_bytes = 64;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        for _ in 0..8 {
            assert_eq!(super::super::decoded_bytes(b"\\X\\GG", ImplementationLevel::LegacyEdition1, ctx).unwrap(), None);
        }
        let result = super::super::decoded_bytes(b"abc", ImplementationLevel::LegacyEdition1, ctx).unwrap();
        assert_eq!(result.as_deref(), Some("abc"));
        assert!(matches!(ctx.copy_retained_text("x", "test retained header owner"),
            Err(cadmpeg_core::CodecError::ResourceLimit(refusal))
                if refusal.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                    && refusal.used == 3));
    });
}

