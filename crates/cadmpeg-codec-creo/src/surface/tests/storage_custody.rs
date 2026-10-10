// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    append_surface_contour_chain, BoundaryType, SurfaceContourRecord, SurfaceKind,
    SurfaceNamedParameter, SurfaceNamedValue, SurfacePrototypeFamily, SurfacePrototypeRecord,
    SurfaceRow,
};

fn prototype(count: usize) -> SurfacePrototypeRecord {
    SurfacePrototypeRecord::new_for_test(
        SurfacePrototypeFamily::Plane,
        (0..count)
            .map(|index| SurfaceNamedParameter {
                name: "radius".to_owned(),
                value: SurfaceNamedValue::Empty,
                body: vec![0xe4],
                offset: index * 3,
                value_offset: index * 3 + 2,
            })
            .collect(),
        0,
    )
}

#[test]
fn prototype_relocation_refuses_before_mutation_and_preserves_parameter_identity() {
    for count in [0, 1, 7, 17] {
        let record = prototype(count);
        let unchanged = record.parameters().to_vec();
        let mut relocated = record.clone();
        let expected = super::work_output(|ctx| {
            let mut record = record.clone();
            record.relocate_parameters(ctx, 19)?;
            Ok(record)
        });
        for (before, after) in unchanged.iter().zip(expected.parameters()) {
            assert_eq!(after.offset, before.offset + 19);
            assert_eq!(after.value_offset, before.value_offset + 19);
            assert_eq!(after.name, before.name);
            assert_eq!(after.body, before.body);
            assert_eq!(after.value, before.value);
        }
        if count == 0 {
            continue;
        }
        let cap = crate::test_support::allocation_limit_at(
            ResourceDimension::WorkUnits,
            Some("creo record child relocation traversal"),
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                record.clone().relocate_parameters(&ctx, 19)
            },
        );
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let Err(CodecError::ResourceLimit(original)) = relocated.relocate_parameters(&ctx, 19)
        else {
            panic!("relocation refuses before mutation");
        };
        assert_eq!(relocated.parameters(), unchanged);
        assert!(matches!(relocated.relocate_parameters(&ctx, 19),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(relocated.parameters(), unchanged);
    }
}

fn row() -> SurfaceRow {
    SurfaceRow {
        id: 7,
        kind: SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

#[test]
fn accepted_contour_chains_retain_only_aggregate_backing_and_bodies() {
    const BODY: [u8; 8] = [0x82, 0x10, 1, 0x0f, 0xe4, 0x0f, 0xe4, 0xe1];
    for scoped in [false, true] {
        for dimension in [
            ResourceDimension::MaterializedBytes,
            ResourceDimension::RetainedBytes,
        ] {
            let arena = DecodeArena::new();
            let policy = DecodePolicy::service();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let build = || {
                let mut records = Vec::new();
                for _ in 0..7 {
                    assert!(append_surface_contour_chain(
                        &ctx,
                        &BODY,
                        0,
                        BODY.len(),
                        &row(),
                        &ScalarCache::default(),
                        &mut records
                    )?);
                }
                Ok::<_, CodecError>(records)
            };
            let (records, storage) = if scoped {
                let (records, storage) = ctx
                    .with_scoped_storage("contour custody parent", build)
                    .expect("scoped accepted chains");
                (records, Some(storage))
            } else {
                (build().expect("accepted chains"), None)
            };
            assert_eq!(records.len(), 7);
            for record in &records {
                assert_eq!(record.surface_id, 7);
                assert_eq!(record.chain_index, 0);
                assert_eq!(record.body, BODY);
            }
            let bytes = u64::try_from(
                records.capacity() * std::mem::size_of::<SurfaceContourRecord>()
                    + records
                        .iter()
                        .map(|record| record.body.capacity())
                        .sum::<usize>(),
            )
            .expect("actual aggregate backing");
            let refusal = match dimension {
                ResourceDimension::MaterializedBytes => ctx
                    .reserve_scoped_limit(
                        policy.limits.max_materialized_bytes + 1,
                        "after contour custody",
                    )
                    .expect_err("probe live materialization"),
                ResourceDimension::RetainedBytes => ctx
                    .charge_retained_limit(
                        policy.limits.max_retained_bytes + 1,
                        "after contour custody",
                    )
                    .expect_err("probe retained bytes"),
                _ => unreachable!("two storage dimensions"),
            };
            assert_eq!(refusal.dimension, dimension);
            assert_eq!(
                refusal.used,
                if scoped == (dimension == ResourceDimension::MaterializedBytes) {
                    bytes
                } else {
                    0
                }
            );
            drop(records);
            drop(storage);
        }
    }
}

#[test]
fn rejected_contour_chains_leave_aggregate_and_storage_unchanged() {
    const BODY: [u8; 8] = [0x82, 0x10, 1, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut records = Vec::new();
    for _ in 0..7 {
        assert!(!append_surface_contour_chain(
            &ctx,
            &BODY,
            0,
            BODY.len(),
            &row(),
            &ScalarCache::default(),
            &mut records
        )
        .expect("incomplete chain"));
    }
    assert!(records.is_empty());
    let refusal = ctx
        .reserve_scoped_limit(
            policy.limits.max_materialized_bytes + 1,
            "after rejected contour",
        )
        .expect_err("probe live scratch");
    assert_eq!(refusal.used, 0);
}
