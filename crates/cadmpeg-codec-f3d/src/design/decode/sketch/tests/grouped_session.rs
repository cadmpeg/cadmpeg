// SPDX-License-Identifier: Apache-2.0

use super::super::{record_streams, Grouped};
use crate::records::decal::DesignRecordHeader;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn native_headers() -> [DesignRecordHeader; 5] {
    [("f3d:z", 4), ("f3d:a", 9), ("f3d:a", 2), ("f3d:a", 9), ("unscoped", 1)]
        .map(|(scope, index)| DesignRecordHeader {
            id: if scope == "unscoped" {
                "unscoped".into()
            } else {
                format!("{scope}:design-record-header#{index}")
            },
            record_index: index,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
            byte_offset: u64::from(index),
        })
}

#[test]
fn grouped_session_preserves_native_record_order_identity_and_refunds_backing() {
    let records = native_headers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (grouped, held) = ctx.with_scoped_storage("test native grouped records", || {
        Grouped::build(&ctx, &records, |record| record_streams::record_stream(&ctx, &record.id),
            "test native grouped records")
    }).unwrap();
    storage = held;
    let run = grouped.run("f3d:a").unwrap();
    assert_eq!(run.len(), 3);
    for (item, original) in run.iter().zip([&records[1], &records[2], &records[3]]) {
        assert_eq!(item.0, "f3d:a");
        assert!(std::ptr::eq(item.1, original));
    }
    let mut visited = grouped.of("f3d:a", "test early native group walk").unwrap();
    assert!(std::ptr::eq(visited.next().unwrap().unwrap(), &records[1]));
    drop(visited);
    assert!(std::ptr::eq(grouped.run("f3d:z").unwrap()[0].1, &records[0]));
    assert!(grouped.run("unscoped").unwrap().is_empty());
    assert!(grouped.of("f3d:missing", "test missing native group").unwrap().next().is_none());
    drop(grouped);
    drop(storage);
    drop(ctx.reserve_scoped(4096, "after native grouped backing destruction").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn grouped_session_queries_and_construction_keep_original_refusal() {
    let records = native_headers();
    for input in [records.as_slice(), &[][..]] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let storage;
        let (grouped, held) = ctx.with_scoped_storage("test original grouped session", || {
            Grouped::build(&ctx, input, |record| record_streams::record_stream(&ctx, &record.id),
                "test original grouped session")
        }).unwrap();
        storage = held;
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original grouped session refusal")
        else { panic!("creating session must refuse excess work"); };
        for query in ["f3d:a", "f3d:missing"] {
            assert!(matches!(grouped.run(query), Err(CodecError::ResourceLimit(later)) if later == original));
            assert!(matches!(grouped.of(query, "test refused group walk"),
                Err(CodecError::ResourceLimit(later)) if later == original));
        }
        assert!(matches!(Grouped::build(&ctx, input,
            |record| record_streams::record_stream(&ctx, &record.id), "test refused group construction"),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert_eq!(other.resource_refusal(), None);
        drop(grouped);
        drop(storage);
        other.finish_session().unwrap();
    }
}

#[test]
fn grouped_session_empty_records_need_no_work_or_backing_then_keep_original_fuse() {
    let records: &[DesignRecordHeader] = &[];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let grouped = Grouped::build(&ctx, records,
        |record| record_streams::record_stream(&ctx, &record.id), "test empty native group").unwrap();
    assert!(grouped.run("f3d:missing").unwrap().is_empty());
    assert!(grouped.of("f3d:missing", "test empty group walk").unwrap().next().is_none());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty grouped refusal")
    else { panic!("zero-work original refusal"); };
    assert!(matches!(grouped.run("f3d:missing"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(grouped.of("f3d:missing", "test refused empty group walk"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(Grouped::build(&ctx, records,
        |record| record_streams::record_stream(&ctx, &record.id), "test refused empty group construction"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(grouped);
}

/// Current core insertion sort admits three source visits, six measuring
/// visits, three levels of slot/key work, then two insertions and three
/// adjacent comparisons. Each of the two key bisections visits two entries;
/// each entry pays one step and both five-byte keys. No vector growth moves
/// an old slot because all three entries fit the initial four-slot backing.
fn three_native_record_build_and_query_work() -> u64 {
    let slot_bytes = u64::try_from(std::mem::size_of::<(&str, &DesignRecordHeader)>()).unwrap();
    let key_bytes = u64::try_from("f3d:a".len()).unwrap();
    let build = 3 + 6 + (3 * slot_bytes + 2 * 3 * key_bytes) * 3 * 8 + 2 + 3;
    let query = 4 * (1 + 2 * key_bytes);
    build + query
}

#[test]
fn grouped_session_early_drop_pays_only_reached_native_records() {
    const KEY: &str = "f3d:a";
    const WALK: &str = "test reached native group walk";
    let records = native_headers();
    let input = &records[1..4];
    let setup = three_native_record_build_and_query_work();
    for visits in 1_u64..=3 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = setup + visits;
        policy.limits.max_materialized_bytes = 4096;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let storage;
        let (grouped, held) = ctx.with_scoped_storage("test bounded native group", || {
            Grouped::build(&ctx, input, |_| Ok(Some(KEY)), "test bounded native group")
        }).unwrap();
        storage = held;
        let mut actual = grouped.of(KEY, WALK).unwrap();
        for original in input.iter().take(usize::try_from(visits).unwrap()) {
            assert!(std::ptr::eq(actual.next().unwrap().unwrap(), original));
        }
        if visits == 3 {
            assert!(actual.next().is_none());
        }
        drop(actual);
        assert!(ctx.resource_refusal().is_none());
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "after reached native prefix")
        else { panic!("only reached native records consume the exact allowance"); };
        assert_eq!(original.used, setup + visits);
        assert_eq!(original.additional, 1);
        assert_eq!(original.limit, setup + visits);
        assert_eq!(original.operation, "after reached native prefix");
        assert_eq!(ctx.resource_refusal(), Some(original));
        drop(grouped);
        drop(storage);
    }
}

#[test]
fn grouped_session_one_below_first_visit_refuses_before_yield_and_ends_the_cursor() {
    const KEY: &str = "f3d:a";
    const WALK: &str = "test refused native first visit";
    let records = native_headers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = three_native_record_build_and_query_work();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (grouped, held) = ctx.with_scoped_storage("test refused native group", || {
        Grouped::build(&ctx, &records[1..4], |_| Ok(Some(KEY)), "test refused native group")
    }).unwrap();
    storage = held;
    let mut actual = grouped.of(KEY, WALK).unwrap();
    let Some(Err(CodecError::ResourceLimit(original))) = actual.next()
    else { panic!("the first record is refused before it is yielded"); };
    assert_eq!(original.used, three_native_record_build_and_query_work());
    assert_eq!(original.additional, 1);
    assert_eq!(original.limit, policy.limits.max_work_units);
    assert_eq!(original.operation, WALK);
    assert!(actual.next().is_none());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(actual);
    drop(grouped);
    drop(storage);
}

#[test]
fn grouped_session_late_original_refusal_dominates_the_next_native_visit() {
    let records = native_headers();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let storage;
    let (grouped, held) = ctx.with_scoped_storage("test late-refused group", || {
        Grouped::build(&ctx, &records, |record| record_streams::record_stream(&ctx, &record.id),
            "test late-refused group")
    }).unwrap();
    storage = held;
    let mut actual = grouped.of("f3d:a", "test late-refused native visit").unwrap();
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
        policy.limits.max_work_units + 1, "original refusal after group cursor construction")
    else { panic!("original session refuses excess work"); };
    assert!(matches!(actual.next(), Some(Err(CodecError::ResourceLimit(later))) if later == original));
    assert!(actual.next().is_none());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(actual);
    drop(grouped);
    drop(storage);
}
