// SPDX-License-Identifier: Apache-2.0
//! Lazy key projection and reached source admission.

use crate::history::{UniqueIndex, UniqueLookup};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn rejected_key(_: &cadmpeg_core::decode::DecodeContext<'_>, _: &u64) -> Result<Option<u64>, CodecError> {
    Err(CodecError::Malformed("invalid key projection".into()))
}

fn optional_key(_: &cadmpeg_core::decode::DecodeContext<'_>, value: &u64) -> Result<Option<u64>, CodecError> {
    Ok((*value != 0).then_some(*value))
}

#[test]
fn unique_index_projection_error_skips_unread_source_and_leaves_no_cache() {
    let source = vec![1_u64; 4096];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let index = UniqueIndex::new(&source, rejected_key, "project F3D test index source");
        assert!(matches!(index.get(ctx, &1), Err(CodecError::Malformed(message)) if message == "invalid key projection"));
        assert!(index.table.get().is_none());
        assert_eq!(ctx.resource_refusal(), None);
        assert!(matches!(ctx.charge_work(1, "after rejected key"), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits && limit.used == 1 && limit.additional == 1));
    });
}

#[test]
fn unique_index_optional_projection_keeps_tombstones_and_native_values() {
    let source = [0_u64, 1, 1, 2];
    crate::test_support::with_decode_context(|ctx| {
        let index = UniqueIndex::new(&source, optional_key, "project F3D test index source");
        assert!(matches!(index.get_entry(ctx, &0).unwrap(), UniqueLookup::Absent));
        assert!(matches!(index.get_entry(ctx, &1).unwrap(), UniqueLookup::Repeated));
        let UniqueLookup::Value(value) = index.get_entry(ctx, &2).unwrap() else { panic!("unique projected value"); };
        assert!(std::ptr::eq(value, &source[3]));
    });
}

#[test]
fn unique_index_source_refusal_is_lazy_sticky_and_does_not_install_cache() {
    let source = [1_u64];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let index = UniqueIndex::new(&source, optional_key, "project F3D test index source");
        assert_eq!(ctx.resource_refusal(), None);
        assert!(index.table.get().is_none());
        let CodecError::ResourceLimit(original) = index.get(ctx, &1).unwrap_err() else { panic!("index source refusal"); };
        assert_eq!(original.operation, "project F3D test index source");
        assert_eq!(original.used, 0);
        assert_eq!(original.additional, 1);
        assert!(index.table.get().is_none());
        let CodecError::ResourceLimit(sticky) = index.get(ctx, &1).unwrap_err() else { panic!("sticky index refusal"); };
        assert_eq!(sticky, original);
        assert!(index.table.get().is_none());
    });
}
