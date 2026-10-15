// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn subshell_ancestry_memoizes_long_and_cyclic_owner_paths() {
    let count = 4096;
    for cyclic in [false, true] {
        let mut records = vec![ref_record(0, "shell", &[])];
        for index in 1..=count {
            let owner = if index == count {
                usize::from(cyclic)
            } else {
                index + 1
            };
            records.push(ref_record(
                index,
                "subshell",
                &[-1, -1, -1, i64::try_from(owner).unwrap()],
            ));
        }
        let by_index = records
            .iter()
            .map(|record| (i64::try_from(record.index).unwrap(), record))
            .collect();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 256 * u64::try_from(count).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let roots = super::super::subshell_ancestor_shells(&ctx, &records, &by_index).unwrap();
        if cyclic {
            assert!(roots.is_empty());
        } else {
            assert_eq!(roots.len(), count);
            assert!(roots.values().all(|root| *root == 0));
        }
        ctx.finish_session().unwrap();
    }
}

#[test]
fn mesh_sentinel_membership_keeps_source_order_with_linear_work() {
    let count = 4096;
    let mut records = Vec::new();
    for index in 0..count {
        records.push(ref_record(index, "mesh_surface", &[]));
    }
    for index in 0..count {
        for duplicate in 0..2 {
            records.push(ref_record(
                count + 2 * index + duplicate,
                "face",
                &[-1, -1, -1, -1, -1, -1, -1, i64::try_from(index).unwrap()],
            ));
        }
    }
    let by_index = records
        .iter()
        .map(|record| (i64::try_from(record.index).unwrap(), record))
        .collect();
    let table = nurbs::toks::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 256 * u64::try_from(records.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = AsmBrep::default();
    let mut carriers = Carriers::default();
    let mut reach = Reachable::default();
    let mut storage = ctx.reserve_scoped(0, "test carrier scratch").unwrap();
    super::super::keep_faces_and_carriers(
        TopologyContext {
            ctx: &ctx,
            by_index: &by_index,
            token_table: &table,
            purpose: DecodePurpose::Model,
            format: crate::asm_format!("sat"),
        },
        &mut out,
        &records,
        &mut carriers,
        &mut reach,
        &mut storage,
    )
    .unwrap();
    assert_eq!(out.mesh_surface_sentinels.len(), count);
    assert_eq!(out.stats.mesh_surface_faces, 2 * count);
    for (index, sentinel) in out.mesh_surface_sentinels.iter().enumerate() {
        assert_eq!(sentinel.record_index, u32::try_from(index).unwrap());
    }
    drop((carriers, reach));
    drop(storage);
    ctx.finish_session().unwrap();
}
