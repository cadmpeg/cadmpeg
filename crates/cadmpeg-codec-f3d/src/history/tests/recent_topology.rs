// SPDX-License-Identifier: Apache-2.0
//! Recent topology cache reuse and replacement storage lifetime.

use crate::history::RecentTopologyValue;
use crate::history_records::AsmHistoricalTopology;
use cadmpeg_core::decode::DecodePolicy;

#[test]
fn recent_topology_cache_hit_is_free_and_skips_builder() {
    let topology = AsmHistoricalTopology::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut cache = RecentTopologyValue::default();
        assert_eq!(*cache.of(&topology, || Ok(7_u64)).unwrap(), 7);
        assert_eq!(
            *cache
                .of(&topology, || panic!("cached builder must not run"))
                .unwrap(),
            7
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn recent_topology_destroys_actual_old_backing_before_replacement() {
    let first = AsmHistoricalTopology::default();
    let second = AsmHistoricalTopology::default();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 1;
    policy.limits.max_retained_bytes = 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut cache = RecentTopologyValue::default();
        assert_eq!(
            cache
                .of(&first, || ctx
                    .with_scoped_storage("actual cached byte", || {
                        ctx.copy_slice(&[42_u8], "copy actual cached byte")
                    }))
                .unwrap()
                .0,
            [42]
        );
        assert_eq!(
            cache
                .of(&second, || ctx
                    .with_scoped_storage("actual replacement byte", || {
                        ctx.copy_slice(&[43_u8], "copy actual replacement byte")
                    }))
                .unwrap()
                .0,
            [43]
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}
