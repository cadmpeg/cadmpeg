// SPDX-License-Identifier: Apache-2.0
//! Backing storage charges, scoped attribution and refusal propagation.

use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

pub(super) fn storage_cases<T>(
    small: u64,
    grown: u64,
    build: impl Fn(&DecodeContext<'_>, usize) -> Result<T, CodecError>,
) {
    for (count, bytes) in [(1, small), (5, grown)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let values = build(&ctx, count).expect("exact storage admission");
        let error = ctx.charge_retained(1, "verify storage charge").expect_err("all bytes used");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == bytes
                && ctx.resource_refusal() == Some(limit)));
        drop(values);

        let arena = DecodeArena::new();
        policy.limits.max_retained_bytes = bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let error = match build(&ctx, count) {
            Ok(_) => panic!("one below the storage charge must refuse"),
            Err(error) => error,
        };
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && ctx.resource_refusal() == Some(limit)));

        let arena = DecodeArena::new();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let mut reservation = ctx.reserve_scoped(0, "test scoped storage").expect("empty scope");
        let values = reservation.with_storage(|| build(&ctx, count)).expect("scoped storage");
        let error = ctx.reserve_scoped(1, "verify scoped charge").expect_err("all scoped bytes used");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == bytes
                && ctx.resource_refusal() == Some(limit)));
        drop(values);
        drop(reservation);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
        let (values, reservation) = ctx.with_scoped_storage("released scoped storage", || build(&ctx, count))
            .expect("scoped storage without refusal");
        drop(values);
        drop(reservation);
        ctx.reserve_scoped(bytes, "released scoped storage").expect("scope released its bytes");
    }
}

macro_rules! storage_case {
    ($name:ident, $small:expr, $grown:expr, $build:expr) => {
        #[test]
        fn $name() {
            storage_cases($small, $grown, $build);
        }
    };
}

storage_case!(collection_vec_storage, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.collection_vec::<u64>(count, "collection storage")
});
storage_case!(reserve_vec_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    let mut values = Vec::new();
    for value in 0..count {
        let capacity = values.capacity();
        let result = ctx.reserve_vec(&mut values, 1, "reserve storage");
        if result.is_err() { assert_eq!(values.capacity(), capacity); }
        result?;
        values.push(u64::try_from(value).expect("small test index"));
    }
    Ok(values)
});
storage_case!(push_vec_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    let mut values = Vec::new();
    for value in 0..count {
        let capacity = values.capacity();
        let result = ctx.push_vec(&mut values, u64::try_from(value).expect("small test index"), "push storage");
        if result.is_err() { assert_eq!(values.capacity(), capacity); }
        result?;
    }
    Ok(values)
});
storage_case!(collect_vec_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    ctx.collect_vec((0..count).map(|value| u64::try_from(value).expect("small test index")), "collect storage")
});
storage_case!(try_collect_vec_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    ctx.try_collect_vec((0..count).map(|value| Ok::<_, CodecError>(u64::try_from(value).expect("small test index"))), "try collect storage")
});
// u64 set buckets use 32/64 bytes, 4/8 control bytes, sixteen trailing
// controls and at most fifteen alignment bytes. Map buckets have two lanes.
storage_case!(reserve_set_storage, 67, 103, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashSet::<u64>::new();
    for value in 0..count {
        let capacity = values.capacity();
        let result = ctx.reserve_set(&mut values, 1, "set storage");
        if result.is_err() { assert_eq!(values.capacity(), capacity); }
        result?;
        values.insert(u64::try_from(value).expect("small test index"));
    }
    Ok(values)
});
storage_case!(reserve_map_storage, 99, 167, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashMap::<u64, u64>::new();
    for value in 0..count {
        let capacity = values.capacity();
        let result = ctx.reserve_map(&mut values, 1, "map storage");
        if result.is_err() { assert_eq!(values.capacity(), capacity); }
        result?;
        values.insert(u64::try_from(value).expect("small test index"), 0);
    }
    Ok(values)
});
storage_case!(insert_hash_set_storage, 67, 103, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashSet::new();
    for value in 0..count {
        ctx.insert_hash_set(&mut values, u64::try_from(value).expect("small test index"), "set insertion storage")?;
    }
    Ok(values)
});
storage_case!(insert_hash_map_storage, 99, 167, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashMap::new();
    for value in 0..count {
        ctx.insert_hash_map(&mut values, u64::try_from(value).expect("small test index"), 0u64, "map insertion storage")?;
    }
    Ok(values)
});
storage_case!(admit_hash_map_entry_storage, 99, 167, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashMap::new();
    for value in 0..count {
        let key = u64::try_from(value).expect("small test index");
        ctx.admit_hash_map_entry(&mut values, &key, "map entry storage")?;
        values.insert(key, 0u64);
    }
    Ok(values)
});
// A u64/u64 B-tree node bound is 11 lane pairs, sixteen pointer widths
// and two alignment widths. Five new entries use 1+2+3+3+4 node bounds.
storage_case!(insert_btree_map_storage, 320, 4160, |ctx: &DecodeContext<'_>, count| {
    let mut values = BTreeMap::new();
    for value in 0..count {
        ctx.insert_btree_map(&mut values, u64::try_from(value).expect("small test index"), 0u64, "tree map storage")?;
    }
    Ok(values)
});
storage_case!(admit_btree_entry_storage, 320, 4160, |ctx: &DecodeContext<'_>, count| {
    let mut values = BTreeMap::new();
    for value in 0..count {
        let key = u64::try_from(value).expect("small test index");
        ctx.admit_btree_entry(&values, &key, "tree entry storage")?;
        values.insert(key, 0u64);
    }
    Ok(values)
});
storage_case!(insert_btree_set_storage, 232, 3016, |ctx: &DecodeContext<'_>, count| {
    let mut values = BTreeSet::new();
    for value in 0..count {
        ctx.insert_btree_set(&mut values, u64::try_from(value).expect("small test index"), "tree set storage")?;
    }
    Ok(values)
});

