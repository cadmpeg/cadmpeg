// SPDX-License-Identifier: Apache-2.0
//! BREP prefix visits, row allocation order and known token exhaustion.

use super::super::{grid_rows, parse_reference_suffix, TokenCursor};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn grid_row_allocation_refuses_before_unmoved_values() {
    for (collection_limit, work_limit, used) in [(3, 0, 2), (5, 2, 4)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = grid_rows(&ctx, vec![1, 2, 3, 4], 2) else {
            panic!("row storage must refuse before its values move")
        };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "FreeCAD B-rep surface row values");
        assert_eq!(original.used, used);
        assert_eq!(original.additional, 2);
        assert!(matches!(grid_rows::<u8>(&ctx, Vec::new(), 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn grid_rows_charge_each_moved_value_once_and_keep_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        grid_rows(&ctx, vec![1, 2, 3, 4], 2).unwrap(),
        [[1, 2], [3, 4]]
    );
}

#[test]
fn empty_and_invalid_grid_dimensions_need_no_work_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(grid_rows::<u8>(&ctx, Vec::new(), 2).unwrap().is_empty());
    assert!(
        matches!(grid_rows(&ctx, vec![1_u8], 2), Err(CodecError::Malformed(message))
        if message == "surface grid dimensions do not match pole count")
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "prior grid refusal") else {
        panic!("work refusal")
    };
    for width in [0, 2] {
        assert!(matches!(grid_rows::<u8>(&ctx, Vec::new(), width),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn invalid_reference_prefix_does_not_scan_or_copy_suffix() {
    for token in ["0a".to_owned(), format!("0{}", "a".repeat(4096))] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = [token.as_str()];
        let mut cursor = TokenCursor::new(&ctx, &tokens);
        assert!(
            matches!(parse_reference_suffix(&mut cursor, "test suffix", 1),
            Err(CodecError::Malformed(message)) if message == "test suffix limit exceeded")
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn reference_prefix_keeps_utf8_suffix_and_digit_only_exhaustion() {
    for (token, maximum, expected) in [
        ("1α", 1, (1, Some("α".to_owned()))),
        ("12", 12, (12, None)),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = [token];
        let mut cursor = TokenCursor::new(&ctx, &tokens);
        assert_eq!(
            parse_reference_suffix(&mut cursor, "test suffix", maximum).unwrap(),
            expected
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn empty_token_cursor_returns_truncation_without_work_and_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cursor = TokenCursor::new(&ctx, &[]);
    assert!(
        matches!(cursor.next("test token"), Err(CodecError::Malformed(message))
        if message == "truncated test token in text B-rep Curves table")
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "prior token refusal") else {
        panic!("work refusal")
    };
    assert!(matches!(cursor.next("test token"),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn token_cursor_charges_before_each_actual_visit_and_keeps_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let tokens = ["first", "second"];
    let mut cursor = TokenCursor::new(&ctx, &tokens);
    assert_eq!(cursor.next("test token").unwrap(), "first");
    let Err(CodecError::ResourceLimit(original)) = cursor.next("test token") else {
        panic!("the second token visit must refuse")
    };
    assert_eq!(original.operation, "FreeCAD text B-rep token");
    assert_eq!(original.used, 1);
    assert_eq!(original.additional, 1);
    assert_eq!(cursor.peek(), Some("second"));
    assert!(matches!(cursor.next("test token"),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn periodic_endpoint_search_charges_only_the_two_bounded_runs() {
    use cadmpeg_ir::scalar::FiniteReal;
    for middle_count in [0, 4096] {
        let mut knots = vec![FiniteReal::ZERO; 3];
        knots.extend(std::iter::repeat_n(
            FiniteReal::new(0.5).unwrap(),
            middle_count,
        ));
        knots.extend([FiniteReal::ONE; 3]);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::super::normalize_periodic_knots(&ctx, knots, 2, true),
            Err(CodecError::Malformed(message)) if message == "periodic B-spline endpoint knots are invalid")
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn periodic_extension_overflow_does_not_copy_the_remaining_knots() {
    use cadmpeg_ir::scalar::FiniteReal;
    for middle_count in [1, 4096] {
        let mut knots = vec![FiniteReal::new(-f64::MAX).unwrap()];
        knots.extend(std::iter::repeat_n(FiniteReal::ZERO, middle_count));
        knots.push(FiniteReal::new(f64::MAX).unwrap());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(
            matches!(super::super::normalize_periodic_knots(&ctx, knots, 1, true),
            Err(CodecError::Malformed(message)) if message == "periodic B-spline extension exceeds finite knot range")
        );
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn periodic_knot_extension_charges_actual_visits_and_keeps_values() {
    use cadmpeg_ir::scalar::FiniteReal;
    let knots = [-1.0, 0.0, 1.0]
        .map(|value| FiniteReal::new(value).unwrap())
        .to_vec();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (knots, padding) = super::super::normalize_periodic_knots(&ctx, knots, 1, true).unwrap();
    assert_eq!(padding, 1);
    assert_eq!(
        knots.iter().map(|value| value.get()).collect::<Vec<_>>(),
        [-2.0, -1.0, 0.0, 1.0, 2.0]
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn periodic_pole_and_weight_copies_are_admitted_and_keep_source_order() {
    use cadmpeg_ir::scalar::FiniteReal;
    for spare_capacity in [false, true] {
        let mut points = vec![1_u8, 2];
        let mut weights = [10.0, 20.0]
            .map(|value| FiniteReal::new(value).unwrap())
            .to_vec();
        if spare_capacity {
            points.reserve_exact(2);
            weights.reserve_exact(2);
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        super::super::append_periodic_curve_poles(&ctx, &mut points, Some(&mut weights), 2)
            .unwrap();
        assert_eq!(points, [1, 2, 1, 2]);
        assert_eq!(
            weights.iter().map(|value| value.get()).collect::<Vec<_>>(),
            [10.0, 20.0, 10.0, 20.0]
        );
    }
}

#[test]
fn periodic_copy_refuses_before_copying_and_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut points = Vec::with_capacity(4);
    points.extend([1_u8, 2]);
    let Err(CodecError::ResourceLimit(original)) =
        super::super::append_periodic_curve_poles(&ctx, &mut points, None, 2)
    else {
        panic!("source-copy admission must refuse before copying")
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!(original.operation, "FreeCAD periodic B-rep curve poles");
    assert_eq!(original.used, 0);
    assert_eq!(points, [1, 2]);
    assert!(
        matches!(super::super::append_periodic_curve_poles(&ctx, &mut points, None, 0),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}

#[test]
fn periodic_weight_error_keeps_the_original_pole_copy_phase() {
    let mut points = vec![1_u8];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut weights = Vec::new();
    assert!(
        matches!(super::super::append_periodic_curve_poles(&ctx, &mut points, Some(&mut weights), 1),
        Err(CodecError::Malformed(message)) if message == "periodic B-spline has insufficient weights")
    );
    assert_eq!(points, [1, 1]);
    assert!(weights.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn nonperiodic_knots_and_zero_padding_move_no_storage_and_preserve_refusal() {
    use cadmpeg_ir::scalar::FiniteReal;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let knots = vec![FiniteReal::ONE];
    let pointer = knots.as_ptr();
    let (knots, padding) = super::super::normalize_periodic_knots(&ctx, knots, 1, false).unwrap();
    assert_eq!(knots, [FiniteReal::ONE]);
    assert_eq!(knots.as_ptr(), pointer);
    assert_eq!(padding, 0);
    let mut points = vec![1_u8];
    super::super::append_periodic_curve_poles(&ctx, &mut points, None, 0).unwrap();
    assert_eq!(points, [1]);
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "prior periodic refusal")
    else {
        panic!("work refusal")
    };
    assert!(
        matches!(super::super::normalize_periodic_knots(&ctx, Vec::new(), 1, false),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
    assert!(
        matches!(super::super::append_periodic_curve_poles(&ctx, &mut points, None, 0),
        Err(CodecError::ResourceLimit(actual)) if actual == original)
    );
}
