// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::CodecError;

#[test]
fn entry_surface_block_invalid_header_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for tokens in [vec![], vec![Token::Ident("nubs".into())], vec![Token::Ident("nurbs".into()), Token::Long(0)], vec![Token::Ident("unknown".into())]] {
            for position in [0, tokens.len(), usize::MAX] {
                let result = super::super::surface_block(ctx, &tokens, position);
                if let Some(first) = original {
                    assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
                } else {
                    assert!(result.is_none());
                }
            }
        }
    });
}

#[test]
fn entry_curve_block_invalid_header_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for tokens in [vec![], vec![Token::Ident("nubs".into())], vec![Token::Ident("nurbs".into()), Token::Long(0)], vec![Token::Ident("unknown".into())]] {
            for position in [0, tokens.len(), usize::MAX] {
                let result = super::super::curve_block(ctx, &tokens, position);
                if let Some(first) = original {
                    assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
                } else {
                    assert!(result.is_none());
                }
            }
        }
    });
}

#[test]
fn entry_empty_subtype_reference_search_preserves_original_refusal() {
    let table = crate::nurbs::toks::SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = super::super::cache_from_subtype_refs::<(), _>(ctx, &[], &table, |_, _| panic!("empty search must not decode a scope"));
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
        } else {
            assert!(result.is_none());
        }
    });
}