#[test]
fn failed_storage_callback_keeps_mutated_collection_bytes_live() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 32;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut values = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "mutated temporary vector").expect("empty scope");
    let error = storage.with_storage(|| {
        ctx.push_vec(&mut values, 1u64, "mutated temporary vector")?;
        Err::<(), _>(CodecError::malformed("callback refusal"))
    }).expect_err("callback refuses after mutation");
    assert!(matches!(error, CodecError::Malformed(message) if message == "callback refusal"));
    assert_eq!(values, [1]);
    let error = ctx.reserve_scoped(1, "verify live failed storage").expect_err("vector remains live");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 32));
    drop(values);
    drop(storage);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut values = Vec::new();
    let mut storage = ctx.reserve_scoped(0, "mutated temporary vector").expect("empty scope");
    let result = storage.with_storage(|| {
        ctx.push_vec(&mut values, 1u64, "mutated temporary vector")?;
        Err::<(), _>(CodecError::malformed("callback refusal"))
    });
    assert!(matches!(result, Err(CodecError::Malformed(message)) if message == "callback refusal"));
    drop(values);
    drop(storage);
    ctx.reserve_scoped(32, "released failed storage").expect("storage released by its guard");
}

// Delegating collection operations retain the same capacity accounting.
storage_case!(optional_collection_vec_storage, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.optional_collection_vec::<u64>(true, count, "optional collection storage")
});
storage_case!(collect_indexed_vec_storage, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.collect_indexed_vec(count, "indexed collection storage", |_| Ok(0u64))
});
storage_case!(copy_slice_storage, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.copy_slice(&[0u64; 5][..count], "copied collection storage")
});
storage_case!(collect_retained_vec_storage, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.collect_retained_vec(std::iter::repeat_n(0u64, count), "retained collection storage")
});
storage_case!(collect_hash_set_storage, 67, 103, |ctx: &DecodeContext<'_>, count| {
    ctx.collect_hash_set((0..count).map(|value| u64::try_from(value).expect("small test index")), "collected set storage")
});
storage_case!(extend_hash_set_storage, 67, 103, |ctx: &DecodeContext<'_>, count| {
    let mut values = HashSet::new();
    ctx.extend_hash_set(&mut values, (0..count).map(|value| u64::try_from(value).expect("small test index")), "extended set storage")?;
    Ok(values)
});
storage_case!(collect_btree_set_storage, 232, 3016, |ctx: &DecodeContext<'_>, count| {
    ctx.collect_btree_set((0..count).map(|value| u64::try_from(value).expect("small test index")), "collected tree storage")
});
storage_case!(reserve_heap_storage, 32, 40, |ctx: &DecodeContext<'_>, count| {
    let mut values = BinaryHeap::<u64>::new();
    ctx.reserve_heap(&mut values, count, "heap storage")?;
    Ok(values)
});
storage_case!(push_back_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    let mut values = VecDeque::new();
    for _ in 0..count { ctx.push_back(&mut values, 0u64, "deque back storage")?; }
    Ok(values)
});
storage_case!(push_front_storage, 32, 64, |ctx: &DecodeContext<'_>, count| {
    let mut values = VecDeque::new();
    for _ in 0..count { ctx.push_front(&mut values, 0u64, "deque front storage")?; }
    Ok(values)
});
storage_case!(join_display_retained_storage, 1, 5, |ctx: &DecodeContext<'_>, count| {
    ctx.join_display_retained(std::iter::repeat_n("x", count), "", "display join storage")
});

storage_case!(vector_storage_bytes, 8, 40, |ctx: &DecodeContext<'_>, count| {
    ctx.vector_storage::<u64>(count, "vector backing storage")
});
storage_case!(reserve_capacity_bytes, 32, 64, |ctx: &DecodeContext<'_>, count| {
    let mut values = Vec::new();
    for _ in 0..count {
        ctx.reserve_capacity(&mut values, 1, "capacity storage")?;
        values.push(0u64);
    }
    Ok(values)
});

mod retained;
mod scoped;
