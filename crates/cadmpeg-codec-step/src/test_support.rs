// SPDX-License-Identifier: Apache-2.0
//! Shared STEP Part 21 byte-fixture helpers for `#[cfg(test)]` suites.

#![allow(clippy::unwrap_used)]

pub(crate) mod exchange;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

pub(crate) fn with_service_context<T>(
    bytes: &[u8],
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    with_policy_context(bytes, &DecodePolicy::service(), use_context)
}

pub(crate) fn with_policy_context<T>(
    bytes: &[u8],
    policy: &DecodePolicy,
    use_context: impl FnOnce(&[u8], &DecodeContext<'_>) -> T,
) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, policy)
        .expect("fixture fits selected policy");
    use_context(bytes, &ctx)
}

/// Admit earlier requests, then refuse the named boundary one unit below its need.
pub(crate) fn resource_refusal_at<T>(
    bytes: &[u8],
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    decode: impl Fn(&[u8], &DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::decode::ResourceLimit {
    let mut cap = 0;
    loop {
        let mut policy = DecodePolicy::service();
        match dimension {
            cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                policy.limits.max_work_units = cap;
            }
            cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes = cap;
            }
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = cap;
            }
            _ => panic!("unsupported boundary dimension"),
        }
        let error = with_policy_context(bytes, &policy, |bytes, ctx| decode(bytes, ctx))
            .err()
            .expect("named boundary precedes completion");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("unexpected boundary refusal: {error}");
        };
        assert_eq!(limit.dimension, dimension);
        let need = limit
            .used
            .checked_add(limit.additional)
            .expect("resource need fits");
        assert!(need > cap);
        if limit.operation == operation {
            match dimension {
                cadmpeg_core::decode::ResourceDimension::WorkUnits => {
                    policy.limits.max_work_units = need - 1;
                }
                cadmpeg_core::decode::ResourceDimension::RetainedBytes => {
                    policy.limits.max_retained_bytes = need - 1;
                }
                cadmpeg_core::decode::ResourceDimension::MaterializedBytes => {
                    policy.limits.max_materialized_bytes = need - 1;
                }
                _ => panic!("unsupported boundary dimension"),
            }
            let error = with_policy_context(bytes, &policy, |bytes, ctx| decode(bytes, ctx))
                .err()
                .expect("one unit below need");
            let cadmpeg_core::CodecError::ResourceLimit(repeated) = error else {
                panic!("unexpected repeated refusal");
            };
            assert_eq!(repeated.operation, operation);
            assert_eq!(repeated.dimension, dimension);
            assert_eq!(repeated.used + repeated.additional, need);
            return repeated;
        }
        cap = need;
    }
}
