// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

#[test]
fn entry_nullable_pcurve_null_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [crate::sab::Token::Ident("nullbs".into())]; let mut cur = crate::nurbs::toks::Cur::at(&tokens, 0);
        let result = super::nullable_embedded_pcurve(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null)))); assert_eq!(cur.pos(), 1);
        }
    });
}

use crate::nurbs::toks::{Cur, SubtypeTable};
use crate::sab::Token;

#[test]
fn entry_g2_side_missing_label_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur::at(&[], 0);
        let result = super::g2_side(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(result.is_none()); assert_eq!(cur.pos(), 0);
        }
    });
}

#[test]
fn entry_compound_scale_null_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::False]; let mut cur = Cur::at(&tokens, 0);
        let result = super::compound_loft_scale(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(crate::nurbs::reader::Nullable::Null)))); assert_eq!(cur.pos(), 0);
        }
    });
}

#[test]
fn entry_fixed_loft_bridge_preserves_original_refusal_and_cursor() {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    crate::test_support::with_entry_context(|ctx, original| {
        for (token, expected) in [(Token::False, LoftBridgeToken::Boolean(false)),
            (Token::Long(7), LoftBridgeToken::Integer(7)),
            (Token::Double(2.0), LoftBridgeToken::Double(2.0)),
            (Token::Enum(3), LoftBridgeToken::Enum(3))] {
            let tokens = [token]; let mut cur = Cur::at(&tokens, 0);
            let result = super::bridge_token(ctx, &mut cur);
            if let Some(first) = original {
                assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
                assert_eq!(cur.pos(), 0);
            } else { assert_eq!(result.unwrap().unwrap(), expected); assert_eq!(cur.pos(), 1); }
        }
    });
}

#[test]
fn entry_ellipse_invalid_major_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {

        let result = super::ellipse_to_nurbs(ctx, [0.0; 3], [0.0, 0.0, 1.0], [0.0; 3], 1.0);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));

        } else {
            assert!(result.is_none());
        }
    });
}

#[test]
fn entry_fixed_loft_subdata_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Long(211), Token::Long(4), Token::Long(0), Token::Double(2.0), Token::Double(3.0)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::loft_subdata_form(ctx, &mut cur, true);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert_eq!(result.unwrap().unwrap(), cadmpeg_ir::geometry::LoftSubdata::type_211([4, 0], [2.0, 3.0])); assert_eq!(cur.pos(), tokens.len());
        }
    });
}

#[test]
fn entry_legacy_loft_section_negative_count_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Long(-1)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::loft_section(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(result.is_none()); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_revision_loft_section_negative_count_preserves_original_refusal() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Long(-1)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::revision_loft_section(ctx, &mut cur, &table, false);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else { assert!(result.is_none()); assert_eq!(cur.pos(), 1); }
    });
}

#[test]
fn entry_revision_cl_scale_negative_count_preserves_original_refusal() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Long(-1)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::revision_cl_scale(ctx, &mut cur, &table, false);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else { assert!(result.is_none()); assert_eq!(cur.pos(), 1); }
    });
}

#[test]
fn entry_revision_loft_missing_resolver_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {

        let result = super::revision_loft(ctx, &[], 0, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));

        } else {
            assert!(result.is_none());
        }
    });
}

#[test]
fn entry_revision_compound_loft_missing_resolver_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {

        let result = super::revision_compound_loft(ctx, &[], None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));

        } else {
            assert!(result.is_none());
        }
    });
}

#[test]
fn entry_sweep_empty_text_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Str(String::new())]; let mut cur = Cur::at(&tokens, 0);
        let result = super::sweep_law_expression(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(result.is_none()); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_law_formula_null_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Str("null_law".into())]; let mut cur = Cur::at(&tokens, 0);
        let result = super::law_formula_resolving(ctx, &mut cur, None);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(super::EmbeddedLawFormula::Null)))); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_revision_sweep_missing_revision_preserves_original_refusal() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = super::revision_sweep_sur(ctx, &[], 0, &table);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else { assert!(result.is_none()); }
    });
}

#[test]
fn entry_revision_surface_unknown_enum_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Enum(1)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::revision_surface_tail(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(result.is_none()); assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn entry_t_spline_reference_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        let tokens = [Token::Ident("ref".into()), Token::Long(7)]; let mut cur = Cur::at(&tokens, 0);
        let result = super::t_spline_subtransform(ctx, &mut cur);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            assert_eq!(cur.pos(), 0);
        } else {
            assert!(matches!(result, Some(Ok(super::EmbeddedTSplineSubtransform::Reference { index: 7, resolved: None })))); assert_eq!(cur.pos(), 2);
        }
    });
}
