// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::CodecError;

#[test]
fn entry_pcurve_block_invalid_header_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for tokens in [vec![], vec![Token::Ident("nubs".into())], vec![Token::Ident("nurbs".into()), Token::Long(0)], vec![Token::Ident("unknown".into())]] {
            for position in [0, tokens.len(), usize::MAX] {
                let result = super::super::pcurve_block_with_end(ctx, &tokens, position);
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
fn entry_explicit_pcurve_invalid_reference_preserves_original_refusal() {
    let table = crate::nurbs::toks::SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    crate::test_support::with_entry_context(|ctx, original| {
        for index in [-1, i64::MIN, 0, 1, i64::MAX] {
            let result = super::super::explicit_pcurve_cache_from_subtype_ref(ctx, index, &table);
            if let Some(first) = original {
                assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));
            } else {
                assert!(result.is_none());
            }
        }
    });
}
