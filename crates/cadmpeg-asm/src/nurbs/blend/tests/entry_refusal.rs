// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::CodecError;

use crate::nurbs::toks::Cur;

#[test]
fn entry_rolling_ball_side_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::rolling_ball_side(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_rolling_ball_surface_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::rolling_ball_surface(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_rolling_ball_curve_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::rolling_ball_curve(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_rolling_ball_third_side_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::rolling_ball_third_side(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_vertex_blend_boundary_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::vertex_blend_boundary(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_revision_vertex_blend_boundary_missing_fields_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::super::revision_vertex_blend_boundary(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
        assert_eq!(cur.pos(), 0);
    });
}

#[test]
fn entry_optional_rolling_ball_null_surface_preserves_original_refusal_and_cursor() {
    let tokens = [Token::Ident("null_surface".into())];
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::optional_rolling_ball_surface(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null))));
            assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_byte_rolling_ball_curve_invalid_marker_preserves_original_refusal_and_position() {
    use crate::kernel_header::RefWidth;
    // A complete B-spline marker without a degree is a fixed recovery route.
    let bytes = crate::nurbs::reader::NUBS_MARKER;
    crate::test_support::with_entry_context(|ctx, original| {
        for width in [RefWidth::Four, RefWidth::Eight] {
            for input in [&[][..], bytes] {
                let mut position = 0;
                let result = super::super::decode_rolling_ball_curve(ctx, input, &mut position, width);
                if let Some(first) = original {
                    assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
                } else {
                    assert!(result.is_none());
                }
                assert_eq!(position, 0);
            }
        }
    });
}

#[test]
fn rolling_ball_curve_propagates_cache_refusal_before_missing_bounds() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let tokens = [Token::Ident("intcurve".into()), Token::False,
        Token::SubtypeOpen, Token::Ident("exact_int_cur".into()),
        Token::Ident("nubs".into()), Token::SubtypeClose];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    // The balanced scope scan uses no collection. Its owned nubs marker needs
    // exactly one index entry before any missing degree or bounds can recover.
    let mut cur = Cur::at(&tokens, 0);
    let Some(Err(CodecError::ResourceLimit(first))) = super::super::rolling_ball_curve(&ctx, &mut cur, None) else {
        panic!("cache marker refusal must precede missing bound recovery");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "ASM owned spline markers");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for _ in 0..64 {
        let mut cur = Cur::at(&tokens, 0);
        assert!(matches!(super::super::rolling_ball_curve(&ctx, &mut cur, None),
            Some(Err(CodecError::ResourceLimit(last))) if last == first));
        assert_eq!(cur.pos(), 0);
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn entry_rolling_ball_all_null_side_keeps_fixed_recovery_free() {
    let tokens = [Token::Str("blend_support_surface".into()),
        Token::Ident("null_surface".into()), Token::Ident("null_curve".into()),
        Token::Ident("nullbs".into()), Token::Position([1.0, 2.0, 3.0]),
        Token::Ident("nullbs".into()), Token::Long(7), Token::Ident("nullbs".into())];
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::rolling_ball_side(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            let side = result.unwrap().unwrap();
            assert!(side.surface.is_none() && side.curve.is_none() && side.pcurve.is_none());
            assert!(side.secondary_pcurve.is_none());
            assert_eq!(side.location, cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0));
            let extension = side.extension.unwrap();
            assert_eq!(extension.value, 7);
            assert!(extension.pcurve.is_none());
            assert_eq!(cur.pos(), tokens.len());
        }
    });
}

#[test]
fn entry_vertex_blend_degenerate_boundary_keeps_fixed_recovery_free() {
    let tokens = [Token::Str("deg".into()), Token::False,
        Token::Position([0.0, 0.0, 1.0]), Token::True, Token::False, Token::Double(0.5),
        Token::Position([1.0, 2.0, 3.0]), Token::Vector3([1.0, 0.0, 0.0]), Token::Vector3([0.0, 1.0, 0.0])];
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&tokens, 0);
        let result = super::super::vertex_blend_boundary(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            let boundary = result.unwrap().unwrap();
            assert!(!boundary.boundary_type && boundary.u_smoothing && !boundary.v_smoothing);
            assert_eq!(boundary.fullness, 0.5);
            assert_eq!(boundary.magic, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            let crate::nurbs::proc_surface::EmbeddedVertexBlendBoundaryGeometry::Degenerate { location, normals } = boundary.geometry else {
                panic!("degenerate boundary grammar");
            };
            assert_eq!(location, cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0));
            assert_eq!(normals, [cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0), cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)]);
            assert_eq!(cur.pos(), tokens.len());
        }
    });
}
