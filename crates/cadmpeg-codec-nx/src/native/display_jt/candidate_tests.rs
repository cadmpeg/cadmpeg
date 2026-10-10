// SPDX-License-Identifier: Apache-2.0
//! Storage release and visited-prefix admission for rejected JT candidates.

use super::*;
use cadmpeg_core::decode::ResourceDimension;

fn partition_body(name: &str) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&1_u32.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&0_u32.to_le_bytes());
    let units: Vec<_> = name.encode_utf16().collect();
    body.extend_from_slice(&u32::try_from(units.len()).unwrap().to_le_bytes());
    for unit in units {
        body.extend_from_slice(&unit.to_le_bytes());
    }
    for _ in 0..13 {
        body.extend_from_slice(&0_f32.to_le_bytes());
    }
    for _ in 0..6 {
        body.extend_from_slice(&0_i32.to_le_bytes());
    }
    body
}

fn assert_released(decode: impl Fn(&DecodeContext<'_>) -> Result<(), CodecError>) {
    const ALLOWANCE: u64 = 1 << 20;
    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = ALLOWANCE;
            policy.limits.max_materialized_bytes = ALLOWANCE;
        },
        |ctx| {
            decode(ctx).unwrap();
            let scratch = ctx.reserve_scoped(ALLOWANCE, "verify candidate scratch release").unwrap();
            drop(scratch);
            let storage = ctx.vector_storage::<u8>(usize::try_from(ALLOWANCE).unwrap(), "verify candidate retained release").unwrap();
            assert_eq!(storage.capacity(), usize::try_from(ALLOWANCE).unwrap());
        },
    );
}

#[test]
fn partition_candidate_releases_children_and_name_after_tail_rejection() {
    let mut body = partition_body("x");
    crate::test_support::with_decode_context(|ctx| {
        let node = parse_jt9_partition_node_body(ctx, &body).unwrap().unwrap();
        assert_eq!(node.child_object_ids, [7]);
        assert_eq!(node.file_name, "x");
    });
    body.push(1);
    assert_released(|ctx| {
        assert!(parse_jt9_partition_node_body(ctx, &body)?.is_none());
        Ok(())
    });
}

#[test]
fn partition_name_control_rejection_charges_only_the_first_character() {
    let body = partition_body(&format!("\n{}", "x".repeat(128)));
    let operation = "validate DisplayJT partition name";
    let error = crate::test_support::resource_refusal_at(
        &[], ResourceDimension::WorkUnits, operation,
        |ctx| parse_jt9_partition_node_body(ctx, &body).map(|_| ()),
    );
    let CodecError::ResourceLimit(limit) = error else { panic!("work refusal"); };
    assert_eq!(limit.operation, operation);
    assert_eq!(limit.additional, 1);
    crate::test_support::with_decode_context_over(
        &[], |policy| policy.limits.max_work_units = limit.used + 1,
        |ctx| assert!(parse_jt9_partition_node_body(ctx, &body).unwrap().is_none()),
    );
}
