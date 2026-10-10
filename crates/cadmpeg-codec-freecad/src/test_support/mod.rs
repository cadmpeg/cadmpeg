// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod test_archive;

pub(crate) fn assert_retained_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                .expect("test input is within the root limit");
            decode(&ctx)
        },
    );
}

pub(crate) fn assert_collection_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                .expect("test input is within the root limit");
            decode(&ctx)
        },
    );
}

pub(crate) fn materialized_refusal_at<T>(
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("test input is within the root limit");
            let result = decode(&ctx);
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    )
}

pub(crate) fn validate_native(
    ir: &cadmpeg_ir::document::CadIr,
) -> Vec<cadmpeg_ir::report::check::Finding> {
    let bytes = serde_json::to_vec(ir).expect("test document serializes");
    with_service_context(&bytes, |ctx| {
        crate::validate_native(ctx, ir).expect("test validation is within resource limits")
    })
}

pub(crate) fn with_service_context<T>(
    input: &[u8],
    use_context: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("test input is within service limits");
    use_context(&ctx)
}

/// Builds a checked entry with charged digest storage for synthetic fixtures.
pub(crate) fn entry_record(
    id: String,
    name: String,
    role: cadmpeg_core::container::ContainerRole,
    references: Vec<String>,
    data: Vec<u8>,
) -> crate::native::EntryRecord {
    with_service_context(&[], |ctx| {
        crate::native::EntryRecord::new(ctx, id, name, role, references, data)
            .expect("valid entry fixture")
    })
}
