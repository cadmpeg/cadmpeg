// SPDX-License-Identifier: Apache-2.0

use super::super::RadiusAgreement;
use crate::feature::definitions::{
    FeatureVariableRow, FeatureVariableTable, ScalarLane, VariableType,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn radius_table() -> FeatureVariableTable {
    FeatureVariableTable {
        declared_count: 1,
        entity_ref: None,
        rows: vec![FeatureVariableRow {
            variable_type: VariableType::Radius,
            key: 7,
            value: ScalarLane::Value(2.0),
            value_body: Vec::new(),
            guess: ScalarLane::Undefined,
            guess_body: Vec::new(),
            known: None,
            homogeneity: None,
            uvar_id: None,
            offset: 0,
        }],
        offset: 0,
    }
}

fn radius_backing() -> u64 {
    // Core hash_storage_bytes admits four initial buckets and their control bytes.
    type Entry = (u32, RadiusAgreement);
    u64::try_from(
        4 * std::mem::size_of::<Entry>() + std::mem::align_of::<Entry>().max(16) - 1 + 4 + 16,
    )
    .expect("fixed radius table bound")
}

fn with_limits<T>(materialized: u64, run: impl FnOnce(DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    run(ctx)
}

#[test]
fn checked_trim_cache_radius_constructor_has_exact_backing_boundary() {
    let table = radius_table();
    let backing = radius_backing();
    crate::test_support::assert_refusal_order(
        ResourceDimension::MaterializedBytes,
        &["creo trim radius groups"],
        |cap| {
            with_limits(cap, |ctx| {
                let refusal = match table.reconciled_trim_geometry(&ctx) {
                    Ok(cache) => {
                        assert_eq!(cap, backing);
                        assert_eq!(cache.radius(7).expect("radius").expect("stored").get(), 2.0);
                        assert!(cache.coordinates.points.is_empty());
                        assert!(cache.coordinates.ambiguous.is_empty());
                        drop(cache);
                        drop(
                            ctx.reserve_scoped(cap, "trim cache cleanup")
                                .expect("backing refunded"),
                        );
                        None
                    }
                    Err(CodecError::ResourceLimit(original)) => {
                        assert!(cap < backing);
                        assert_eq!(
                            (
                                original.dimension,
                                original.used,
                                original.additional,
                                original.limit
                            ),
                            (ResourceDimension::MaterializedBytes, 0, backing, cap)
                        );
                        assert_eq!(original.operation, "creo trim radius groups");
                        for _ in 0..2 {
                            assert!(matches!(table.reconciled_trim_geometry(&ctx),
                            Err(CodecError::ResourceLimit(actual)) if actual == original));
                        }
                        Some(original)
                    }
                    Err(error) => panic!("unexpected cache error: {error:?}"),
                };
                if let Some(original) = refusal {
                    assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
                    Err(CodecError::ResourceLimit(original))
                } else {
                    ctx.finish_session().expect("active zero-retained session");
                    Ok(())
                }
            })
        },
    );
}

#[test]
fn checked_trim_cache_keeps_backing_live_until_drop() {
    let table = radius_table();
    let backing = radius_backing();
    with_limits(backing, |ctx| {
        let cache = table.reconciled_trim_geometry(&ctx).expect("cache");
        let CodecError::ResourceLimit(original) = ctx
            .reserve_scoped(1, "live trim cache")
            .expect_err("cache backing remains live")
        else {
            panic!("resource refusal");
        };
        assert_eq!(
            (
                original.dimension,
                original.used,
                original.additional,
                original.limit
            ),
            (ResourceDimension::MaterializedBytes, backing, 1, backing)
        );
        assert_eq!(cache.radius(7).expect("radius").expect("stored").get(), 2.0);
        drop(cache);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    });
}

#[test]
fn checked_trim_cache_outlives_actual_parent_without_refunding_its_backing() {
    const PARENT_BYTES: u64 = 37;
    let table = radius_table();
    let backing = radius_backing();
    with_limits(PARENT_BYTES + backing, |ctx| {
        let mut parent = ctx.reserve_scoped(0, "trim cache parent").expect("parent");
        let parent_value = parent
            .with_storage(|| ctx.copy_retained(&[0; 37], "actual parent bytes"))
            .expect("parent value");
        let cache = parent
            .with_storage(|| table.reconciled_trim_geometry(&ctx))
            .expect("child cache");
        assert_eq!(parent_value.len(), 37);
        drop(parent_value);
        drop(parent);
        let rest = ctx
            .reserve_scoped(PARENT_BYTES, "parent refunded independently")
            .expect("remaining allowance");
        let CodecError::ResourceLimit(original) = ctx
            .reserve_scoped(1, "surviving trim cache")
            .expect_err("child still holds its backing")
        else {
            panic!("resource refusal");
        };
        assert_eq!(
            (original.used, original.additional, original.limit),
            (PARENT_BYTES + backing, 1, PARENT_BYTES + backing)
        );
        assert_eq!(cache.radius(7).expect("radius").expect("stored").get(), 2.0);
        drop(cache);
        drop(rest);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    });
}

#[test]
fn empty_checked_trim_cache_keeps_original_refusal_and_uses_no_backing() {
    let mut table = radius_table();
    table.rows.clear();
    table.declared_count = 0;
    with_limits(0, |ctx| {
        let cache = table.reconciled_trim_geometry(&ctx).expect("empty cache");
        assert!(cache.coordinates.points.is_empty());
        assert!(cache.coordinates.ambiguous.is_empty());
        assert!(cache.radius(7).expect("absent radius").is_none());
        drop(cache);
        let original = ctx
            .charge_retained_limit(1, "before empty trim cache")
            .expect_err("zero retained budget");
        for _ in 0..2 {
            assert!(matches!(table.reconciled_trim_geometry(&ctx),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    });
}

#[test]
fn empty_checked_trim_cache_accepts_zero_limits_and_keeps_work_refusal() {
    let mut table = radius_table();
    table.rows.clear();
    table.declared_count = 0;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let cache = table.reconciled_trim_geometry(&ctx).expect("empty cache");
    assert!(cache.coordinates.points.is_empty());
    assert!(cache.coordinates.ambiguous.is_empty());
    assert!(cache.radius(7).expect("absent radius").is_none());
    drop(cache);
    let original = ctx
        .charge_work_limit(1, "before empty trim cache")
        .expect_err("zero work budget");
    assert_eq!(
        (
            original.dimension,
            original.used,
            original.additional,
            original.limit
        ),
        (ResourceDimension::WorkUnits, 0, 1, 0)
    );
    for _ in 0..2 {
        assert!(matches!(table.reconciled_trim_geometry(&ctx),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
