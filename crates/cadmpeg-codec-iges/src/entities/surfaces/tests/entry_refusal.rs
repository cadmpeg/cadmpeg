// SPDX-License-Identifier: Apache-2.0

use super::super::interval_certified_linear_bezier;
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

fn rail(degree: u32, knots: Vec<f64>, poles: Vec<Point3>) -> NurbsCurve {
    crate::test_support::with_service_context(&[], |ctx| {
        NurbsCurve::from_lanes(ctx, degree, knots, poles, None, false)
            .expect("fixture admission")
            .expect("valid fixture")
    })
}

fn global() -> ProjectedGlobal {
    let bytes = crate::test_support::test_owned::owned_test_file(&[]);
    crate::test_support::with_service_context(&bytes, |ctx| {
        let scan = crate::card::scan_with_context(&bytes, ctx).expect("fixture cards");
        let (global, _, _storage) = crate::global::parse(&scan, ctx).expect("fixture global");
        global.length_context().expect("fixture length context")
    })
}

fn cubic_interval_inputs(adjacent_only: bool) -> (NurbsCurve, ParameterRecord) {
    use crate::parameter::{Token, TokenValue};
    let middle = if adjacent_only { 1.000_006 } else { 1.0 };
    let curve = rail(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        [0.0, middle, 2.0, 3.0]
            .map(|x| Point3::new(x, 0.0, 0.0))
            .to_vec(),
    );
    let mut values = [126, 3, 3, 0, 0, 1, 0].map(TokenValue::Integer).to_vec();
    values.extend([0, 0, 0, 0, 1, 1, 1, 1].map(TokenValue::Integer));
    values.extend([1, 1, 1, 1].map(TokenValue::Integer));
    for (control, x) in [0, 1, 2, 3].into_iter().enumerate() {
        values.push(if adjacent_only && (control == 1 || control == 2) {
            TokenValue::real(if control == 1 { middle } else { 2.0 })
        } else {
            TokenValue::Integer(x)
        });
        values.extend([TokenValue::Integer(0), TokenValue::Integer(0)]);
    }
    values.extend([TokenValue::Integer(0), TokenValue::Integer(1)]);
    let count = values.len();
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect();
    (
        curve,
        ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), count, tokens, Vec::new()),
    )
}

#[test]
fn cubic_interval_proof_rejects_an_adjacent_only_false_certificate() {
    use super::super::DeclaredInterval;
    let (curve, record) = cubic_interval_inputs(true);
    let global = global();
    assert_eq!(global.real_precision().single_significance, 6);
    // Integer endpoints are exact; the two real middle poles have sender significance.
    // Adjacent differences can share a slope that conflicts with the endpoint slope.
    let intervals = [19, 22, 25, 28].map(|index| {
        let value = record.number(index).unwrap();
        DeclaredInterval::around(
            value,
            record.number_uncertainty(index, value, global.real_precision()),
        )
    });
    let mut lower = f64::NEG_INFINITY;
    let mut upper = f64::INFINITY;
    for pair in intervals.windows(2) {
        lower = lower.max(pair[1].lower_bound() - pair[0].upper_bound());
        upper = upper.min(pair[1].upper_bound() - pair[0].lower_bound());
    }
    assert!(lower <= upper);
    assert!(lower > 1.0);
    assert_eq!(intervals[3].lower_bound() - intervals[0].upper_bound(), 3.0);
    crate::test_support::with_service_context(&[], |ctx| {
        assert!(!interval_certified_linear_bezier(&curve, &record, &global, ctx).unwrap());
    });
}

#[test]
fn cubic_interval_proof_accepts_exact_scratch_and_releases_it() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (curve, record) = cubic_interval_inputs(false);
    let global = global();
    let bytes =
        u64::try_from(4 * std::mem::size_of::<[super::super::DeclaredInterval; 3]>()).unwrap();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 4;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(interval_certified_linear_bezier(&curve, &record, &global, &ctx).unwrap());
    // The complete returned bool retains no interval-vector backing.
    let storage = ctx
        .reserve_scoped(bytes, "test interval scratch released")
        .unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn cubic_interval_proof_refuses_one_short_before_source_controls_and_fuses() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let (curve, record) = cubic_interval_inputs(false);
    let global = global();
    let bytes =
        u64::try_from(4 * std::mem::size_of::<[super::super::DeclaredInterval; 3]>()).unwrap();
    for (dimension, limit, additional) in [
        (ResourceDimension::MaterializedBytes, bytes - 1, bytes),
        (ResourceDimension::CollectionItems, 3, 4),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = limit,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            _ => unreachable!("interval-vector dimensions"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(first)) =
            interval_certified_linear_bezier(&curve, &record, &global, &ctx)
        else {
            panic!("scratch refuses");
        };
        assert_eq!(first.dimension, dimension);
        assert_eq!(first.operation, "iges ruled linear interval controls");
        assert_eq!(
            (first.limit, first.used, first.additional),
            (limit, 0, additional)
        );
        for _ in 0..64 {
            assert!(
                matches!(interval_certified_linear_bezier(&curve, &record, &global, &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first)
            );
        }
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
}
