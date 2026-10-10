// SPDX-License-Identifier: Apache-2.0
//! Scratch knot and surface backing does not accumulate as retained storage.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use super::super::{TokenCursor, parse_bezier_surface, normalize_periodic_knots};

#[test]
fn periodic_source_knots_release_before_the_next_normalized_curve() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 256;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut normalized = Vec::new();
    for _ in 0..4 {
        let source = ctx.with_scoped_storage("test knot source", || {
            let mut knots = ctx.collection_vec(5, "test source knots")?;
            knots.extend([0.0, 0.0, 0.5, 1.0, 1.0].map(|value| FiniteReal::new(value).unwrap()));
            Ok::<_, CodecError>(knots)
        }).unwrap();
        let result = normalize_periodic_knots(&ctx, (source.0, Some(source.1)), 2, true).unwrap();
        assert_eq!(result.1, 1);
        assert_eq!(result.0.iter().map(|knot| knot.get()).collect::<Vec<_>>(), [-0.5, 0.0, 0.0, 0.5, 1.0, 1.0, 1.5]);
        normalized.push(result.0);
    }
    assert_eq!(normalized.len(), 4);
    let source = ctx.with_scoped_storage("test knot source", || ctx.collection_vec::<FiniteReal>(5, "test source knots")).unwrap();
    let result = normalize_periodic_knots(&ctx, (source.0, Some(source.1)), 2, true);
    assert!(matches!(result, Err(CodecError::Malformed(message)) if message == "periodic B-spline has no knots"));
}

#[test]
fn bezier_surface_flat_lanes_are_scoped_through_row_conversion() {
    for count in [1, 4] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 320 * count;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = ["0", "0", "1", "1", "0", "0", "0", "0", "1", "0", "1", "0", "0", "1", "1", "0"];
        let mut surfaces = Vec::new();
        for _ in 0..count {
            let mut cursor = TokenCursor::new(&ctx, &tokens);
            let surface = parse_bezier_surface(&mut cursor).unwrap();
            assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
            assert_eq!(surface.poles().iter().map(|point| point.get()).collect::<Vec<_>>(), [
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(0.0, 1.0, 0.0),
                cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(1.0, 1.0, 0.0),
            ]);
            surfaces.push(surface);
        }
        assert_eq!(surfaces.len(), usize::try_from(count).unwrap());
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn surface_rows_still_refuse_retained_output_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let tokens = ["0", "0", "1", "1", "0", "0", "0", "0", "1", "0", "1", "0", "0", "1", "1", "0"];
    let mut cursor = TokenCursor::new(&ctx, &tokens);
    assert!(matches!(parse_bezier_surface(&mut cursor), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn periodic_surface_releases_original_and_extended_flat_backing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 512;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = ctx.with_scoped_storage("test source grid", || {
        let mut points = ctx.collection_vec(6, "test source poles")?;
        for u in 0..3 { for v in 0..2 {
            points.push(cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(f64::from(u), f64::from(v), 0.0)).unwrap());
        } }
        Ok::<_, CodecError>(points)
    }).unwrap();
    let surface = super::super::normalize_periodic_surface(
        &ctx, [2, 1],
        [([0.0, 0.0, 0.5, 1.0, 1.0].map(|value| FiniteReal::new(value).unwrap()).to_vec(), None),
         (vec![FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ONE], None)],
        [3, 2], (points.0, Some(points.1)), None, [true, false],
    ).unwrap();
    assert_eq!((surface.u_count(), surface.v_count()), (4, 2));
    let poles = surface.poles();
    assert_eq!(&poles[6..8], &poles[0..2]);
    assert_eq!(surface.u_knots().as_slice(), [-0.5, 0.0, 0.0, 0.5, 1.0, 1.0, 1.5]);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn rational_surface_flat_weights_are_scoped() {
    let tokens = ["1", "0", "1", "1", "0", "0", "0", "1", "0", "1", "0", "2", "1", "0", "0", "3", "1", "1", "0", "4"];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 768;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let surface = parse_bezier_surface(&mut TokenCursor::new(&ctx, &tokens)).unwrap();
    assert_eq!((surface.u_count(), surface.v_count()), (2, 2));
    for (u, v, weight) in [(0, 0, 1.0), (0, 1, 2.0), (1, 0, 3.0), (1, 1, 4.0)] {
        assert_eq!(surface.weight(u, v).unwrap().get(), weight);
        assert_eq!(surface.pole(u, v).unwrap().get(), cadmpeg_ir::math::Point3::new(f64::from(u32::try_from(u).unwrap()), f64::from(u32::try_from(v).unwrap()), 0.0));
    }
    crate::test_support::materialized_refusal_at("FreeCAD B-rep weights", |ctx| parse_bezier_surface(&mut TokenCursor::new(ctx, &tokens)));
}

fn periodic_weighted_surface(ctx: &DecodeContext<'_>) -> Result<cadmpeg_ir::geometry::nurbs::NurbsSurface, CodecError> {
    let points = ctx.with_scoped_storage("test source grid", || {
        let mut points = ctx.collection_vec(6, "test source poles")?;
        for u in 0..3 { for v in 0..2 {
            points.push(cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(f64::from(u), f64::from(v), 0.0)).unwrap());
        } }
        Ok::<_, CodecError>(points)
    })?;
    let weights = ctx.with_scoped_storage("test source weights", || {
        let mut weights = ctx.collection_vec(6, "test source weights")?;
        weights.extend([1.0, 2.0, 3.0, 4.0, 5.0, 6.0].map(|value| FiniteReal::new(value).unwrap()));
        Ok::<_, CodecError>(weights)
    })?;
    super::super::normalize_periodic_surface(
        ctx, [2, 1],
        [([0.0, 0.0, 0.5, 1.0, 1.0].map(|value| FiniteReal::new(value).unwrap()).to_vec(), None),
         (vec![FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ONE], None)],
        [3, 2], (points.0, Some(points.1)), Some((weights.0, Some(weights.1))), [true, false],
    )
}

#[test]
fn periodic_surface_preserves_scoped_weight_lanes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1536;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let surface = periodic_weighted_surface(&ctx).unwrap();
    assert_eq!((surface.u_count(), surface.v_count()), (4, 2));
    for (u, expected) in [(0, [1.0, 2.0]), (1, [3.0, 4.0]), (2, [5.0, 6.0]), (3, [1.0, 2.0])] {
        for (v, weight) in expected.into_iter().enumerate() {
            assert_eq!(surface.weight(u, v).unwrap().get(), weight);
            assert_eq!(surface.pole(u, v).unwrap().get(), cadmpeg_ir::math::Point3::new(f64::from(u32::try_from(u % 3).unwrap()), f64::from(u32::try_from(v).unwrap()), 0.0));
        }
    }
    assert_eq!(ctx.resource_refusal(), None);
    crate::test_support::materialized_refusal_at("FreeCAD periodic B-rep surface weights", periodic_weighted_surface);
}

