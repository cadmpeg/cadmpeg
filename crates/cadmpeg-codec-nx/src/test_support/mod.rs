// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads.
//! Native projection fixtures use checked source tokens.
#![allow(clippy::unwrap_used)]

pub(crate) mod native_references;
pub(crate) mod test_bytes;
pub(crate) mod test_cfb;
pub(crate) mod test_deltas;
pub(crate) mod test_om;
pub(crate) mod test_prt;
pub(crate) mod test_streams;
pub(crate) mod test_wire;

pub(crate) fn with_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    with_decode_context_over(&[], |_| {}, f)
}

pub(crate) fn with_decode_context_over<T>(
    root: &[u8],
    adjust: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    adjust(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(root, &arena, &policy)
        .expect("test root is admitted");
    f(&ctx)
}

pub(crate) fn extract_streams(bytes: &[u8]) -> Vec<crate::parasolid::Stream> {
    with_decode_context_over(
        bytes,
        |_| {},
        |ctx| {
            let root = cadmpeg_core::decode::View::over_retained(bytes);
            let container =
                crate::container::scan_bytes(ctx, bytes.to_vec()).expect("test SPLMSSTR container");
            crate::parasolid::extract_streams(ctx, root, &container)
                .expect("test Parasolid streams")
        },
    )
}

/// Admit parser requests before refusing the selected collection operation.
pub(crate) fn collection_refusal_at<T>(
    operation: &str,
    decode: impl Fn(u64) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let mut ceiling = 0;
    for _ in 0..4096 {
        let Err(CodecError::ResourceLimit(limit)) = decode(ceiling) else {
            panic!("{operation} must refuse before decode completes");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
        let threshold = limit
            .used
            .checked_add(limit.additional)
            .expect("collection threshold");
        assert!(threshold > ceiling);
        if limit.operation == operation {
            let error = decode(threshold - 1)
                .err()
                .expect("one item below admission");
            assert!(matches!(&error, CodecError::ResourceLimit(refusal)
                if refusal.dimension == ResourceDimension::CollectionItems
                    && refusal.operation == operation
                    && refusal.used + refusal.additional == threshold));
            return error;
        }
        ceiling = threshold;
    }
    panic!("{operation} was not reached within 4096 admissions");
}

/// Admit earlier collection storage, then refuse the named operation.
pub(crate) fn resource_refusal_at<T>(
    root: &[u8],
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::ResourceDimension;
    cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        with_decode_context_over(
            root,
            |policy| match dimension {
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
                ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
                _ => panic!("unsupported resource test dimension"),
            },
            |ctx| {
                let result = decode(ctx);
                if let Err(cadmpeg_core::CodecError::ResourceLimit(resource)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(resource));
                }
                result
            },
        )
    })
}
