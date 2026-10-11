// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::CodecError;

use crate::nurbs::toks::Cur;

#[test]
fn rolling_ball_curve_propagates_cache_refusal_before_missing_bounds() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let tokens = [
        Token::Ident("intcurve".into()),
        Token::False,
        Token::SubtypeOpen,
        Token::Ident("exact_int_cur".into()),
        Token::Ident("nubs".into()),
        Token::SubtypeClose,
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // The balanced scope scan uses no collection. Its owned nubs marker needs
    // exactly one index entry before any missing degree or bounds can recover.
    let mut cur = Cur::at(&tokens, 0);
    let Some(Err(CodecError::ResourceLimit(first))) =
        super::super::rolling_ball_curve(&ctx, &mut cur, None)
    else {
        panic!("cache marker refusal must precede missing bound recovery");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "ASM owned spline markers");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for _ in 0..64 {
        let mut cur = Cur::at(&tokens, 0);
        assert!(
            matches!(super::super::rolling_ball_curve(&ctx, &mut cur, None),
            Some(Err(CodecError::ResourceLimit(last))) if last == first)
        );
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

fn inline_curve(width: crate::kernel_header::RefWidth, rational: bool) -> Vec<u8> {
    let mut bytes = if rational {
        b"\x0d\x05nurbs".to_vec()
    } else {
        b"\x0d\x04nubs".to_vec()
    };
    for (tag, value) in [(0x04, 1i64), (0x15, 0), (0x04, 2)] {
        bytes.push(tag);
        bytes.extend_from_slice(&value.to_le_bytes()[..width.bytes()]);
    }
    for knot in [0.0f64, 1.0] {
        bytes.push(0x06);
        bytes.extend_from_slice(&knot.to_le_bytes());
        bytes.push(0x04);
        bytes.extend_from_slice(&1i64.to_le_bytes()[..width.bytes()]);
    }
    for (point, weight) in [([0.0f64, 0.0, 0.0], 1.0f64), ([1.0, 2.0, 3.0], 2.0)] {
        for value in point.into_iter().chain(rational.then_some(weight)) {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[test]
fn inline_rolling_ball_curve_refuses_its_actual_byte_scans() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    for width in [
        crate::kernel_header::RefWidth::Four,
        crate::kernel_header::RefWidth::Eight,
    ] {
        let mut bytes = inline_curve(width, false);
        bytes.extend([0x0b, 0x0b]);
        for operation in [
            "scan ASM inline byte curve knot pairs",
            "scan ASM inline byte curve poles",
        ] {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                operation,
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    let mut position = 0;
                    let result =
                        super::super::decode_rolling_ball_curve(&ctx, &bytes, &mut position, width)
                            .transpose()
                            .map(|_| ());
                    if result.is_err() {
                        assert_eq!(position, 0);
                    }
                    result
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("byte curve scan refusal");
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(limit.additional, 1);
            assert_eq!(limit.limit, limit.used);
        }
    }
}

#[test]
fn inline_rolling_ball_curve_preserves_polynomial_and_rational_geometry() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    for width in [
        crate::kernel_header::RefWidth::Four,
        crate::kernel_header::RefWidth::Eight,
    ] {
        for rational in [false, true] {
            let mut bytes = inline_curve(width, rational);
            bytes.extend([0x0b, 0x0b]);
            let arena = DecodeArena::new();
            let (ctx, _) =
                DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
            let mut position = 0;
            let decoded =
                super::super::decode_rolling_ball_curve(&ctx, &bytes, &mut position, width)
                    .unwrap()
                    .unwrap();
            assert_eq!(position, bytes.len());
            assert_eq!(decoded.parameter_range, [None, None]);
            let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)) = decoded.curve else {
                panic!("inline NURBS curve");
            };
            assert_eq!(curve.degree(), 1);
            assert_eq!(curve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
            assert_eq!(
                curve.pole_rows().point_at(1).unwrap().get(),
                cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
            );
            assert_eq!(curve.pole_rows().weight_at(1), rational.then_some(2.0));
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn inline_rolling_ball_curve_rejects_missing_bounds_without_retaining_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let bytes = inline_curve(crate::kernel_header::RefWidth::Four, true);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16384;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut position = 0;
    assert!(super::super::decode_rolling_ball_curve(
        &ctx,
        &bytes,
        &mut position,
        crate::kernel_header::RefWidth::Four
    )
    .is_none());
    assert_eq!(position, bytes.len());
    let storage = ctx
        .reserve_scoped(16384, "test rejected inline curve storage")
        .unwrap();
    drop(storage);
    ctx.finish_session().unwrap();
}

#[test]
fn rolling_ball_all_null_side_keeps_location_extension_and_cursor() {
    let tokens = [
        Token::Str("blend_support_surface".into()),
        Token::Ident("null_surface".into()),
        Token::Ident("null_curve".into()),
        Token::Ident("nullbs".into()),
        Token::Position([1.0, 2.0, 3.0]),
        Token::Ident("nullbs".into()),
        Token::Long(7),
        Token::Ident("nullbs".into()),
    ];
    crate::test_support::with_zero_context(|ctx| {
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::rolling_ball_side(ctx, &mut cur, None);

        let side = result.unwrap().unwrap();
        assert!(side.surface.is_none() && side.curve.is_none() && side.pcurve.is_none());
        assert!(side.secondary_pcurve.is_none());
        assert_eq!(
            side.location,
            cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
        );
        let extension = side.extension.unwrap();
        assert_eq!(extension.value, 7);
        assert!(extension.pcurve.is_none());
        assert_eq!(cur.pos(), tokens.len());
    });
}

#[test]
fn vertex_blend_degenerate_boundary_keeps_values_and_cursor() {
    let tokens = [
        Token::Str("deg".into()),
        Token::False,
        Token::Position([0.0, 0.0, 1.0]),
        Token::True,
        Token::False,
        Token::Double(0.5),
        Token::Position([1.0, 2.0, 3.0]),
        Token::Vector3([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 1.0, 0.0]),
    ];
    crate::test_support::with_zero_context(|ctx| {
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::vertex_blend_boundary(ctx, &mut cur);

        let boundary = result.unwrap().unwrap();
        assert!(!boundary.boundary_type && boundary.u_smoothing && !boundary.v_smoothing);
        assert_eq!(boundary.fullness, 0.5);
        assert_eq!(
            boundary.magic,
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        let crate::nurbs::proc_surface::EmbeddedVertexBlendBoundaryGeometry::Degenerate {
            location,
            normals,
        } = boundary.geometry
        else {
            panic!("degenerate boundary grammar");
        };
        assert_eq!(location, cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0));
        assert_eq!(
            normals,
            [
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
            ]
        );
        assert_eq!(cur.pos(), tokens.len());
    });
}