#[test]
fn periodic_curve_sources_are_scoped_by_text_and_binary_parsers() {
    for binary in [false, true] {
        for two_dimensional in [false, true] {
            let mut bytes = vec![7_u8, 0, 1];
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&3_i32.to_le_bytes());
            bytes.extend_from_slice(&3_i32.to_le_bytes());
            for x in [0.0_f64, 1.0, 2.0] {
                bytes.extend_from_slice(&x.to_le_bytes());
                bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                if !two_dimensional {
                    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
                }
            }
            for (knot, multiplicity) in [(0.0_f64, 2_i32), (0.5, 1), (1.0, 2)] {
                bytes.extend_from_slice(&knot.to_le_bytes());
                bytes.extend_from_slice(&multiplicity.to_le_bytes());
            }
            let text = if two_dimensional {
                "7 0 1 2 3 3 0 0 1 0 2 0 0 2 0.5 1 1 2"
            } else {
                "7 0 1 2 3 3 0 0 0 1 0 0 2 0 0 0 2 0.5 1 1 2"
            };
            let tokens: Vec<_> = text.split_ascii_whitespace().collect();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = if two_dimensional { 768 } else { 1152 };
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut curves2d = Vec::new();
            let mut curves3d = Vec::new();
            for _ in 0..4 {
                if two_dimensional {
                    let curve = if binary {
                        super::super::parse_binary_curve2d(&mut super::super::BinaryCursor::new(&ctx, &bytes), 0).unwrap()
                    } else {
                        super::super::parse_curve2d(&mut TokenCursor::new(&ctx, &tokens), 0, 1).unwrap()
                    };
                    let super::super::TextCurve2d::Nurbs(curve) = curve else { panic!("expected NURBS curve"); };
                    assert_eq!(curve.degree, 2);
                    assert!(curve.periodic);
                    assert_eq!(curve.control_points.len(), 4);
                    assert_eq!(curve.control_points[3], curve.control_points[0]);
                    assert_eq!(curve.knots.iter().map(|value| value.get()).collect::<Vec<_>>(), [-0.5, 0.0, 0.0, 0.5, 1.0, 1.0, 1.5]);
                    curves2d.push(curve);
                } else {
                    let curve = if binary {
                        super::super::parse_binary_curve(&mut super::super::BinaryCursor::new(&ctx, &bytes), 0).unwrap()
                    } else {
                        super::super::parse_curve(&mut TokenCursor::new(&ctx, &tokens), 0, 1).unwrap()
                    };
                    let super::super::TextCurve::Nurbs(curve) = curve else { panic!("expected NURBS curve"); };
                    assert_eq!(curve.degree(), 2);
                    assert_eq!(curve.pole_count(), 4);
                    let points = curve.control_points();
                    assert_eq!(points[3], points[0]);
                    assert_eq!(curve.knots().as_slice(), [-0.5, 0.0, 0.0, 0.5, 1.0, 1.0, 1.5]);
                    curves3d.push(curve);
                }
            }
            assert_eq!(curves2d.len() + curves3d.len(), 4);
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}

#[test]
fn surface_flat_sources_are_scoped_by_text_nurbs_and_binary_parsers() {
    for kind in 0..3 {
        let nurbs = kind != 1;
        let mut bytes = vec![if nurbs { 9_u8 } else { 8 }, 0, 0];
        if nurbs { bytes.extend_from_slice(&[1, 0]); }
        bytes.extend_from_slice(&(if nurbs { 2_u16 } else { 1 }).to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        let u_count = if nurbs { 3_i32 } else { 2 };
        if nurbs {
            for count in [u_count, 2, 3, 2] { bytes.extend_from_slice(&count.to_le_bytes()); }
        }
        for u in 0..u_count { for v in 0..2 {
            for value in [f64::from(u), f64::from(v), 0.0] { bytes.extend_from_slice(&value.to_le_bytes()); }
        } }
        if nurbs {
            for (knot, multiplicity) in [(0.0_f64, 2_i32), (0.5, 1), (1.0, 2), (0.0, 2), (1.0, 2)] {
                bytes.extend_from_slice(&knot.to_le_bytes());
                bytes.extend_from_slice(&multiplicity.to_le_bytes());
            }
        }
        let tokens: Vec<_> = "0 0 1 0 2 1 3 2 3 2 0 0 0 0 1 0 1 0 0 1 1 0 2 0 0 2 1 0 0 2 0.5 1 1 2 0 2 1 2".split_ascii_whitespace().collect();
        for count in [1_u64, 4] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = (if nurbs { 512 } else { 320 }) * count;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut surfaces = Vec::new();
            for _ in 0..count {
                let surface = if kind == 0 {
                    super::super::parse_nurbs_surface(&mut TokenCursor::new(&ctx, &tokens)).unwrap()
                } else {
                    let surface = super::super::parse_binary_surface(&mut super::super::BinaryCursor::new(&ctx, &bytes), 0).unwrap();
                    let super::super::TextSurface::Nurbs(surface) = surface else { panic!("expected NURBS surface"); };
                    surface
                };
                let expected_u = if nurbs { 4 } else { 2 };
                assert_eq!((surface.u_count(), surface.v_count()), (expected_u, 2));
                for u in 0..expected_u { for v in 0..2 {
                    let source_u = if nurbs { u % 3 } else { u };
                    assert_eq!(surface.pole(u, v).unwrap().get(), cadmpeg_ir::math::Point3::new(f64::from(u32::try_from(source_u).unwrap()), f64::from(u32::try_from(v).unwrap()), 0.0));
                } }
                if nurbs {
                    assert_eq!(surface.u_knots().as_slice(), [-0.5, 0.0, 0.0, 0.5, 1.0, 1.0, 1.5]);
                } else { assert_eq!(surface.u_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]); }
                assert_eq!(surface.v_knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
                surfaces.push(surface);
            }
            assert_eq!(surfaces.len(), usize::try_from(count).unwrap());
            assert_eq!(ctx.resource_refusal(), None);
        }
    }
}
