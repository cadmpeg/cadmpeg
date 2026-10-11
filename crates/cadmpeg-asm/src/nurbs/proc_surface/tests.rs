// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn fixed_loft_bridge_tokens_keep_values_and_advance_cursor() {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    crate::test_support::with_zero_context(|ctx| {
        for (token, expected) in [
            (Token::False, LoftBridgeToken::Boolean(false)),
            (Token::Long(7), LoftBridgeToken::Integer(7)),
            (Token::Double(2.0), LoftBridgeToken::Double(2.0)),
            (Token::Enum(3), LoftBridgeToken::Enum(3)),
        ] {
            let tokens = [token];
            let mut cur = Cur::at(&tokens, 0);
            let result = super::bridge_token(ctx, &mut cur);
            assert_eq!(result.unwrap().unwrap(), expected);
            assert_eq!(cur.pos(), 1);
        }
    });
}

#[test]
fn t_spline_reference_keeps_index_and_advances_cursor() {
    crate::test_support::with_zero_context(|ctx| {
        let tokens = [Token::Ident("ref".into()), Token::Long(7)];
        let mut cur = Cur::at(&tokens, 0);
        let result = super::t_spline_subtransform(ctx, &mut cur);

        assert!(matches!(
            result,
            Some(Ok(super::EmbeddedTSplineSubtransform::Reference {
                index: 7,
                resolved: None
            }))
        ));
        assert_eq!(cur.pos(), 2);
    });
}
