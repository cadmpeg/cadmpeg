// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;

#[test]
fn entry_marker_empty_priority_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {

        let result = super::super::find_owned_subtype_marker(ctx, &[], &[]);
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));

        } else {
            assert!(result.is_none());
        }
    });
}

#[test]
fn entry_token_intcurve_empty_name_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {

        let result = super::super::find_owned_intcurve_subtype(ctx, &[], "");
        if let Some(first) = original {
            assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if last == first));

        } else {
            assert!(result.is_none());
        }
    });
}

#[test]
fn entry_float_array_invalid_count_preserves_original_refusal_and_cursor() {
    crate::test_support::with_entry_context(|ctx, original| {
        for tokens in [&[][..], &[crate::sab::Token::False][..], &[crate::sab::Token::Long(-1)][..]] {
            let mut cur = super::super::Cur::at(tokens, 0);
            let result = cur.take_float_array(ctx);
            if let Some(first) = &original {
                assert!(matches!(result, Some(Err(CodecError::ResourceLimit(last))) if &last == first));
            } else { assert!(result.is_none()); }
            assert_eq!(cur.pos(), 0);
        }
    });
}
