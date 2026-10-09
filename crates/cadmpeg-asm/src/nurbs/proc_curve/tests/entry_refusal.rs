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
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("actual identifier refusal"); };
                assert_eq!(first.operation, "ASM trailing surface bounds prefix");
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                for replay in [&[][..], tokens.as_slice()] {
                    assert!(matches!(super::super::record_trailing_surface_bounds(&ctx, replay),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn entry_canonical_support_chart_preserves_original_refusal_and_pcurve() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut curve = pcurve(); let before = curve.clone();
        let result = super::super::normalize_support_pcurve(ctx, super::super::NativeSupportChart::Canonical, &mut curve);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(curve, before);
        } else {
            result.unwrap().unwrap(); assert_eq!(curve, before);
        }
    });
}

#[test]
fn entry_law_surface_null_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("null_surface".into())]; let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::super::nullable_law_surface(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null)))); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_optional_surface_null_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("null_surface".into())]; let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::super::optional_embedded_surface(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null)))); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_optional_pcurve_null_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("nullbs".into())]; let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::super::optional_pcurve(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null)))); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_analytic_surface_missing_position_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("plane".into())]; let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::super::embedded_surface_fields(ctx, &mut cur, false);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(result.is_none()); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_cache_context_invalid_revision_or_enum_preserves_original_refusal_and_cursor() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let zero = [crate::sab::Token::Long(0)];
        let unknown = [crate::sab::Token::Long(1), crate::sab::Token::Enum(3)];
        for (tokens, end) in [(&[][..], 0), (zero.as_slice(), 1), (unknown.as_slice(), 2)] {
            let mut cur = crate::nurbs::toks::Cur::at(tokens, 0);
            let result = super::super::cache_first_curve_context(ctx, &mut cur, &table);
            if let Some(first) = &original {
                assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if &last == first));
                assert_eq!(cur.pos(), 0);
            } else { assert!(result.is_none()); assert_eq!(cur.pos(), end); }
        }
    });
}

#[test]
fn entry_surface_bounds_null_preserves_original_refusal_and_cursor() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("null_surface".into())];
        let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::super::optional_embedded_surface_with_bounds(ctx, &mut cur, &table);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            let value = result.unwrap().unwrap();
            assert!(value.surface.is_none()); assert_eq!(value.bounds, [None; 4]); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_intersection_cache_invalid_revision_or_enum_preserves_original_refusal() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    let solved = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)],
        None, false,
    ).unwrap().unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let zero = [crate::sab::Token::Long(0)];
        let unknown = [crate::sab::Token::Long(1), crate::sab::Token::Enum(3)];
        for tokens in [&[][..], zero.as_slice(), unknown.as_slice()] {
            let result = super::super::cache_first_intersection(ctx, tokens, 0, &solved, &table);
            if let Some(first) = original {
                assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            } else { assert!(result.is_none()); }
        }
    });
}
