// SPDX-License-Identifier: Apache-2.0
//! One indexed list needs no identifier copy; unions retain only live scratch.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use super::super::{EntityIds, EntityIndex};

fn no_storage_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn single_index_list_is_borrowed_without_copy_work_or_storage() {
    let arena = DecodeArena::new();
    let policy = no_storage_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let input = [1, 1, 2];
    let mut ids = EntityIndex::ordered_ids(&[&input], &ctx).unwrap();
    assert!(matches!(&ids, EntityIds::Borrowed(_)));
    assert_eq!(ids.len(), 3);
    // The previous one-list route copies this exact sequence without deduplication.
    assert_eq!(ids.next(), Some(1));
    assert_eq!(ids.next(), Some(1));
    assert_eq!(ids.next(), Some(2));
    assert_eq!(ids.next(), None);
    drop(ids);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn single_index_list_keeps_the_original_refusal() {
    let arena = DecodeArena::new();
    let policy = no_storage_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "STEP original union refusal").unwrap_err()
        else { panic!("original work refusal"); };
    let input = [1, 2];
    let Some(CodecError::ResourceLimit(refusal)) = EntityIndex::ordered_ids(&[&input], &ctx).err()
        else { panic!("borrowed replay must observe the original fuse"); };
    assert_eq!(refusal, original);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn single_selected_index_queries_allocate_only_the_list_reference() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=A();#1=A();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    // Each of the two sequential queries selects one borrowed slice. Core's
    // first amortized vector growth reserves four reference slots.
    policy.limits.max_materialized_bytes = u64::try_from(4 * std::mem::size_of::<&[u64]>()).unwrap();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let rows = exchange.entities_any(&ctx, &["A"]).unwrap();
    let ids = rows.map(|row| row.unwrap().0).collect::<Vec<_>>();
    assert_eq!(ids, [1, 2]);
    let ids = exchange.matching_entity_ids(&ctx, |name| name == "A").unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(ids, [1, 2]);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn multiple_index_lists_release_the_union_backing_after_replay() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::try_from(4 * std::mem::size_of::<u64>()).unwrap();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = [1, 3];
    let second = [2, 3];
    for _ in 0..2 {
        let mut ids = EntityIndex::ordered_ids(&[&first, &second], &ctx).unwrap();
        assert!(matches!(&ids, EntityIds::Scoped { .. }));
        assert_eq!(ids.len(), 3);
        assert_eq!(ids.next(), Some(1));
        assert_eq!(ids.next(), Some(2));
        assert_eq!(ids.next(), Some(3));
        assert_eq!(ids.next(), None);
        drop(ids);
    }
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}
