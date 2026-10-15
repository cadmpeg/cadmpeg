// SPDX-License-Identifier: Apache-2.0

use super::super::{normalize_pcurve_for_surface_record, pcurve_for_selector_with_chart};
use crate::nurbs::toks::SubtypeTable;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
use cadmpeg_ir::math::Point2;

fn pcurve() -> PcurveNurbs {
    PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point2::new(2.0, 3.0), Point2::new(5.0, 7.0)],
        None,
        false,
    )
    .unwrap()
    .unwrap()
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
        assert!(matches!(
            normalize_pcurve_for_surface_record(&ctx, head, &[], &mut curve),
            Some(Ok(()))
        ));
        assert_eq!(curve, original);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn invalid_pcurve_selector_executes_no_input_work_or_allocation() {
    let table =
        SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
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

#[test]
fn manual_surface_bounds_prefix_stops_at_the_last_actual_identifier() {
    for count in [0_usize, 1, 64] {
        let tokens = vec![crate::sab::Token::Ident("x".into()); count];
        let required = u64::try_from(count).unwrap();
        for cap in [0, required.saturating_sub(1), required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_collection_items = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::super::record_trailing_surface_bounds(&ctx, &tokens);
            if cap == required {
                assert!(result.unwrap().is_none());
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else {
                    panic!("actual identifier refusal");
                };
                assert_eq!(first.operation, "ASM trailing surface bounds prefix");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                assert!(
                    matches!(super::super::record_trailing_surface_bounds(&ctx, tokens.as_slice()),
                        Err(CodecError::ResourceLimit(last)) if last == first)
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
                );
            }
        }
    }
}

#[test]
fn embedded_surface_propagates_knot_refusal_before_missing_bounds() {
    use crate::nurbs::toks::Cur;
    use crate::sab::Token;
    use cadmpeg_core::decode::ResourceDimension;
    let table =
        SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    let tokens = [
        Token::Ident("spline".into()),
        Token::Ident("nubs".into()),
        Token::Long(1),
        Token::Long(1),
        Token::Enum(0),
        Token::Enum(0),
        Token::Enum(0),
        Token::Enum(0),
        Token::Long(2),
        Token::Long(2),
    ];
    // The fixed surface header reaches a two-entry knot table. The first
    // knot read needs one work unit; no knot payload or bound is present.
    for cap in [0, 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::optional_embedded_surface_with_bounds(&ctx, &mut cur, &table);
        if cap == 0 {
            let Some(Err(CodecError::ResourceLimit(first))) = result else {
                panic!("knot work refusal must precede missing bound recovery");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "scan ASM knot pairs");
            assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
            for _ in 0..64 {
                let mut cur = Cur::at(&tokens, 0);
                assert!(
                    matches!(super::super::optional_embedded_surface_with_bounds(&ctx, &mut cur, &table),
                    Some(Err(CodecError::ResourceLimit(last))) if last == first)
                );
            }
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
            );
        } else {
            assert!(result.is_none());
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn canonical_support_chart_keeps_populated_pcurve_without_allocation() {
    crate::test_support::with_zero_context(|ctx| {
        let mut curve = pcurve();
        let before = curve.clone();
        let result = super::super::normalize_support_pcurve(
            ctx,
            super::super::NativeSupportChart::Canonical,
            &mut curve,
        );

        result.unwrap().unwrap();
        assert_eq!(curve, before);
    });
}
