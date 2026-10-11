// SPDX-License-Identifier: Apache-2.0
//! BREP prefix visits, row allocation order and known token exhaustion.

use super::super::{grid_rows, parse_binary_prefix, parse_reference_suffix, BinaryCursor, TokenCursor};
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
        let Err(CodecError::ResourceLimit(original)) = grid_rows(&ctx, (vec![1, 2, 3, 4], None), 2)
        else {
            panic!("row storage must refuse before its values move")
        };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "FreeCAD B-rep surface row values");
        assert_eq!(original.used, used);
        assert_eq!(original.additional, 2);
    }
}

#[test]
fn grid_rows_charge_each_moved_value_once_and_keep_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        grid_rows(&ctx, (vec![1, 2, 3, 4], None), 2).unwrap(),
        [[1, 2], [3, 4]]
    );
}

#[test]
fn empty_and_invalid_grid_dimensions_need_no_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(grid_rows::<u8>(&ctx, (Vec::new(), None), 2)
        .unwrap()
        .is_empty());
    assert!(
        matches!(grid_rows(&ctx, (vec![1_u8], None), 2), Err(CodecError::Malformed(message))
        if message == "surface grid dimensions do not match pole count")
    );
    assert_eq!(ctx.resource_refusal(), None);
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
    for (token, maximum, expected) in [("1α", 1, (1, Some("α".to_owned()))), ("12", 12, (12, None))]
    {
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
fn empty_token_cursor_returns_truncation_without_work() {
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
            matches!(super::super::normalize_periodic_knots(&ctx, (knots, None), 2, true),
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
            matches!(super::super::normalize_periodic_knots(&ctx, (knots, None), 1, true),
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
    let (knots, padding) =
        super::super::normalize_periodic_knots(&ctx, (knots, None), 1, true).unwrap();
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
fn periodic_copy_refuses_before_copying() {
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
fn nonperiodic_knots_and_zero_padding_move_no_storage() {
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
    let (knots, padding) =
        super::super::normalize_periodic_knots(&ctx, (knots, None), 1, false).unwrap();
    assert_eq!(knots, [FiniteReal::ONE]);
    assert_eq!(knots.as_ptr(), pointer);
    assert_eq!(padding, 0);
    let mut points = vec![1_u8];
    super::super::append_periodic_curve_poles(&ctx, &mut points, None, 0).unwrap();
    assert_eq!(points, [1]);
    assert_eq!(ctx.resource_refusal(), None);
}

fn binary_work_used(ctx: &DecodeContext<'_>) -> u64 {
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(u64::MAX, "test work probe") else {
        panic!("the work probe must exceed the policy limit")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    limit.used
}

fn binary_polygon_payload(nodes: i32, header_padding: usize) -> Vec<u8> {
    let mut bytes = format!(
        "Open CASCADE Topology V3{}\nLocations 0\nCurve2ds 0\nCurves 0\nPolygon3D 1\n",
        "x".repeat(header_padding)
    ).into_bytes();
    bytes.extend(nodes.to_le_bytes());
    bytes.push(0);
    bytes.extend(1.0_f64.to_le_bytes());
    for _ in 0..nodes {
        for value in [0.0_f64, 1.0, 2.0] {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes.extend(b"PolygonOnTriangulations 0\nSurfaces 0\nTriangulations 0\nTShapes 0\n");
    bytes
}

fn binary_payload_work(nodes: i32, header_padding: usize) -> u64 {
    let bytes = binary_polygon_payload(nodes, header_padding);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::desktop();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let (facts, _) = parse_binary_prefix(&ctx, &bytes).unwrap();
    assert_eq!(facts.polygons3d.len(), 1);
    assert_eq!(facts.polygons3d[0].nodes.len(), usize::try_from(nodes).unwrap());
    assert_eq!(facts.polygons3d[0].nodes[0].coordinates().map(|value| value.get()), [0.0, 1.0, 2.0]);
    binary_work_used(&ctx)
}

#[test]
fn binary_cursor_invalid_header_does_not_pay_for_unread_suffix() {
    for suffix_length in [0, 4096] {
        let mut bytes = b"wrong\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', suffix_length));
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        assert!(matches!(parse_binary_prefix(&ctx, &bytes),
            Err(CodecError::Malformed(message)) if message == "unsupported binary B-rep header"));
        // Search includes the newline; validation and trim each visit five bytes.
        assert_eq!(binary_work_used(&ctx), 6 + 5 + 5);
    }
}

#[test]
fn binary_cursor_valid_payload_growth_pays_each_pass_once() {
    for padding in [0, 4096] {
        for nodes in [10, 1000] {
            assert_eq!(binary_payload_work(nodes + 1, padding) - binary_payload_work(nodes, padding), 24);
        }
    }
    for nodes in [10, 1000] {
        for padding in [100, 4096] {
            // A header byte is visited by search, UTF-8 validation and trimming.
            assert_eq!(binary_payload_work(nodes, padding + 1) - binary_payload_work(nodes, padding), 3);
        }
    }
}

#[test]
fn binary_cursor_fixed_width_batches_charge_once_before_the_loop() {
    for count in [1, 1024] {
        let bytes = [1.0_f64.to_le_bytes(), 2.0_f64.to_le_bytes(), 3.0_f64.to_le_bytes()].concat().repeat(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::try_from(bytes.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut cursor = BinaryCursor::new(&ctx, &bytes);
        let points = cursor.read_counted(count, 24, "test point batch", |cursor| cursor.finite_point3("test point")).unwrap();
        assert_eq!(points.len(), count);
        assert_eq!(cursor.remaining(), 0);
        assert_eq!(binary_work_used(&ctx), u64::try_from(bytes.len()).unwrap());
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 23;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let bytes = [0_u8; 24];
    let mut cursor = BinaryCursor::new(&ctx, &bytes);
    let mut visits = 0;
    let result = cursor.read_counted(1, 24, "test point batch", |cursor| {
        visits += 1;
        cursor.finite_point3("test point")
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit)) if limit.used == 0 && limit.additional == 24));
    assert_eq!(visits, 0);
    assert_eq!(cursor.remaining(), 24);
}

#[test]
fn binary_cursor_bounds_are_checked_before_read_admission() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cursor = BinaryCursor::new(&ctx, &[0; 7]);
    assert!(matches!(cursor.finite_f64("test scalar"), Err(CodecError::Malformed(message)) if message == "truncated test scalar"));
    let result = cursor.read_counted(usize::MAX, 8, "test batch", |_| -> Result<u8, CodecError> {
        panic!("an oversized count must not start its loop")
    });
    assert!(matches!(result, Err(CodecError::Malformed(message)) if message == "test batch count exceeds remaining input"));
    assert_eq!(cursor.remaining(), 7);
    assert_eq!(binary_work_used(&ctx), 0);
}

#[test]
fn binary_cursor_scalar_reads_pay_consumed_widths_and_refuse_before_advance() {
    let mut bytes = vec![1];
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(3_i32.to_le_bytes());
    bytes.extend(4.0_f64.to_le_bytes());
    bytes.extend(5.0_f32.to_le_bytes());
    bytes.push(6);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 19;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut cursor = BinaryCursor::new(&ctx, &bytes);
    assert_eq!(cursor.u8("test byte").unwrap(), 1);
    assert_eq!(cursor.u16("test word").unwrap(), 2);
    assert_eq!(cursor.i32("test integer").unwrap(), 3);
    assert_eq!(cursor.finite_f64("test double").unwrap().get(), 4.0);
    assert_eq!(cursor.f32("test float").unwrap().get(), 5.0);
    assert!(matches!(cursor.u8("test refused byte"), Err(CodecError::ResourceLimit(limit)) if limit.used == 19 && limit.additional == 1));
    assert_eq!(cursor.remaining(), 1);
}
