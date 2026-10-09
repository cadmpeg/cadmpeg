// SPDX-License-Identifier: Apache-2.0

use super::super::{indexed_operand_headers, next_stream_offset, KeyCollector, RecordGroups, RecordIndex, StreamOrdered};
use crate::records::decal::DesignRecordHeader;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

fn native_headers() -> [DesignRecordHeader; 5] {
    [("f3d:z", 4, 30), ("f3d:a", 7, 20), ("f3d:a", 2, 10), ("f3d:a", 7, 20), ("unscoped", 1, 0)]
        .map(|(scope, index, offset)| DesignRecordHeader {
            id: if scope == "unscoped" { "unscoped".into() }
                else { format!("{scope}:design-record-header#{index}") },
            record_index: index,
            class_tag: crate::records::references::DesignClassTag::try_from("305".to_owned()).unwrap(),
            byte_offset: offset,
        })
}

#[test]
fn operand_index_session_preserves_native_winners_groups_offset_order_and_refunds_backing() {
    let records = native_headers();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = indexed_operand_headers(&ctx, &records).unwrap();
    let groups = RecordGroups::build(&ctx, &records,
        |record| (record.id.as_str(), record.record_index), "test native grouped header index").unwrap();
    let ordered = StreamOrdered::build(&ctx, &records, |_| true, |record| record.id.as_str(),
        |record| record.byte_offset, ("test native ordered headers", "test native ordered header backing")).unwrap();
    let mut keys = KeyCollector::new(&ctx, "test native membership backing").unwrap();
    for key in [7_u32, 2, 7] { keys.push(key, "test native membership backing").unwrap(); }
    let keys = keys.into_sorted("test native membership sort").unwrap();
    assert!(std::ptr::eq(index.get("f3d:a", 7).unwrap().unwrap(), &records[3]));
    assert!(std::ptr::eq(index.get("f3d:z", 4).unwrap().unwrap(), &records[0]));
    assert!(index.get("f3d:a", 99).unwrap().is_none());
    assert!(index.get("unscoped", 1).unwrap().is_none());
    let group = groups.get("f3d:a", 7).unwrap();
    assert_eq!(group.len(), 2);
    assert!(std::ptr::eq(group[0], &records[1]));
    assert!(std::ptr::eq(group[1], &records[3]));
    assert!(groups.get("f3d:missing", 7).unwrap().is_empty());
    let stream = ordered.of_stream("f3d:a").unwrap();
    assert_eq!(stream.len(), 3);
    for (item, original) in stream.iter().zip([&records[2], &records[1], &records[3]]) {
        assert_eq!(item.0, "f3d:a");
        assert!(std::ptr::eq(item.1, original));
    }
    assert_eq!(ordered.stream_start("f3d:a").unwrap(), 0);
    assert_eq!(ordered.stream_start("f3d:z").unwrap(), 3);
    assert!(ordered.of_stream("f3d:missing").unwrap().is_empty());
    assert_eq!(next_stream_offset(&ordered, "f3d:a", 10).unwrap(), Some(20));
    assert_eq!(next_stream_offset(&ordered, "f3d:a", 20).unwrap(), None);
    assert_eq!(keys.keys, [2, 7, 7]);
    for (key, present) in [(2, true), (7, true), (9, false)] {
        assert_eq!(keys.contains(&key, "test native membership query").unwrap(), present);
    }
    drop(index);
    drop(groups);
    drop(ordered);
    drop(keys);
    drop(ctx.reserve_scoped(4096, "after native operand indexes are destroyed").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn operand_index_session_queries_mutation_sort_and_construction_keep_original_refusal() {
    let records = native_headers();
    for input in [records.as_slice(), &[][..]] {
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (other, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let index = indexed_operand_headers(&ctx, input).unwrap();
        let groups = RecordGroups::build(&ctx, input,
            |record| (record.id.as_str(), record.record_index), "test bound native grouped index").unwrap();
        let ordered = StreamOrdered::build(&ctx, input, |_| true, |record| record.id.as_str(),
            |record| record.byte_offset, ("test bound native ordered headers", "test bound ordered backing")).unwrap();
        let mut pending = KeyCollector::<u32>::new(&ctx, "test bound pending keys").unwrap();
        let mut for_sort = KeyCollector::<u32>::new(&ctx, "test bound sort keys").unwrap();
        let mut for_query = KeyCollector::<u32>::new(&ctx, "test bound query keys").unwrap();
        if !input.is_empty() {
            pending.push(7, "test bound pending keys").unwrap();
            for_sort.push(7, "test bound sort keys").unwrap();
            for_query.push(7, "test bound query keys").unwrap();
        }
        let keys = for_query.into_sorted("test bound sorted keys").unwrap();
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original native operand index refusal")
        else { panic!("creating session refuses excess work"); };
        for scope in ["f3d:a", "f3d:missing"] {
            assert!(matches!(index.get(scope, 7), Err(CodecError::ResourceLimit(later)) if later == original));
            assert!(matches!(groups.get(scope, 7), Err(CodecError::ResourceLimit(later)) if later == original));
            assert!(matches!(ordered.stream_start(scope), Err(CodecError::ResourceLimit(later)) if later == original));
            assert!(matches!(ordered.of_stream(scope), Err(CodecError::ResourceLimit(later)) if later == original));
            assert!(matches!(next_stream_offset(&ordered, scope, u64::MAX),
                Err(CodecError::ResourceLimit(later)) if later == original));
        }
        for key in [7, 99] {
            assert!(matches!(keys.contains(&key, "test refused membership"),
                Err(CodecError::ResourceLimit(later)) if later == original));
        }
        let expected = if input.is_empty() { &[][..] } else { &[7][..] };
        assert_eq!(pending.keys, expected);
        assert!(matches!(pending.push(99, "test refused pending key"),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(pending.keys, expected);
        assert!(matches!(for_sort.into_sorted("test refused native key sort"),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(RecordIndex::build(&ctx, input, |_| panic!("refused index must not read identities"),
            "test refused index scan", "test refused index storage"),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(RecordGroups::build(&ctx, input, |_| panic!("refused groups must not read identities"),
            "test refused native groups"), Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(StreamOrdered::build(&ctx, input, |_| panic!("refused ordering must not select records"),
            |_| panic!("refused ordering must not read identities"), |_| panic!("refused ordering must not read offsets"),
            ("test refused ordering scan", "test refused ordering backing")),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert!(matches!(KeyCollector::<u32>::new(&ctx, "test refused key collector"),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert_eq!(other.resource_refusal(), None);
        drop(index);
        drop(groups);
        drop(ordered);
        drop(pending);
        drop(keys);
        other.finish_session().unwrap();
    }
}

#[test]
fn operand_index_session_empty_owners_need_no_work_storage_or_items_then_keep_original_fuse() {
    let input: &[DesignRecordHeader] = &[];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let index = indexed_operand_headers(&ctx, input).unwrap();
    let groups = RecordGroups::build(&ctx, input, |_| panic!("empty groups have no identities"),
        "test empty native groups").unwrap();
    let ordered = StreamOrdered::build(&ctx, input, |_| panic!("empty ordering has no selection"),
        |_| panic!("empty ordering has no identities"), |_| panic!("empty ordering has no offsets"),
        ("test empty ordering scan", "test empty ordering storage")).unwrap();
    let mut pending = KeyCollector::<u32>::new(&ctx, "test empty pending keys").unwrap();
    let for_sort = KeyCollector::<u32>::new(&ctx, "test empty sort keys").unwrap();
    let keys = KeyCollector::<u32>::new(&ctx, "test empty query keys").unwrap()
        .into_sorted("test empty sorted keys").unwrap();
    assert!(index.get("f3d:missing", 7).unwrap().is_none());
    assert!(groups.get("f3d:missing", 7).unwrap().is_empty());
    assert_eq!(ordered.stream_start("f3d:missing").unwrap(), 0);
    assert!(ordered.of_stream("f3d:missing").unwrap().is_empty());
    assert_eq!(next_stream_offset(&ordered, "f3d:missing", u64::MAX).unwrap(), None);
    assert!(!keys.contains(&7, "test empty membership").unwrap());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty operand refusal")
    else { panic!("zero-work creating session refuses positive work"); };
    assert!(matches!(index.get("f3d:missing", 7), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(groups.get("f3d:missing", 7), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(ordered.of_stream("f3d:missing"), Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(keys.contains(&7, "test refused empty membership"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(pending.push(7, "test refused empty key mutation"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(pending.keys.is_empty());
    assert!(matches!(for_sort.into_sorted("test refused empty key sort"),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(index);
    drop(groups);
    drop(ordered);
    drop(pending);
    drop(keys);
}
