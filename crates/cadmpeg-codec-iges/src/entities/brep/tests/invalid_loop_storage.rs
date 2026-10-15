// SPDX-License-Identifier: Apache-2.0

use crate::directory::SourceStatus;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::BTreeMap;
use std::mem::{align_of, size_of};

#[test]
fn invalid_loop_releases_nested_pcurves_before_its_diagnostic() {
    let mut vertex = crate::test_support::directory_target(1, 502);
    vertex.form = 1;
    let mut pcurve = crate::test_support::directory_target(3, 110);
    pcurve.status = SourceStatus::from_codes([0, 0, 5, 0]);
    let mut loop_entry = crate::test_support::directory_target(5, 508);
    loop_entry.form = 1;
    let directory = [vertex, pcurve, loop_entry];
    let record = |sequence, values: &[i64]| ParameterRecord::from_test_tokens(
        sequence, 1..2, Vec::new(), values.len(),
        values.iter().map(|value| Token {
            value: TokenValue::Integer(*value), span: 0..0,
        }).collect(), Vec::new(),
    );
    let vertex_record = record(1, &[502, 1, 0, 0, 0]);
    // Two vertex uses each name one parametric pcurve. The second orientation is invalid.
    let loop_record = record(5, &[508, 2, 1, 1, 1, 0, 1, 0, 3, 1, 1, 1, 0, 1, 2, 3]);
    let entries: BTreeMap<_, _> = directory.iter().map(|entry| (entry.sequence, entry)).collect();
    let records = BTreeMap::from([(1, &vertex_record), (5, &loop_record)]);
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    let global = crate::test_support::with_service_context(&bytes, |setup| {
        let scan = crate::card::scan_with_context(&bytes, setup).unwrap();
        let (global, _, _) = crate::global::parse(&scan, setup).unwrap();
        global.length_context().unwrap()
    });
    let node_bytes = 11 * (size_of::<u32>() + size_of::<Vec<Point3>>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<u32>().max(align_of::<Vec<Point3>>()).max(align_of::<usize>());
    let definition_bytes = size_of::<Point3>() + node_bytes;
    let record_bytes = 2 * size_of::<super::super::LoopUse>() + 2 * size_of::<(bool, u32)>();
    // The first loss-slot growth uses the core amortized minimum for this element size.
    let loss_capacity = match size_of::<LossNote>() { 1 => 8, 2..=1024 => 4, _ => 1 };
    let diagnostic_bytes = loss_capacity * size_of::<LossNote>();
    assert!(record_bytes < diagnostic_bytes);
    let peak = u64_from_index(definition_bytes + diagnostic_bytes);
    for cap in [peak - 1, peak] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let mut sequences = super::super::super::geometry::SourceSequences::default();
        let result = super::super::project(
            &mut ir, &directory, (&entries, &records), &global, &ctx, &mut sequences,
        );
        if cap < peak {
            let first = match result.as_ref() {
                Err(CodecError::ResourceLimit(first)) => *first,
                _ => panic!("expected invalid-loop diagnostic-slot refusal"),
            };
            drop(result);
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(first.operation, "iges entity loss slots");
            assert_eq!((first.limit, first.used, first.additional),
                (cap, u64_from_index(definition_bytes), u64_from_index(diagnostic_bytes)));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        } else {
            let outcome = result.unwrap();
            assert!(outcome.decoded.is_empty());
            assert_eq!(outcome.losses.len(), 1);
            assert_eq!(outcome.losses[0].message,
                "IGES entity type 508 form 1 was not projected: loop edge-use tuple is invalid");
            assert!(ir.model.bodies.is_empty());
            drop(outcome);
            let released = ctx.reserve_scoped(peak, "invalid-loop scratch released").unwrap();
            drop(released);
            ctx.finish_session().unwrap();
        }
    }
}
