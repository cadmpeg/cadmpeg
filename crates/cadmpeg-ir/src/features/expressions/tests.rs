// SPDX-License-Identifier: Apache-2.0
use super::{Expression, ExpressionToken};

#[test]
fn identifier_spans_preserve_text_and_render_current_names() {
    let id = "test:design:parameter#width".try_into().unwrap();
    let expression = Expression::from_spans("2 * Width + 1", [(4..9, id)]).unwrap();
    assert_eq!(expression.dependencies().count(), 1);
    assert_eq!(
        expression
            .render_with(|_| Some("RenamedWidth".into()))
            .unwrap(),
        "2 * RenamedWidth + 1"
    );
    assert!(expression.render_with(|_| None).is_none());
    assert!(matches!(&expression.tokens[0], ExpressionToken::Text(text) if text == "2 * "));
}

#[test]
fn overlapping_and_non_utf8_spans_are_rejected() {
    let id = || "test:design:parameter#width".try_into().unwrap();
    assert!(Expression::from_spans("Width", [(0..4, id()), (3..5, id())]).is_err());
    assert!(Expression::from_spans("βeta", [(1..3, id())]).is_err());
    assert!(Expression::from_spans("Width", [(0..8, id())]).is_err());
}
