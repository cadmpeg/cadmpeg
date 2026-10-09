// SPDX-License-Identifier: Apache-2.0
//! Original refusal through callbacks and cached topology ownership.

use crate::history::{
    unique_entry_by, AsmHistoricalTopology, BodySelectionIndex, FeatureBodySelectionInputs,
    RecentTopologyValue, UniqueIndex,
};
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn unique_predicate_cannot_hide_original_refusal_with_success_or_error() {
    for outcome in [0, 1, 2] {
        crate::test_support::with_decode_policy(&policy(1), |ctx| {
            let result = unique_entry_by(ctx, &[7_u64], |_| {
                let _ = ctx.charge_work(1, "original predicate refusal");
                match outcome {
                    0 => Ok(false),
                    1 => Ok(true),
                    _ => Err(CodecError::Malformed("later predicate error".into())),
                }
            }, "reached predicate source");
            let Err(CodecError::ResourceLimit(actual)) = result else {
                panic!("predicate must keep original refusal");
            };
            assert_eq!(Some(actual.clone()), ctx.resource_refusal());
            assert_eq!(actual.operation, "original predicate refusal");
            assert_eq!(actual.dimension, ResourceDimension::WorkUnits);
            assert_eq!((actual.used, actual.additional), (1, 1));
        });
    }
}

fn fused_optional_key(ctx: &DecodeContext<'_>, _: &u64) -> Result<Option<u64>, CodecError> {
    let _ = ctx.charge_work(1, "original key refusal");
    Ok(None)
}

fn fused_error_key(ctx: &DecodeContext<'_>, _: &u64) -> Result<Option<u64>, CodecError> {
    let _ = ctx.charge_work(1, "original key refusal");
    Err(CodecError::Malformed("later key error".into()))
}

#[test]
fn key_callback_refusal_does_not_install_an_empty_cache_or_mask_error() {
    for key in [fused_optional_key, fused_error_key] {
        crate::test_support::with_decode_policy(&policy(1), |ctx| {
            let source = [7_u64];
            let index = UniqueIndex::new(&source, key, "reached key source");
            let Err(CodecError::ResourceLimit(actual)) = index.get(ctx, &7) else {
                panic!("key callback must keep original refusal");
            };
            assert_eq!(Some(actual.clone()), ctx.resource_refusal());
            assert_eq!(actual.operation, "original key refusal");
            assert_eq!((actual.used, actual.additional), (1, 1));
            assert!(index.table.get().is_none());
        });
    }
}

#[test]
fn cached_body_indexes_keep_original_refusal_without_evicting_data() {
    let inputs = FeatureBodySelectionInputs {
        scopes: &[], groups: &[], body_recipe_operands: &[], construction_recipes: &[],
        persistent_design_links: &[], histories: &[], bodies: &[], regions: &[], shells: &[],
    };
    crate::test_support::with_decode_context(|ctx| {
        let index = BodySelectionIndex::new(&inputs);
        let external = std::ptr::from_ref(index.external(ctx).unwrap());
        let links = std::ptr::from_ref(index.links(ctx).unwrap());
        assert_eq!(std::ptr::from_ref(index.external(ctx).unwrap()), external);
        assert_eq!(std::ptr::from_ref(index.links(ctx).unwrap()), links);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(u64::MAX, "original cached index refusal") else {
            panic!("original cached index refusal");
        };
        let Err(CodecError::ResourceLimit(actual)) = index.external(ctx) else {
            panic!("external cache must keep original refusal");
        };
        assert_eq!(actual, original);
        let Err(CodecError::ResourceLimit(actual)) = index.links(ctx) else {
            panic!("links cache must keep original refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(std::ptr::from_ref(index.external.get().unwrap()), external);
        assert_eq!(std::ptr::from_ref(index.links.get().unwrap()), links);
    });
}

#[test]
fn recent_topology_reuses_value_for_free_and_keeps_it_on_original_refusal() {
    let topology = AsmHistoricalTopology::default();
    crate::test_support::with_decode_policy(&policy(0), |ctx| {
        let mut cache = RecentTopologyValue::default();
        assert_eq!(*cache.of(ctx, &topology, || Ok(7_u64)).unwrap(), 7);
        assert_eq!(*cache.of(ctx, &topology, || panic!("cached builder must not run")).unwrap(), 7);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original recent topology refusal") else {
            panic!("original recent topology refusal");
        };
        let Err(CodecError::ResourceLimit(actual)) = cache.of(ctx, &topology, || panic!("fused builder must not run")) else {
            panic!("cached value must keep original refusal");
        };
        assert_eq!(actual, original);
        assert_eq!(cache.value, Some(7));
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
        assert_eq!(cache.of(ctx, &first, || ctx.with_scoped_storage("actual cached byte", || {
            ctx.copy_slice(&[42_u8], "copy actual cached byte")
        })).unwrap().0, [42]);
        assert_eq!(cache.of(ctx, &second, || ctx.with_scoped_storage("actual replacement byte", || {
            ctx.copy_slice(&[43_u8], "copy actual replacement byte")
        })).unwrap().0, [43]);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn recent_topology_builder_cannot_install_after_refusal_or_mask_error() {
    let topology = AsmHistoricalTopology::default();
    for ordinary_error in [false, true] {
        crate::test_support::with_decode_policy(&policy(0), |ctx| {
            let mut cache = RecentTopologyValue::default();
            let result = cache.of(ctx, &topology, || {
                let _ = ctx.charge_work(1, "original builder refusal");
                if ordinary_error {
                    Err(CodecError::Malformed("later builder error".into()))
                } else {
                    Ok(7_u64)
                }
            });
            let Err(CodecError::ResourceLimit(actual)) = result else {
                panic!("builder must keep original refusal");
            };
            assert_eq!(Some(actual.clone()), ctx.resource_refusal());
            assert_eq!(actual.operation, "original builder refusal");
            assert_eq!((actual.used, actual.additional), (0, 1));
            assert_eq!(cache.value, None);
        });
    }
}

struct FuseOnDrop<'ctx, 'arena>(&'ctx DecodeContext<'arena>);

impl Drop for FuseOnDrop<'_, '_> {
    fn drop(&mut self) {
        let _ = self.0.charge_work(1, "original old value drop refusal");
    }
}

#[test]
fn recent_topology_observes_refusal_from_old_drop_before_replacement_callback() {
    let first = AsmHistoricalTopology::default();
    let second = AsmHistoricalTopology::default();
    crate::test_support::with_decode_policy(&policy(0), |ctx| {
        let mut cache = RecentTopologyValue::default();
        let _ = cache.of(ctx, &first, || Ok(FuseOnDrop(ctx))).unwrap();
        let Err(CodecError::ResourceLimit(actual)) = cache.of(ctx, &second, || panic!("refused replacement builder must not run")) else {
            panic!("old drop refusal must stop replacement");
        };
        assert_eq!(Some(actual.clone()), ctx.resource_refusal());
        assert_eq!(actual.operation, "original old value drop refusal");
        assert_eq!((actual.used, actual.additional), (0, 1));
        assert!(cache.value.is_none());
    });
}
