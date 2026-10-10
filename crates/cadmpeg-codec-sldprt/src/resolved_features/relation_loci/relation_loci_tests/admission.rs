//! Indexed locus operations bill query work and growth separately.

use super::super::{reserve_profile_locus_map_slot, reserve_profile_locus_set_slot};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::{HashMap, HashSet};

#[test]
fn existing_locus_keys_do_not_bill_the_complete_index() {
    let keys = (0..1000)
        .map(|i| format!("synthetic:native:marker#{i}"))
        .collect::<Vec<_>>();
    let source_bytes = keys
        .iter()
        .map(|key| u64::try_from(key.len()).unwrap())
        .sum();
    let mut map = keys
        .iter()
        .map(|key| (key.as_str(), 7))
        .collect::<HashMap<_, _>>();
    let mut set = keys.iter().map(String::as_str).collect::<HashSet<_>>();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 600_000;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for key in &keys {
        let key_bytes = u64::try_from(key.len()).unwrap();
        assert!(!reserve_profile_locus_map_slot(
            &ctx,
            &mut map,
            &key.as_str(),
            source_bytes,
            key_bytes,
            "lookup locus"
        )
        .unwrap());
        assert!(!reserve_profile_locus_set_slot(
            &ctx,
            &mut set,
            &key.as_str(),
            source_bytes,
            key_bytes,
            "lookup locus"
        )
        .unwrap());
    }
    assert_eq!(map.len(), keys.len());
    assert_eq!(set.len(), keys.len());
    assert!(ctx.resource_refusal().is_none());
}
