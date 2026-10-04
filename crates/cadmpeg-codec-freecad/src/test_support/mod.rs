// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic `FCStd` archive fixtures for crate tests.

pub(crate) mod test_archive;

pub(crate) fn assert_retained_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    for _ in 0..1024 {
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
            .expect("test input is within the root limit");
        match decode(&ctx) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes =>
            {
                let threshold = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("retained admission fits u64");
                assert!(threshold > policy.limits.max_retained_bytes);
                if limit.operation == operation {
                    policy.limits.max_retained_bytes = threshold - 1;
                    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                        .expect("test input is within the root limit");
                    assert!(matches!(decode(&ctx),
                        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                            if refusal.dimension == ResourceDimension::RetainedBytes
                                && refusal.operation == operation
                                && refusal.used + refusal.additional == threshold));
                    return;
                }
                policy.limits.max_retained_bytes = threshold;
            }
            Err(error) => panic!("expected {operation} refusal; got {error:?}"),
            Ok(_) => panic!("{operation} did not refuse"),
        }
    }
    panic!("{operation} was not reached within 1024 retained admissions");
}

pub(crate) fn assert_collection_refusal_at<T>(
    input: &[u8],
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    for _ in 0..4096 {
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
            .expect("test input is within the root limit");
        match decode(&ctx) {
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems =>
            {
                let threshold = limit
                    .used
                    .checked_add(limit.additional)
                    .expect("collection admission fits u64");
                assert!(threshold > policy.limits.max_collection_items);
                if limit.operation == operation {
                    policy.limits.max_collection_items = threshold - 1;
                    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
                        .expect("test input is within the root limit");
                    assert!(matches!(decode(&ctx),
                        Err(cadmpeg_core::CodecError::ResourceLimit(ref refusal))
                            if refusal.dimension == ResourceDimension::CollectionItems
                                && refusal.operation == operation
                                && refusal.used + refusal.additional == threshold));
                    return;
                }
                policy.limits.max_collection_items = threshold;
            }
            Err(error) => panic!("expected {operation} refusal; got {error:?}"),
            Ok(_) => panic!("{operation} did not refuse"),
        }
    }
    panic!("{operation} was not reached within 4096 collection admissions");
}

pub(crate) fn materialized_refusal_at<T>(
    operation: &str,
    decode: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    for _ in 0..1024 {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = decode(&ctx) else {
            panic!("{operation} must refuse before decode completes");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        let threshold = limit
            .used
            .checked_add(limit.additional)
            .expect("materialized admission fits u64");
        assert!(threshold > policy.limits.max_materialized_bytes);
        if limit.operation == operation {
            policy.limits.max_materialized_bytes = threshold - 1;
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
            let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = decode(&ctx) else {
                panic!("{operation} must refuse one byte below its boundary");
            };
            assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(refusal.operation, operation);
            assert_eq!(refusal.used + refusal.additional, threshold);
            assert_eq!(ctx.resource_refusal(), Some(refusal));
            return cadmpeg_core::CodecError::ResourceLimit(refusal);
        }
        policy.limits.max_materialized_bytes = threshold;
    }
    panic!("{operation} was not reached within 1024 materialized admissions");
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
