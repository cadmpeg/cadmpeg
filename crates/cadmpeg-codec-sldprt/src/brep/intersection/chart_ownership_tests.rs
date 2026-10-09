// SPDX-License-Identifier: Apache-2.0
//! Scoped chart source backing and candidate-shell teardown.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{scoped_chart_records, SourceRecords, ScopedSourceRecords, MISSING_PARAMETER};
use super::chart_publication::ScopedChart;

const POINTS: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.01, 0.01, 0.0]];

fn chart_bytes() -> Vec<u8> {
    let mut bytes = vec![0, 0x28];
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    bytes.extend_from_slice(&4_u16.to_be_bytes());
    bytes.extend_from_slice(&0.0_f64.to_be_bytes());
    bytes.extend_from_slice(&1.0_f64.to_be_bytes());
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    bytes.extend_from_slice(&0.00001_f64.to_be_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
    bytes.extend_from_slice(&MISSING_PARAMETER.to_be_bytes());
    for point in POINTS {
        for value in point { bytes.extend_from_slice(&value.to_be_bytes()); }
    }
    bytes
}

fn source_bytes() -> u64 {
    // The one chart has one exact interior-point slot. push_btree_group adds
    // one node and one exact group slot. The four-slot candidate Vec is gone.
    type Group<'ctx> = Vec<ScopedChart<'ctx>>;
    let alignment = std::mem::align_of::<u16>()
        .max(std::mem::align_of::<Group<'_>>())
        .max(std::mem::align_of::<usize>());
    u64::try_from(11 * (std::mem::size_of::<u16>() + std::mem::size_of::<Group<'_>>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment
        + std::mem::size_of::<ScopedChart<'_>>() + std::mem::size_of::<[f64; 3]>())
        .expect("source chart backing")
}

fn candidate_bytes() -> u64 {
    u64::try_from(4 * std::mem::size_of::<ScopedChart<'_>>()).expect("four candidate slots")
}

#[test]
fn scoped_chart_source_releases_the_consumed_candidate_shell() {
    let bytes = chart_bytes();
    let live = source_bytes();
    let peak = live + candidate_bytes();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let (charts, storage) = ctx.with_scoped_storage("test chart source shell", || scoped_chart_records(&ctx, &bytes))
        .expect("source and candidate shells coexist during publication");
    let source = SourceRecords { value: ScopedSourceRecords {
        charts, terms: BTreeMap::new(), uvs: BTreeMap::new(),
    }, _storage: storage };
    let chart = &source.value.charts[&4][0].value;
    assert_eq!(chart.endpoints, [POINTS[0], POINTS[2]]);
    assert_eq!(chart.interior_points, [POINTS[1]]);
    assert_eq!(chart.base_parameter, 0.0);
    assert_eq!(chart.base_scale, 1.0);
    assert_eq!(chart.chordal_error, 0.00001);
    drop(ctx.reserve_scoped(candidate_bytes(), "test consumed chart candidate shell")
        .expect("candidate backing is released while source points survive"));
    drop(source);
    drop(ctx.reserve_scoped(peak, "test complete chart source teardown").expect("all backing released"));
    ctx.finish_session().expect("no resource refusal");
}

#[test]
fn scoped_chart_source_has_the_exact_surviving_backing_bound() {
    let bytes = chart_bytes();
    let live = source_bytes();
    let peak = live + candidate_bytes();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = peak;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let (charts, storage) = ctx.with_scoped_storage("test chart source shell", || scoped_chart_records(&ctx, &bytes))
        .expect("source charts");
    let source = SourceRecords { value: ScopedSourceRecords {
        charts, terms: BTreeMap::new(), uvs: BTreeMap::new(),
    }, _storage: storage };
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(candidate_bytes() + 1, "test live chart source bound") else {
        panic!("one byte beyond the remaining source capacity");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, live);
    assert_eq!(limit.additional, candidate_bytes() + 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(source);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn scoped_chart_point_storage_is_admitted_before_copy_allocation() {
    let bytes = chart_bytes();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| scoped_chart_records(&ctx, &bytes));
    let Err(CodecError::ResourceLimit(limit)) = &result else { panic!("chart point storage limit"); };
    let limit = *limit;
    drop(result);
    assert_eq!(limit.operation, "decode Parasolid chart interior points");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, u64::try_from(std::mem::size_of::<[f64; 3]>()).expect("one point"));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn scoped_chart_source_preserves_the_original_refusal() {
    let bytes = chart_bytes();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original chart source refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| scoped_chart_records(&ctx, &bytes));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
