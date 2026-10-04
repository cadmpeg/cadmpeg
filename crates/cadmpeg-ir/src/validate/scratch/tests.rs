// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn geometric_scratch_preserves_first_and_later_slot_refusals() {
    for (dimension, cap, later) in [
        (ResourceDimension::MaterializedBytes, 0, false),
        (ResourceDimension::CollectionItems, 0, false),
        (ResourceDimension::CollectionItems, 1, true),
        (ResourceDimension::WorkUnits, 0, false),
        (ResourceDimension::WorkUnits, 1, true),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut scratch = super::Scratch::new(&ctx).unwrap();
        if later {
            scratch.push(1u64).unwrap();
        }
        let Err(CodecError::ResourceLimit(limit)) = scratch.push(2u64) else {
            panic!("scratch must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert_eq!(&scratch[..], if later { &[1u64][..] } else { &[] });
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn geometric_scratch_iterator_holds_storage_until_drop() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = super::Scratch::new(&ctx).unwrap();
    scratch.push(1u64).unwrap();
    let mut values = scratch.into_iter();
    assert_eq!(values.next(), Some(1));
    assert_eq!(values.next(), None);
    let Err(CodecError::ResourceLimit(limit)) =
        ctx.reserve_scoped(64, "iterator still owns storage")
    else {
        panic!("iterator reservation must be held");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    drop(values);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn geometric_scratch_releases_consumed_storage_without_retained_copies() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scratch = super::Scratch::new(&ctx).unwrap();
    scratch.extend(&[1u64, 2], |value| *value).unwrap();
    assert_eq!(scratch.into_iter().collect::<Vec<_>>(), [1, 2]);
    drop(ctx.reserve_scoped(64, "scratch storage released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn validation_filter_admits_source_before_projection_and_keeps_original_refusal() {
    for cap in [0, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let visits = std::cell::Cell::new(0);
        let Err(CodecError::ResourceLimit(limit)) =
            super::Scratch::filter_map(&ctx, [0, 1, 2], |value| {
                visits.set(visits.get() + 1);
                Ok((value != 0).then_some(value))
            })
        else {
            panic!("filter must refuse");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "validation filter scan");
        assert_eq!(visits.get(), if cap == 0 { 0 } else { 2 });
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}
