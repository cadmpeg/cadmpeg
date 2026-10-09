// SPDX-License-Identifier: Apache-2.0

use super::super::{normalize_pcurve_for_surface_record, pcurve_for_selector_with_chart};
use crate::nurbs::toks::SubtypeTable;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;

fn pcurve() -> PcurveNurbs {
    PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(2.0, 3.0), Point2::new(5.0, 7.0)], None, false,
    ).unwrap().unwrap()
}

#[test]
fn unsupported_surface_chart_preserves_original_entry_refusal_and_pcurve() {
    let mut curve = pcurve();
    let original = curve.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original chart refusal") else {
        panic!("expected original work refusal");
    };
    for head in ["unknown", "", "plane", "cone"] {
        assert!(matches!(normalize_pcurve_for_surface_record(&ctx, head, &[], &mut curve),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
        assert_eq!(curve, original);
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn unsupported_surface_chart_executes_no_input_work_or_allocation() {
    let mut curve = pcurve();
    let original = curve.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for head in ["unknown", ""] {
        assert!(matches!(normalize_pcurve_for_surface_record(&ctx, head, &[], &mut curve), Some(Ok(()))));
        assert_eq!(curve, original);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_pcurve_selector_preserves_original_entry_refusal() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original selector refusal") else {
        panic!("expected original work refusal");
    };
    for selector in [0, 3, -3, i64::MIN, i64::MAX, 1, -1, 2, -2] {
        assert!(matches!(pcurve_for_selector_with_chart(&ctx, &[], selector, &table),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn invalid_pcurve_selector_executes_no_input_work_or_allocation() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for selector in [0, 3, -3, i64::MIN, i64::MAX] {
        assert!(pcurve_for_selector_with_chart(&ctx, &[], selector, &table).is_none());
    }
    ctx.finish_session().unwrap();
}
