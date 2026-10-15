// SPDX-License-Identifier: Apache-2.0

use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use std::collections::BTreeMap;
use std::mem::{align_of, size_of};

#[test]
fn brep_successful_definition_transfers_bytes_without_a_guard_registry() {
    let mut entry = crate::test_support::directory_target(1, 502);
    entry.form = 1;
    let directory = [entry];
    let values = [502, 1, 0, 0, 0];
    let record = ParameterRecord::from_test_tokens(
        1, 1..2, Vec::new(), values.len(),
        values.into_iter().map(|value| Token {
            value: TokenValue::Integer(value), span: 0..0,
        }).collect(), Vec::new(),
    );
    let entries = BTreeMap::from([(1, &directory[0])]);
    let records = BTreeMap::from([(1, &record)]);
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    // One coordinate slot and one u32 -> Vec<Point3> ordered-map node.
    let point_bytes = size_of::<Point3>();
    let node_bytes = 11 * (size_of::<u32>() + size_of::<Vec<Point3>>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<Vec<Point3>>()).max(align_of::<usize>());
    let peak = u64_from_index(point_bytes + node_bytes);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 2;
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(
            &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
        );
        if cap < peak {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected vertex-list node refusal"),
            };
            drop(result);
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges B-rep vertex-list nodes");
            assert_eq!(first.limit, cap);
            assert_eq!(first.used, u64_from_index(point_bytes));
            assert_eq!(first.additional, u64_from_index(node_bytes));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let outcome = result.unwrap();
            assert!(outcome.decoded.is_empty());
            assert!(outcome.losses.is_empty());
            assert!(ir.model.points.is_empty());
            drop(outcome);
            // All definition scratch is dead after project returns.
            let storage = ctx.reserve_scoped(peak, "test released BRep definitions").unwrap();
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}
