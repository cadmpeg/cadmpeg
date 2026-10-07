// SPDX-License-Identifier: Apache-2.0
//! Charges of the scoped tables that layer metadata parsing builds: the seen
//! layer UUIDs, the per-index occurrence counts and the parent references.

use super::{layer_fixture, retained_limit_context};

/// Admits one layer UUID twice on a fresh context with the given
/// materialized and retained allowances.
fn repeated_layer_uuid(
    materialized: u64,
    retained: u64,
) -> Result<(bool, bool), cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut workspace = ctx.reserve_scoped(0, "Rhino layer UUID workspace")?;
    let mut ids = std::collections::HashSet::new();
    let id = crate::wire::Uuid::from_canonical([0x44; 16]);
    let first = crate::settings::admit_layer_uuid(&ctx, &mut workspace, &mut ids, id)?;
    let second = crate::settings::admit_layer_uuid(&ctx, &mut workspace, &mut ids, id)?;
    Ok((first, second))
}

/// A layer UUID enters the seen table once, as scoped storage: the
/// materialized need of the first admission also admits the repeat, which
/// reports the duplicate, and no retained storage is used.
#[test]
fn layer_uuid_table_is_scoped_and_charges_each_uuid_once() {
    use cadmpeg_core::decode::ResourceDimension;
    let cadmpeg_core::CodecError::ResourceLimit(refusal) =
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::MaterializedBytes,
            "Rhino layer UUID keys",
            |cap| repeated_layer_uuid(cap, 0),
        )
    else {
        panic!("the UUID table refuses with a resource limit");
    };
    assert_eq!(refusal.used, 0, "nothing scoped precedes the first UUID");
    let need = refusal.additional;
    assert!(need > 0);
    assert_eq!(
        repeated_layer_uuid(need, 0).expect("one entry admits the repeat"),
        (true, false)
    );
}

/// Counts indexes given as plain values.
fn occurrences(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    indexes: &[i32],
) -> Result<Vec<(i32, usize)>, cadmpeg_core::CodecError> {
    crate::settings::index_occurrences(ctx, indexes, |index| *index, "Rhino layer index counts")
        .map(|(counts, _workspace)| counts)
}

fn occurrence_context(
    policy: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
    indexes: &[i32],
) -> Result<Vec<(i32, usize)>, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut limits = DecodePolicy::service();
    policy(&mut limits);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &limits).expect("context");
    occurrences(&ctx, indexes)
}

/// Repeated indexes fold into one ascending count each, whatever order the
/// records arrive in.
#[test]
fn layer_index_occurrences_fold_runs_in_ascending_order() {
    assert_eq!(
        occurrence_context(|_| {}, &[3, 1, 3, 2, 1, 3]).expect("counts"),
        [(1, 2), (2, 1), (3, 3)]
    );
    assert_eq!(occurrence_context(|_| {}, &[]).expect("counts"), []);
}

/// The index copy is one scoped vector of one slot per record, charged before
/// any slot is written; nothing is retained.
#[test]
fn layer_index_occurrences_charge_one_scoped_slot_per_record() {
    use cadmpeg_core::decode::ResourceDimension;
    let indexes = [3, 1, 3];
    let slots = cadmpeg_core::decode::u64_from_index(3 * std::mem::size_of::<(i32, usize)>());
    for limit in 0..slots {
        let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = occurrence_context(
            |policy| policy.limits.max_materialized_bytes = limit,
            &indexes,
        ) else {
            panic!("the index copy must refuse at {limit} bytes");
        };
        assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(refusal.operation, "Rhino layer index counts");
        assert_eq!((refusal.used, refusal.additional), (0, slots));
    }
    let counts = occurrence_context(
        |policy| {
            policy.limits.max_materialized_bytes = slots;
            policy.limits.max_retained_bytes = 0;
        },
        &indexes,
    )
    .expect("the slots fit");
    assert_eq!(counts, [(1, 1), (3, 2)]);
}

/// The smallest work allowance that completes the count.
fn occurrence_work(indexes: &[i32]) -> u64 {
    let fits = |limit: u64| {
        occurrence_context(|policy| policy.limits.max_work_units = limit, indexes).is_ok()
    };
    let mut high = 1_u64;
    while !fits(high) {
        high *= 2;
    }
    let mut low = 0_u64;
    while low < high {
        let middle = low + (high - low) / 2;
        if fits(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// Counting costs the same for descending indexes, the order that made a
/// per-record sorted insertion quadratic, as for ascending ones.
#[test]
fn layer_index_occurrence_work_does_not_depend_on_record_order() {
    let ascending: Vec<i32> = (0..512).collect();
    let descending: Vec<i32> = (0..512).rev().collect();
    assert_eq!(occurrence_work(&ascending), occurrence_work(&descending));
}

/// The parent-reference pass starts after the layer UUID set has charged its
/// larger transient bound, so a whole-metadata ladder never meets the parent
/// workspace first; the pass is driven alone over the parsed layers.
#[test]
fn layer_parent_workspace_refuses_materialized_limit() {
    let (data, layer_tables) = layer_fixture(&[0], Some(200_912_010), 1, [0x44; 16], &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let ctx = retained_limit_context(&data, &arena, &policy);
    let metadata = crate::settings::parse_metadata(
        &ctx,
        &data,
        crate::chunks::ArchiveVersion::V8,
        &layer_tables,
        &mut crate::loss::Diagnostics::new(),
    )
    .expect("layer metadata");
    assert_eq!(metadata.layers.len(), 1);
    // The only workspace is the hash table's bucket and control storage.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "Rhino layer parent counts",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
            let ctx = retained_limit_context(&data, &arena, &policy);
            crate::settings::report_layer_parent_references(
                &ctx,
                &metadata.layers,
                &mut crate::loss::Diagnostics::new(),
            )
        },
    );
    let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
        panic!("materialized refusal")
    };
    assert_eq!(refusal.used, 0);
    assert!(refusal.additional > 0);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = refusal.additional;
    policy.limits.max_retained_bytes = 0;
    let ctx = retained_limit_context(&data, &arena, &policy);
    crate::settings::report_layer_parent_references(
        &ctx,
        &metadata.layers,
        &mut crate::loss::Diagnostics::new(),
    )
    .expect("exact table bound admitted");
    ctx.reserve_scoped(refusal.additional, "reclaimed layer parent workspace")
        .expect("workspace released after the pass");
}
