// SPDX-License-Identifier: Apache-2.0
//! One indexed list needs no identifier copy; unions retain only live scratch.

use super::super::{EntityIds, EntityIndex};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

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
    // A borrowed list preserves duplicates in source order.
    assert_eq!(ids.next(), Some(1));
    assert_eq!(ids.next(), Some(1));
    assert_eq!(ids.next(), Some(2));
    assert_eq!(ids.next(), None);
    drop(ids);
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
